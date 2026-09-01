//! Public state snapshot and incremental update DTOs.

use koushi_state::{
    AccountManagementCapabilities, AccountManagementState, AccountManagementUrl, ActivityState,
    AppError, AuthDiscoveryState, BasicOperationState, CjkTextPolicyState,
    CurrentSessionStatusState, DeviceCleanupState, DirectoryState, E2eeTrustState, FilesViewState,
    FocusedContextState, InvitePreview, InviteWorkflowState, LinkPreviewSettingsState,
    LiveSignalsState, LocalEncryptionState, MentionCandidatesState, NativeAttentionState,
    NavigationState, ProfileState, QrLoginState, RoomInteractionState, RoomListProjection,
    RoomManagementState, RoomNotificationSettings, RoomPreferencesState, RoomSummary,
    SearchCrawlerState, SearchState, SecureBackupGateState, SessionState, SettingsState,
    SidebarModel, SoftLogoutReauthState, SpaceMembersState, SpaceSummary, SyncState,
    ThreadAttentionState, ThreadPaneState, ThreadsListState, TimelinePaneState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub type AppStateSnapshot = koushi_state::AppState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionedAppStateSnapshot {
    pub generation: u64,
    pub state: AppStateSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreCommandAdmission {
    pub admitted_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateDelta {
    pub generation: u64,
    pub changed: StateDeltaChangedSlices,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateDeltaChangedSlices {
    pub session: Option<SessionState>,
    pub session_lock_reason: Option<Option<koushi_state::SessionLockReason>>,
    pub secure_backup_gate: Option<SecureBackupGateState>,
    pub device_cleanup: Option<DeviceCleanupState>,
    pub current_session_status: Option<CurrentSessionStatusState>,
    pub auth: Option<AuthDiscoveryState>,
    pub account_management_url: Option<Option<AccountManagementUrl>>,
    pub account_management: Option<AccountManagementState>,
    pub account_management_capabilities: Option<AccountManagementCapabilities>,
    pub soft_logout_reauth: Option<SoftLogoutReauthState>,
    pub qr_login: Option<QrLoginState>,
    pub settings: Option<SettingsState>,
    pub link_preview_settings: Option<LinkPreviewSettingsState>,
    pub room_preferences: Option<RoomPreferencesState>,
    pub profile: Option<ProfileState>,
    pub space_members: Option<SpaceMembersState>,
    pub sync: Option<SyncState>,
    pub navigation: Option<NavigationState>,
    pub spaces: Option<Vec<SpaceSummary>>,
    pub rooms: Option<Vec<RoomSummary>>,
    pub invites: Option<Vec<InvitePreview>>,
    pub invite_workflow: Option<InviteWorkflowState>,
    pub room_list: Option<RoomListProjection>,
    pub room_notification_settings: Option<HashMap<String, RoomNotificationSettings>>,
    pub room_interactions: Option<BTreeMap<String, RoomInteractionState>>,
    pub directory: Option<DirectoryState>,
    pub room_management: Option<RoomManagementState>,
    pub mention_candidates: Option<MentionCandidatesState>,
    pub activity: Option<ActivityState>,
    pub timeline: Option<TimelinePaneState>,
    pub thread: Option<ThreadPaneState>,
    pub thread_attention: Option<ThreadAttentionState>,
    pub threads_list: Option<ThreadsListState>,
    pub focused_context: Option<FocusedContextState>,
    pub search: Option<SearchState>,
    pub search_crawler: Option<SearchCrawlerState>,
    pub files_view: Option<FilesViewState>,
    pub basic_operation: Option<BasicOperationState>,
    pub live_signals: Option<LiveSignalsState>,
    pub e2ee_trust: Option<E2eeTrustState>,
    pub local_encryption: Option<LocalEncryptionState>,
    pub native_attention: Option<NativeAttentionState>,
    pub cjk_text_policy: Option<CjkTextPolicyState>,
    pub errors: Option<Vec<AppError>>,
    pub sidebar: Option<SidebarModel>,
}

impl StateDeltaChangedSlices {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}
