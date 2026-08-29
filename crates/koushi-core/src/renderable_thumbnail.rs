use std::{
    collections::{HashMap, VecDeque},
    fmt, fs,
    hash::{DefaultHasher, Hasher},
    path::Path,
    sync::{Mutex, OnceLock},
};

use crate::cached_image::cached_image_kind;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::AvatarThumbnailState;

const RENDERABLE_THUMBNAIL_SCHEME: &str = "koushi-thumbnail://localhost/";
pub(crate) const MAX_RENDERABLE_THUMBNAIL_ENTRIES: usize = 256;
pub(crate) const MAX_RENDERABLE_THUMBNAIL_BYTES: usize = 32 * 1024 * 1024;

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderableThumbnailContent {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
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

#[derive(Clone)]
struct RenderableThumbnailEntry {
    bytes: Vec<u8>,
    mime_type: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderableThumbnailCacheStats {
    pub entry_count: usize,
    pub retained_bytes: usize,
    pub high_water_entry_count: usize,
    pub high_water_bytes: usize,
    pub eviction_count: u64,
    pub clear_count: u64,
    pub oversize_rejection_count: u64,
}

#[derive(Default)]
struct RenderableThumbnailCache {
    // Ready protocol URLs are stored in AppState while their bytes remain in
    // this count-and-byte-bounded LRU. Access refreshes recency; session clear
    // drops all retained bytes.
    entries: HashMap<String, RenderableThumbnailEntry>,
    // Oldest at the front, most recently accessed at the back. The bound is
    // deliberately larger than the existing 129-entry session churn contract.
    lru: VecDeque<String>,
    retained_bytes: usize,
    high_water_entry_count: usize,
    high_water_bytes: usize,
    eviction_count: u64,
    clear_count: u64,
    oversize_rejection_count: u64,
}

impl RenderableThumbnailCache {
    fn insert(
        &mut self,
        cache_key: String,
        bytes: Vec<u8>,
        mime_type: String,
    ) -> Result<RenderableThumbnailEntry, RenderableThumbnailStoreError> {
        if bytes.len() > MAX_RENDERABLE_THUMBNAIL_BYTES {
            self.oversize_rejection_count = self.oversize_rejection_count.saturating_add(1);
            record(
                DiagnosticEvent::new(DiagnosticLevel::Warn, "core.renderable_thumbnail", "reject")
                    .field(DiagnosticField::token("reason", "oversize"))
                    .field(DiagnosticField::count("byte_count", bytes.len() as u64))
                    .field(DiagnosticField::count(
                        "max_bytes",
                        MAX_RENDERABLE_THUMBNAIL_BYTES as u64,
                    ))
                    .field(DiagnosticField::count(
                        "oversize_rejection_count",
                        self.oversize_rejection_count,
                    )),
            );
            return Err(RenderableThumbnailStoreError::TooLarge {
                byte_count: bytes.len(),
                max_bytes: MAX_RENDERABLE_THUMBNAIL_BYTES,
            });
        }
        let entry = RenderableThumbnailEntry { bytes, mime_type };
        if let Some(previous) = self.entries.remove(&cache_key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(previous.bytes.len());
            self.remove_from_lru(&cache_key);
        }
        self.retained_bytes = self.retained_bytes.saturating_add(entry.bytes.len());
        self.entries.insert(cache_key.clone(), entry.clone());
        self.lru.push_back(cache_key);
        self.update_high_water();
        self.evict_if_needed();
        Ok(entry)
    }

    fn get(&mut self, cache_key: &str) -> Option<RenderableThumbnailContent> {
        let entry = self.entries.get(cache_key)?.clone();
        self.touch(cache_key);
        Some(RenderableThumbnailContent {
            bytes: entry.bytes,
            mime_type: Some(entry.mime_type),
        })
    }

    fn stats(&self) -> RenderableThumbnailCacheStats {
        RenderableThumbnailCacheStats {
            entry_count: self.entries.len(),
            retained_bytes: self.retained_bytes,
            high_water_entry_count: self.high_water_entry_count,
            high_water_bytes: self.high_water_bytes,
            eviction_count: self.eviction_count,
            clear_count: self.clear_count,
            oversize_rejection_count: self.oversize_rejection_count,
        }
    }

    fn clear(&mut self) {
        let removed_entries = self.entries.len();
        let removed_bytes = self.retained_bytes;
        self.entries.clear();
        self.lru.clear();
        self.retained_bytes = 0;
        self.clear_count = self.clear_count.saturating_add(1);
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

    fn evict_if_needed(&mut self) {
        let mut evicted_entries = 0usize;
        let mut evicted_bytes = 0usize;
        while self.entries.len() > MAX_RENDERABLE_THUMBNAIL_ENTRIES
            || self.retained_bytes > MAX_RENDERABLE_THUMBNAIL_BYTES
        {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.remove(&oldest) else {
                continue;
            };
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
}

fn renderable_thumbnail_cache() -> &'static Mutex<RenderableThumbnailCache> {
    static CACHE: OnceLock<Mutex<RenderableThumbnailCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(RenderableThumbnailCache::default()))
}

fn mime_type_from_bytes(bytes: &[u8]) -> String {
    cached_image_kind(bytes)
        .map(|kind| kind.mime_type.to_owned())
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

fn renderable_thumbnail_cache_key(kind: RenderableThumbnailKind, source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(kind.path_segment().as_bytes());
    hasher.write(source.as_bytes());
    format!("{}/{:016x}", kind.path_segment(), hasher.finish())
}

fn renderable_thumbnail_source_url(cache_key: &str) -> String {
    format!("{RENDERABLE_THUMBNAIL_SCHEME}{cache_key}")
}

fn renderable_thumbnail_cache_key_from_path(path: &str) -> Option<String> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let mut segments = trimmed.split('/');
    let kind = segments.next()?;
    let key = segments.next()?;
    if key.is_empty() || segments.next().is_some() {
        return None;
    }

    match kind {
        "avatar" | "link-preview" => Some(format!("{kind}/{key}")),
        _ => None,
    }
}

pub fn store_renderable_thumbnail(
    kind: RenderableThumbnailKind,
    source: &str,
    bytes: Vec<u8>,
) -> Result<AvatarThumbnailState, RenderableThumbnailStoreError> {
    let mime_type = mime_type_from_bytes(&bytes);
    let cache_key = renderable_thumbnail_cache_key(kind, source);
    {
        let mut cache = renderable_thumbnail_cache()
            .lock()
            .expect("renderable thumbnail cache should not be poisoned");
        cache.insert(cache_key.clone(), bytes, mime_type.clone())?;
    }

    Ok(AvatarThumbnailState::Ready {
        source_url: renderable_thumbnail_source_url(&cache_key),
        width: None,
        height: None,
        mime_type: Some(mime_type),
    })
}

pub fn lookup_renderable_thumbnail(path: &str) -> Option<RenderableThumbnailContent> {
    let cache_key = renderable_thumbnail_cache_key_from_path(path)?;
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    cache.get(&cache_key)
}

pub fn clear_renderable_thumbnail_cache() {
    let mut cache = renderable_thumbnail_cache()
        .lock()
        .expect("renderable thumbnail cache should not be poisoned");
    cache.clear();
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
        "count_and_byte_bounded_lru",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex as TestMutex, MutexGuard, OnceLock as TestOnceLock};

    fn cache_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: TestOnceLock<TestMutex<()>> = TestOnceLock::new();
        LOCK.get_or_init(|| TestMutex::new(()))
            .lock()
            .expect("renderable thumbnail cache test lock should not be poisoned")
    }

    #[test]
    fn stores_avatar_and_link_preview_thumbnails_in_memory_with_protocol_urls() {
        let _guard = cache_test_lock();
        clear_renderable_thumbnail_cache();

        let avatar = store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            "mxc://example.test/avatar",
            b"avatar-bytes".to_vec(),
        )
        .expect("avatar bytes are within the cache bound");
        let link_preview = store_renderable_thumbnail(
            RenderableThumbnailKind::LinkPreview,
            "https://example.test/page",
            b"preview-bytes".to_vec(),
        )
        .expect("link-preview bytes are within the cache bound");

        let AvatarThumbnailState::Ready {
            source_url,
            mime_type,
            ..
        } = avatar
        else {
            panic!("avatar thumbnail should be ready");
        };
        assert!(source_url.starts_with("koushi-thumbnail://localhost/avatar/"));
        assert_eq!(mime_type.as_deref(), Some("application/octet-stream"));

        let AvatarThumbnailState::Ready {
            source_url,
            mime_type,
            ..
        } = link_preview
        else {
            panic!("link-preview thumbnail should be ready");
        };
        assert!(source_url.starts_with("koushi-thumbnail://localhost/link-preview/"));
        assert_eq!(mime_type.as_deref(), Some("application/octet-stream"));
    }

    #[test]
    fn lookup_renderable_thumbnail_returns_bytes_for_protocol_path() {
        let _guard = cache_test_lock();
        clear_renderable_thumbnail_cache();

        let ready = store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            "mxc://example.test/lookup",
            b"lookup-bytes".to_vec(),
        )
        .expect("thumbnail bytes are within the cache bound");
        let AvatarThumbnailState::Ready { source_url, .. } = ready else {
            panic!("thumbnail should be ready");
        };

        let path = source_url
            .strip_prefix("koushi-thumbnail://localhost")
            .expect("protocol url should have localhost authority");
        let content = lookup_renderable_thumbnail(path).expect("thumbnail should be cached");
        assert_eq!(content.bytes, b"lookup-bytes");
        assert_eq!(
            content.mime_type.as_deref(),
            Some("application/octet-stream")
        );
    }

    #[test]
    fn ready_thumbnail_protocol_urls_survive_session_cache_churn() {
        let _guard = cache_test_lock();
        clear_renderable_thumbnail_cache();

        let ready = store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            "mxc://example.test/pinned",
            b"pinned-bytes".to_vec(),
        )
        .expect("pinned thumbnail bytes are within the cache bound");
        let AvatarThumbnailState::Ready { source_url, .. } = ready else {
            panic!("thumbnail should be ready");
        };
        let path = source_url
            .strip_prefix("koushi-thumbnail://localhost")
            .expect("protocol url should have localhost authority");

        for index in 0..=128 {
            let source = format!("mxc://example.test/churn/{index}");
            let bytes = format!("bytes-{index}").into_bytes();
            store_renderable_thumbnail(RenderableThumbnailKind::Avatar, &source, bytes)
                .expect("churn thumbnail bytes are within the cache bound");
        }

        let content = lookup_renderable_thumbnail(path)
            .expect("Ready thumbnail URL must remain reconstructible until session clear");
        assert_eq!(content.bytes, b"pinned-bytes");
    }

    #[test]
    fn thumbnail_cache_is_bounded_by_entry_count_and_retained_bytes() {
        let mut cache = RenderableThumbnailCache::default();
        for index in 0..=(MAX_RENDERABLE_THUMBNAIL_ENTRIES + 8) {
            cache
                .insert(
                    format!("avatar/{index}"),
                    vec![u8::try_from(index % 251).unwrap(); 1024],
                    "image/png".to_owned(),
                )
                .expect("test entry is within the cache bound");
        }

        let stats = cache.stats();
        assert!(stats.entry_count <= MAX_RENDERABLE_THUMBNAIL_ENTRIES);
        assert!(stats.retained_bytes <= MAX_RENDERABLE_THUMBNAIL_BYTES);
        assert!(stats.eviction_count > 0);
        assert!(cache.get("avatar/0").is_none(), "oldest entry is evicted");
    }

    #[test]
    fn oversized_thumbnail_is_rejected_without_publishing_a_ready_url() {
        let _guard = cache_test_lock();
        clear_renderable_thumbnail_cache();
        let rejection_count_before = renderable_thumbnail_cache_stats().oversize_rejection_count;

        let result = store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            "mxc://example.test/oversized",
            vec![0; MAX_RENDERABLE_THUMBNAIL_BYTES + 1],
        );

        assert_eq!(
            result,
            Err(RenderableThumbnailStoreError::TooLarge {
                byte_count: MAX_RENDERABLE_THUMBNAIL_BYTES + 1,
                max_bytes: MAX_RENDERABLE_THUMBNAIL_BYTES,
            })
        );
        let stats = renderable_thumbnail_cache_stats();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.retained_bytes, 0);
        assert_eq!(
            stats.oversize_rejection_count,
            rejection_count_before.saturating_add(1)
        );
    }

    #[test]
    fn clear_renderable_thumbnail_cache_drops_previous_session_bytes() {
        let _guard = cache_test_lock();
        clear_renderable_thumbnail_cache();

        let ready = store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            "mxc://example.test/session-scoped",
            b"session-bytes".to_vec(),
        )
        .expect("thumbnail bytes are within the cache bound");
        let AvatarThumbnailState::Ready { source_url, .. } = ready else {
            panic!("thumbnail should be ready");
        };
        let path = source_url
            .strip_prefix("koushi-thumbnail://localhost")
            .expect("protocol url should have localhost authority");

        assert!(lookup_renderable_thumbnail(path).is_some());

        clear_renderable_thumbnail_cache();

        assert!(lookup_renderable_thumbnail(path).is_none());
    }

    #[test]
    fn cleanup_legacy_plaintext_thumbnail_dirs_preserves_media_downloads() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let data_dir = tempdir.path();

        fs::create_dir_all(data_dir.join("avatar_thumbnails")).expect("seed avatar dir");
        fs::write(
            data_dir.join("avatar_thumbnails").join("thumb.bin"),
            b"avatar",
        )
        .expect("seed avatar file");
        fs::create_dir_all(data_dir.join("link_preview_thumbnails")).expect("seed preview dir");
        fs::write(
            data_dir.join("link_preview_thumbnails").join("preview.bin"),
            b"preview",
        )
        .expect("seed preview file");
        fs::create_dir_all(data_dir.join("media_downloads")).expect("seed media dir");
        fs::write(
            data_dir.join("media_downloads").join("download.bin"),
            b"download",
        )
        .expect("seed download file");

        cleanup_legacy_plaintext_thumbnail_dirs(data_dir).expect("cleanup should succeed");

        assert!(!data_dir.join("avatar_thumbnails").exists());
        assert!(!data_dir.join("link_preview_thumbnails").exists());
        assert!(data_dir.join("media_downloads").exists());
        assert_eq!(
            fs::read(data_dir.join("media_downloads").join("download.bin")).expect("media file"),
            b"download"
        );
    }
}
