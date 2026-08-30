use std::fmt;

use super::{
    ReportKind, RoomKeyRequestStage, RoomKeyRequestWithheldCode, timeline_projection_own_user_id,
};
use crate::ids::{RequestId, TimelineKey};
use koushi_state::{
    AppState, DirectoryQuery, DirectoryRoomPreview, DirectoryRoomSummary, InviteDestinationResult,
    PinnedEvent, RoomModerationAction, RoomSettingsSnapshot, RoomTagKind, SpaceMemberInviteOutcome,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum RoomEvent {
    RoomCreated {
        request_id: RequestId,
        room_id: String,
    },
    SpaceCreated {
        request_id: RequestId,
        space_id: String,
    },
    SpaceChildSet {
        request_id: RequestId,
        space_id: String,
        child_room_id: String,
    },
    UserInvited {
        request_id: RequestId,
        room_id: String,
        user_id: String,
    },
    SpaceMembersLoaded {
        request_id: RequestId,
        generation: u64,
        joined_count: usize,
        invited_count: usize,
        child_room_only_count: usize,
        incomplete_child_room_count: usize,
    },
    SpaceMemberInviteSettled {
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
        outcome: SpaceMemberInviteOutcome,
    },
    SpaceMemberInviteCancellationSettled {
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
        outcome: SpaceMemberInviteOutcome,
    },
    InviteBatchCompleted {
        request_id: RequestId,
        room_id: String,
        results: Vec<InviteDestinationResult>,
    },
    InviteAccepted {
        request_id: RequestId,
        room_id: String,
    },
    InviteDeclined {
        request_id: RequestId,
        room_id: String,
    },
    DirectMessageStarted {
        request_id: RequestId,
        room_id: String,
    },
    RoomJoined {
        request_id: RequestId,
        room_id: String,
    },
    RoomLeft {
        request_id: RequestId,
        room_id: String,
    },
    RoomForgotten {
        request_id: RequestId,
        room_id: String,
    },
    RoomTagSet {
        request_id: RequestId,
        room_id: String,
        tag: RoomTagKind,
    },
    RoomTagRemoved {
        request_id: RequestId,
        room_id: String,
        tag: RoomTagKind,
    },
    PinnedEventsUpdated {
        request_id: Option<RequestId>,
        room_id: String,
        pinned: Vec<PinnedEvent>,
    },
    PinEventCompleted {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    UnpinEventCompleted {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    DirectoryQueryCompleted {
        request_id: RequestId,
        query: DirectoryQuery,
        rooms: Vec<DirectoryRoomSummary>,
        next_batch: Option<String>,
    },
    DirectoryPreviewLoaded {
        request_id: RequestId,
        room: DirectoryRoomPreview,
    },
    RoomSettingsLoaded {
        request_id: RequestId,
        settings: RoomSettingsSnapshot,
    },
    RoomSettingUpdated {
        request_id: RequestId,
        settings: RoomSettingsSnapshot,
    },
    RoomMemberModerated {
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        action: RoomModerationAction,
    },
    RoomMemberRoleUpdated {
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        power_level: i64,
    },
    SpaceMemberRoleUpdateSettled {
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
        outcome: koushi_state::SpaceMemberRoleUpdateOutcome,
    },
    RoomKeyReshared {
        request_id: RequestId,
        room_id: String,
        outcome: RoomKeyReshareOutcome,
    },
    OutboundSessionForced {
        request_id: RequestId,
        room_id: String,
        outcome: EncryptionDebugOperationOutcome,
    },
    Index0RoomKeyShared {
        request_id: RequestId,
        room_id: String,
        outcome: EncryptionDebugOperationOutcome,
    },
    Index0RoomKeyResent {
        request_id: RequestId,
        room_id: String,
        outcome: EncryptionDebugOperationOutcome,
    },
    RoomKeyRequestStateChanged {
        key: TimelineKey,
        event_id: String,
        request_id: Option<RequestId>,
        stage: RoomKeyRequestStage,
        withheld_code: Option<RoomKeyRequestWithheldCode>,
    },
    /// Issue #450: a recognized-but-unavailable slash command (/join, /invite)
    /// was rejected for a composer target; the key names which composer should
    /// show the localized notice and the request id lets the Tauri submission
    /// waiter settle immediately instead of waiting out its timeout.
    ComposerSlashCommandRejected {
        key: TimelineKey,
        request_id: RequestId,
    },
    MarkedAsRead {
        request_id: RequestId,
        room_id: String,
    },
    MarkedAsUnread {
        request_id: RequestId,
        room_id: String,
        unread: bool,
    },
    RoomListUpdated,
    ReportCompleted {
        request_id: RequestId,
        kind: ReportKind,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoomKeyReshareOutcome {
    Sent {
        request_count: usize,
        recipient_count: usize,
        failed_recipient_count: usize,
    },
    NoSession,
    NoRecipients,
    StaleSession,
}

/// Closed outcome of a manual encryption-debug operation (issue #538).
/// Re-exported from koushi-state; tokens mirror the diagnostic allowlist and
/// the aggregate detail (own/peer buckets, claim outcome, elapsed) is
/// carried by the diagnostics only.
pub use koushi_state::EncryptionDebugOperationOutcome;

impl fmt::Debug for RoomEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RoomCreated { request_id, .. } => formatter
                .debug_struct("RoomCreated")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::SpaceCreated { request_id, .. } => formatter
                .debug_struct("SpaceCreated")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .finish(),
            Self::SpaceChildSet { request_id, .. } => formatter
                .debug_struct("SpaceChildSet")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("child_room_id", &"RoomId(..)")
                .finish(),
            Self::UserInvited { request_id, .. } => formatter
                .debug_struct("UserInvited")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .finish(),
            Self::SpaceMembersLoaded {
                request_id,
                generation,
                joined_count,
                invited_count,
                child_room_only_count,
                incomplete_child_room_count,
            } => formatter
                .debug_struct("SpaceMembersLoaded")
                .field("request_id", request_id)
                .field("generation", generation)
                .field("joined_count", joined_count)
                .field("invited_count", invited_count)
                .field("child_room_only_count", child_room_only_count)
                .field("incomplete_child_room_count", incomplete_child_room_count)
                .finish(),
            Self::SpaceMemberInviteSettled {
                request_id,
                generation,
                outcome,
                ..
            } => formatter
                .debug_struct("SpaceMemberInviteSettled")
                .field("request_id", request_id)
                .field("generation", generation)
                .field("outcome", outcome)
                .finish(),
            Self::SpaceMemberInviteCancellationSettled {
                request_id,
                generation,
                outcome,
                ..
            } => formatter
                .debug_struct("SpaceMemberInviteCancellationSettled")
                .field("request_id", request_id)
                .field("generation", generation)
                .field("outcome", outcome)
                .finish(),
            Self::InviteBatchCompleted {
                request_id,
                results,
                ..
            } => formatter
                .debug_struct("InviteBatchCompleted")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("result_count", &results.len())
                .finish(),
            Self::InviteAccepted { request_id, .. } => formatter
                .debug_struct("InviteAccepted")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::InviteDeclined { request_id, .. } => formatter
                .debug_struct("InviteDeclined")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::DirectMessageStarted { request_id, .. } => formatter
                .debug_struct("DirectMessageStarted")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::RoomJoined { request_id, .. } => formatter
                .debug_struct("RoomJoined")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::RoomLeft { request_id, .. } => formatter
                .debug_struct("RoomLeft")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::RoomForgotten { request_id, .. } => formatter
                .debug_struct("RoomForgotten")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::RoomTagSet {
                request_id, tag, ..
            } => formatter
                .debug_struct("RoomTagSet")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("tag", tag)
                .finish(),
            Self::RoomTagRemoved {
                request_id, tag, ..
            } => formatter
                .debug_struct("RoomTagRemoved")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("tag", tag)
                .finish(),
            Self::PinnedEventsUpdated { pinned, .. } => formatter
                .debug_struct("PinnedEventsUpdated")
                .field("room_id", &"RoomId(..)")
                .field("pinned_count", &pinned.len())
                .finish(),
            Self::PinEventCompleted { request_id, .. } => formatter
                .debug_struct("PinEventCompleted")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::UnpinEventCompleted { request_id, .. } => formatter
                .debug_struct("UnpinEventCompleted")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::DirectoryQueryCompleted {
                request_id, rooms, ..
            } => formatter
                .debug_struct("DirectoryQueryCompleted")
                .field("request_id", request_id)
                .field("query", &"DirectoryQuery(..)")
                .field("rooms_count", &rooms.len())
                .finish(),
            Self::DirectoryPreviewLoaded { request_id, room } => formatter
                .debug_struct("DirectoryPreviewLoaded")
                .field("request_id", request_id)
                .field("room", room)
                .finish(),
            Self::RoomSettingsLoaded { request_id, .. } => formatter
                .debug_struct("RoomSettingsLoaded")
                .field("request_id", request_id)
                .field("settings", &"RoomSettingsSnapshot(..)")
                .finish(),
            Self::RoomSettingUpdated { request_id, .. } => formatter
                .debug_struct("RoomSettingUpdated")
                .field("request_id", request_id)
                .field("settings", &"RoomSettingsSnapshot(..)")
                .finish(),
            Self::RoomMemberModerated {
                request_id, action, ..
            } => formatter
                .debug_struct("RoomMemberModerated")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("target_user_id", &"UserId(..)")
                .field("action", action)
                .finish(),
            Self::RoomMemberRoleUpdated {
                request_id,
                power_level,
                ..
            } => formatter
                .debug_struct("RoomMemberRoleUpdated")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("target_user_id", &"UserId(..)")
                .field("power_level", power_level)
                .finish(),
            Self::SpaceMemberRoleUpdateSettled {
                request_id,
                generation,
                outcome,
                ..
            } => formatter
                .debug_struct("SpaceMemberRoleUpdateSettled")
                .field("request_id", request_id)
                .field("generation", generation)
                .field("outcome", outcome)
                .finish(),
            Self::RoomKeyReshared {
                request_id,
                outcome,
                ..
            } => formatter
                .debug_struct("RoomKeyReshared")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("outcome", outcome)
                .finish(),
            Self::OutboundSessionForced {
                request_id,
                outcome,
                ..
            } => formatter
                .debug_struct("OutboundSessionForced")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("outcome", outcome)
                .finish(),
            Self::Index0RoomKeyShared {
                request_id,
                outcome,
                ..
            } => formatter
                .debug_struct("Index0RoomKeyShared")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("outcome", outcome)
                .finish(),
            Self::Index0RoomKeyResent {
                request_id,
                outcome,
                ..
            } => formatter
                .debug_struct("Index0RoomKeyResent")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("outcome", outcome)
                .finish(),
            Self::RoomKeyRequestStateChanged {
                key: _,
                event_id: _,
                request_id: _,
                stage,
                withheld_code,
            } => formatter
                .debug_struct("RoomKeyRequestStateChanged")
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .field("stage", stage)
                .field("withheld_code", withheld_code)
                .finish(),
            Self::ComposerSlashCommandRejected { key: _, request_id } => formatter
                .debug_struct("ComposerSlashCommandRejected")
                .field("key", &"TimelineKey(..)")
                .field("request_id", request_id)
                .finish(),
            Self::MarkedAsRead { request_id, .. } => formatter
                .debug_struct("MarkedAsRead")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::MarkedAsUnread {
                request_id,
                room_id,
                unread,
            } => formatter
                .debug_struct("MarkedAsUnread")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("unread", unread)
                .finish(),
            Self::RoomListUpdated => formatter.write_str("RoomListUpdated"),
            Self::ReportCompleted { request_id, kind } => formatter
                .debug_struct("ReportCompleted")
                .field("request_id", request_id)
                .field("kind", kind)
                .finish(),
        }
    }
}
pub fn project_room_event_display_labels(event: &mut RoomEvent, state: &AppState) {
    match event {
        RoomEvent::RoomSettingsLoaded { settings, .. }
        | RoomEvent::RoomSettingUpdated { settings, .. } => {
            koushi_state::refresh_room_settings_member_display_projection(
                settings,
                &state.profile,
                timeline_projection_own_user_id(state),
            );
        }
        RoomEvent::RoomListUpdated
        | RoomEvent::RoomCreated { .. }
        | RoomEvent::SpaceCreated { .. }
        | RoomEvent::SpaceChildSet { .. }
        | RoomEvent::SpaceMembersLoaded { .. }
        | RoomEvent::SpaceMemberInviteSettled { .. }
        | RoomEvent::SpaceMemberInviteCancellationSettled { .. }
        | RoomEvent::RoomJoined { .. }
        | RoomEvent::RoomLeft { .. }
        | RoomEvent::RoomForgotten { .. }
        | RoomEvent::UserInvited { .. }
        | RoomEvent::InviteBatchCompleted { .. }
        | RoomEvent::InviteAccepted { .. }
        | RoomEvent::InviteDeclined { .. }
        | RoomEvent::DirectMessageStarted { .. }
        | RoomEvent::RoomTagSet { .. }
        | RoomEvent::RoomTagRemoved { .. }
        | RoomEvent::PinnedEventsUpdated { .. }
        | RoomEvent::PinEventCompleted { .. }
        | RoomEvent::UnpinEventCompleted { .. }
        | RoomEvent::DirectoryQueryCompleted { .. }
        | RoomEvent::DirectoryPreviewLoaded { .. }
        | RoomEvent::RoomMemberModerated { .. }
        | RoomEvent::RoomMemberRoleUpdated { .. }
        | RoomEvent::SpaceMemberRoleUpdateSettled { .. }
        | RoomEvent::RoomKeyReshared { .. }
        | RoomEvent::OutboundSessionForced { .. }
        | RoomEvent::Index0RoomKeyShared { .. }
        | RoomEvent::Index0RoomKeyResent { .. }
        | RoomEvent::RoomKeyRequestStateChanged { .. }
        | RoomEvent::ComposerSlashCommandRejected { .. }
        | RoomEvent::MarkedAsRead { .. }
        | RoomEvent::MarkedAsUnread { .. }
        | RoomEvent::ReportCompleted { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fake_rid;
    use super::*;
    use koushi_state::SessionState;
    #[test]
    fn room_member_role_event_debug_redacts_room_and_user_ids() {
        let event = RoomEvent::RoomMemberRoleUpdated {
            request_id: fake_rid(44),
            room_id: "!private-room:example.invalid".to_owned(),
            target_user_id: "@private-target:example.invalid".to_owned(),
            power_level: 50,
        };

        let debug = format!("{event:?}");
        assert!(debug.contains("RoomMemberRoleUpdated"), "{debug}");
        assert!(debug.contains("power_level"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
        assert!(
            !debug.contains("@private-target:example.invalid"),
            "{debug}"
        );
    }
    #[test]
    fn space_invite_cancellation_event_debug_redacts_request_details() {
        let event = RoomEvent::SpaceMemberInviteCancellationSettled {
            request_id: fake_rid(45),
            space_id: "!private-space:example.invalid".to_owned(),
            user_id: "@private-target:example.invalid".to_owned(),
            generation: 4,
            outcome: SpaceMemberInviteOutcome::Cancelled,
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("SpaceMemberInviteCancellationSettled"));
        assert!(!debug.contains("@private-target:example.invalid"));
        assert!(!debug.contains("!private-space:example.invalid"));
    }
    #[test]
    fn room_settings_events_project_member_display_labels_from_profile_state() {
        let mut state = AppState::default();
        state.session = SessionState::Ready(koushi_state::SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@me:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        });
        state.profile.local_aliases.insert(
            "@member:example.invalid".to_owned(),
            "Local Remark".to_owned(),
        );

        let mut event = RoomEvent::RoomSettingsLoaded {
            request_id: fake_rid(70),
            settings: RoomSettingsSnapshot {
                room_id: "!room:example.invalid".to_owned(),
                name: Some("Room".to_owned()),
                topic: None,
                avatar_url: None,
                canonical_alias: None,
                alternate_aliases: Vec::new(),
                share_link: None,
                join_rule: koushi_state::RoomJoinRule::Invite,
                history_visibility: koushi_state::RoomHistoryVisibility::Shared,
                permissions: koushi_state::RoomPermissionFacts::default(),
                members: vec![koushi_state::RoomMemberSummary {
                    user_id: "@member:example.invalid".to_owned(),
                    display_name: Some("Upstream Member".to_owned()),
                    display_label: "Upstream Member".to_owned(),
                    original_display_label: "Upstream Member".to_owned(),
                    avatar_url: None,
                    power_level: Some(0),
                    role: koushi_state::RoomMemberRole::User,
                    user_trust: None,
                }],
            },
        };

        project_room_event_display_labels(&mut event, &state);

        let RoomEvent::RoomSettingsLoaded { settings, .. } = event else {
            panic!("expected room settings event");
        };
        assert_eq!(settings.members[0].display_label, "Local Remark");
        assert_eq!(
            settings.members[0].display_name.as_deref(),
            Some("Upstream Member")
        );
    }
    #[test]
    fn room_key_reshare_outcomes_serialize_without_session_identifiers() {
        let request_id = RequestId {
            connection_id: crate::ids::RuntimeConnectionId(4),
            sequence: 9,
        };
        for (outcome, expected) in [
            (
                RoomKeyReshareOutcome::Sent {
                    request_count: 2,
                    recipient_count: 3,
                    failed_recipient_count: 1,
                },
                serde_json::json!({
                    "kind": "sent",
                    "request_count": 2,
                    "recipient_count": 3,
                    "failed_recipient_count": 1
                }),
            ),
            (
                RoomKeyReshareOutcome::NoSession,
                serde_json::json!({"kind": "no_session"}),
            ),
            (
                RoomKeyReshareOutcome::NoRecipients,
                serde_json::json!({"kind": "no_recipients"}),
            ),
            (
                RoomKeyReshareOutcome::StaleSession,
                serde_json::json!({"kind": "stale_session"}),
            ),
        ] {
            let event = RoomEvent::RoomKeyReshared {
                request_id,
                room_id: "!room:example.invalid".to_owned(),
                outcome,
            };
            assert_eq!(
                serde_json::to_value(event).unwrap(),
                serde_json::json!({
                    "RoomKeyReshared": {
                        "request_id": {"connection_id": 4, "sequence": 9},
                        "room_id": "!room:example.invalid",
                        "outcome": expected
                    }
                })
            );
        }
    }
}
