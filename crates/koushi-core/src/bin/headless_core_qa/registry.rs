const ENV_HOMESERVER: &str = "KOUSHI_LOCAL_QA_HOMESERVER";
const ENV_SERVER_NAME: &str = "KOUSHI_LOCAL_QA_SERVER_NAME";
const ENV_SERVER_KIND: &str = "KOUSHI_LOCAL_QA_SERVER_KIND";
const ENV_USER_A: &str = "KOUSHI_LOCAL_QA_USER_A";
const ENV_PASSWORD_A: &str = "KOUSHI_LOCAL_QA_PASSWORD_A";
const ENV_USER_B: &str = "KOUSHI_LOCAL_QA_USER_B";
const ENV_PASSWORD_B: &str = "KOUSHI_LOCAL_QA_PASSWORD_B";
const ENV_USER_C: &str = "KOUSHI_LOCAL_QA_USER_C";
const ENV_QA_SCENARIO: &str = "KOUSHI_QA_SCENARIO";
const ENV_ALLOW_IDENTITY_RESET: &str = "KOUSHI_QA_ALLOW_IDENTITY_RESET";
const ENV_E2EE_RECIPIENT_SECOND_DEVICE: &str = "KOUSHI_QA_E2EE_RECIPIENT_SECOND_DEVICE";
#[cfg(any(debug_assertions, feature = "qa-bin"))]
const ENV_FILE_CREDENTIAL_STORE_DIR: &str = "KOUSHI_QA_FILE_CREDENTIAL_STORE_DIR";
const DEVICE_A: &str = "Koushi Core QA A";
const DEVICE_B: &str = "Koushi Core QA B";
/// Maximum time to wait for a single event.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const GATE_RESTORE_READY_BUDGET: Duration = Duration::from_secs(10);
const LOGIN_EVENT_TIMEOUT: Duration = Duration::from_secs(180);
const ROOM_LIST_EVENT_TIMEOUT: Duration = Duration::from_secs(90);
const TIMELINE_INITIAL_EVENT_TIMEOUT: Duration = Duration::from_secs(90);
const E2EE_EVENT_TIMEOUT: Duration = Duration::from_secs(90);
const SEND_QUEUE_EVENT_TIMEOUT: Duration = Duration::from_secs(300);
const TIMELINE_UNSUBSCRIBE_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
const TIMELINE_RECONNECT_EXPECTED_BODY_COUNT: usize = 21;
const TIMELINE_RECONNECT_MIN_INITIAL_BODIES: usize = 20;
const TIMELINE_RECONNECT_PAGINATE_EVENT_COUNT: u16 = 64;
const DEFAULT_STRESS_SPACE_COUNT: usize = 2;
const DEFAULT_STRESS_ROOMS_PER_SPACE: usize = 2;
const DEFAULT_STRESS_MESSAGES_PER_ROOM: usize = 8;
const MAX_STRESS_SPACE_COUNT: usize = 6;
const MAX_STRESS_ROOMS_PER_SPACE: usize = 8;
const MAX_STRESS_MESSAGES_PER_ROOM: usize = 80;
const ENV_STRESS_SPACE_COUNT: &str = "KOUSHI_QA_STRESS_SPACES";
const ENV_STRESS_ROOMS_PER_SPACE: &str = "KOUSHI_QA_STRESS_ROOMS_PER_SPACE";
const ENV_STRESS_MESSAGES_PER_ROOM: &str = "KOUSHI_QA_STRESS_MESSAGES_PER_ROOM";
const ENV_STRESS_REPLAY_EXISTING: &str = "KOUSHI_QA_STRESS_REPLAY_EXISTING";
const QA_WRONG_RECOVERY_SECRET: &str = "koushi-desktop-headless-qa-wrong-recovery-secret";
const ENV_CACHE_RESTORE_ROOMS: &str = "KOUSHI_QA_CACHE_RESTORE_ROOMS";
const ENV_CACHE_RESTORE_DEPTH: &str = "KOUSHI_QA_CACHE_RESTORE_DEPTH";
const DEFAULT_CACHE_RESTORE_ROOMS: usize = 3;
const DEFAULT_CACHE_RESTORE_DEPTH: usize = 200;
/// Batch size used for backward pagination during the populate (EndReached) pass.
const CACHE_RESTORE_PAGINATE_BATCH: u16 = 20;
/// Production-faithful restore parameters, matching the app's live-room constants.
/// Source: apps/desktop/src/components/TimelineView.tsx:406-407
/// (LIVE_ROOM_ANCHOR_RESTORE_MAX_BATCHES=6, EVENT_COUNT=100).
/// These are intentionally small. Room entry should fail fast for stale or
/// very deep persisted anchors and let the UI fall back to live edge; deep
/// event-centered restore belongs to an explicit focused-event timeline.
const CACHE_RESTORE_PROD_MAX_BATCHES: u16 = 6;
const CACHE_RESTORE_PROD_EVENT_COUNT: u16 = 100;
/// Speed gate: maximum backward-paginate cycles allowed per room during an
/// offline anchor restore. Deep anchors may end as BudgetExhausted, but they
/// must not walk history long enough to block room entry.
const CACHE_RESTORE_MAX_CYCLES: u16 = 3;
/// Number of messages in the shallow-anchor room.  Enough to exceed the SDK's
/// initial visible window (~20 items) so that m0 (oldest) is hidden behind a
/// lazy-reveal skip when the session restarts.  All events fit in a single
/// stored chunk (well under 128), so chunks_loaded == 0 during the restore.
/// The anchor (m0) lives in the live in-memory prefix that
/// live_lazy_paginate_backwards reveals (lazy_reveal_batches == 1).
/// The P1 lazy-reveal-fence fix gates on this: without it the settle fence
/// misses the lazy-reveal DiffBatch and may conclude before items settle.
const CACHE_RESTORE_SHALLOW_DEPTH: usize = 30;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QaScenario {
    All,
    Safety,
    LoginSync,
    SessionStatus,
    CredentialHealth,
    NativeAttention,
    EncryptionDebug,
    E2eeTrust,
    DeviceCleanup,
    GateRestore,
    GateNegative,
    GateNoProof,
    InvitesDm,
    RoomSpace,
    Directory,
    RoomManagement,
    RoomPeopleProjection,
    Timeline,
    TimelineReconnect,
    TimelineStress,
    Activity,
    Composer,
    Reply,
    Media,
    LiveSignals,
    Thread,
    EditRedactSearch,
    SearchCrawler,
    ScheduledSend,
    SendQueue,
    RestoreCleanup,
    LinkPreview,
    CacheRestore,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QaStage {
    Safety,
    LoginSync,
    SessionStatus,
    CredentialHealth,
    NativeAttention,
    EncryptionDebug,
    E2eeTrust,
    DeviceCleanup,
    GateRestore,
    GateNegative,
    GateNoProof,
    InvitesDm,
    RoomSpace,
    Directory,
    RoomManagement,
    RoomPeopleProjection,
    Timeline,
    TimelineReconnect,
    TimelineStress,
    Activity,
    Composer,
    Reply,
    Media,
    LiveSignals,
    Thread,
    EditRedactSearch,
    SearchCrawler,
    ScheduledSend,
    SendQueue,
    RestoreCleanup,
    LinkPreview,
    CacheRestore,
}
impl QaScenario {
    fn from_env() -> Result<Self, String> {
        match std::env::var(ENV_QA_SCENARIO) {
            Ok(value) => Self::from_env_value(&value),
            Err(_) => Ok(Self::All),
        }
    }

    fn from_env_value(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "safety" => Ok(Self::Safety),
            "login_sync" => Ok(Self::LoginSync),
            "session_status" => Ok(Self::SessionStatus),
            "credential_health" => Ok(Self::CredentialHealth),
            "native_attention" => Ok(Self::NativeAttention),
            "encryption_debug" => Ok(Self::EncryptionDebug),
            "e2ee_trust" => Ok(Self::E2eeTrust),
            "device_cleanup" => Ok(Self::DeviceCleanup),
            "gate_restore" => Ok(Self::GateRestore),
            "gate_negative" => Ok(Self::GateNegative),
            "gate_no_proof" => Ok(Self::GateNoProof),
            "invites_dm" => Ok(Self::InvitesDm),
            "room_space" => Ok(Self::RoomSpace),
            "directory" => Ok(Self::Directory),
            "room_management" => Ok(Self::RoomManagement),
            "room_people_projection" => Ok(Self::RoomPeopleProjection),
            "timeline" => Ok(Self::Timeline),
            "timeline_reconnect" => Ok(Self::TimelineReconnect),
            "timeline_stress" => Ok(Self::TimelineStress),
            "activity" => Ok(Self::Activity),
            "composer" => Ok(Self::Composer),
            "reply" => Ok(Self::Reply),
            "media" => Ok(Self::Media),
            "live_signals" => Ok(Self::LiveSignals),
            "thread" => Ok(Self::Thread),
            "edit_redact_search" => Ok(Self::EditRedactSearch),
            "search_crawler" => Ok(Self::SearchCrawler),
            "scheduled_send" => Ok(Self::ScheduledSend),
            "send_queue" => Ok(Self::SendQueue),
            "restore_cleanup" => Ok(Self::RestoreCleanup),
            "link_preview" => Ok(Self::LinkPreview),
            "cache_restore" => Ok(Self::CacheRestore),
            other => Err(format!(
                "{ENV_QA_SCENARIO} must be one of all, safety, login_sync, session_status, credential_health, native_attention, encryption_debug, e2ee_trust, device_cleanup, invites_dm, room_space, directory, room_management, room_people_projection, timeline, timeline_reconnect, timeline_stress, activity, composer, reply, media, live_signals, thread, edit_redact_search, search_crawler, scheduled_send, restore_cleanup, link_preview, cache_restore; got {other}"
            )),
        }
    }

    fn should_run_stage(self, stage: QaStage) -> bool {
        match self {
            Self::All => !matches!(
                stage,
                QaStage::TimelineReconnect | QaStage::TimelineStress | QaStage::DeviceCleanup
            ),
            Self::Safety => matches!(stage, QaStage::Safety),
            Self::LoginSync => matches!(stage, QaStage::Safety | QaStage::LoginSync),
            Self::SessionStatus => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::SessionStatus
            ),
            Self::CredentialHealth => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::CredentialHealth
            ),
            Self::NativeAttention => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::NativeAttention
            ),
            Self::EncryptionDebug => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::EncryptionDebug
            ),
            Self::E2eeTrust => {
                matches!(
                    stage,
                    QaStage::Safety | QaStage::LoginSync | QaStage::E2eeTrust
                )
            }
            Self::DeviceCleanup => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::DeviceCleanup
            ),
            Self::GateRestore => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::GateRestore
            ),
            Self::GateNegative => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::GateNegative
            ),
            Self::GateNoProof => matches!(stage, QaStage::Safety | QaStage::GateNoProof),
            Self::InvitesDm => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::InvitesDm
            ),
            Self::RoomSpace => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::RoomSpace
            ),
            Self::Directory => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::Directory
            ),
            Self::RoomManagement => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::RoomSpace | QaStage::RoomManagement
            ),
            Self::RoomPeopleProjection => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::RoomPeopleProjection
            ),
            Self::Timeline => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::RoomSpace | QaStage::Timeline
            ),
            Self::TimelineReconnect => {
                matches!(stage, QaStage::Safety | QaStage::TimelineReconnect)
            }
            Self::TimelineStress => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::TimelineStress
            ),
            Self::Activity => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Activity
            ),
            Self::Composer => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Composer
            ),
            Self::Reply => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Composer
                    | QaStage::Reply
            ),
            Self::Media => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Media
            ),
            Self::LiveSignals => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::LiveSignals
            ),
            Self::Thread => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Reply
                    | QaStage::Thread
            ),
            Self::EditRedactSearch => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::EditRedactSearch
            ),
            Self::SearchCrawler => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::EditRedactSearch
                    | QaStage::SearchCrawler
            ),
            Self::ScheduledSend => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::ScheduledSend
            ),
            Self::SendQueue => matches!(
                stage,
                QaStage::Safety | QaStage::LoginSync | QaStage::SendQueue
            ),
            Self::RestoreCleanup => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::EditRedactSearch
                    | QaStage::RestoreCleanup
            ),
            Self::LinkPreview => matches!(
                stage,
                QaStage::Safety
                    | QaStage::LoginSync
                    | QaStage::RoomSpace
                    | QaStage::Timeline
                    | QaStage::Composer
                    | QaStage::LinkPreview
            ),
            Self::CacheRestore => matches!(stage, QaStage::Safety | QaStage::CacheRestore),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn suppress_matrix_identifiers(self) -> bool {
        let _ = self;
        true
    }
}
fn scenario_preflight_error(scenario: QaScenario) -> Result<(), String> {
    let _ = scenario;
    Ok(())
}
fn tokens_for_stage(stage: QaStage) -> &'static [&'static str] {
    match stage {
        QaStage::Safety => &["safety=ok"],
        QaStage::LoginSync => &["login_sync=ok"],
        QaStage::SessionStatus => &[
            "session_status_checking=ok",
            "session_status_ready=ok",
            "session_status_device=ok",
            "session_status=ok",
        ],
        QaStage::CredentialHealth => &["credential_health=ok", "fail_closed=ok"],
        QaStage::NativeAttention => &[
            "notification_candidate=ok",
            "badge_state=ok",
            "suppress_focus=ok",
            "clear_badge=ok",
        ],
        QaStage::EncryptionDebug => &[
            "encryption_debug_cross_signing=ok",
            "encryption_debug_room=ok",
            "encryption_debug_recipient=ok",
            "force_new_outbound_session=ok",
            "share_index0_room_key=ok",
            "index0_not_consumed=ok",
            "encryption_debug_index_advanced=ok",
            "resend_index0_room_key=ok",
            "resend_index_unchanged=ok",
            "encryption_debug=ok",
        ],
        QaStage::E2eeTrust => &[
            "joined_room_restore=ok",
            "e2ee_second_device_decrypt=ok",
            "e2ee_multi_user_multi_device_decrypt=ok",
            "e2ee_unverified_peer_send_nonblocking=ok",
            "e2ee_blocked_device_withheld=ok",
            "e2ee_trust=ok",
        ],
        QaStage::DeviceCleanup => &[
            "device_cleanup_remote_first=ok",
            "device_cleanup_relogin_new_device=ok",
        ],
        QaStage::GateRestore => &[
            "gate_restore_bootstrapped=ok",
            "gate_restore_shutdown_complete=ok",
            "gate_restore_runtime_spawned=ok",
            "gate_restore_query_sent=ok",
            "gate_restore_query_result=ok",
            "gate_restore_restore_sent=ok",
            "gate_restore_restore_result=ok",
            "gate_restore_ready=ok",
            "gate_verified_restore=ok",
        ],
        QaStage::GateNegative => &[
            "gate_sas_mismatch_retryable=ok",
            "gate_sas_retry_ready=ok",
            "gate_sas_user_cancel_retryable=ok",
            "gate_sas_user_cancel_retry_ready=ok",
            "gate_sas_timeout_retryable=ok",
            "gate_sas_timeout_retry_ready=ok",
            "gate_recovery_invalid_retryable=ok",
            "gate_recovery_retry_ready=ok",
            "gate_recovery_cancel_retryable=ok",
            "gate_recovery_cancel_retry_ready=ok",
            "gate_trust_loss_locked=ok",
            "gate_trust_loss_commands_blocked=ok",
        ],
        QaStage::GateNoProof => &[
            "gate_no_proof_rejected=ok",
            "gate_no_proof_restart_signed_out=ok",
        ],
        QaStage::InvitesDm => &[
            "invite_recv=ok",
            "invite_accept=ok",
            "invite_decline=ok",
            "member_list=ok",
            "dm_start=ok",
            "dm_space_scope=ok",
        ],
        QaStage::RoomSpace => &["room_space=ok"],
        QaStage::Directory => &["directory_query=ok", "directory_join=ok"],
        QaStage::RoomManagement => &["room_settings=ok", "moderation=ok", "permission_guard=ok"],
        QaStage::RoomPeopleProjection => &[
            "room_people_joined_scope=ok",
            "room_people_alias_search=ok",
            "room_people_surface_isolation=ok",
            "room_people_membership_refresh=ok",
            "room_people_mentions_content=ok",
            "room_people_projection=ok",
        ],
        QaStage::Timeline => &["timeline=ok", "timeline_nav=ok", "hide_redacted=ok"],
        QaStage::TimelineReconnect => &[
            "timeline_reconnect_recv_after_reconnect=ok",
            "live_catchup_checkpoint=ok",
            "live_catchup_gap_repaired=ok",
            "timeline_reconnect=ok",
        ],
        QaStage::TimelineStress => &[
            "timeline_stress=ok",
            "stress_no_blank=ok",
            "stress_space_scope=ok",
        ],
        QaStage::Activity => &[
            "activity_recent=ok",
            "activity_unread=ok",
            "activity_resolution=ok",
            "activity_markread=ok",
        ],
        QaStage::Composer => &[
            "mention_send=ok",
            "markdown_send=ok",
            "slash_command=ok",
            "ime_guard=ok",
        ],
        QaStage::Reply => &[
            "reply=ok",
            "reply_quote=ok",
            "pin_event=ok",
            "pinned_state=ok",
            "unpin_event=ok",
        ],
        QaStage::Media => &[
            "send_media=ok",
            "media_caption=ok",
            "image_compress=ok",
            "upload_staging=ok",
            "media_gallery=ok",
            "recv_media=ok",
            "media_caption_edit=ok",
        ],
        QaStage::LiveSignals => &[
            "read_receipt=ok",
            "fully_read=ok",
            "typing=ok",
            "presence=ok",
            "live_signals=ok",
        ],
        QaStage::Thread => &[
            "thread_canonical=ok",
            "thread_summary=ok",
            "thread_recv=ok",
            "thread_paginate=end_reached",
        ],
        QaStage::EditRedactSearch => &["edit_redact_search=ok"],
        QaStage::SearchCrawler => &[
            "crawl_backfill=ok",
            "crawl_no_media_bytes=ok",
            "crawl_throttle=ok",
            "crawl_failure=ok",
        ],
        QaStage::ScheduledSend => &[
            "scheduled_capability=local_fallback",
            "scheduled_create=ok",
            "scheduled_reschedule=ok",
            "scheduled_cancel=ok",
            "scheduled_fire=ok",
        ],
        QaStage::SendQueue => &[
            "send_fail=ok",
            "resend=ok",
            "cancel_send=ok",
            "fifo=ok",
            "unsent_restart=ok",
            "display_projection_reset_fallbacks=0",
        ],
        QaStage::RestoreCleanup => &["restore_cleanup=ok"],
        QaStage::LinkPreview => &[
            "link_preview_global=ok",
            "link_preview_room=ok",
            "link_preview_e2ee_default=ok",
            "link_preview_hide=ok",
        ],
        QaStage::CacheRestore => &["cache_restore=ok"],
    }
}
fn implemented_final_tokens() -> Vec<&'static str> {
    vec![
        "safety=ok",
        "login_sync=ok",
        "session_status_checking=ok",
        "session_status_ready=ok",
        "session_status_device=ok",
        "session_status=ok",
        "credential_health=ok",
        "fail_closed=ok",
        "notification_candidate=ok",
        "badge_state=ok",
        "suppress_focus=ok",
        "clear_badge=ok",
        "invite_recv=ok",
        "invite_accept=ok",
        "invite_decline=ok",
        "member_list=ok",
        "dm_start=ok",
        "dm_space_scope=ok",
        "room_space=ok",
        "directory_query=ok",
        "directory_join=ok",
        "room_settings=ok",
        "moderation=ok",
        "permission_guard=ok",
        "timeline=ok",
        "timeline_nav=ok",
        "hide_redacted=ok",
        "activity_recent=ok",
        "activity_unread=ok",
        "activity_resolution=ok",
        "activity_markread=ok",
        "mention_send=ok",
        "markdown_send=ok",
        "slash_command=ok",
        "ime_guard=ok",
        "reply=ok",
        "reply_quote=ok",
        "pin_event=ok",
        "pinned_state=ok",
        "unpin_event=ok",
        "thread_canonical=ok",
        "thread_summary=ok",
        "thread_recv=ok",
        "thread_paginate=end_reached",
        "send_media=ok",
        "media_caption=ok",
        "image_compress=ok",
        "upload_staging=ok",
        "media_gallery=ok",
        "recv_media=ok",
        "media_caption_edit=ok",
        "read_receipt=ok",
        "fully_read=ok",
        "typing=ok",
        "presence=ok",
        "live_signals=ok",
        "edit_redact_search=ok",
        "crawl_backfill=ok",
        "crawl_no_media_bytes=ok",
        "crawl_throttle=ok",
        "crawl_failure=ok",
        "scheduled_capability=local_fallback",
        "scheduled_create=ok",
        "scheduled_reschedule=ok",
        "scheduled_cancel=ok",
        "scheduled_fire=ok",
        "send_fail=ok",
        "resend=ok",
        "cancel_send=ok",
        "fifo=ok",
        "unsent_restart=ok",
        "display_projection_reset_fallbacks=0",
        "joined_room_restore=ok",
        "e2ee_second_device_decrypt=ok",
        "e2ee_multi_user_multi_device_decrypt=ok",
        "e2ee_unverified_peer_send_nonblocking=ok",
        "e2ee_blocked_device_withheld=ok",
        "e2ee_trust=ok",
        "restore_cleanup=ok",
        "link_preview_global=ok",
        "link_preview_room=ok",
        "link_preview_e2ee_default=ok",
        "link_preview_hide=ok",
    ]
}
fn stages_for_scenario(scenario: QaScenario) -> Vec<QaStage> {
    match scenario {
        QaScenario::Safety => vec![QaStage::Safety],
        QaScenario::LoginSync => vec![QaStage::Safety, QaStage::LoginSync],
        QaScenario::SessionStatus => {
            vec![QaStage::Safety, QaStage::LoginSync, QaStage::SessionStatus]
        }
        QaScenario::CredentialHealth => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::CredentialHealth,
        ],
        QaScenario::NativeAttention => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::NativeAttention,
        ],
        QaScenario::EncryptionDebug => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::EncryptionDebug,
        ],
        QaScenario::E2eeTrust => {
            vec![QaStage::Safety, QaStage::LoginSync, QaStage::E2eeTrust]
        }
        QaScenario::DeviceCleanup => {
            vec![QaStage::Safety, QaStage::LoginSync, QaStage::DeviceCleanup]
        }
        QaScenario::GateRestore => vec![QaStage::Safety, QaStage::LoginSync, QaStage::GateRestore],
        QaScenario::GateNegative => {
            vec![QaStage::Safety, QaStage::LoginSync, QaStage::GateNegative]
        }
        QaScenario::GateNoProof => vec![QaStage::Safety, QaStage::GateNoProof],
        QaScenario::InvitesDm => {
            vec![QaStage::Safety, QaStage::LoginSync, QaStage::InvitesDm]
        }
        QaScenario::RoomSpace => vec![QaStage::Safety, QaStage::LoginSync, QaStage::RoomSpace],
        QaScenario::Directory => vec![QaStage::Safety, QaStage::LoginSync, QaStage::Directory],
        QaScenario::RoomManagement => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::RoomManagement,
        ],
        QaScenario::RoomPeopleProjection => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::RoomPeopleProjection,
        ],
        QaScenario::Timeline => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
        ],
        QaScenario::TimelineReconnect => vec![QaStage::Safety, QaStage::TimelineReconnect],
        QaScenario::TimelineStress => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::TimelineStress,
        ],
        QaScenario::Activity => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Activity,
        ],
        QaScenario::Composer => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Composer,
        ],
        QaScenario::Reply => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Composer,
            QaStage::Reply,
        ],
        QaScenario::Media => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Media,
        ],
        QaScenario::LiveSignals => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::LiveSignals,
        ],
        QaScenario::Thread => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Reply,
            QaStage::Thread,
        ],
        QaScenario::EditRedactSearch => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::EditRedactSearch,
        ],
        QaScenario::SearchCrawler => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::EditRedactSearch,
            QaStage::SearchCrawler,
        ],
        QaScenario::ScheduledSend => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::ScheduledSend,
        ],
        QaScenario::SendQueue => vec![QaStage::Safety, QaStage::LoginSync, QaStage::SendQueue],
        QaScenario::RestoreCleanup => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::EditRedactSearch,
            QaStage::RestoreCleanup,
        ],
        QaScenario::LinkPreview => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::RoomSpace,
            QaStage::Timeline,
            QaStage::Composer,
            QaStage::LinkPreview,
        ],
        QaScenario::CacheRestore => vec![QaStage::Safety, QaStage::CacheRestore],
        QaScenario::All => vec![
            QaStage::Safety,
            QaStage::LoginSync,
            QaStage::SessionStatus,
            QaStage::CredentialHealth,
            QaStage::NativeAttention,
            QaStage::InvitesDm,
            QaStage::RoomSpace,
            QaStage::Directory,
            QaStage::RoomManagement,
            QaStage::RoomPeopleProjection,
            QaStage::Timeline,
            QaStage::Activity,
            QaStage::Composer,
            QaStage::Reply,
            QaStage::Media,
            QaStage::LiveSignals,
            QaStage::Thread,
            QaStage::EditRedactSearch,
            QaStage::SearchCrawler,
            QaStage::ScheduledSend,
            QaStage::SendQueue,
            QaStage::E2eeTrust,
            QaStage::RestoreCleanup,
            QaStage::LinkPreview,
        ],
    }
}
fn final_tokens_for_scenario(scenario: QaScenario) -> Vec<&'static str> {
    match scenario {
        QaScenario::Safety => vec!["safety=ok"],
        QaScenario::LoginSync => {
            let mut tokens = stages_for_scenario(scenario)
                .into_iter()
                .flat_map(|stage| tokens_for_stage(stage).iter().copied())
                .collect::<Vec<_>>();
            tokens.push("restore_cleanup=ok");
            tokens.dedup();
            tokens
        }
        QaScenario::RoomSpace
        | QaScenario::Directory
        | QaScenario::RoomManagement
        | QaScenario::RoomPeopleProjection
        | QaScenario::SessionStatus
        | QaScenario::CredentialHealth
        | QaScenario::NativeAttention
        | QaScenario::EncryptionDebug
        | QaScenario::E2eeTrust
        | QaScenario::InvitesDm
        | QaScenario::Timeline
        | QaScenario::TimelineStress
        | QaScenario::Activity
        | QaScenario::Composer
        | QaScenario::Reply
        | QaScenario::Media
        | QaScenario::LiveSignals
        | QaScenario::Thread
        | QaScenario::EditRedactSearch
        | QaScenario::SearchCrawler
        | QaScenario::ScheduledSend
        | QaScenario::SendQueue
        | QaScenario::RestoreCleanup
        | QaScenario::LinkPreview => {
            let mut tokens = stages_for_scenario(scenario)
                .into_iter()
                .flat_map(|stage| tokens_for_stage(stage).iter().copied())
                .collect::<Vec<_>>();
            tokens.push("restore_cleanup=ok");
            tokens.dedup();
            tokens
        }
        QaScenario::TimelineReconnect
        | QaScenario::CacheRestore
        | QaScenario::DeviceCleanup
        | QaScenario::GateRestore
        | QaScenario::GateNegative
        | QaScenario::GateNoProof => stages_for_scenario(scenario)
            .into_iter()
            .flat_map(|stage| tokens_for_stage(stage).iter().copied())
            .collect(),
        QaScenario::All => implemented_final_tokens(),
    }
}
fn scenario_report(server_kind: &str, scenario: QaScenario) -> String {
    format!(
        "server={server_kind}\n{}",
        final_tokens_for_scenario(scenario).join("\n")
    )
}
struct QaConfig {
    homeserver: String,
    server_name: String,
    server_kind: String,
    user_a: String,
    password_a: String,
    user_b: String,
    password_b: String,
    user_c: Option<String>,
    /// Identity reset changes cross-signing identity for the account. Keep it
    /// opt-in so real-account QA cannot accidentally invalidate other devices.
    allow_identity_reset: bool,
}
impl QaConfig {
    fn from_env() -> Result<Self, String> {
        Ok(Self {
            homeserver: env_required(ENV_HOMESERVER)?,
            server_name: env_required(ENV_SERVER_NAME)?,
            server_kind: std::env::var(ENV_SERVER_KIND).unwrap_or_else(|_| "local".to_owned()),
            user_a: env_required(ENV_USER_A)?,
            password_a: env_required(ENV_PASSWORD_A)?,
            user_b: env_required(ENV_USER_B)?,
            password_b: env_required(ENV_PASSWORD_B)?,
            user_c: std::env::var(ENV_USER_C).ok(),
            allow_identity_reset: env_flag_enabled(ENV_ALLOW_IDENTITY_RESET)?,
        })
    }

    fn dm_scope_control_user_id(&self) -> Result<String, String> {
        let user_c = self.user_c.as_deref().ok_or_else(|| {
            format!("{ENV_USER_C} is required for the invites_dm dm_space_scope check")
        })?;
        Ok(format!("@{}:{}", user_c, self.server_name))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TimelineStressConfig {
    space_count: usize,
    rooms_per_space: usize,
    messages_per_room: usize,
    replay_existing: bool,
}
impl TimelineStressConfig {
    fn from_env() -> Result<Self, String> {
        Ok(Self {
            space_count: bounded_usize_env(
                ENV_STRESS_SPACE_COUNT,
                DEFAULT_STRESS_SPACE_COUNT,
                MAX_STRESS_SPACE_COUNT,
            )?,
            rooms_per_space: bounded_usize_env(
                ENV_STRESS_ROOMS_PER_SPACE,
                DEFAULT_STRESS_ROOMS_PER_SPACE,
                MAX_STRESS_ROOMS_PER_SPACE,
            )?,
            messages_per_room: bounded_usize_env(
                ENV_STRESS_MESSAGES_PER_ROOM,
                DEFAULT_STRESS_MESSAGES_PER_ROOM,
                MAX_STRESS_MESSAGES_PER_ROOM,
            )?,
            replay_existing: env_flag_enabled(ENV_STRESS_REPLAY_EXISTING)?,
        })
    }

    fn total_rooms(self) -> usize {
        self.space_count * self.rooms_per_space
    }

    fn total_messages(self) -> usize {
        self.total_rooms() * self.messages_per_room + self.empty_formatted_probe_count()
    }

    fn empty_formatted_probe_count(self) -> usize {
        usize::from(self.total_rooms() > 0)
    }
}
fn bounded_usize_env(name: &str, default: usize, max: usize) -> Result<usize, String> {
    let Ok(value) = std::env::var(name) else {
        return Ok(default);
    };
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer no greater than {max}"))?;
    if parsed == 0 || parsed > max {
        return Err(format!(
            "{name} must be a positive integer no greater than {max}"
        ));
    }
    Ok(parsed)
}
fn env_flag_enabled(name: &str) -> Result<bool, String> {
    match std::env::var(name) {
        Ok(value) => parse_env_flag(name, &value),
        Err(_) => Ok(false),
    }
}
fn parse_env_flag(name: &str, value: &str) -> Result<bool, String> {
    if value == "1" || value.eq_ignore_ascii_case("true") {
        return Ok(true);
    }
    if value == "0" || value.eq_ignore_ascii_case("false") || value.is_empty() {
        return Ok(false);
    }
    Err(format!(
        "{name} must be 1, true, 0, false, or unset; got {value}"
    ))
}
fn env_required(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is required"))
}

#[test]
fn parses_all_scenarios_from_env_value_including_directory() {
    assert_eq!(QaScenario::from_env_value("all").unwrap(), QaScenario::All);
    assert_eq!(
        QaScenario::from_env_value("safety").unwrap(),
        QaScenario::Safety
    );
    assert_eq!(
        QaScenario::from_env_value("login_sync").unwrap(),
        QaScenario::LoginSync
    );
    assert_eq!(
        QaScenario::from_env_value("session_status").unwrap(),
        QaScenario::SessionStatus
    );
    assert_eq!(
        QaScenario::from_env_value("room_space").unwrap(),
        QaScenario::RoomSpace
    );
    assert_eq!(
        QaScenario::from_env_value("directory").unwrap(),
        QaScenario::Directory
    );
    assert_eq!(
        QaScenario::from_env_value("room_management").unwrap(),
        QaScenario::RoomManagement
    );
    assert_eq!(
        QaScenario::from_env_value("invites_dm").unwrap(),
        QaScenario::InvitesDm
    );
    assert_eq!(
        QaScenario::from_env_value("timeline").unwrap(),
        QaScenario::Timeline
    );
    assert_eq!(
        QaScenario::from_env_value("timeline_reconnect").unwrap(),
        QaScenario::TimelineReconnect
    );
    assert_eq!(
        QaScenario::from_env_value("activity").unwrap(),
        QaScenario::Activity
    );
    assert_eq!(
        QaScenario::from_env_value("credential_health").unwrap(),
        QaScenario::CredentialHealth
    );
    assert_eq!(
        QaScenario::from_env_value("native_attention").unwrap(),
        QaScenario::NativeAttention
    );
    assert_eq!(
        QaScenario::from_env_value("reply").unwrap(),
        QaScenario::Reply
    );
    assert_eq!(
        QaScenario::from_env_value("composer").unwrap(),
        QaScenario::Composer
    );
    assert_eq!(
        QaScenario::from_env_value("media").unwrap(),
        QaScenario::Media
    );
    assert_eq!(
        QaScenario::from_env_value("live_signals").unwrap(),
        QaScenario::LiveSignals
    );
    assert_eq!(
        QaScenario::from_env_value("thread").unwrap(),
        QaScenario::Thread
    );
    assert_eq!(
        QaScenario::from_env_value("edit_redact_search").unwrap(),
        QaScenario::EditRedactSearch
    );
    assert_eq!(
        QaScenario::from_env_value("search_crawler").unwrap(),
        QaScenario::SearchCrawler
    );
    assert_eq!(
        QaScenario::from_env_value("scheduled_send").unwrap(),
        QaScenario::ScheduledSend
    );
    assert_eq!(
        QaScenario::from_env_value("restore_cleanup").unwrap(),
        QaScenario::RestoreCleanup
    );
    assert_eq!(
        QaScenario::from_env_value("send_queue").unwrap(),
        QaScenario::SendQueue
    );
    assert_eq!(
        QaScenario::from_env_value("e2ee_trust").unwrap(),
        QaScenario::E2eeTrust
    );
    assert_eq!(
        QaScenario::from_env_value("link_preview").unwrap(),
        QaScenario::LinkPreview
    );
    assert_eq!(
        QaScenario::from_env_value("timeline_stress").unwrap(),
        QaScenario::TimelineStress
    );
}
#[test]
fn rejects_unknown_scenario_names() {
    let error = QaScenario::from_env_value("unknown").unwrap_err();

    assert!(error.contains("KOUSHI_QA_SCENARIO"));
    assert!(error.contains("unknown"));
}
#[test]
fn supported_scenarios_are_allowed_by_preflight() {
    for scenario in [
        QaScenario::Safety,
        QaScenario::LoginSync,
        QaScenario::SessionStatus,
        QaScenario::CredentialHealth,
        QaScenario::NativeAttention,
        QaScenario::RoomSpace,
        QaScenario::Directory,
        QaScenario::RoomManagement,
        QaScenario::InvitesDm,
        QaScenario::Timeline,
        QaScenario::TimelineReconnect,
        QaScenario::TimelineStress,
        QaScenario::Reply,
        QaScenario::Composer,
        QaScenario::Media,
        QaScenario::LiveSignals,
        QaScenario::Thread,
        QaScenario::EditRedactSearch,
        QaScenario::SearchCrawler,
        QaScenario::ScheduledSend,
        QaScenario::SendQueue,
        QaScenario::RestoreCleanup,
        QaScenario::E2eeTrust,
        QaScenario::LinkPreview,
    ] {
        scenario_preflight_error(scenario).unwrap();
    }
}
#[test]
fn session_status_scenario_runs_after_login_and_reports_only_safe_tokens() {
    assert_eq!(
        stages_for_scenario(QaScenario::SessionStatus),
        [QaStage::Safety, QaStage::LoginSync, QaStage::SessionStatus]
    );
    let report = scenario_report("local", QaScenario::SessionStatus);
    assert!(report.contains("session_status_checking=ok"));
    assert!(report.contains("session_status_ready=ok"));
    assert!(report.contains("session_status_device=ok"));
    assert!(report.contains("session_status=ok"));
    assert!(!report.contains('@'));
    assert!(!report.contains("http"));
    assert!(!report.contains(DEVICE_A));
}
#[test]
fn thread_is_allowed_by_preflight() {
    scenario_preflight_error(QaScenario::Thread).unwrap();
}
#[test]
fn all_core_qa_scenarios_suppress_matrix_identifiers() {
    for scenario in [
        QaScenario::All,
        QaScenario::Safety,
        QaScenario::LoginSync,
        QaScenario::SessionStatus,
        QaScenario::CredentialHealth,
        QaScenario::NativeAttention,
        QaScenario::E2eeTrust,
        QaScenario::InvitesDm,
        QaScenario::RoomSpace,
        QaScenario::Directory,
        QaScenario::RoomManagement,
        QaScenario::Timeline,
        QaScenario::TimelineReconnect,
        QaScenario::TimelineStress,
        QaScenario::Activity,
        QaScenario::Composer,
        QaScenario::Reply,
        QaScenario::Media,
        QaScenario::LiveSignals,
        QaScenario::Thread,
        QaScenario::EditRedactSearch,
        QaScenario::SearchCrawler,
        QaScenario::ScheduledSend,
        QaScenario::SendQueue,
        QaScenario::RestoreCleanup,
        QaScenario::LinkPreview,
    ] {
        assert!(
            scenario.suppress_matrix_identifiers(),
            "{scenario:?} should keep core QA stdout private-data-free"
        );
    }
}
#[test]
fn staged_scenarios_stop_after_their_requested_stage() {
    assert!(QaScenario::Safety.should_run_stage(QaStage::Safety));
    assert!(!QaScenario::Safety.should_run_stage(QaStage::LoginSync));

    assert!(QaScenario::LoginSync.should_run_stage(QaStage::Safety));
    assert!(QaScenario::LoginSync.should_run_stage(QaStage::LoginSync));
    assert!(!QaScenario::LoginSync.should_run_stage(QaStage::RoomSpace));
    assert!(!QaScenario::LoginSync.should_run_stage(QaStage::InvitesDm));

    assert!(QaScenario::InvitesDm.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::InvitesDm.should_run_stage(QaStage::InvitesDm));
    assert!(!QaScenario::InvitesDm.should_run_stage(QaStage::RoomSpace));

    assert!(QaScenario::RoomSpace.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::RoomSpace.should_run_stage(QaStage::RoomSpace));
    assert!(!QaScenario::RoomSpace.should_run_stage(QaStage::InvitesDm));
    assert!(!QaScenario::RoomSpace.should_run_stage(QaStage::E2eeTrust));
    assert!(!QaScenario::RoomSpace.should_run_stage(QaStage::Timeline));

    assert!(QaScenario::Timeline.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Timeline.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::Timeline.should_run_stage(QaStage::Timeline));
    assert!(!QaScenario::Timeline.should_run_stage(QaStage::E2eeTrust));
    assert!(!QaScenario::Timeline.should_run_stage(QaStage::Activity));
    assert!(!QaScenario::Timeline.should_run_stage(QaStage::Reply));
    assert!(!QaScenario::Timeline.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::TimelineReconnect.should_run_stage(QaStage::Safety));
    assert!(QaScenario::TimelineReconnect.should_run_stage(QaStage::TimelineReconnect));
    assert!(!QaScenario::TimelineReconnect.should_run_stage(QaStage::LoginSync));
    assert!(!QaScenario::TimelineReconnect.should_run_stage(QaStage::Timeline));
    assert!(!QaScenario::TimelineReconnect.should_run_stage(QaStage::SendQueue));

    assert!(QaScenario::TimelineStress.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::TimelineStress.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::TimelineStress.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::TimelineStress.should_run_stage(QaStage::TimelineStress));
    assert!(!QaScenario::TimelineStress.should_run_stage(QaStage::Activity));
    assert!(!QaScenario::TimelineStress.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::Activity.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Activity.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::Activity.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::Activity.should_run_stage(QaStage::Activity));
    assert!(QaScenario::Activity.suppress_matrix_identifiers());
    assert!(!QaScenario::Activity.should_run_stage(QaStage::Composer));
    assert!(!QaScenario::Activity.should_run_stage(QaStage::Reply));

    assert!(QaScenario::CredentialHealth.should_run_stage(QaStage::Safety));
    assert!(QaScenario::CredentialHealth.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::CredentialHealth.should_run_stage(QaStage::CredentialHealth));
    assert!(!QaScenario::CredentialHealth.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::CredentialHealth.suppress_matrix_identifiers());

    assert!(QaScenario::NativeAttention.should_run_stage(QaStage::Safety));
    assert!(QaScenario::NativeAttention.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::NativeAttention.should_run_stage(QaStage::NativeAttention));
    assert!(!QaScenario::NativeAttention.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::NativeAttention.suppress_matrix_identifiers());

    assert!(QaScenario::Reply.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Reply.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::Reply.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::Reply.should_run_stage(QaStage::Reply));
    assert!(!QaScenario::Reply.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::Media.should_run_stage(QaStage::Safety));
    assert!(QaScenario::Media.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Media.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::Media.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::Media.should_run_stage(QaStage::Media));
    assert!(!QaScenario::Media.should_run_stage(QaStage::LiveSignals));
    assert!(!QaScenario::Media.should_run_stage(QaStage::Thread));
    assert!(!QaScenario::Media.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::LiveSignals.should_run_stage(QaStage::Safety));
    assert!(QaScenario::LiveSignals.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::LiveSignals.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::LiveSignals.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::LiveSignals.should_run_stage(QaStage::LiveSignals));
    assert!(!QaScenario::LiveSignals.should_run_stage(QaStage::Media));
    assert!(!QaScenario::LiveSignals.should_run_stage(QaStage::Thread));
    assert!(!QaScenario::LiveSignals.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::Thread.should_run_stage(QaStage::Safety));
    assert!(QaScenario::Thread.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Thread.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::Thread.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::Thread.should_run_stage(QaStage::Reply));
    assert!(QaScenario::Thread.should_run_stage(QaStage::Thread));
    assert!(!QaScenario::Thread.should_run_stage(QaStage::Media));
    assert!(!QaScenario::Thread.should_run_stage(QaStage::EditRedactSearch));

    assert!(QaScenario::Directory.should_run_stage(QaStage::Safety));
    assert!(QaScenario::Directory.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::Directory.should_run_stage(QaStage::Directory));
    assert!(!QaScenario::Directory.should_run_stage(QaStage::Timeline));
    assert!(!QaScenario::Directory.should_run_stage(QaStage::Reply));

    assert!(QaScenario::RoomManagement.should_run_stage(QaStage::Safety));
    assert!(QaScenario::RoomManagement.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::RoomManagement.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::RoomManagement.should_run_stage(QaStage::RoomManagement));
    assert!(!QaScenario::RoomManagement.should_run_stage(QaStage::Timeline));
    assert!(!QaScenario::RoomManagement.should_run_stage(QaStage::Reply));

    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::Safety));
    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::Timeline));
    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::Composer));
    assert!(QaScenario::LinkPreview.should_run_stage(QaStage::LinkPreview));
    assert!(!QaScenario::LinkPreview.should_run_stage(QaStage::Reply));
    assert!(QaScenario::LinkPreview.suppress_matrix_identifiers());

    assert!(QaScenario::All.should_run_stage(QaStage::Safety));
    assert!(QaScenario::All.should_run_stage(QaStage::LoginSync));
    assert!(QaScenario::All.should_run_stage(QaStage::E2eeTrust));
    assert!(QaScenario::All.should_run_stage(QaStage::InvitesDm));
    assert!(QaScenario::All.should_run_stage(QaStage::RoomSpace));
    assert!(QaScenario::All.should_run_stage(QaStage::Directory));
    assert!(QaScenario::All.should_run_stage(QaStage::RoomManagement));
    assert!(QaScenario::All.should_run_stage(QaStage::Timeline));
    assert!(!QaScenario::All.should_run_stage(QaStage::TimelineReconnect));
    assert!(!QaScenario::All.should_run_stage(QaStage::TimelineStress));
    assert!(QaScenario::All.should_run_stage(QaStage::Activity));
    assert!(QaScenario::All.should_run_stage(QaStage::CredentialHealth));
    assert!(QaScenario::All.should_run_stage(QaStage::Reply));
    assert!(QaScenario::All.should_run_stage(QaStage::Media));
    assert!(QaScenario::All.should_run_stage(QaStage::LiveSignals));
    assert!(QaScenario::All.should_run_stage(QaStage::Thread));
    assert!(QaScenario::All.should_run_stage(QaStage::EditRedactSearch));
    assert!(QaScenario::All.should_run_stage(QaStage::ScheduledSend));
    assert!(QaScenario::All.should_run_stage(QaStage::SendQueue));
    assert!(QaScenario::All.should_run_stage(QaStage::RestoreCleanup));
    assert!(QaScenario::All.should_run_stage(QaStage::LinkPreview));
}
#[test]
fn implemented_final_tokens_include_thread() {
    assert_eq!(
        &implemented_final_tokens()[..],
        &[
            "safety=ok",
            "login_sync=ok",
            "session_status_checking=ok",
            "session_status_ready=ok",
            "session_status_device=ok",
            "session_status=ok",
            "credential_health=ok",
            "fail_closed=ok",
            "notification_candidate=ok",
            "badge_state=ok",
            "suppress_focus=ok",
            "clear_badge=ok",
            "invite_recv=ok",
            "invite_accept=ok",
            "invite_decline=ok",
            "member_list=ok",
            "dm_start=ok",
            "dm_space_scope=ok",
            "room_space=ok",
            "directory_query=ok",
            "directory_join=ok",
            "room_settings=ok",
            "moderation=ok",
            "permission_guard=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "activity_recent=ok",
            "activity_unread=ok",
            "activity_resolution=ok",
            "activity_markread=ok",
            "mention_send=ok",
            "markdown_send=ok",
            "slash_command=ok",
            "ime_guard=ok",
            "reply=ok",
            "reply_quote=ok",
            "pin_event=ok",
            "pinned_state=ok",
            "unpin_event=ok",
            "thread_canonical=ok",
            "thread_summary=ok",
            "thread_recv=ok",
            "thread_paginate=end_reached",
            "send_media=ok",
            "media_caption=ok",
            "image_compress=ok",
            "upload_staging=ok",
            "media_gallery=ok",
            "recv_media=ok",
            "media_caption_edit=ok",
            "read_receipt=ok",
            "fully_read=ok",
            "typing=ok",
            "presence=ok",
            "live_signals=ok",
            "edit_redact_search=ok",
            "crawl_backfill=ok",
            "crawl_no_media_bytes=ok",
            "crawl_throttle=ok",
            "crawl_failure=ok",
            "scheduled_capability=local_fallback",
            "scheduled_create=ok",
            "scheduled_reschedule=ok",
            "scheduled_cancel=ok",
            "scheduled_fire=ok",
            "send_fail=ok",
            "resend=ok",
            "cancel_send=ok",
            "fifo=ok",
            "unsent_restart=ok",
            "display_projection_reset_fallbacks=0",
            "joined_room_restore=ok",
            "e2ee_second_device_decrypt=ok",
            "e2ee_multi_user_multi_device_decrypt=ok",
            "e2ee_unverified_peer_send_nonblocking=ok",
            "e2ee_blocked_device_withheld=ok",
            "e2ee_trust=ok",
            "restore_cleanup=ok",
            "link_preview_global=ok",
            "link_preview_room=ok",
            "link_preview_e2ee_default=ok",
            "link_preview_hide=ok",
        ][..]
    );
}
#[test]
fn parse_env_flag_accepts_only_explicit_boolean_values() {
    for (value, expected) in [
        ("1", true),
        ("true", true),
        ("TRUE", true),
        ("0", false),
        ("false", false),
        ("FALSE", false),
        ("", false),
    ] {
        assert_eq!(parse_env_flag("QA_FLAG", value), Ok(expected));
    }

    assert!(parse_env_flag("QA_FLAG", "yes").is_err());
}
#[test]
fn final_tokens_follow_the_requested_scenario_including_composer() {
    assert_eq!(final_tokens_for_scenario(QaScenario::Safety), ["safety=ok"]);
    assert_eq!(
        final_tokens_for_scenario(QaScenario::LoginSync),
        ["safety=ok", "login_sync=ok", "restore_cleanup=ok"]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Composer),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "mention_send=ok",
            "markdown_send=ok",
            "slash_command=ok",
            "ime_guard=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::RoomSpace),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "restore_cleanup=ok"
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::InvitesDm),
        [
            "safety=ok",
            "login_sync=ok",
            "invite_recv=ok",
            "invite_accept=ok",
            "invite_decline=ok",
            "member_list=ok",
            "dm_start=ok",
            "dm_space_scope=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Timeline),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::TimelineReconnect),
        [
            "safety=ok",
            "timeline_reconnect_recv_after_reconnect=ok",
            "live_catchup_checkpoint=ok",
            "live_catchup_gap_repaired=ok",
            "timeline_reconnect=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::TimelineStress),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "timeline_stress=ok",
            "stress_no_blank=ok",
            "stress_space_scope=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Activity),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "activity_recent=ok",
            "activity_unread=ok",
            "activity_resolution=ok",
            "activity_markread=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::CredentialHealth),
        [
            "safety=ok",
            "login_sync=ok",
            "credential_health=ok",
            "fail_closed=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::NativeAttention),
        [
            "safety=ok",
            "login_sync=ok",
            "notification_candidate=ok",
            "badge_state=ok",
            "suppress_focus=ok",
            "clear_badge=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Directory),
        [
            "safety=ok",
            "login_sync=ok",
            "directory_query=ok",
            "directory_join=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Reply),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "mention_send=ok",
            "markdown_send=ok",
            "slash_command=ok",
            "ime_guard=ok",
            "reply=ok",
            "reply_quote=ok",
            "pin_event=ok",
            "pinned_state=ok",
            "unpin_event=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Media),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "send_media=ok",
            "media_caption=ok",
            "image_compress=ok",
            "upload_staging=ok",
            "media_gallery=ok",
            "recv_media=ok",
            "media_caption_edit=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::LiveSignals),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "read_receipt=ok",
            "fully_read=ok",
            "typing=ok",
            "presence=ok",
            "live_signals=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::Thread),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "reply=ok",
            "reply_quote=ok",
            "pin_event=ok",
            "pinned_state=ok",
            "unpin_event=ok",
            "thread_canonical=ok",
            "thread_summary=ok",
            "thread_recv=ok",
            "thread_paginate=end_reached",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::EditRedactSearch),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "edit_redact_search=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::SearchCrawler),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "edit_redact_search=ok",
            "crawl_backfill=ok",
            "crawl_no_media_bytes=ok",
            "crawl_throttle=ok",
            "crawl_failure=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::ScheduledSend),
        [
            "safety=ok",
            "login_sync=ok",
            "room_space=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "scheduled_capability=local_fallback",
            "scheduled_create=ok",
            "scheduled_reschedule=ok",
            "scheduled_cancel=ok",
            "scheduled_fire=ok",
            "restore_cleanup=ok",
        ]
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::All),
        implemented_final_tokens()
    );
    assert_eq!(
        final_tokens_for_scenario(QaScenario::E2eeTrust),
        [
            "safety=ok",
            "login_sync=ok",
            "joined_room_restore=ok",
            "e2ee_second_device_decrypt=ok",
            "e2ee_multi_user_multi_device_decrypt=ok",
            "e2ee_unverified_peer_send_nonblocking=ok",
            "e2ee_blocked_device_withheld=ok",
            "e2ee_trust=ok",
            "restore_cleanup=ok",
        ]
    );
}
#[test]
fn implemented_final_tokens_include_safety() {
    assert_eq!(
        &implemented_final_tokens()[..],
        &[
            "safety=ok",
            "login_sync=ok",
            "session_status_checking=ok",
            "session_status_ready=ok",
            "session_status_device=ok",
            "session_status=ok",
            "credential_health=ok",
            "fail_closed=ok",
            "notification_candidate=ok",
            "badge_state=ok",
            "suppress_focus=ok",
            "clear_badge=ok",
            "invite_recv=ok",
            "invite_accept=ok",
            "invite_decline=ok",
            "member_list=ok",
            "dm_start=ok",
            "dm_space_scope=ok",
            "room_space=ok",
            "directory_query=ok",
            "directory_join=ok",
            "room_settings=ok",
            "moderation=ok",
            "permission_guard=ok",
            "timeline=ok",
            "timeline_nav=ok",
            "hide_redacted=ok",
            "activity_recent=ok",
            "activity_unread=ok",
            "activity_resolution=ok",
            "activity_markread=ok",
            "mention_send=ok",
            "markdown_send=ok",
            "slash_command=ok",
            "ime_guard=ok",
            "reply=ok",
            "reply_quote=ok",
            "pin_event=ok",
            "pinned_state=ok",
            "unpin_event=ok",
            "thread_canonical=ok",
            "thread_summary=ok",
            "thread_recv=ok",
            "thread_paginate=end_reached",
            "send_media=ok",
            "media_caption=ok",
            "image_compress=ok",
            "upload_staging=ok",
            "media_gallery=ok",
            "recv_media=ok",
            "media_caption_edit=ok",
            "read_receipt=ok",
            "fully_read=ok",
            "typing=ok",
            "presence=ok",
            "live_signals=ok",
            "edit_redact_search=ok",
            "crawl_backfill=ok",
            "crawl_no_media_bytes=ok",
            "crawl_throttle=ok",
            "crawl_failure=ok",
            "scheduled_capability=local_fallback",
            "scheduled_create=ok",
            "scheduled_reschedule=ok",
            "scheduled_cancel=ok",
            "scheduled_fire=ok",
            "send_fail=ok",
            "resend=ok",
            "cancel_send=ok",
            "fifo=ok",
            "unsent_restart=ok",
            "display_projection_reset_fallbacks=0",
            "joined_room_restore=ok",
            "e2ee_second_device_decrypt=ok",
            "e2ee_multi_user_multi_device_decrypt=ok",
            "e2ee_unverified_peer_send_nonblocking=ok",
            "e2ee_blocked_device_withheld=ok",
            "e2ee_trust=ok",
            "restore_cleanup=ok",
            "link_preview_global=ok",
            "link_preview_room=ok",
            "link_preview_e2ee_default=ok",
            "link_preview_hide=ok",
        ][..]
    );
}
