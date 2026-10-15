use std::{
    collections::{HashMap, VecDeque},
    fmt, fs,
    hash::{DefaultHasher, Hasher},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_media::image_kind;
use koushi_state::AvatarThumbnailState;

/// Long edge requested for every avatar thumbnail (#1396). The largest avatar
/// slot is 40 CSS px (`--message-avatar-size`), so 120 px covers it at 3x DPI.
pub(crate) const AVATAR_THUMBNAIL_LONG_EDGE: u32 = 120;
/// Cache identity of the one avatar variant the product requests. The key has
/// to distinguish it from any other size so two call sites asking for different
/// sizes cannot collide.
pub(crate) const AVATAR_THUMBNAIL_IDENTITY: &str = "fit-120-webp";
pub(crate) const MAX_RENDERABLE_THUMBNAIL_ENTRIES: usize = 256;
/// Avatars and link previews no longer share one 32 MiB pool (#1396): a 120 px
/// avatar is tens of KiB, while a link preview legitimately needs more.
const MAX_AVATAR_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;
const MAX_AVATAR_THUMBNAIL_ENTRY_BYTES: usize = 256 * 1024;
const MAX_LINK_PREVIEW_THUMBNAIL_BYTES: usize = 32 * 1024 * 1024;
const MAX_LINK_PREVIEW_THUMBNAIL_ENTRY_BYTES: usize = 32 * 1024 * 1024;
const MAX_THUMBNAIL_LEASE_BYTES: usize = 96 * 1024 * 1024;

/// Count-and-byte bounds for one cache kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RenderableThumbnailBudget {
    max_entries: usize,
    max_total_bytes: usize,
    max_entry_bytes: usize,
}

/// Identity recorded for kinds whose bytes are not derived from a sized
/// request (a link preview stores the page image the server returned).
const LINK_PREVIEW_IDENTITY: &str = "page";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderableThumbnailKind {
    Avatar,
    LinkPreview,
}

impl RenderableThumbnailKind {
    fn path_segment(self) -> &'static str {
        match self {
            Self::Avatar => "avatar",
            Self::LinkPreview => "link-preview",
        }
    }

    fn budget(self) -> RenderableThumbnailBudget {
        match self {
            Self::Avatar => RenderableThumbnailBudget {
                max_entries: MAX_RENDERABLE_THUMBNAIL_ENTRIES,
                max_total_bytes: MAX_AVATAR_THUMBNAIL_BYTES,
                max_entry_bytes: MAX_AVATAR_THUMBNAIL_ENTRY_BYTES,
            },
            Self::LinkPreview => RenderableThumbnailBudget {
                max_entries: MAX_RENDERABLE_THUMBNAIL_ENTRIES,
                max_total_bytes: MAX_LINK_PREVIEW_THUMBNAIL_BYTES,
                max_entry_bytes: MAX_LINK_PREVIEW_THUMBNAIL_ENTRY_BYTES,
            },
        }
    }

    fn default_identity(self) -> &'static str {
        match self {
            Self::Avatar => AVATAR_THUMBNAIL_IDENTITY,
            Self::LinkPreview => LINK_PREVIEW_IDENTITY,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderableThumbnailContent {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
}

impl RenderableThumbnailContent {
    /// Content with only bytes known; the mime type is detected on store and
    /// the dimensions stay unknown.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            mime_type: None,
            width: None,
            height: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderableThumbnailStoreError {
    TooLarge { byte_count: usize, max_bytes: usize },
}

impl fmt::Display for RenderableThumbnailStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { .. } => {
                formatter.write_str("renderable thumbnail exceeds cache bound")
            }
        }
    }
}

impl std::error::Error for RenderableThumbnailStoreError {}

struct RenderableThumbnailEntry {
    kind: RenderableThumbnailKind,
    account_scope: String,
    /// The media source this entry was derived from, so a superseded owner
    /// avatar can retire exactly its own bytes (#1396).
    source: String,
    bytes: Vec<u8>,
    mime_type: String,
    width: Option<u64>,
    height: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ThumbnailLeaseError {
    Unavailable,
    Capacity,
}

/// Private byte ownership, not permission to access a retired or foreign scope.
#[derive(Clone)]
pub(crate) struct RenderableThumbnailLease(Arc<ChargedThumbnail>);

struct ChargedThumbnail {
    source_ref: String,
    entry: Arc<RenderableThumbnailEntry>,
    usage: Arc<Mutex<usize>>,
}

impl Drop for ChargedThumbnail {
    fn drop(&mut self) {
        *self.usage.lock().expect("thumbnail lease budget poisoned") -= self.entry.bytes.len();
    }
}

impl RenderableThumbnailLease {
    pub(crate) fn source_ref(&self) -> &str {
        &self.0.source_ref
    }

    pub(crate) fn control_bytes(&self) -> usize {
        std::mem::size_of::<ChargedThumbnail>() + self.0.source_ref.len()
    }

    pub(crate) fn thumbnail_state(&self) -> AvatarThumbnailState {
        AvatarThumbnailState::Ready {
            source_ref: self.0.source_ref.clone(),
            width: self.0.entry.width,
            height: self.0.entry.height,
            mime_type: Some(self.0.entry.mime_type.clone()),
        }
    }

    pub(crate) fn content(&self) -> RenderableThumbnailContent {
        RenderableThumbnailContent {
            bytes: self.0.entry.bytes.clone(),
            mime_type: Some(self.0.entry.mime_type.clone()),
            width: self.0.entry.width,
            height: self.0.entry.height,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderableThumbnailCacheStats {
    pub entry_count: usize,
    pub retained_bytes: usize,
    /// Per-kind split (#1396): avatars and link previews no longer share one
    /// budget, so the aggregate alone cannot say which kind occupies it.
    pub avatar_entry_count: usize,
    pub avatar_retained_bytes: usize,
    pub link_preview_entry_count: usize,
    pub link_preview_retained_bytes: usize,
    /// Bytes currently charged to independent leases. A lease charges its
    /// entry's bytes once per lease, so this is a conservative budget charge,
    /// not unique pinned memory: it can exceed `retained_bytes` and it overlaps
    /// the same entries. Never add it to `retained_bytes`.
    pub lease_charged_bytes: usize,
    pub high_water_entry_count: usize,
    pub high_water_bytes: usize,
    pub eviction_count: u64,
    pub clear_count: u64,
    pub retirement_count: u64,
    pub retired_bytes: u64,
    pub oversize_rejection_count: u64,
}

#[derive(Default)]
struct RenderableThumbnailCache {
    // Opaque references are stored in AppState while their bytes remain in this
    // count-and-byte-bounded LRU. Access refreshes recency; session clear drops
    // all retained bytes.
    entries: HashMap<String, Arc<RenderableThumbnailEntry>>,
    lease_bytes: Arc<Mutex<usize>>,
    // Oldest at the front, most recently accessed at the back. The bound is
    // deliberately larger than the existing 129-entry session churn contract.
    lru: VecDeque<String>,
    retained_bytes: usize,
    high_water_entry_count: usize,
    high_water_bytes: usize,
    eviction_count: u64,
    clear_count: u64,
    retirement_count: u64,
    retired_bytes: u64,
    oversize_rejection_count: u64,
}

impl RenderableThumbnailCache {
    fn kind_usage(&self, kind: RenderableThumbnailKind) -> (usize, usize) {
        self.entries
            .values()
            .filter(|entry| entry.kind == kind)
            .fold((0, 0), |(count, bytes), entry| {
                (count + 1, bytes + entry.bytes.len())
            })
    }

    fn insert(
        &mut self,
        kind: RenderableThumbnailKind,
        cache_key: String,
        account_scope: &str,
        source: &str,
        content: RenderableThumbnailContent,
    ) -> Result<(), RenderableThumbnailStoreError> {
        let budget = kind.budget();
        let mime_type = content
            .mime_type
            .clone()
            .unwrap_or_else(|| mime_type_from_bytes(&content.bytes));
        if content.bytes.len() > budget.max_entry_bytes {
            self.oversize_rejection_count = self.oversize_rejection_count.saturating_add(1);
            record(
                DiagnosticEvent::new(DiagnosticLevel::Warn, "core.renderable_thumbnail", "reject")
                    .field(DiagnosticField::token("reason", "oversize"))
                    .field(DiagnosticField::token("kind", kind.path_segment()))
                    .field(DiagnosticField::count(
                        "byte_count",
                        content.bytes.len() as u64,
                    ))
                    .field(DiagnosticField::count(
                        "max_bytes",
                        budget.max_entry_bytes as u64,
                    ))
                    .field(DiagnosticField::count(
                        "oversize_rejection_count",
                        self.oversize_rejection_count,
                    )),
            );
            return Err(RenderableThumbnailStoreError::TooLarge {
                byte_count: content.bytes.len(),
                max_bytes: budget.max_entry_bytes,
            });
        }
        let entry = RenderableThumbnailEntry {
            kind,
            account_scope: account_scope.to_owned(),
            source: source.to_owned(),
            bytes: content.bytes,
            mime_type,
            width: content.width,
            height: content.height,
        };
        if let Some(previous) = self.entries.remove(&cache_key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(previous.bytes.len());
            self.remove_from_lru(&cache_key);
        }
        self.retained_bytes = self.retained_bytes.saturating_add(entry.bytes.len());
        self.entries.insert(cache_key.clone(), Arc::new(entry));
        self.lru.push_back(cache_key);
        self.update_high_water();
        self.evict_if_needed(kind);
        Ok(())
    }

    fn get(&mut self, cache_key: &str) -> Option<RenderableThumbnailContent> {
        let entry = self.entries.get(cache_key)?.clone();
        self.touch(cache_key);
        Some(RenderableThumbnailContent {
            bytes: entry.bytes.clone(),
            mime_type: Some(entry.mime_type.clone()),
            width: entry.width,
            height: entry.height,
        })
    }

    fn lease(&mut self, cache_key: &str) -> Result<RenderableThumbnailLease, ThumbnailLeaseError> {
        let entry = self
            .entries
            .get(cache_key)
            .ok_or(ThumbnailLeaseError::Unavailable)?
            .clone();
        let mut usage = self
            .lease_bytes
            .lock()
            .expect("thumbnail lease budget poisoned");
        let total = usage
            .checked_add(entry.bytes.len())
            .filter(|total| *total <= MAX_THUMBNAIL_LEASE_BYTES)
            .ok_or(ThumbnailLeaseError::Capacity)?;
        *usage = total;
        drop(usage);
        self.touch(cache_key);
        Ok(RenderableThumbnailLease(Arc::new(ChargedThumbnail {
            source_ref: cache_key.into(),
            entry,
            usage: self.lease_bytes.clone(),
        })))
    }

    fn stats(&self) -> RenderableThumbnailCacheStats {
        let (avatar_entry_count, avatar_retained_bytes) =
            self.kind_usage(RenderableThumbnailKind::Avatar);
        let (link_preview_entry_count, link_preview_retained_bytes) =
            self.kind_usage(RenderableThumbnailKind::LinkPreview);
        RenderableThumbnailCacheStats {
            entry_count: self.entries.len(),
            retained_bytes: self.retained_bytes,
            avatar_entry_count,
            avatar_retained_bytes,
            link_preview_entry_count,
            link_preview_retained_bytes,
            lease_charged_bytes: *self
                .lease_bytes
                .lock()
                .expect("thumbnail lease budget poisoned"),
            high_water_entry_count: self.high_water_entry_count,
            high_water_bytes: self.high_water_bytes,
            eviction_count: self.eviction_count,
            clear_count: self.clear_count,
            retirement_count: self.retirement_count,
            retired_bytes: self.retired_bytes,
            oversize_rejection_count: self.oversize_rejection_count,
        }
    }

    fn clear(&mut self) {
        self.clear_matching(|_| true);
    }

    fn clear_account(&mut self, account_scope: &str) {
        self.clear_matching(|entry| entry.account_scope == account_scope);
    }

    fn clear_matching(&mut self, matches: impl Fn(&RenderableThumbnailEntry) -> bool) {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| matches(entry).then_some(key.clone()))
            .collect();
        let mut removed_bytes = 0usize;
        for key in &keys {
            if let Some(entry) = self.entries.remove(key) {
                removed_bytes = removed_bytes.saturating_add(entry.bytes.len());
                self.remove_from_lru(key);
            }
        }
        self.retained_bytes = self.retained_bytes.saturating_sub(removed_bytes);
        self.clear_count = self.clear_count.saturating_add(1);
        let removed_entries = keys.len();
        if removed_entries > 0 || removed_bytes > 0 {
            record(
                DiagnosticEvent::new(DiagnosticLevel::Debug, "core.renderable_thumbnail", "clear")
                    .field(DiagnosticField::count(
                        "removed_entries",
                        removed_entries as u64,
                    ))
                    .field(DiagnosticField::count(
                        "removed_bytes",
                        removed_bytes as u64,
                    ))
                    .field(DiagnosticField::count("clear_count", self.clear_count))
                    .field(DiagnosticField::count(
                        "high_water_entries",
                        self.high_water_entry_count as u64,
                    ))
                    .field(DiagnosticField::count(
                        "high_water_bytes",
                        self.high_water_bytes as u64,
                    )),
            );
        }
    }

    fn touch(&mut self, cache_key: &str) {
        self.remove_from_lru(cache_key);
        self.lru.push_back(cache_key.to_owned());
    }

    fn remove_from_lru(&mut self, cache_key: &str) {
        self.lru.retain(|key| key != cache_key);
    }

    fn update_high_water(&mut self) {
        self.high_water_entry_count = self.high_water_entry_count.max(self.entries.len());
        self.high_water_bytes = self.high_water_bytes.max(self.retained_bytes);
    }

    fn evict_if_needed(&mut self, kind: RenderableThumbnailKind) {
        let budget = kind.budget();
        let mut evicted_entries = 0usize;
        let mut evicted_bytes = 0usize;
        loop {
            let (kind_entries, kind_bytes) = self.kind_usage(kind);
            if kind_entries <= budget.max_entries && kind_bytes <= budget.max_total_bytes {
                break;
            }
            // Evict the oldest entry of the kind that exceeded its own budget;
            // another kind's newer or older entries are unaffected.
            let Some(oldest) = self
                .lru
                .iter()
                .find(|key| {
                    self.entries
                        .get(*key)
                        .is_some_and(|entry| entry.kind == kind)
                })
                .cloned()
            else {
                break;
            };
            let Some(entry) = self.entries.remove(&oldest) else {
                continue;
            };
            self.remove_from_lru(&oldest);
            self.retained_bytes = self.retained_bytes.saturating_sub(entry.bytes.len());
            evicted_entries += 1;
            evicted_bytes = evicted_bytes.saturating_add(entry.bytes.len());
            self.eviction_count = self.eviction_count.saturating_add(1);
        }
        if evicted_entries > 0 {
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Debug,
                    "core.renderable_thumbnail",
                    "eviction",
                )
                .field(DiagnosticField::token("kind", kind.path_segment()))
                .field(DiagnosticField::count(
                    "evicted_entries",
                    evicted_entries as u64,
                ))
                .field(DiagnosticField::count(
                    "evicted_bytes",
                    evicted_bytes as u64,
                ))
                .field(DiagnosticField::count("entries", self.entries.len() as u64))
                .field(DiagnosticField::count("bytes", self.retained_bytes as u64))
                .field(DiagnosticField::count(
                    "eviction_count",
                    self.eviction_count,
                )),
            );
        }
    }

    /// Remove every entry derived from `source` in `account_scope` and report
    /// the retired entry count (#1396). A superseded avatar must not stay until
    /// LRU pressure; a live lease keeps its own `Arc` and simply becomes
    /// undiscoverable, exactly like an eviction.
    fn retire_source(&mut self, account_scope: &str, source: &str) -> usize {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                (entry.account_scope == account_scope && entry.source == source)
                    .then_some(key.clone())
            })
            .collect();
        let mut removed_bytes = 0usize;
        for key in &keys {
            if let Some(entry) = self.entries.remove(key) {
                removed_bytes = removed_bytes.saturating_add(entry.bytes.len());
                self.remove_from_lru(key);
            }
        }
        if !keys.is_empty() {
            self.retained_bytes = self.retained_bytes.saturating_sub(removed_bytes);
            self.retirement_count = self.retirement_count.saturating_add(1);
            self.retired_bytes = self.retired_bytes.saturating_add(removed_bytes as u64);
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Debug,
                    "core.renderable_thumbnail",
                    "retire",
                )
                .field(DiagnosticField::count("retired_entries", keys.len() as u64))
                .field(DiagnosticField::count(
                    "retired_bytes",
                    removed_bytes as u64,
                ))
                .field(DiagnosticField::count(
                    "retirement_count",
                    self.retirement_count,
                )),
            );
        }
        keys.len()
    }
}

fn renderable_thumbnail_cache() -> &'static Mutex<RenderableThumbnailCache> {
    static CACHE: OnceLock<Mutex<RenderableThumbnailCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(RenderableThumbnailCache::default()))
}

fn mime_type_from_bytes(bytes: &[u8]) -> String {
    image_kind(bytes)
        .map(|kind| kind.mime_type.to_owned())
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

/// Cache key for one exact thumbnail identity (#1396). `identity` distinguishes
/// two requests for the same media source at different sizes or encodings, so
/// the first arrival cannot answer for the other.
pub(crate) fn renderable_thumbnail_cache_key_for_account(
    kind: RenderableThumbnailKind,
    account_scope: &str,
    source: &str,
    identity: &str,
) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(kind.path_segment().as_bytes());
    hasher.write(&[0]);
    hasher.write(account_scope.as_bytes());
    hasher.write(&[0]);
    hasher.write(source.as_bytes());
    hasher.write(&[0]);
    hasher.write(identity.as_bytes());
    format!("{}/{:016x}", kind.path_segment(), hasher.finish())
}

/// Cache key for the kind's single product variant.
pub(crate) fn renderable_thumbnail_cache_key(
    kind: RenderableThumbnailKind,
    account_scope: &str,
    source: &str,
) -> String {
    renderable_thumbnail_cache_key_for_account(kind, account_scope, source, kind.default_identity())
}

fn validated_renderable_thumbnail_ref(source_ref: &str) -> Option<&str> {
    let (kind, hash) = source_ref.split_once('/')?;
    if !matches!(kind, "avatar" | "link-preview")
        || hash.len() != 16
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(source_ref)
}

pub fn store_renderable_thumbnail(
    kind: RenderableThumbnailKind,
    source: &str,
    bytes: Vec<u8>,
) -> Result<AvatarThumbnailState, RenderableThumbnailStoreError> {
    store_renderable_thumbnail_for_account(kind, "", source, bytes)
}

pub(crate) fn store_renderable_thumbnail_for_account(
    kind: RenderableThumbnailKind,
    account_scope: &str,
    source: &str,
    bytes: Vec<u8>,
) -> Result<AvatarThumbnailState, RenderableThumbnailStoreError> {
    store_renderable_thumbnail_variant_for_account(
        kind,
        account_scope,
        source,
        kind.default_identity(),
        RenderableThumbnailContent::from_bytes(bytes),
    )
}

/// Store one exact thumbnail identity for a source.
pub(crate) fn store_renderable_thumbnail_variant_for_account(
    kind: RenderableThumbnailKind,
    account_scope: &str,
    source: &str,
    identity: &str,
    content: RenderableThumbnailContent,
) -> Result<AvatarThumbnailState, RenderableThumbnailStoreError> {
    let cache_key =
        renderable_thumbnail_cache_key_for_account(kind, account_scope, source, identity);
    let width = content.width;
    let height = content.height;
    let mime_type = content
        .mime_type
        .clone()
        .unwrap_or_else(|| mime_type_from_bytes(&content.bytes));
    {
        let mut cache = renderable_thumbnail_cache()
            .lock()
            .expect("renderable thumbnail cache should not be poisoned");
        cache.insert(kind, cache_key.clone(), account_scope, source, content)?;
    }

    Ok(AvatarThumbnailState::Ready {
        source_ref: cache_key,
        width,
        height,
        mime_type: Some(mime_type),
    })
}

/// Retire every retained entry derived from `source` in `account_scope`, for
/// every identity. Called when a source is no longer demanded, so a replaced
/// avatar cannot keep its superseded bytes until LRU pressure (#1396).
pub(crate) fn retire_renderable_thumbnail_source_for_account(
    account_scope: &str,
    source: &str,
) -> usize {
    renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned")
        .retire_source(account_scope, source)
}

pub(crate) fn lease_renderable_thumbnail(
    source_ref: &str,
) -> Result<RenderableThumbnailLease, ThumbnailLeaseError> {
    let cache_key =
        validated_renderable_thumbnail_ref(source_ref).ok_or(ThumbnailLeaseError::Unavailable)?;
    renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned")
        .lease(cache_key)
}

/// Check availability without cloning image bytes, reserving a resource lease,
/// or perturbing LRU recency. Projection reconciliation uses this: it must not
/// make every live entry look recently used.
pub(crate) fn contains_renderable_thumbnail(source_ref: &str) -> bool {
    let Some(cache_key) = validated_renderable_thumbnail_ref(source_ref) else {
        return false;
    };
    renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned")
        .entries
        .contains_key(cache_key)
}

/// Check availability without cloning image bytes or reserving a resource lease.
pub(crate) fn is_renderable_thumbnail_cached(source_ref: &str) -> bool {
    let Some(cache_key) = validated_renderable_thumbnail_ref(source_ref) else {
        return false;
    };
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    if !cache.entries.contains_key(cache_key) {
        return false;
    }
    cache.touch(cache_key);
    true
}

pub fn lookup_renderable_thumbnail(source_ref: &str) -> Option<RenderableThumbnailContent> {
    let cache_key = validated_renderable_thumbnail_ref(source_ref)?;
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    cache.get(cache_key)
}

pub fn clear_renderable_thumbnail_cache() {
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    cache.clear();
}

pub(crate) fn clear_renderable_thumbnail_cache_for_account(account_scope: &str) {
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    cache.clear_account(account_scope);
}

/// Serializes tests that share the process-global thumbnail cache. The mutex
/// is async-aware because asynchronous tests hold it across await points.
#[cfg(test)]
static TEST_CACHE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Synchronous tests only; panics inside an async runtime.
#[cfg(test)]
pub(crate) fn test_cache_lock() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_CACHE_LOCK.blocking_lock()
}

/// Asynchronous tests: waiting yields instead of blocking a runtime worker.
#[cfg(test)]
pub(crate) async fn test_cache_lock_async() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_CACHE_LOCK.lock().await
}

pub fn renderable_thumbnail_cache_stats() -> RenderableThumbnailCacheStats {
    renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned")
        .stats()
}

pub fn renderable_thumbnail_summary_event(stats: RenderableThumbnailCacheStats) -> DiagnosticEvent {
    DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "core.renderable_thumbnail",
        "summary",
    )
    .field(DiagnosticField::token(
        "policy",
        "per_kind_count_and_byte_bounded_lru",
    ))
    // Named cache only: these counters cover this cache, not the native heap.
    .field(DiagnosticField::token("cache_scope", "named_caches_only"))
    .field(DiagnosticField::count(
        "max_entry_count",
        MAX_RENDERABLE_THUMBNAIL_ENTRIES as u64,
    ))
    .field(DiagnosticField::count(
        "max_avatar_bytes",
        MAX_AVATAR_THUMBNAIL_BYTES as u64,
    ))
    .field(DiagnosticField::count(
        "max_avatar_entry_bytes",
        MAX_AVATAR_THUMBNAIL_ENTRY_BYTES as u64,
    ))
    .field(DiagnosticField::count(
        "max_link_preview_bytes",
        MAX_LINK_PREVIEW_THUMBNAIL_BYTES as u64,
    ))
    .field(DiagnosticField::count(
        "entry_count",
        stats.entry_count as u64,
    ))
    .field(DiagnosticField::count(
        "retained_bytes",
        stats.retained_bytes as u64,
    ))
    .field(DiagnosticField::count(
        "avatar_entry_count",
        stats.avatar_entry_count as u64,
    ))
    .field(DiagnosticField::count(
        "avatar_retained_bytes",
        stats.avatar_retained_bytes as u64,
    ))
    .field(DiagnosticField::count(
        "link_preview_entry_count",
        stats.link_preview_entry_count as u64,
    ))
    .field(DiagnosticField::count(
        "link_preview_retained_bytes",
        stats.link_preview_retained_bytes as u64,
    ))
    .field(DiagnosticField::count(
        "average_entry_bytes",
        stats
            .retained_bytes
            .checked_div(stats.entry_count)
            .unwrap_or(0) as u64,
    ))
    .field(DiagnosticField::count(
        "lease_charged_bytes",
        stats.lease_charged_bytes as u64,
    ))
    .field(DiagnosticField::count(
        "max_lease_bytes",
        MAX_THUMBNAIL_LEASE_BYTES as u64,
    ))
    .field(DiagnosticField::token(
        "lease_accounting",
        "charged_budget_overlaps_retained_bytes",
    ))
    .field(DiagnosticField::count(
        "high_water_entry_count",
        stats.high_water_entry_count as u64,
    ))
    .field(DiagnosticField::count(
        "high_water_bytes",
        stats.high_water_bytes as u64,
    ))
    .field(DiagnosticField::count(
        "eviction_count",
        stats.eviction_count,
    ))
    .field(DiagnosticField::count("clear_count", stats.clear_count))
    .field(DiagnosticField::count(
        "retirement_count",
        stats.retirement_count,
    ))
    .field(DiagnosticField::count("retired_bytes", stats.retired_bytes))
    .field(DiagnosticField::count(
        "oversize_rejection_count",
        stats.oversize_rejection_count,
    ))
}

pub fn record_renderable_thumbnail_summary(stats: RenderableThumbnailCacheStats) {
    record(renderable_thumbnail_summary_event(stats));
}

pub fn cleanup_legacy_plaintext_thumbnail_dirs(data_dir: &Path) -> std::io::Result<()> {
    for dir in [
        data_dir.join("avatar_thumbnails"),
        data_dir.join("link_preview_thumbnails"),
    ] {
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Old media downloads were device-wide and keyed only by room/event IDs. They
/// are a re-downloadable cache, so remove it when moving to account-rooted paths.
pub fn cleanup_legacy_media_downloads(data_dir: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(data_dir.join("media_downloads")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
