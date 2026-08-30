//! Tauri command handlers: transport adapter only.
//!
//! Each handler allocates a `RequestId` and submits a `CoreCommand`.
//! Handlers return the current `FrontendDesktopSnapshot` only when the React
//! caller actually applies it; high-frequency fire-and-forget commands return
//! a tiny acknowledgement. Side-effects (state changes, timeline diffs) flow
//! back to the webview as Tauri events — not as command return values.
//!
//! No Matrix semantics live here. No SDK types. No `koushi_sdk` calls.
//! (Secret-bearing QA helpers remain behind `#[cfg(any(debug_assertions, test))]`.)

use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use koushi_core::{
    AccountCommand, AccountKey, AppCommand, CoreCommand, CoreConnection, CoreEvent, CoreFailure,
    CreateRoomOptions, EncryptionDebugOperationKind, EncryptionDebugOperationOutcome,
    IntentNoOpReason, IntentOutcome, MediaDownloadSelection, OutcomeCorrelation,
    PaginationDirection, RequestId, RequestOutcome, RequestOutcomeError, RequestOutcomeExpectation,
    RoomCommand, RoomKeyExportRequest, RoomKeyImportRequest, RoomKeyReshareOutcome,
    RoomOperationKind, SearchCommand, SearchScope, SecureBackupPassphraseChangeRequest,
    SecureBackupSetupRequest, SetAvatarRequest, SyncCommand, TimelineBatchId, TimelineCommand,
    TimelineEvent, TimelineGapId, TimelineGeneration, TimelineKey, TimelineKind,
    TimelineViewportObservation,
};
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::{
    ActivityMarkReadTarget, ActivityTab, AttachmentFilter, AttachmentSort, AuthSecret,
    ComposerDocument, ComposerDraftRevision, ComposerFormattingOptions, ComposerKeyEvent,
    ComposerResolvedAction, ComposerResolverContext, ComposerSurface, DirectoryQuery,
    DisplayPlatform, FilesViewScope, IdentityResetAuthRequest, ImageUploadCompressionMode,
    InviteScopeSelection, LoginRequest, MentionIntent, MentionSurface, PresenceKind,
    RecoveryRequest, RoomListFilter, RoomModerationAction, RoomNotificationMode, RoomSettingChange,
    RoomTagKind, SessionInfo, SettingsPatch, StagedUploadCompressionChoice, SubmissionId,
    ThreadOpenIntent, ThreadsListScope, TimelineScrollAnchor, VerificationCancelReason,
    build_formatted_message_draft,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{
    CoreRuntimeState,
    dto::{FrontendDesktopSnapshot, SearchScopeKind},
};

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);
static PROCESS_NONCE: OnceLock<u128> = OnceLock::new();

pub(crate) fn next_transaction_id(prefix: &str) -> String {
    let nonce = *PROCESS_NONCE.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos()
            ^ u128::from(std::process::id())
    });
    format!(
        "{prefix}-{nonce:x}-{}",
        NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod transaction_id_tests {
    use super::next_transaction_id;

    #[test]
    fn transaction_ids_have_process_nonce_and_counter() {
        let first = next_transaction_id("test");
        let second = next_transaction_id("test");
        assert!(first.starts_with("test-"));
        assert_ne!(first, second);
        assert!(first.rsplit_once('-').unwrap().1.parse::<u64>().is_ok());
    }
}

const CORE_COMMAND_SUBMIT_TIMEOUT: Duration = Duration::from_secs(2);
const QA_TITLE_ENV: &str = "KOUSHI_QA_TITLE";

pub(crate) mod account;
pub(crate) mod activity;
pub(crate) mod diagnostics;
pub(crate) mod directory;
pub(crate) mod e2ee;
pub(crate) mod live_signals;
pub(crate) mod local_encryption;
pub(crate) mod native_attention;
pub(crate) mod navigation;
pub(crate) mod profile;
pub(crate) mod room;
pub(crate) mod search;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod timeline;
pub(crate) mod views;

// ---- Core command dispatch helpers ----

/// Submit a `CoreCommand` over the command-dispatch connection.
///
/// This is the ONLY way commands leave the Tauri adapter.
/// Clones a lightweight submit handle before awaiting the bounded command
/// queue so snapshot reads are not blocked behind backpressured sends.
pub(crate) async fn submit_core_command(
    state: &CoreRuntimeState,
    command: CoreCommand,
) -> Result<(), String> {
    let command_handle = { state.connection.lock().await.command_handle() };

    match tokio::time::timeout(CORE_COMMAND_SUBMIT_TIMEOUT, command_handle.command(command)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("command submit failed: {error}")),
        Err(_) => Err("command submit timed out".to_owned()),
    }
}

/// Allocate a `RequestId` from the command-dispatch connection.
async fn next_request_id(state: &CoreRuntimeState) -> koushi_core::RequestId {
    state.connection.lock().await.next_request_id()
}

/// Read the latest `AppStateSnapshot` and convert to `FrontendDesktopSnapshot`.
async fn current_snapshot(state: &CoreRuntimeState) -> Result<FrontendDesktopSnapshot, String> {
    let snapshot = state.connection.lock().await.versioned_snapshot();
    Ok(FrontendDesktopSnapshot::from_versioned(
        snapshot.state,
        snapshot.generation,
    ))
}

pub(crate) fn invoke_error_from_core_failure(context: &str, failure: CoreFailure) -> String {
    format!("{context}: {failure:?}")
}

pub(crate) fn invoke_error_from_request_outcome(
    context: &str,
    error: RequestOutcomeError,
) -> String {
    match error {
        RequestOutcomeError::OperationFailed { failure } => {
            invoke_error_from_core_failure(context, failure)
        }
        RequestOutcomeError::FailedNoOp { reason } => {
            format!("{context}: failed no-op ({reason:?})")
        }
        RequestOutcomeError::Lagged => format!("{context}: request event stream lagged"),
        RequestOutcomeError::Disconnected => {
            format!("{context}: request event stream disconnected")
        }
        RequestOutcomeError::TimedOut => format!("{context}: request timed out"),
        RequestOutcomeError::InvalidOutcome => format!("{context}: invalid request outcome"),
    }
}

// ---- QA window title ----

fn qa_window_title_enabled() -> bool {
    matches!(
        std::env::var(QA_TITLE_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

async fn update_qa_window_title_from_state(app: &AppHandle, state: &CoreRuntimeState) {
    if !qa_window_title_enabled() {
        return;
    }
    let snapshot = state.connection.lock().await.snapshot();
    let timeline_items = state.timeline_items_count.load(Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&qa_window_title_string(&snapshot, timeline_items));
    }
}

/// Publish one private-data-free viewport result after the adapter has finished
/// its native repair and DOM observation. This is a QA-only title extension;
/// ordinary window-title semantics remain unchanged when QA title mode is off.
pub(crate) async fn update_qa_window_title_from_viewport_receipt<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &CoreRuntimeState,
    receipt: &crate::viewport_sync::ViewportSyncReceipt,
) {
    if !qa_window_title_enabled() {
        return;
    }
    let snapshot = state.connection.lock().await.snapshot();
    let timeline_items = state.timeline_items_count.load(Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&qa_window_title_with_viewport_receipt(
            &snapshot,
            timeline_items,
            receipt,
        ));
    }
}

pub(crate) fn qa_window_title_with_viewport_receipt(
    snapshot: &koushi_state::AppState,
    timeline_items: usize,
    receipt: &crate::viewport_sync::ViewportSyncReceipt,
) -> String {
    let parent_observed = matches!(
        receipt.native_support,
        crate::viewport_sync::NativeViewportSupport::Supported
    ) && receipt.parent.is_some();
    let aligned = parent_observed
        && receipt.native_aligned
        && receipt.dom_js_aligned
        && receipt.dom_root_aligned;
    let decision = match receipt.decision {
        crate::viewport_sync::ViewportSyncDecision::InSync => "in_sync",
        crate::viewport_sync::ViewportSyncDecision::RepairToParentBounds => {
            "repair_to_parent_bounds"
        }
        crate::viewport_sync::ViewportSyncDecision::Unsupported => "unsupported",
    };
    format!(
        "{} viewport={} viewport_generation={} viewport_parent={} viewport_webview={} viewport_js={} viewport_root={} viewport_decision={decision}",
        qa_window_title_string(snapshot, timeline_items),
        if aligned { "aligned" } else { "misaligned" },
        receipt.generation,
        parent_observed,
        receipt.native_aligned,
        receipt.dom_js_aligned,
        receipt.dom_root_aligned,
    )
}

pub(crate) fn qa_window_title_string(
    snapshot: &koushi_state::AppState,
    timeline_items: usize,
) -> String {
    [
        "koushi-desktop qa".to_owned(),
        format!("session={}", qa_session_label(&snapshot.session)),
        format!("sync={}", qa_sync_label(&snapshot.sync)),
        format!("rooms={}", snapshot.rooms.len()),
        format!("spaces={}", snapshot.spaces.len()),
        format!(
            "active_room={}",
            snapshot.navigation.active_room_id.is_some()
        ),
        format!("timeline_subscribed={}", snapshot.timeline.is_subscribed),
        format!("timeline_items={timeline_items}"),
        format!("errors={}", snapshot.errors.len()),
    ]
    .join(" ")
}

fn qa_session_label(session: &koushi_state::SessionState) -> &'static str {
    use koushi_state::SessionState;
    match session {
        SessionState::SignedOut => "signedOut",
        SessionState::Restoring => "restoring",
        SessionState::SwitchingAccount { .. } => "switchingAccount",
        SessionState::Authenticating { .. } => "authenticating",
        SessionState::Provisional { .. } => "provisional",
        SessionState::AwaitingVerification { .. } => "awaitingVerification",
        SessionState::Verifying { .. } => "verifying",
        SessionState::AwaitingBootstrapConfirmation { .. } => "awaitingBootstrapConfirmation",
        SessionState::Rejecting { .. } => "rejecting",
        SessionState::Ready(_) => "ready",
        SessionState::Locked(_) => "locked",
        SessionState::CapabilityBlocked { .. } => "capabilityBlocked",
        SessionState::LoggingOut => "loggingOut",
    }
}

fn qa_sync_label(sync: &koushi_state::SyncState) -> &'static str {
    match sync {
        koushi_state::SyncState::Stopped => "stopped",
        koushi_state::SyncState::Starting => "starting",
        koushi_state::SyncState::Running => "running",
        koushi_state::SyncState::Failed { .. } => "failed",
        koushi_state::SyncState::Reconnecting { .. } => "reconnecting",
    }
}

fn optional_non_blank(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

/// Derive the `AccountKey` for the currently active session from the snapshot.
///
/// Returns an empty key if no session is active (commands that require a Ready
/// session will be rejected by `AppActor::requires_ready_session`).
async fn account_key_from_snapshot(state: &CoreRuntimeState) -> AccountKey {
    let snapshot = state.connection.lock().await.snapshot();
    account_key_from_app_state(&snapshot)
}

fn account_key_from_app_state(snapshot: &koushi_state::AppState) -> AccountKey {
    match &snapshot.session {
        koushi_state::SessionState::Ready(info)
        | koushi_state::SessionState::Provisional { info, .. }
        | koushi_state::SessionState::AwaitingVerification { info, .. }
        | koushi_state::SessionState::Verifying { info, .. }
        | koushi_state::SessionState::AwaitingBootstrapConfirmation { info, .. }
        | koushi_state::SessionState::Rejecting { info, .. }
        | koushi_state::SessionState::Locked(info)
        | koushi_state::SessionState::CapabilityBlocked { info, .. }
        | koushi_state::SessionState::SwitchingAccount { info } => AccountKey(info.user_id.clone()),
        _ => AccountKey(String::new()),
    }
}

#[cfg(test)]
mod contracts;
