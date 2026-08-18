use std::{collections::BTreeMap, fmt};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_media::{
    ImageOutputFormat, ImageOutputRequest, ImagePreparationPolicy, ImageResizeScale,
    PreparedImageFormat, PreparedImageVariant, heif_mime_type, prepare_image_output,
};
use koushi_state::{
    ComposerTarget, ImageUploadCompressionPolicy, MediaPreparationFailureKind,
    PreparedUploadFormat, PreparedUploadVariant, StagedUploadCompressionChoice,
    StagedUploadFormatChoice, StagedUploadItem, StagedUploadKind, StagedUploadOutputSelection,
    StagedUploadPreparation, StagedUploadResizeChoice,
};
use tokio::sync::{Mutex, MutexGuard};

pub const MAX_PREPARATION_BATCH_SIZE: usize = 16;

#[derive(Default)]
pub struct MediaPreparationService {
    registry: Mutex<MediaPreparationRegistry>,
    transitions: Mutex<()>,
}

impl MediaPreparationService {
    pub async fn transition(&self) -> MediaPreparationTransition<'_> {
        let transition = self.transitions.lock().await;
        let registry = self.registry.lock().await;
        MediaPreparationTransition {
            _transition: transition,
            registry,
        }
    }

    pub async fn reconcile_snapshot(&self, snapshot: &koushi_state::AppState) {
        self.registry.lock().await.reconcile_snapshot(snapshot);
    }

    pub async fn stats(&self) -> MediaPreparationStats {
        self.registry.lock().await.stats()
    }
}

pub struct MediaPreparationTransition<'a> {
    _transition: MutexGuard<'a, ()>,
    registry: MutexGuard<'a, MediaPreparationRegistry>,
}

impl MediaPreparationTransition<'_> {
    pub fn reconcile_snapshot(&mut self, snapshot: &koushi_state::AppState) {
        self.registry.reconcile_snapshot(snapshot);
    }

    pub fn merge_prepared(&mut self, prepared: MediaPreparationRegistry) {
        self.registry.merge_prepared(prepared);
    }

    pub fn source_input(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<StageUploadBytesInput> {
        self.registry.source_input(target, staged_id)
    }

    /// Cache a lazily encoded output and select it for upload.
    pub fn insert_prepared_output(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
        descriptor: PreparedUploadVariant,
        bytes: Vec<u8>,
    ) {
        self.registry
            .insert_prepared_output(target, staged_id, descriptor, bytes);
    }

    pub fn select_variant(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
        variant_id: &str,
    ) -> bool {
        self.registry.select_variant(target, staged_id, variant_id)
    }

    pub fn selected_upload(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<PreparedMediaUpload> {
        self.registry.selected_upload(target, staged_id)
    }

    pub fn variant_bytes(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
        variant_id: &str,
    ) -> Option<Vec<u8>> {
        self.registry.variant_bytes(target, staged_id, variant_id)
    }

    pub fn use_original(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<StagedUploadItem> {
        self.registry.use_original(target, staged_id)
    }

    pub fn remove_item(&mut self, target: &ComposerTarget, staged_id: &str) {
        self.registry.remove_item(target, staged_id);
    }

    pub fn clear_target(&mut self, target: &ComposerTarget) {
        self.registry.clear_target(target);
    }

    pub fn stats(&self) -> MediaPreparationStats {
        self.registry.stats()
    }
}

#[derive(Clone)]
pub struct StageUploadBytesInput {
    pub staged_id: String,
    pub position: u64,
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for StageUploadBytesInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageUploadBytesInput")
            .field("staged_id", &"StagedUploadId(..)")
            .field("position", &self.position)
            .field("filename", &"MediaFilename(..)")
            .field("mime_type", &self.mime_type)
            .field("byte_count", &self.bytes.len())
            .finish()
    }
}

#[derive(Clone)]
struct CachedVariant {
    descriptor: PreparedUploadVariant,
    storage: CachedVariantStorage,
}

#[derive(Clone)]
enum CachedVariantStorage {
    Owned(Vec<u8>),
    Source,
}

impl CachedVariantStorage {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Owned(bytes) => bytes.len(),
            Self::Source => 0,
        }
    }

    fn is_source_backed(&self) -> bool {
        matches!(self, Self::Source)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MediaPreparationStats {
    pub source_count: usize,
    pub source_bytes: usize,
    pub variant_count: usize,
    pub source_backed_variant_count: usize,
    pub variant_bytes: usize,
    pub selected_count: usize,
    pub high_water_source_count: usize,
    pub high_water_source_bytes: usize,
    pub high_water_variant_count: usize,
    pub high_water_variant_bytes: usize,
}

pub fn media_preparation_summary_event(stats: MediaPreparationStats) -> DiagnosticEvent {
    DiagnosticEvent::new(DiagnosticLevel::Info, "core.media_preparation", "summary")
        .field(DiagnosticField::token("retention", "source_backed"))
        .field(DiagnosticField::count(
            "source_count",
            stats.source_count as u64,
        ))
        .field(DiagnosticField::count(
            "source_bytes",
            stats.source_bytes as u64,
        ))
        .field(DiagnosticField::count(
            "variant_count",
            stats.variant_count as u64,
        ))
        .field(DiagnosticField::count(
            "source_backed_variant_count",
            stats.source_backed_variant_count as u64,
        ))
        .field(DiagnosticField::count(
            "variant_bytes",
            stats.variant_bytes as u64,
        ))
        .field(DiagnosticField::count(
            "selected_count",
            stats.selected_count as u64,
        ))
        .field(DiagnosticField::count(
            "high_water_source_count",
            stats.high_water_source_count as u64,
        ))
        .field(DiagnosticField::count(
            "high_water_source_bytes",
            stats.high_water_source_bytes as u64,
        ))
        .field(DiagnosticField::count(
            "high_water_variant_count",
            stats.high_water_variant_count as u64,
        ))
        .field(DiagnosticField::count(
            "high_water_variant_bytes",
            stats.high_water_variant_bytes as u64,
        ))
}

pub fn record_media_preparation_summary(stats: MediaPreparationStats) {
    record(media_preparation_summary_event(stats));
}

#[derive(Clone)]
pub struct PreparedMediaUpload {
    pub descriptor: PreparedUploadVariant,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for PreparedMediaUpload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedMediaUpload")
            .field("descriptor", &self.descriptor)
            .field("bytes", &format_args!("{} byte(s)", self.bytes.len()))
            .finish()
    }
}

#[derive(Default)]
pub struct MediaPreparationRegistry {
    variants: BTreeMap<(ComposerTarget, String, String), CachedVariant>,
    selected: BTreeMap<(ComposerTarget, String), String>,
    sources: BTreeMap<(ComposerTarget, String), StageUploadBytesInput>,
    account_user_id: Option<String>,
    high_water_source_count: usize,
    high_water_source_bytes: usize,
    high_water_variant_count: usize,
    high_water_variant_bytes: usize,
}

impl MediaPreparationRegistry {
    pub fn stats(&self) -> MediaPreparationStats {
        let source_bytes = self.sources.values().fold(0usize, |total, input| {
            total.saturating_add(input.bytes.len())
        });
        let variant_bytes = self.variants.values().fold(0usize, |total, variant| {
            total.saturating_add(variant.storage.retained_bytes())
        });
        MediaPreparationStats {
            source_count: self.sources.len(),
            source_bytes,
            variant_count: self.variants.len(),
            source_backed_variant_count: self
                .variants
                .values()
                .filter(|variant| variant.storage.is_source_backed())
                .count(),
            variant_bytes,
            selected_count: self.selected.len(),
            high_water_source_count: self.high_water_source_count,
            high_water_source_bytes: self.high_water_source_bytes,
            high_water_variant_count: self.high_water_variant_count,
            high_water_variant_bytes: self.high_water_variant_bytes,
        }
    }

    fn refresh_high_water(&mut self) {
        let stats = self.stats();
        self.high_water_source_count = self.high_water_source_count.max(stats.source_count);
        self.high_water_source_bytes = self.high_water_source_bytes.max(stats.source_bytes);
        self.high_water_variant_count = self.high_water_variant_count.max(stats.variant_count);
        self.high_water_variant_bytes = self.high_water_variant_bytes.max(stats.variant_bytes);
    }

    fn record_cleanup_diagnostic(
        &self,
        reason: &'static str,
        before: MediaPreparationStats,
        force: bool,
    ) {
        let after = self.stats();
        if !force && before == after {
            return;
        }
        record(
            DiagnosticEvent::new(DiagnosticLevel::Debug, "core.media_preparation", "cleanup")
                .field(DiagnosticField::token("reason", reason))
                .field(DiagnosticField::count(
                    "source_count",
                    after.source_count as u64,
                ))
                .field(DiagnosticField::count(
                    "source_bytes",
                    after.source_bytes as u64,
                ))
                .field(DiagnosticField::count(
                    "variant_count",
                    after.variant_count as u64,
                ))
                .field(DiagnosticField::count(
                    "source_backed_variant_count",
                    after.source_backed_variant_count as u64,
                ))
                .field(DiagnosticField::count(
                    "variant_bytes",
                    after.variant_bytes as u64,
                ))
                .field(DiagnosticField::count(
                    "selected_count",
                    after.selected_count as u64,
                ))
                .field(DiagnosticField::count(
                    "high_water_source_count",
                    after.high_water_source_count as u64,
                ))
                .field(DiagnosticField::count(
                    "high_water_source_bytes",
                    after.high_water_source_bytes as u64,
                ))
                .field(DiagnosticField::count(
                    "high_water_variant_count",
                    after.high_water_variant_count as u64,
                ))
                .field(DiagnosticField::count(
                    "high_water_variant_bytes",
                    after.high_water_variant_bytes as u64,
                )),
        );
    }

    pub fn prepare_target(
        &mut self,
        target: &ComposerTarget,
        inputs: Vec<StageUploadBytesInput>,
        policy: ImageUploadCompressionPolicy,
    ) -> Vec<StagedUploadItem> {
        self.clear_target(target);
        self.prepare_items(target, inputs, policy)
    }

    pub fn prepare_items(
        &mut self,
        target: &ComposerTarget,
        inputs: Vec<StageUploadBytesInput>,
        policy: ImageUploadCompressionPolicy,
    ) -> Vec<StagedUploadItem> {
        inputs
            .into_iter()
            .take(MAX_PREPARATION_BATCH_SIZE)
            .map(|input| self.prepare_one(target, input, policy))
            .collect()
    }

    /// Cache identity for one resize/format pair.
    pub fn output_identity(selection: StagedUploadOutputSelection) -> String {
        ImageOutputRequest {
            resize: image_resize_scale(selection.resize),
            format: image_output_format(selection.format),
        }
        .identity()
    }

    /// Encode one pair from a retained source, or `None` when it cannot be
    /// decoded or encoded.
    pub fn encode_output(
        source: &StageUploadBytesInput,
        selection: StagedUploadOutputSelection,
        policy: ImageUploadCompressionPolicy,
    ) -> Option<(PreparedUploadVariant, Vec<u8>)> {
        let request = ImageOutputRequest {
            resize: image_resize_scale(selection.resize),
            format: image_output_format_for_source(&source.bytes, selection),
        };
        let variant = prepare_image_output(
            &source.bytes,
            &source.filename,
            request,
            &ImagePreparationPolicy {
                target_long_edge: u32::try_from(policy.target_long_edge).unwrap_or(u32::MAX),
                quality_percent: policy.quality_percent,
            },
        )
        .ok()?;
        let descriptor = descriptor_from_image_variant(&variant, source.bytes.len(), selection);
        Some((descriptor, variant.bytes))
    }

    /// Cache a lazily encoded pair and select it, so the upload uses its exact
    /// bytes.
    pub fn insert_prepared_output(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
        descriptor: PreparedUploadVariant,
        bytes: Vec<u8>,
    ) {
        let variant_id = descriptor.variant_id.clone();
        let source_backed = descriptor.resize == StagedUploadResizeChoice::Original
            && descriptor.format_choice == StagedUploadFormatChoice::Keep
            && self
                .sources
                .get(&(target.clone(), staged_id.to_owned()))
                .is_some_and(|source| source.bytes == bytes);
        let storage = if source_backed {
            CachedVariantStorage::Source
        } else {
            CachedVariantStorage::Owned(bytes)
        };
        self.variants.insert(
            (target.clone(), staged_id.to_owned(), variant_id.clone()),
            CachedVariant {
                descriptor,
                storage,
            },
        );
        self.selected
            .insert((target.clone(), staged_id.to_owned()), variant_id);
        self.refresh_high_water();
    }

    pub fn select_variant(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
        variant_id: &str,
    ) -> bool {
        let cache_key = (target.clone(), staged_id.to_owned(), variant_id.to_owned());
        if !self.variants.contains_key(&cache_key) {
            return false;
        }
        self.selected.insert(
            (target.clone(), staged_id.to_owned()),
            variant_id.to_owned(),
        );
        true
    }

    pub fn selected_upload(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<PreparedMediaUpload> {
        let selected = self.selected.get(&(target.clone(), staged_id.to_owned()))?;
        let cached =
            self.variants
                .get(&(target.clone(), staged_id.to_owned(), selected.clone()))?;
        Some(PreparedMediaUpload {
            descriptor: cached.descriptor.clone(),
            bytes: self.cached_variant_bytes(target, staged_id, cached)?,
        })
    }

    pub fn variant_bytes(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
        variant_id: &str,
    ) -> Option<Vec<u8>> {
        let cached =
            self.variants
                .get(&(target.clone(), staged_id.to_owned(), variant_id.to_owned()))?;
        self.cached_variant_bytes(target, staged_id, cached)
    }

    fn cached_variant_bytes(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
        cached: &CachedVariant,
    ) -> Option<Vec<u8>> {
        match &cached.storage {
            CachedVariantStorage::Owned(bytes) => Some(bytes.clone()),
            CachedVariantStorage::Source => self
                .sources
                .get(&(target.clone(), staged_id.to_owned()))
                .map(|source| source.bytes.clone()),
        }
    }

    pub fn remove_item(&mut self, target: &ComposerTarget, staged_id: &str) {
        let before = self.stats();
        self.variants
            .retain(|(item_target, item_id, _), _| item_target != target || item_id != staged_id);
        self.selected
            .remove(&(target.clone(), staged_id.to_owned()));
        self.sources.remove(&(target.clone(), staged_id.to_owned()));
        self.record_cleanup_diagnostic("remove_item", before, false);
    }

    pub fn clear_target(&mut self, target: &ComposerTarget) {
        let before = self.stats();
        self.variants
            .retain(|(item_target, _, _), _| item_target != target);
        self.selected
            .retain(|(item_target, _), _| item_target != target);
        self.sources
            .retain(|(item_target, _), _| item_target != target);
        self.record_cleanup_diagnostic("clear_target", before, false);
    }

    pub fn clear(&mut self) {
        let before = self.stats();
        let had_account = self.account_user_id.is_some();
        self.variants.clear();
        self.selected.clear();
        self.sources.clear();
        self.account_user_id = None;
        self.record_cleanup_diagnostic("clear", before, had_account);
    }

    pub fn merge_prepared(&mut self, prepared: Self) {
        self.high_water_source_count = self
            .high_water_source_count
            .max(prepared.high_water_source_count);
        self.high_water_source_bytes = self
            .high_water_source_bytes
            .max(prepared.high_water_source_bytes);
        self.high_water_variant_count = self
            .high_water_variant_count
            .max(prepared.high_water_variant_count);
        self.high_water_variant_bytes = self
            .high_water_variant_bytes
            .max(prepared.high_water_variant_bytes);
        self.variants.extend(prepared.variants);
        self.selected.extend(prepared.selected);
        self.sources.extend(prepared.sources);
        self.refresh_high_water();
    }

    pub fn source_input(
        &self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<StageUploadBytesInput> {
        self.sources
            .get(&(target.clone(), staged_id.to_owned()))
            .cloned()
    }

    pub fn clear_thread_targets(&mut self) {
        let before = self.stats();
        self.variants
            .retain(|(target, _, _), _| !matches!(target, ComposerTarget::Thread { .. }));
        self.selected
            .retain(|(target, _), _| !matches!(target, ComposerTarget::Thread { .. }));
        self.sources
            .retain(|(target, _), _| !matches!(target, ComposerTarget::Thread { .. }));
        self.record_cleanup_diagnostic("clear_thread_targets", before, false);
    }

    fn reconcile_snapshot(&mut self, snapshot: &koushi_state::AppState) {
        let before = self.stats();
        let mut account_changed = false;
        if let SessionAccountObservation::Stable(account_user_id) =
            session_account_observation(&snapshot.session)
        {
            let account_user_id = account_user_id.map(str::to_owned);
            if account_user_id != self.account_user_id {
                account_changed = true;
                self.variants.clear();
                self.selected.clear();
                self.sources.clear();
                self.account_user_id = account_user_id;
            }
        }
        self.variants.retain(|(target, staged_id, _), _| {
            snapshot
                .upload_staging
                .items
                .contains_key(&(target.clone(), staged_id.clone()))
        });
        self.selected.retain(|(target, staged_id), _| {
            snapshot
                .upload_staging
                .items
                .contains_key(&(target.clone(), staged_id.clone()))
        });
        self.sources.retain(|(target, staged_id), _| {
            snapshot
                .upload_staging
                .items
                .contains_key(&(target.clone(), staged_id.clone()))
        });
        self.record_cleanup_diagnostic(
            if account_changed {
                "account_change"
            } else {
                "reconcile_snapshot"
            },
            before,
            account_changed,
        );
    }

    pub fn retry_item(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
        policy: ImageUploadCompressionPolicy,
    ) -> Option<StagedUploadItem> {
        let input = self
            .sources
            .get(&(target.clone(), staged_id.to_owned()))?
            .clone();
        self.remove_prepared_variants(target, staged_id);
        Some(self.prepare_one(target, input, policy))
    }

    pub fn use_original(
        &mut self,
        target: &ComposerTarget,
        staged_id: &str,
    ) -> Option<StagedUploadItem> {
        let input = self
            .sources
            .get(&(target.clone(), staged_id.to_owned()))?
            .clone();
        if input.bytes.is_empty() {
            return None;
        }
        let byte_count = u64::try_from(input.bytes.len()).unwrap_or(u64::MAX);
        self.remove_prepared_variants(target, staged_id);
        Some(self.store_original_file(target, input, byte_count))
    }

    fn remove_prepared_variants(&mut self, target: &ComposerTarget, staged_id: &str) {
        self.variants
            .retain(|(item_target, item_id, _), _| item_target != target || item_id != staged_id);
        self.selected
            .remove(&(target.clone(), staged_id.to_owned()));
    }

    fn prepare_one(
        &mut self,
        target: &ComposerTarget,
        input: StageUploadBytesInput,
        policy: ImageUploadCompressionPolicy,
    ) -> StagedUploadItem {
        let detected_heif = heif_mime_type(&input.bytes);
        let mut input = input;
        if let Some(mime_type) = detected_heif {
            input.mime_type = mime_type.to_owned();
        }
        self.sources
            .insert((target.clone(), input.staged_id.clone()), input.clone());
        self.refresh_high_water();
        let byte_count = u64::try_from(input.bytes.len()).unwrap_or(u64::MAX);
        if let Some(mime_type) = detected_heif {
            return self.prepare_heif_one(target, input, policy, mime_type);
        }
        let image_candidate = matches!(
            input.mime_type.to_ascii_lowercase().as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        );
        if input.bytes.is_empty() {
            return staged_failure(
                target,
                input,
                byte_count,
                MediaPreparationFailureKind::Empty,
            );
        }

        if !image_candidate {
            return self.store_original_file(target, input, byte_count);
        }

        // #305: the staging dialog always asks, so preparation encodes exactly
        // the untouched output (scale 1, source encoding). Every other
        // resize/format pair is encoded lazily when the user selects it.
        let encode_policy = ImagePreparationPolicy {
            target_long_edge: u32::try_from(policy.target_long_edge).unwrap_or(u32::MAX),
            quality_percent: policy.quality_percent,
        };
        let selected = StagedUploadOutputSelection::default();
        let request = ImageOutputRequest {
            resize: image_resize_scale(selected.resize),
            format: image_output_format(selected.format),
        };
        let Ok(variant) =
            prepare_image_output(&input.bytes, &input.filename, request, &encode_policy)
        else {
            return staged_failure(
                target,
                input,
                byte_count,
                MediaPreparationFailureKind::Encode,
            );
        };
        let descriptor = descriptor_from_image_variant(&variant, input.bytes.len(), selected);
        self.variants.insert(
            (
                target.clone(),
                input.staged_id.clone(),
                descriptor.variant_id.clone(),
            ),
            CachedVariant {
                descriptor: descriptor.clone(),
                storage: CachedVariantStorage::Source,
            },
        );
        self.selected.insert(
            (target.clone(), input.staged_id.clone()),
            descriptor.variant_id.clone(),
        );
        self.refresh_high_water();
        StagedUploadItem {
            staged_id: input.staged_id,
            room_id: target.room_id().to_owned(),
            position: input.position,
            filename: descriptor.filename.clone(),
            mime_type: descriptor.mime_type.clone(),
            byte_count: descriptor.byte_count,
            kind: StagedUploadKind::Image {
                width: descriptor.width,
                height: descriptor.height,
            },
            caption: None,
            compression_choice: StagedUploadCompressionChoice::Original,
            preparation: StagedUploadPreparation::Ready {
                variants: vec![descriptor],
                selected,
                pending: None,
                generation: 0,
            },
        }
    }

    fn store_original_file(
        &mut self,
        target: &ComposerTarget,
        input: StageUploadBytesInput,
        byte_count: u64,
    ) -> StagedUploadItem {
        // A non-image attachment is only ever its untouched self.
        let selected = StagedUploadOutputSelection::default();
        let descriptor = PreparedUploadVariant {
            variant_id: "original".to_owned(),
            resize: selected.resize,
            format_choice: selected.format,
            filename: input.filename.clone(),
            mime_type: normalized_mime(&input.mime_type),
            byte_count,
            width: None,
            height: None,
            format: PreparedUploadFormat::Original,
            savings_percent: 0,
            metadata_stripped: false,
            thumbnail_refreshed: false,
        };
        self.variants.insert(
            (
                target.clone(),
                input.staged_id.clone(),
                descriptor.variant_id.clone(),
            ),
            CachedVariant {
                descriptor: descriptor.clone(),
                storage: CachedVariantStorage::Source,
            },
        );
        self.selected.insert(
            (target.clone(), input.staged_id.clone()),
            descriptor.variant_id.clone(),
        );
        self.refresh_high_water();
        StagedUploadItem {
            staged_id: input.staged_id,
            room_id: target.room_id().to_owned(),
            position: input.position,
            filename: descriptor.filename.clone(),
            mime_type: descriptor.mime_type.clone(),
            byte_count,
            kind: StagedUploadKind::File,
            caption: None,
            compression_choice: StagedUploadCompressionChoice::NotApplicable,
            preparation: StagedUploadPreparation::Ready {
                variants: vec![descriptor],
                selected,
                pending: None,
                generation: 0,
            },
        }
    }
}

impl MediaPreparationRegistry {
    fn prepare_heif_one(
        &mut self,
        target: &ComposerTarget,
        input: StageUploadBytesInput,
        policy: ImageUploadCompressionPolicy,
        mime_type: &'static str,
    ) -> StagedUploadItem {
        let byte_count = u64::try_from(input.bytes.len()).unwrap_or(u64::MAX);
        let encode_policy = ImagePreparationPolicy {
            target_long_edge: u32::try_from(policy.target_long_edge).unwrap_or(u32::MAX),
            quality_percent: policy.quality_percent,
        };
        let selected = StagedUploadOutputSelection {
            resize: StagedUploadResizeChoice::Original,
            format: StagedUploadFormatChoice::Jpeg,
        };
        let Ok(converted) = prepare_image_output(
            &input.bytes,
            &input.filename,
            ImageOutputRequest {
                resize: ImageResizeScale::Original,
                format: ImageOutputFormat::Jpeg,
            },
            &encode_policy,
        ) else {
            return staged_failure(
                target,
                input,
                byte_count,
                MediaPreparationFailureKind::Decode,
            );
        };
        let original_selection = StagedUploadOutputSelection::default();
        let original = PreparedUploadVariant {
            variant_id: Self::output_identity(original_selection),
            resize: original_selection.resize,
            format_choice: original_selection.format,
            filename: normalized_heif_filename(&input.filename, mime_type),
            mime_type: mime_type.to_owned(),
            byte_count: u64::try_from(input.bytes.len()).unwrap_or(u64::MAX),
            width: Some(u64::from(converted.dimensions.0)),
            height: Some(u64::from(converted.dimensions.1)),
            format: PreparedUploadFormat::Original,
            savings_percent: 0,
            metadata_stripped: false,
            thumbnail_refreshed: false,
        };
        let converted_descriptor =
            descriptor_from_image_variant(&converted, input.bytes.len(), selected);
        self.variants.insert(
            (
                target.clone(),
                input.staged_id.clone(),
                original.variant_id.clone(),
            ),
            CachedVariant {
                descriptor: original.clone(),
                storage: CachedVariantStorage::Source,
            },
        );
        self.variants.insert(
            (
                target.clone(),
                input.staged_id.clone(),
                converted_descriptor.variant_id.clone(),
            ),
            CachedVariant {
                descriptor: converted_descriptor.clone(),
                storage: CachedVariantStorage::Owned(converted.bytes),
            },
        );
        self.selected.insert(
            (target.clone(), input.staged_id.clone()),
            converted_descriptor.variant_id.clone(),
        );
        self.refresh_high_water();
        StagedUploadItem {
            staged_id: input.staged_id,
            room_id: target.room_id().to_owned(),
            position: input.position,
            filename: converted_descriptor.filename.clone(),
            mime_type: converted_descriptor.mime_type.clone(),
            byte_count: converted_descriptor.byte_count,
            kind: StagedUploadKind::Image {
                width: converted_descriptor.width,
                height: converted_descriptor.height,
            },
            caption: None,
            compression_choice: StagedUploadCompressionChoice::Original,
            preparation: StagedUploadPreparation::Ready {
                variants: vec![original, converted_descriptor],
                selected,
                pending: None,
                generation: 0,
            },
        }
    }
}

enum SessionAccountObservation<'a> {
    Stable(Option<&'a str>),
    Transitional,
}

fn session_account_observation(
    session: &koushi_state::SessionState,
) -> SessionAccountObservation<'_> {
    match session {
        koushi_state::SessionState::Ready(info)
        | koushi_state::SessionState::Locked(info)
        | koushi_state::SessionState::CapabilityBlocked { info, .. } => {
            SessionAccountObservation::Stable(Some(info.user_id.as_str()))
        }
        koushi_state::SessionState::SignedOut | koushi_state::SessionState::LoggingOut => {
            SessionAccountObservation::Stable(None)
        }
        koushi_state::SessionState::Restoring
        | koushi_state::SessionState::SwitchingAccount { .. }
        | koushi_state::SessionState::Authenticating { .. }
        | koushi_state::SessionState::Provisional { .. }
        | koushi_state::SessionState::AwaitingVerification { .. }
        | koushi_state::SessionState::Verifying { .. }
        | koushi_state::SessionState::AwaitingBootstrapConfirmation { .. }
        | koushi_state::SessionState::Rejecting { .. } => SessionAccountObservation::Transitional,
    }
}

impl fmt::Debug for MediaPreparationRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stats = self.stats();
        formatter
            .debug_struct("MediaPreparationRegistry")
            .field("source_count", &stats.source_count)
            .field("source_bytes", &stats.source_bytes)
            .field("variant_count", &stats.variant_count)
            .field(
                "source_backed_variant_count",
                &stats.source_backed_variant_count,
            )
            .field("variant_bytes", &stats.variant_bytes)
            .field("selected_count", &stats.selected_count)
            .field("high_water_source_count", &stats.high_water_source_count)
            .field("high_water_source_bytes", &stats.high_water_source_bytes)
            .field("high_water_variant_count", &stats.high_water_variant_count)
            .field("high_water_variant_bytes", &stats.high_water_variant_bytes)
            .finish()
    }
}

fn staged_failure(
    target: &ComposerTarget,
    input: StageUploadBytesInput,
    byte_count: u64,
    failure_kind: MediaPreparationFailureKind,
) -> StagedUploadItem {
    StagedUploadItem {
        staged_id: input.staged_id,
        room_id: target.room_id().to_owned(),
        position: input.position,
        filename: input.filename,
        mime_type: normalized_mime(&input.mime_type),
        byte_count,
        kind: StagedUploadKind::File,
        caption: None,
        compression_choice: StagedUploadCompressionChoice::NotApplicable,
        preparation: StagedUploadPreparation::Failed {
            failure_kind,
            can_use_original: !input.bytes.is_empty(),
        },
    }
}

/// Map a state-level resize choice onto the encoder's linear scale.
fn image_resize_scale(resize: StagedUploadResizeChoice) -> ImageResizeScale {
    match resize {
        StagedUploadResizeChoice::Original => ImageResizeScale::Original,
        StagedUploadResizeChoice::Half => ImageResizeScale::Half,
        StagedUploadResizeChoice::Quarter => ImageResizeScale::Quarter,
        StagedUploadResizeChoice::Eighth => ImageResizeScale::Eighth,
    }
}

/// Map a state-level format choice onto the encoder's output format.
fn image_output_format(format: StagedUploadFormatChoice) -> ImageOutputFormat {
    match format {
        StagedUploadFormatChoice::Keep => ImageOutputFormat::Keep,
        StagedUploadFormatChoice::Png => ImageOutputFormat::Png,
        StagedUploadFormatChoice::Jpeg => ImageOutputFormat::Jpeg,
        StagedUploadFormatChoice::Webp => ImageOutputFormat::WebP,
    }
}

fn image_output_format_for_source(
    source: &[u8],
    selection: StagedUploadOutputSelection,
) -> ImageOutputFormat {
    if heif_mime_type(source).is_some()
        && selection.format == StagedUploadFormatChoice::Keep
        && selection.resize != StagedUploadResizeChoice::Original
    {
        // HEIF is decode-only. A resized "Keep" selection is the compatible
        // JPEG conversion; the unscaled Original/Keep pair remains exact.
        return ImageOutputFormat::Jpeg;
    }
    image_output_format(selection.format)
}

/// Project one encoded output, tagged with the pair it was prepared for.
///
/// `savings_percent` and the reported dimensions describe these exact bytes, so
/// the dialog never shows an estimate.
fn descriptor_from_image_variant(
    variant: &PreparedImageVariant,
    original_len: usize,
    selection: StagedUploadOutputSelection,
) -> PreparedUploadVariant {
    let byte_count = u64::try_from(variant.bytes.len()).unwrap_or(u64::MAX);
    let savings_percent = if original_len == 0 {
        0
    } else {
        100 - i64::try_from(variant.bytes.len().saturating_mul(100) / original_len).unwrap_or(100)
    };
    PreparedUploadVariant {
        variant_id: MediaPreparationRegistry::output_identity(selection),
        resize: selection.resize,
        format_choice: selection.format,
        filename: variant.filename.clone(),
        mime_type: variant.mime_type.clone(),
        byte_count,
        width: Some(u64::from(variant.dimensions.0)),
        height: Some(u64::from(variant.dimensions.1)),
        format: match variant.format {
            PreparedImageFormat::Png => PreparedUploadFormat::Png,
            PreparedImageFormat::Jpeg => PreparedUploadFormat::Jpeg,
            PreparedImageFormat::WebP => PreparedUploadFormat::Webp,
            PreparedImageFormat::Gif | PreparedImageFormat::Heif | PreparedImageFormat::Other => {
                PreparedUploadFormat::Original
            }
        },
        savings_percent,
        metadata_stripped: variant.metadata_stripped,
        thumbnail_refreshed: variant.thumbnail_refreshed,
    }
}

fn normalized_mime(mime_type: &str) -> String {
    let mime_type = mime_type.trim();
    if mime_type.is_empty() {
        "application/octet-stream".to_owned()
    } else {
        mime_type.to_owned()
    }
}

fn normalized_heif_filename(filename: &str, mime_type: &str) -> String {
    let extension = if mime_type == "image/heic" {
        "heic"
    } else {
        "heif"
    };
    let filename = filename.trim();
    if filename.is_empty() {
        return format!("attachment.{extension}");
    }
    match filename.rfind('.') {
        Some(index) if index > 0 => format!("{}.{}", &filename[..index], extension),
        _ => format!("{filename}.{extension}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn target(root: Option<&str>) -> ComposerTarget {
        match root {
            Some(root_event_id) => ComposerTarget::Thread {
                room_id: "!room:example.invalid".to_owned(),
                root_event_id: root_event_id.to_owned(),
            },
            None => ComposerTarget::Main {
                room_id: "!room:example.invalid".to_owned(),
            },
        }
    }

    fn file(id: &str, bytes: &[u8]) -> StageUploadBytesInput {
        StageUploadBytesInput {
            staged_id: id.to_owned(),
            position: 1,
            filename: "private.pdf".to_owned(),
            mime_type: "application/pdf".to_owned(),
            bytes: bytes.to_vec(),
        }
    }

    /// Decodable PNG fixture for output-preparation tests.
    fn png_input(id: &str, width: u32, height: u32) -> StageUploadBytesInput {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        let image = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x % 251) as u8, (y % 239) as u8, 120, 255])
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode fixture");
        StageUploadBytesInput {
            staged_id: id.to_owned(),
            position: 1,
            filename: "shot.png".to_owned(),
            mime_type: "image/png".to_owned(),
            bytes: bytes.into_inner(),
        }
    }

    fn heif_input(id: &str) -> StageUploadBytesInput {
        StageUploadBytesInput {
            staged_id: id.to_owned(),
            position: 1,
            filename: "camera.png".to_owned(),
            mime_type: "image/png".to_owned(),
            bytes: include_bytes!("../../koushi-media/tests/fixtures/heif/opaque.heic").to_vec(),
        }
    }

    #[test]
    fn heif_staging_defaults_to_jpeg_but_retains_exact_original_bytes() {
        let target = target(None);
        let mut registry = MediaPreparationRegistry::default();
        let source = heif_input("heif-1");
        let original_bytes = source.bytes.clone();
        let item = registry
            .prepare_items(
                &target,
                vec![source],
                ImageUploadCompressionPolicy::default(),
            )
            .pop()
            .expect("one staged HEIF image");

        assert!(matches!(
            item.kind,
            StagedUploadKind::Image {
                width: Some(64),
                height: Some(64)
            }
        ));
        let StagedUploadPreparation::Ready {
            variants, selected, ..
        } = &item.preparation
        else {
            panic!("HEIF should expose converted image choices");
        };
        assert_eq!(
            *selected,
            StagedUploadOutputSelection {
                resize: StagedUploadResizeChoice::Original,
                format: StagedUploadFormatChoice::Jpeg
            }
        );
        assert!(variants.iter().any(|variant| {
            variant.resize == StagedUploadResizeChoice::Original
                && variant.format_choice == StagedUploadFormatChoice::Keep
                && variant.mime_type == "image/heic"
        }));

        let converted = registry
            .selected_upload(&target, "heif-1")
            .expect("default converted bytes");
        assert_eq!(converted.descriptor.mime_type, "image/jpeg");
        assert_eq!(converted.descriptor.width, Some(64));
        assert_eq!(converted.descriptor.height, Some(64));
        assert_eq!(
            converted.bytes.len() as u64,
            converted.descriptor.byte_count
        );
        assert_eq!(
            image::load_from_memory(&converted.bytes)
                .unwrap()
                .dimensions(),
            (64, 64)
        );

        let source = registry
            .source_input(&target, "heif-1")
            .expect("source bytes retained for lazy output");
        let (resized_keep, resized_bytes) = MediaPreparationRegistry::encode_output(
            &source,
            StagedUploadOutputSelection {
                resize: StagedUploadResizeChoice::Half,
                format: StagedUploadFormatChoice::Keep,
            },
            ImageUploadCompressionPolicy::default(),
        )
        .expect("resized HEIF Keep should use the compatible JPEG path");
        assert_eq!(resized_keep.variant_id, "half-keep");
        assert_eq!(resized_keep.mime_type, "image/jpeg");
        assert_eq!(resized_keep.width, Some(32));
        assert_eq!(resized_keep.height, Some(32));
        assert_eq!(resized_keep.byte_count, resized_bytes.len() as u64);

        let original = registry
            .use_original(&target, "heif-1")
            .expect("the exact source remains selectable");
        assert_eq!(original.mime_type, "image/heic");
        assert_eq!(
            registry
                .selected_upload(&target, "heif-1")
                .expect("original bytes")
                .bytes,
            original_bytes
        );
    }

    /// #305 regression guard: after a lazily encoded pair is selected, the send
    /// path must still resolve prepared bytes. `send_prepared_uploads` fails
    /// outright when `selected_upload` returns `None`.
    #[test]
    fn selecting_a_lazily_encoded_output_keeps_prepared_upload_bytes_available() {
        let target = target(None);
        let mut registry = MediaPreparationRegistry::default();
        let staged = registry.prepare_items(
            &target,
            vec![png_input("staged-1", 64, 32)],
            ImageUploadCompressionPolicy::default(),
        );
        let item = staged.first().expect("one staged image").clone();
        assert!(
            registry.selected_upload(&target, "staged-1").is_some(),
            "staging must leave prepared bytes available"
        );

        let selection = StagedUploadOutputSelection {
            resize: StagedUploadResizeChoice::Half,
            format: StagedUploadFormatChoice::Keep,
        };
        let variant_id = MediaPreparationRegistry::output_identity(selection);
        assert!(
            !registry.select_variant(&target, "staged-1", &variant_id),
            "the newly requested pair must not be cached yet"
        );

        let source = registry
            .source_input(&target, "staged-1")
            .expect("the source is retained for lazy encoding");
        let (descriptor, bytes) = MediaPreparationRegistry::encode_output(
            &source,
            selection,
            ImageUploadCompressionPolicy::default(),
        )
        .expect("the requested pair must encode");
        registry.insert_prepared_output(&target, "staged-1", descriptor.clone(), bytes);

        let prepared = registry
            .selected_upload(&target, "staged-1")
            .expect("send must still resolve prepared bytes after a lazy encode");
        assert_eq!(prepared.descriptor.variant_id, variant_id);
        assert_eq!(prepared.descriptor.width, Some(32));
        assert_eq!(prepared.descriptor.height, Some(16));
        assert_eq!(
            prepared.bytes.len() as u64,
            prepared.descriptor.byte_count,
            "the reported byte count must describe the bytes that upload"
        );

        // Drive the same order production uses: the selection reaches state
        // first, then the encode is adopted under the generation state handed
        // out at selection time.
        let mut store = koushi_state::UploadStagingStore::default();
        store
            .items
            .insert((target.clone(), "staged-1".to_owned()), item);
        let selected_item = store
            .select_output(&target, "staged-1", selection)
            .expect("selecting an unprepared pair must be accepted");
        let (pending, generation) = match &selected_item.preparation {
            StagedUploadPreparation::Ready {
                pending,
                generation,
                ..
            } => (*pending, *generation),
            other => panic!("staged image must stay ready, got {other:?}"),
        };
        assert_eq!(
            pending,
            Some(selection),
            "an unprepared pair must be reported as pending"
        );

        let adopted = store
            .complete_output(&target, "staged-1", descriptor, generation)
            .expect("the completed output must be adopted");
        assert_eq!(
            adopted.byte_count, prepared.descriptor.byte_count,
            "state and registry must describe the same output"
        );
        match &adopted.preparation {
            StagedUploadPreparation::Ready {
                pending, variants, ..
            } => {
                assert!(pending.is_none(), "adopting the output must clear pending");
                assert_eq!(variants.len(), 2, "both outputs stay cached for reuse");
            }
            other => panic!("staged image must stay ready, got {other:?}"),
        }
    }

    #[test]
    fn registry_isolates_equal_ids_by_target_and_clears_bytes() {
        let mut registry = MediaPreparationRegistry::default();
        let main = target(None);
        let thread = target(Some("$root"));
        let policy = ImageUploadCompressionPolicy::default();
        registry.prepare_target(&main, vec![file("shared", b"main")], policy);
        registry.prepare_target(&thread, vec![file("shared", b"thread")], policy);

        let retained = registry.stats();
        assert_eq!(retained.source_count, 2);
        assert_eq!(retained.source_bytes, b"main".len() + b"thread".len());
        assert_eq!(retained.variant_count, 2);
        assert_eq!(retained.source_backed_variant_count, 2);
        assert_eq!(retained.variant_bytes, 0);

        assert_eq!(
            registry.selected_upload(&main, "shared").unwrap().bytes,
            b"main"
        );
        assert_eq!(
            registry.selected_upload(&thread, "shared").unwrap().bytes,
            b"thread"
        );
        assert_eq!(
            registry.variant_bytes(&main, "shared", "original").unwrap(),
            b"main"
        );
        registry.clear_target(&thread);
        assert!(registry.selected_upload(&thread, "shared").is_none());
        assert_eq!(
            registry.selected_upload(&main, "shared").unwrap().bytes,
            b"main"
        );
        let after_thread_clear = registry.stats();
        assert_eq!(after_thread_clear.source_count, 1);
        assert_eq!(after_thread_clear.variant_count, 1);
        registry.remove_item(&main, "shared");
        let after_remove = registry.stats();
        assert_eq!(after_remove.source_count, 0);
        assert_eq!(after_remove.source_bytes, 0);
        assert_eq!(after_remove.variant_count, 0);
        assert_eq!(after_remove.variant_bytes, 0);
        assert_eq!(after_remove.high_water_source_count, 2);
        assert_eq!(after_remove.high_water_variant_count, 2);
    }

    #[test]
    fn clear_releases_retained_media_and_keeps_private_free_high_water_stats() {
        let mut registry = MediaPreparationRegistry::default();
        let target = target(None);
        registry.prepare_target(
            &target,
            vec![file("private-stage", b"private bytes")],
            ImageUploadCompressionPolicy::default(),
        );

        registry.clear();

        let stats = registry.stats();
        assert_eq!(stats.source_count, 0);
        assert_eq!(stats.source_bytes, 0);
        assert_eq!(stats.variant_count, 0);
        assert_eq!(stats.variant_bytes, 0);
        assert!(stats.high_water_source_bytes >= b"private bytes".len());
        assert_eq!(stats.high_water_variant_bytes, 0);
        let debug = format!("{registry:?}");
        assert!(!debug.contains("private-stage"));
        assert!(!debug.contains("private bytes"));
    }

    #[test]
    fn original_image_and_heif_variants_reuse_retained_source_bytes() {
        let target = target(None);
        let mut registry = MediaPreparationRegistry::default();
        let png = png_input("png-source", 8, 4);
        let png_bytes = png.bytes.clone();
        let heif = heif_input("heif-source");
        let heif_bytes = heif.bytes.clone();

        registry.prepare_items(
            &target,
            vec![png, heif],
            ImageUploadCompressionPolicy::default(),
        );

        let heif_converted_bytes = registry
            .selected_upload(&target, "heif-source")
            .expect("HEIF JPEG conversion")
            .bytes
            .len();
        let stats = registry.stats();
        assert_eq!(stats.source_backed_variant_count, 2);
        assert_eq!(stats.variant_bytes, heif_converted_bytes);
        assert_eq!(
            registry
                .variant_bytes(&target, "png-source", "original-keep")
                .expect("PNG original bytes"),
            png_bytes
        );
        assert_eq!(
            registry
                .variant_bytes(&target, "heif-source", "original-keep")
                .expect("HEIF original bytes"),
            heif_bytes
        );
    }

    #[test]
    fn empty_input_is_a_typed_failure_and_debug_is_private() {
        let mut registry = MediaPreparationRegistry::default();
        let target = target(None);
        let items = registry.prepare_target(
            &target,
            vec![file("private-stage", b"")],
            ImageUploadCompressionPolicy::default(),
        );
        assert!(matches!(
            items[0].preparation,
            StagedUploadPreparation::Failed {
                failure_kind: MediaPreparationFailureKind::Empty,
                ..
            }
        ));
        let debug = format!("{:?}", file("private-stage", b"private bytes"));
        assert!(!debug.contains("private.pdf"));
        assert!(!debug.contains("private bytes"));
    }

    #[test]
    fn failed_item_can_promote_its_captured_nonempty_source_to_original() {
        let mut registry = MediaPreparationRegistry::default();
        let target = target(Some("$root"));
        registry.sources.insert(
            (target.clone(), "failed".to_owned()),
            StageUploadBytesInput {
                staged_id: "failed".to_owned(),
                position: 2,
                filename: "private.bin".to_owned(),
                mime_type: "application/octet-stream".to_owned(),
                bytes: b"captured source".to_vec(),
            },
        );

        let item = registry
            .use_original(&target, "failed")
            .expect("captured original should be selectable");
        assert!(matches!(
            item.preparation,
            StagedUploadPreparation::Ready { .. }
        ));
        assert_eq!(
            registry.selected_upload(&target, "failed").unwrap().bytes,
            b"captured source"
        );
    }

    #[test]
    fn snapshot_reconcile_retains_only_items_backed_by_staging_state() {
        let mut registry = MediaPreparationRegistry::default();
        let main = target(None);
        let thread = target(Some("$root"));
        let stale_thread = target(Some("$stale"));
        let policy = ImageUploadCompressionPolicy::default();
        let mut staged = Vec::new();
        for (target, id) in [
            (&main, "main"),
            (&thread, "thread"),
            (&stale_thread, "stale"),
        ] {
            let items = registry.prepare_target(target, vec![file(id, id.as_bytes())], policy);
            staged.push(((*target).clone(), items[0].clone()));
        }
        let mut snapshot = koushi_state::AppState::default();
        for (target, item) in staged.into_iter().take(2) {
            snapshot
                .upload_staging
                .items
                .insert((target, item.staged_id.clone()), item);
        }

        registry.reconcile_snapshot(&snapshot);

        assert!(registry.selected_upload(&main, "main").is_some());
        assert!(registry.selected_upload(&thread, "thread").is_some());
        assert!(registry.selected_upload(&stale_thread, "stale").is_none());
        registry.clear_thread_targets();
        assert!(registry.selected_upload(&thread, "thread").is_none());
        assert!(registry.selected_upload(&main, "main").is_some());
        let stats = registry.stats();
        assert_eq!(stats.source_count, 1);
        assert_eq!(stats.variant_count, 1);
    }

    #[test]
    fn account_change_clears_bytes_even_when_room_ids_match() {
        let mut registry = MediaPreparationRegistry::default();
        let target = target(None);
        let mut snapshot = koushi_state::AppState::default();
        snapshot.timeline.room_id = Some("!room:example.invalid".to_owned());
        snapshot.session = koushi_state::SessionState::Ready(koushi_state::SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@first:example.invalid".to_owned(),
            device_id: "FIRST".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        });
        registry.reconcile_snapshot(&snapshot);
        let items = registry.prepare_target(
            &target,
            vec![file("private", b"first account")],
            ImageUploadCompressionPolicy::default(),
        );
        snapshot
            .upload_staging
            .items
            .insert((target.clone(), "private".to_owned()), items[0].clone());
        assert!(registry.selected_upload(&target, "private").is_some());

        snapshot.session = koushi_state::SessionState::SwitchingAccount {
            info: koushi_state::SessionInfo {
                homeserver: "https://example.invalid".to_owned(),
                user_id: "@second:example.invalid".to_owned(),
                device_id: "SECOND".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        };
        registry.reconcile_snapshot(&snapshot);
        assert!(registry.selected_upload(&target, "private").is_some());

        snapshot.session = koushi_state::SessionState::Ready(koushi_state::SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@second:example.invalid".to_owned(),
            device_id: "SECOND".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        });
        registry.reconcile_snapshot(&snapshot);

        assert!(registry.selected_upload(&target, "private").is_none());
        let stats = registry.stats();
        assert_eq!(stats.source_count, 0);
        assert_eq!(stats.variant_count, 0);
    }

    #[test]
    fn detached_batch_merge_preserves_an_overlapping_stage_result() {
        let target = target(None);
        let policy = ImageUploadCompressionPolicy::default();
        let mut committed = MediaPreparationRegistry::default();
        committed.prepare_items(&target, vec![file("first", b"first")], policy);
        let mut later = MediaPreparationRegistry::default();
        later.prepare_items(&target, vec![file("second", b"second")], policy);

        committed.merge_prepared(later);

        assert_eq!(
            committed.selected_upload(&target, "first").unwrap().bytes,
            b"first"
        );
        assert_eq!(
            committed.selected_upload(&target, "second").unwrap().bytes,
            b"second"
        );
    }
}
