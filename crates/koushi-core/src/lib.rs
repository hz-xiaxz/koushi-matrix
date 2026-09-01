#![recursion_limit = "256"]

//! Koushi core runtime.
//!
//! The only production runtime owner: actor lifecycle, command routing,
//! event emission, SDK session handles, background tasks, and AppState
//! projection live here, behind the `CoreCommand`/`CoreEvent` boundary.
//!
//! Normative architecture: `docs/architecture/overview.md`.
//! Migration spec: `docs/superpowers/specs/2026-06-12-headless-core-runtime-design.md`.

pub mod account;
pub(crate) mod account_work;
mod activity_resolution;
pub(crate) mod cached_image;
mod causal_projection;
pub mod command;
pub mod composer_draft_lifecycle;
mod credential_vault;
mod direct_message_classification;
mod event_projection;
pub mod executor;
mod file;
pub mod link_preview;
mod live_catchup;
mod live_tail_freshness;
#[cfg(any(test, feature = "test-hooks"))]
pub mod login_store_test_support;
pub mod media_preparation;
pub mod media_save;
pub mod media_staging;
pub(crate) mod mention_candidates;
pub mod native_artifact;
pub(crate) mod read_state;
pub mod renderable_thumbnail;
mod report;
pub mod room;
mod room_key_receive;
mod room_key_recovery;
#[cfg(feature = "test-hooks")]
pub mod room_subscription_residency_test_support;
pub mod runtime;
pub(crate) mod scheduled_send;
pub mod search;
pub(crate) mod search_crawler;
mod send_diagnostics;
pub mod settings;
mod sliding_sync_diagnostics;
pub(crate) mod startup_trace;
pub mod state_delta;
pub mod store;
pub mod sync;
pub mod threads_list;
mod time;
pub mod timeline;
pub(crate) mod unread_trace;

pub use command::{
    AccountCommand, AppCommand, CoreCommand, CreateRoomOptions, CreateRoomParentSpace,
    CreateRoomVisibility, ImageUploadCompressionPolicy, ImageUploadCompressionState,
    ImageUploadDimensions, ImageUploadVariantInfo, ImageUploadVariantKind, KeyRequestOrigin,
    MediaDownloadSelection, RoomCommand, RoomKeyExportRequest, RoomKeyImportRequest, SearchCommand,
    SearchScope, SecureBackupPassphraseChangeRequest, SecureBackupSetupRequest, SetAvatarRequest,
    SyncCommand, TimelineCommand, UploadMediaKind, UploadMediaRequest, UploadMediaThumbnail,
};
pub use direct_message_classification::DirectAccountDataSource;
pub use koushi_protocol::*;
pub use koushi_state::{EncryptionDebugOperationKind, MediaTransferProgress};
pub use media_save::{
    MediaSaveError, MediaSaveFilesystem, MediaSaveIoError, default_media_save_path,
    safe_media_save_filename, save_downloaded_media,
};
pub use native_artifact::{
    NativeArtifactError, NativeArtifactKind, NativeArtifactPort, NativeArtifactRegistry,
};
pub use runtime::{
    COMMAND_INBOX_CAPACITY, CommandSubmitError, CoreCommandHandle, CoreConnection, CoreRuntime,
    EVENT_QUEUE_CAPACITY, EventStreamLag, OutcomeCorrelation, RequestOutcome, RequestOutcomeError,
    RequestOutcomeExpectation, RoomOperationKind, SelectRoomError,
};
pub use sliding_sync_diagnostics::{
    DiagnosticAgeBucket, SlidingSyncDiagnostics, SlidingSyncDiagnosticsSnapshot,
    SlidingSyncDiscoveryDiagnostic, SlidingSyncDiscoverySource, SlidingSyncDiscoveryState,
    SlidingSyncEngine, SlidingSyncFailureDiagnostic, SlidingSyncFailureKind,
    SlidingSyncFailureOrigin, SlidingSyncFailureRetryability, SlidingSyncFailureStage,
    SlidingSyncHttpErrorSource, SlidingSyncHttpStatus, SlidingSyncHttpStatusClass,
    SlidingSyncLifecycle, SlidingSyncMatrixErrorKind, SlidingSyncProvisionalHandoffBucket,
    SlidingSyncRequestSchema, SlidingSyncSdkVersion,
};
pub use state_delta::build_state_delta;

#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub fn project_timeline_event_for_qa(
    event: &mut koushi_protocol::TimelineEvent,
    state: &koushi_state::AppState,
) {
    event_projection::project_timeline_event_display_labels(event, state);
}
