//! Core-owned staged-upload orchestration.
//!
//! This module owns the lifecycle around the reducer and the in-memory
//! preparation registry. Byte preparation is deliberately detached from the
//! registry lock; only the short state/registry commit sections are serialized.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[cfg(any(test, feature = "test-hooks"))]
use tokio::sync::oneshot;

use koushi_state::{
    ComposerDocument, ComposerTarget, ImageUploadCompressionPolicy, StagedUploadCompressionChoice,
    StagedUploadItem, StagedUploadKind, StagedUploadOutputSelection, StagedUploadPreparation,
};

use crate::{
    command::{AppCommand, CoreCommand},
    executor,
    ids::AccountKey,
    media_preparation::{
        MAX_PREPARATION_BATCH_SIZE, MediaPreparationRegistry, MediaPreparationService,
        StageUploadBytesInput,
    },
    runtime::{
        CoreConnection, OutcomeCorrelation, RequestOutcome, RequestOutcomeError,
        RequestOutcomeExpectation,
    },
};

/// Maximum total source bytes accepted by one staging request.
pub const MAX_MEDIA_STAGING_BATCH_BYTES: usize = 128 * 1024 * 1024;
/// Maximum number of source items accepted by one staging request.
pub const MAX_MEDIA_STAGING_BATCH_SIZE: usize = MAX_PREPARATION_BATCH_SIZE;
const MEDIA_STAGING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MediaStagingError {
    #[error("media staging batch is empty")]
    BatchEmpty,
    #[error("media staging batch exceeds its limit")]
    BatchTooLarge,
    #[error("media staging contains a duplicate staged id")]
    DuplicateStagedId,
    #[error("media staging positions must be nonzero and unique")]
    InvalidPosition,
    #[error("media staging target is no longer active")]
    TargetInactive,
    #[error("staged upload item is not ready for this operation")]
    PreparationNotReady,
    #[error("staged upload selection is invalid for this item")]
    InvalidSelection,
    #[error("staged upload compression choice is invalid for this item")]
    InvalidCompressionChoice,
    #[error("staged upload item is no longer available")]
    MissingStagedItem,
    #[error("prepared media bytes are no longer available")]
    PreparedBytesUnavailable,
    #[error("media preparation did not produce an output")]
    PreparationFailed,
    #[error("media preparation task did not complete")]
    PreparationTask,
    #[error("media staging became stale")]
    Stale,
    #[error("media staging command could not be submitted: {0}")]
    CommandSubmit(crate::runtime::CommandSubmitError),
    #[error("media staging outcome was not observed: {0}")]
    Outcome(RequestOutcomeError),
}

#[derive(Clone)]
pub struct MediaStagingService {
    preparation: Arc<MediaPreparationService>,
    #[cfg(any(test, feature = "test-hooks"))]
    preparation_pause: Arc<std::sync::Mutex<Option<PreparationPause>>>,
}

#[cfg(any(test, feature = "test-hooks"))]
struct PreparationPause {
    started: oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

#[cfg(any(test, feature = "test-hooks"))]
pub struct PreparationBarrierForTesting {
    started: oneshot::Receiver<()>,
    release: Option<std::sync::mpsc::Sender<()>>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl PreparationBarrierForTesting {
    pub async fn wait_started(&mut self) {
        (&mut self.started)
            .await
            .expect("preparation barrier must start");
    }

    pub fn release(mut self) {
        self.release
            .take()
            .expect("preparation barrier releases once")
            .send(())
            .expect("preparation must be waiting at barrier");
    }
}

impl MediaStagingService {
    pub(crate) fn new(preparation: Arc<MediaPreparationService>) -> Self {
        Self {
            preparation,
            #[cfg(any(test, feature = "test-hooks"))]
            preparation_pause: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn install_preparation_barrier_for_testing(&self) -> PreparationBarrierForTesting {
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        *self
            .preparation_pause
            .lock()
            .expect("preparation barrier mutex") = Some(PreparationPause {
            started,
            release: release_rx,
        });
        PreparationBarrierForTesting {
            started: started_rx,
            release: Some(release),
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn pause_preparation_for_testing(&self) {
        let pause = self
            .preparation_pause
            .lock()
            .expect("preparation barrier mutex")
            .take();
        if let Some(pause) = pause {
            let _ = pause.started.send(());
            let _ = pause.release.recv();
        }
    }

    /// Publish Preparing, prepare bytes off-lock, then publish the exact
    /// prepared result through the existing AppCommand/reducer path.
    pub async fn stage_upload_bytes(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        items: Vec<StageUploadBytesInput>,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        validate_batch(&items)?;
        let initial = connection.snapshot();
        let policy = authoritative_policy(&initial);
        let initial_account = account_key(&initial);
        let existing = active_items(&initial, &target).ok_or(MediaStagingError::TargetInactive)?;
        let existing_ids = existing
            .iter()
            .map(|item| item.staged_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen = std::collections::BTreeSet::new();
        let mut positions = existing
            .iter()
            .map(|item| item.position)
            .collect::<std::collections::BTreeSet<_>>();
        for item in &items {
            if !seen.insert(item.staged_id.as_str())
                || existing_ids.contains(item.staged_id.as_str())
            {
                return Err(MediaStagingError::DuplicateStagedId);
            }
            if item.position == 0 || !positions.insert(item.position) {
                return Err(MediaStagingError::InvalidPosition);
            }
        }

        let mut preparing_items = existing
            .iter()
            .cloned()
            .chain(items.iter().map(|item| preparing_item(&target, item)))
            .collect::<Vec<_>>();
        preparing_items.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.staged_id.cmp(&right.staged_id))
        });
        let preparing_ids = ids(&preparing_items);
        self.publish(
            connection,
            initial_account.clone(),
            target.clone(),
            preparing_items,
            preparing_ids.clone(),
            false,
        )
        .await?;

        let preparation_target = target.clone();
        let prepared_inputs = items;
        #[cfg(any(test, feature = "test-hooks"))]
        let preparation_service = self.clone();
        let prepared = executor::spawn_blocking(move || {
            #[cfg(any(test, feature = "test-hooks"))]
            preparation_service.pause_preparation_for_testing();
            let mut registry = MediaPreparationRegistry::default();
            let items = registry.prepare_items(&preparation_target, prepared_inputs, policy);
            (registry, items)
        })
        .await
        .map_err(|_| MediaStagingError::PreparationTask)?;
        let (prepared_registry, prepared_items) = prepared;

        let current = connection.snapshot();
        let current_items = active_items(&current, &target).ok_or(MediaStagingError::Stale)?;
        if account_key(&current) != initial_account
            || authoritative_policy(&current) != policy
            || ids(current_items) != preparing_ids
            || prepared_items.iter().any(|prepared| {
                !current_items.iter().any(|current| {
                    current.staged_id == prepared.staged_id
                        && matches!(current.preparation, StagedUploadPreparation::Preparing)
                })
            })
        {
            return Err(MediaStagingError::Stale);
        }

        let mut prepared_by_id = prepared_items
            .into_iter()
            .map(|item| (item.staged_id.clone(), item))
            .collect::<BTreeMap<_, _>>();
        let mut ready_items = current_items.to_vec();
        for item in &mut ready_items {
            if let Some(mut prepared) = prepared_by_id.remove(&item.staged_id) {
                // Captions and user-selected compression are state-owned
                // metadata, not preparation output.
                prepared.caption = item.caption.clone();
                prepared.compression_choice = item.compression_choice;
                *item = prepared;
            }
        }
        if !prepared_by_id.is_empty() {
            return Err(MediaStagingError::Stale);
        }
        self.merge_after_revalidation(prepared_registry).await;
        self.publish(
            connection,
            initial_account,
            target,
            ready_items.clone(),
            ids(&ready_items),
            false,
        )
        .await
    }

    pub async fn select_staged_upload_output(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        staged_id: String,
        selection: StagedUploadOutputSelection,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let initial = connection.snapshot();
        let policy = authoritative_policy(&initial);
        let account = account_key(&initial);
        if !target_is_active(&initial, &target) {
            return Err(MediaStagingError::TargetInactive);
        }
        let item = staged_item(&initial, &target, &staged_id)
            .ok_or(MediaStagingError::MissingStagedItem)?;
        if !matches!(item.preparation, StagedUploadPreparation::Ready { .. }) {
            return Err(MediaStagingError::PreparationNotReady);
        }
        if !matches!(item.kind, StagedUploadKind::Image { .. }) {
            return Err(MediaStagingError::InvalidSelection);
        }
        let selected = self
            .publish_selection(
                connection,
                account.clone(),
                target.clone(),
                staged_id.clone(),
                selection,
            )
            .await?;

        let cached = {
            let mut transition = self.preparation.transition().await;
            transition.select_variant(
                &target,
                &staged_id,
                &MediaPreparationRegistry::output_identity(selection),
            )
        };
        if cached {
            return Ok(selected);
        }

        let current = connection.snapshot();
        let generation = staged_item(&current, &target, &staged_id)
            .and_then(|item| match item.preparation {
                StagedUploadPreparation::Ready { generation, .. } => Some(generation),
                StagedUploadPreparation::Preparing | StagedUploadPreparation::Failed { .. } => None,
            })
            .ok_or(MediaStagingError::MissingStagedItem)?;
        let source = {
            let transition = self.preparation.transition().await;
            transition
                .source_input(&target, &staged_id)
                .ok_or(MediaStagingError::PreparedBytesUnavailable)?
        };
        #[cfg(any(test, feature = "test-hooks"))]
        let preparation_service = self.clone();
        let encoded = executor::spawn_blocking(move || {
            #[cfg(any(test, feature = "test-hooks"))]
            preparation_service.pause_preparation_for_testing();
            MediaPreparationRegistry::encode_output(&source, selection, policy)
        })
        .await
        .map_err(|_| MediaStagingError::PreparationTask)?
        .ok_or(MediaStagingError::PreparationFailed)?;
        let (descriptor, bytes) = encoded;

        let current = connection.snapshot();
        let item = staged_item(&current, &target, &staged_id).ok_or(MediaStagingError::Stale)?;
        if account_key(&current) != account
            || authoritative_policy(&current) != policy
            || !target_is_active(&current, &target)
            || !matches!(
                item.preparation,
                StagedUploadPreparation::Ready { generation: current_generation, .. }
                    if current_generation == generation
            )
        {
            return Err(MediaStagingError::Stale);
        }
        let replacement = koushi_state::staged_upload_item_with_completed_output(
            item,
            descriptor.clone(),
            generation,
        )
        .ok_or(MediaStagingError::Stale)?;
        {
            let mut transition = self.preparation.transition().await;
            transition.insert_prepared_output(&target, &staged_id, descriptor, bytes);
        }
        self.replace_staged_upload_item(connection, account, target, staged_id, replacement)
            .await
    }

    pub async fn retry_staged_upload_preparation(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        staged_id: String,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let initial = connection.snapshot();
        let policy = authoritative_policy(&initial);
        let account = account_key(&initial);
        if !target_is_active(&initial, &target) {
            return Err(MediaStagingError::TargetInactive);
        }
        let item = staged_item(&initial, &target, &staged_id)
            .ok_or(MediaStagingError::MissingStagedItem)?;
        if !matches!(item.preparation, StagedUploadPreparation::Failed { .. }) {
            return Err(MediaStagingError::PreparationNotReady);
        }
        let source = {
            let transition = self.preparation.transition().await;
            transition
                .source_input(&target, &staged_id)
                .ok_or(MediaStagingError::PreparedBytesUnavailable)?
        };
        let retry_target = target.clone();
        #[cfg(any(test, feature = "test-hooks"))]
        let preparation_service = self.clone();
        let (prepared_registry, replacement) = executor::spawn_blocking(move || {
            #[cfg(any(test, feature = "test-hooks"))]
            preparation_service.pause_preparation_for_testing();
            let mut registry = MediaPreparationRegistry::default();
            let replacement = registry
                .prepare_items(&retry_target, vec![source], policy)
                .into_iter()
                .next();
            (registry, replacement)
        })
        .await
        .map_err(|_| MediaStagingError::PreparationTask)?;
        let replacement = replacement.ok_or(MediaStagingError::PreparationFailed)?;
        let current = connection.snapshot();
        if account_key(&current) != account
            || authoritative_policy(&current) != policy
            || !target_is_active(&current, &target)
        {
            return Err(MediaStagingError::Stale);
        }
        let current_item = staged_item(&current, &target, &staged_id)
            .filter(|item| matches!(item.preparation, StagedUploadPreparation::Failed { .. }))
            .ok_or(MediaStagingError::Stale)?;
        let mut replacement = replacement;
        replacement.caption = current_item.caption.clone();
        replacement.compression_choice = current_item.compression_choice;
        if replacement == *current_item {
            return Ok(connection.versioned_snapshot());
        }
        {
            let mut transition = self.preparation.transition().await;
            transition.remove_item(&target, &staged_id);
            transition.merge_prepared(prepared_registry);
        }
        self.replace_staged_upload_item(connection, account, target, staged_id, replacement)
            .await
    }

    pub async fn use_original(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        staged_id: String,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let snapshot = connection.snapshot();
        let account = account_key(&snapshot);
        if !target_is_active(&snapshot, &target) {
            return Err(MediaStagingError::TargetInactive);
        }
        if staged_item(&snapshot, &target, &staged_id).is_none() {
            return Err(MediaStagingError::MissingStagedItem);
        }
        let replacement = {
            let mut transition = self.preparation.transition().await;
            transition
                .use_original(&target, &staged_id)
                .ok_or(MediaStagingError::PreparedBytesUnavailable)?
        };
        self.replace_staged_upload_item(connection, account, target, staged_id, replacement)
            .await
    }

    pub async fn update_caption(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        staged_id: String,
        caption: Option<ComposerDocument>,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let snapshot = connection.snapshot();
        let account = account_key(&snapshot);
        let staged_id = staged_id.clone();
        if !target_is_active(&snapshot, &target) {
            return Err(MediaStagingError::TargetInactive);
        }
        if staged_item(&snapshot, &target, &staged_id).is_none() {
            return Err(MediaStagingError::MissingStagedItem);
        }
        let expected_ids = active_ids(&snapshot, &target)?;
        let caption = caption
            .and_then(|document| (!document.plain_body().trim().is_empty()).then_some(document));
        let request_id = connection.next_request_id();
        let baseline = connection.versioned_snapshot().generation;
        connection
            .command(CoreCommand::App(AppCommand::UpdateStagedUploadCaption {
                request_id,
                target: target.clone(),
                staged_id,
                caption,
            }))
            .await
            .map_err(MediaStagingError::CommandSubmit)?;
        self.wait(
            connection,
            request_id,
            account,
            target,
            expected_ids,
            baseline,
            false,
        )
        .await
    }

    pub async fn update_compression(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
        staged_id: String,
        compression_choice: StagedUploadCompressionChoice,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let snapshot = connection.snapshot();
        let account = account_key(&snapshot);
        if !target_is_active(&snapshot, &target) {
            return Err(MediaStagingError::TargetInactive);
        }
        if staged_item(&snapshot, &target, &staged_id).is_none() {
            return Err(MediaStagingError::MissingStagedItem);
        }
        let expected_ids = active_ids(&snapshot, &target)?;
        let request_id = connection.next_request_id();
        let baseline = connection.versioned_snapshot().generation;
        connection
            .command(CoreCommand::App(
                AppCommand::UpdateStagedUploadCompression {
                    request_id,
                    target: target.clone(),
                    staged_id,
                    compression_choice,
                },
            ))
            .await
            .map_err(MediaStagingError::CommandSubmit)?;
        self.wait(
            connection,
            request_id,
            account,
            target,
            expected_ids,
            baseline,
            false,
        )
        .await
    }

    pub async fn clear(
        &self,
        connection: &mut CoreConnection,
        target: ComposerTarget,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let snapshot = connection.snapshot();
        let account = account_key(&snapshot);
        let Some(items) = active_items(&snapshot, &target) else {
            return Err(MediaStagingError::TargetInactive);
        };
        if items.is_empty() {
            self.preparation.reconcile_snapshot(&snapshot).await;
            return Ok(connection.versioned_snapshot());
        }
        let request_id = connection.next_request_id();
        let baseline = connection.versioned_snapshot().generation;
        connection
            .command(CoreCommand::App(AppCommand::ClearUploadStaging {
                request_id,
                target: target.clone(),
            }))
            .await
            .map_err(MediaStagingError::CommandSubmit)?;
        let result = self
            .wait(
                connection,
                request_id,
                account,
                target,
                Vec::new(),
                baseline,
                false,
            )
            .await?;
        self.preparation.reconcile_snapshot(&result.state).await;
        Ok(result)
    }

    async fn publish(
        &self,
        connection: &mut CoreConnection,
        account: AccountKey,
        target: ComposerTarget,
        items: Vec<StagedUploadItem>,
        expected_ids: Vec<String>,
        allow_initial: bool,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let request_id = connection.next_request_id();
        let baseline = connection.versioned_snapshot().generation;
        connection
            .command(CoreCommand::App(AppCommand::SetUploadStaging {
                request_id,
                target: target.clone(),
                items,
            }))
            .await
            .map_err(MediaStagingError::CommandSubmit)?;
        self.wait(
            connection,
            request_id,
            account,
            target,
            expected_ids,
            baseline,
            allow_initial,
        )
        .await
    }

    async fn publish_selection(
        &self,
        connection: &mut CoreConnection,
        account: AccountKey,
        target: ComposerTarget,
        staged_id: String,
        selection: StagedUploadOutputSelection,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let expected_ids = active_ids(&connection.snapshot(), &target)?;
        let request_id = connection.next_request_id();
        let baseline = connection.versioned_snapshot().generation;
        connection
            .command(CoreCommand::App(AppCommand::SelectStagedUploadOutput {
                request_id,
                target: target.clone(),
                staged_id,
                selection,
            }))
            .await
            .map_err(MediaStagingError::CommandSubmit)?;
        self.wait(
            connection,
            request_id,
            account,
            target,
            expected_ids,
            baseline,
            false,
        )
        .await
    }

    async fn replace_staged_upload_item(
        &self,
        connection: &mut CoreConnection,
        account: AccountKey,
        target: ComposerTarget,
        staged_id: String,
        replacement: StagedUploadItem,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        let current = connection.snapshot();
        let mut items = active_items(&current, &target)
            .ok_or(MediaStagingError::TargetInactive)?
            .to_vec();
        let item = items
            .iter_mut()
            .find(|item| item.staged_id == staged_id)
            .ok_or(MediaStagingError::MissingStagedItem)?;
        *item = replacement;
        let expected_ids = ids(&items);
        self.publish(connection, account, target, items, expected_ids, false)
            .await
    }

    async fn merge_after_revalidation(&self, prepared: MediaPreparationRegistry) {
        self.preparation.transition().await.merge_prepared(prepared);
    }

    async fn wait(
        &self,
        connection: &mut CoreConnection,
        request_id: crate::ids::RequestId,
        account_key: AccountKey,
        target: ComposerTarget,
        staged_ids: Vec<String>,
        baseline_generation: u64,
        allow_initial: bool,
    ) -> Result<crate::event::VersionedAppStateSnapshot, MediaStagingError> {
        match connection
            .wait_for_request_outcome(
                OutcomeCorrelation::Request(request_id),
                RequestOutcomeExpectation::UploadStaging {
                    request_id,
                    account_key,
                    target,
                    staged_ids,
                    allow_initial,
                },
                baseline_generation,
                executor::Instant::now() + MEDIA_STAGING_TIMEOUT,
            )
            .await
            .map_err(MediaStagingError::Outcome)?
        {
            RequestOutcome::UploadStaging { snapshot, .. } => Ok(snapshot),
            _ => Err(MediaStagingError::Outcome(
                RequestOutcomeError::InvalidOutcome,
            )),
        }
    }
}

fn validate_batch(items: &[StageUploadBytesInput]) -> Result<(), MediaStagingError> {
    if items.is_empty() {
        return Err(MediaStagingError::BatchEmpty);
    }
    if items.len() > MAX_MEDIA_STAGING_BATCH_SIZE {
        return Err(MediaStagingError::BatchTooLarge);
    }
    let total = items
        .iter()
        .try_fold(0usize, |total, item| total.checked_add(item.bytes.len()))
        .ok_or(MediaStagingError::BatchTooLarge)?;
    if total > MAX_MEDIA_STAGING_BATCH_BYTES {
        return Err(MediaStagingError::BatchTooLarge);
    }
    Ok(())
}

fn authoritative_policy(state: &koushi_state::AppState) -> ImageUploadCompressionPolicy {
    state.settings.values.media.image_upload_compression_policy
}

fn preparing_item(target: &ComposerTarget, input: &StageUploadBytesInput) -> StagedUploadItem {
    let mime_type = crate::media_preparation::normalized_mime(&input.mime_type);
    let kind = if mime_type.to_ascii_lowercase().starts_with("image/") {
        StagedUploadKind::Image {
            width: None,
            height: None,
        }
    } else {
        StagedUploadKind::File
    };
    StagedUploadItem {
        staged_id: input.staged_id.clone(),
        room_id: target.room_id().to_owned(),
        position: input.position,
        filename: input.filename.clone(),
        mime_type,
        byte_count: input.bytes.len() as u64,
        kind,
        caption: None,
        compression_choice: StagedUploadCompressionChoice::NotApplicable,
        preparation: StagedUploadPreparation::Preparing,
    }
}

fn target_is_active(state: &koushi_state::AppState, target: &ComposerTarget) -> bool {
    matches!(state.session, koushi_state::SessionState::Ready(_))
        && active_items(state, target).is_some()
}

fn active_items<'a>(
    state: &'a koushi_state::AppState,
    target: &ComposerTarget,
) -> Option<&'a [StagedUploadItem]> {
    if !matches!(state.session, koushi_state::SessionState::Ready(_)) {
        return None;
    }
    match target {
        ComposerTarget::Main { room_id }
            if state.timeline.room_id.as_deref() == Some(room_id.as_str()) =>
        {
            Some(&state.timeline.staged_uploads)
        }
        ComposerTarget::Thread {
            room_id,
            root_event_id,
        } => match &state.thread {
            koushi_state::ThreadPaneState::Open {
                room_id: current_room,
                root_event_id: current_root,
                staged_uploads,
                ..
            } if current_room == room_id && current_root == root_event_id => Some(staged_uploads),
            _ => None,
        },
        _ => None,
    }
}

fn active_ids(
    state: &koushi_state::AppState,
    target: &ComposerTarget,
) -> Result<Vec<String>, MediaStagingError> {
    active_items(state, target)
        .map(ids)
        .ok_or(MediaStagingError::TargetInactive)
}

fn staged_item<'a>(
    state: &'a koushi_state::AppState,
    target: &ComposerTarget,
    staged_id: &str,
) -> Option<&'a StagedUploadItem> {
    active_items(state, target)?
        .iter()
        .find(|item| item.staged_id == staged_id)
}

fn ids(items: &[StagedUploadItem]) -> Vec<String> {
    items.iter().map(|item| item.staged_id.clone()).collect()
}

fn account_key(state: &koushi_state::AppState) -> AccountKey {
    let user_id = match &state.session {
        koushi_state::SessionState::Ready(info)
        | koushi_state::SessionState::Locked(info)
        | koushi_state::SessionState::CapabilityBlocked { info, .. }
        | koushi_state::SessionState::SwitchingAccount { info }
        | koushi_state::SessionState::Provisional { info, .. }
        | koushi_state::SessionState::AwaitingVerification { info, .. }
        | koushi_state::SessionState::Verifying { info, .. }
        | koushi_state::SessionState::AwaitingBootstrapConfirmation { info, .. }
        | koushi_state::SessionState::Rejecting { info, .. } => info.user_id.clone(),
        _ => String::new(),
    };
    AccountKey(user_id)
}
