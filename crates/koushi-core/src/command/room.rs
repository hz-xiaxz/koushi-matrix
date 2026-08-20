use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{AccountKey, RequestId};
use koushi_state::{
    DirectoryQuery, InviteScopeSelection, RoomModerationAction, RoomSettingChange, RoomTagKind,
};

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomOptions {
    pub name: String,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub alias_localpart: Option<String>,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(default)]
    pub visibility: CreateRoomVisibility,
    #[serde(default)]
    pub parent_space: Option<CreateRoomParentSpace>,
}

impl fmt::Debug for CreateRoomOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateRoomOptions")
            .field("name", &"RoomName(..)")
            .field("topic", &self.topic.as_ref().map(|_| "RoomTopic(..)"))
            .field(
                "alias_localpart",
                &self
                    .alias_localpart
                    .as_ref()
                    .map(|_| "RoomAliasLocalpart(..)"),
            )
            .field("encrypted", &self.encrypted)
            .field("visibility", &self.visibility)
            .field("parent_space", &self.parent_space)
            .finish()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CreateRoomVisibility {
    #[default]
    Private,
    Public,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomParentSpace {
    pub space_id: String,
    pub via_server: String,
}

impl fmt::Debug for CreateRoomParentSpace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateRoomParentSpace")
            .field("space_id", &"RoomId(..)")
            .field("via_server", &"ServerName(..)")
            .finish()
    }
}

pub enum RoomCommand {
    CreateRoom {
        request_id: RequestId,
        options: CreateRoomOptions,
    },
    CreatePublicDirectoryRoom {
        request_id: RequestId,
        name: String,
        alias_localpart: String,
    },
    CreateSpace {
        request_id: RequestId,
        name: String,
    },
    SetSpaceChild {
        request_id: RequestId,
        space_id: String,
        child_room_id: String,
        via_server: String,
    },
    InviteUser {
        request_id: RequestId,
        room_id: String,
        user_id: String,
    },
    LoadSpaceMembers {
        request_id: RequestId,
        space_id: String,
        generation: u64,
    },
    InviteUserToSpace {
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    },
    CancelSpaceInvite {
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    },
    InviteTargets {
        request_id: RequestId,
        room_id: String,
        user_ids: Vec<String>,
        scope: InviteScopeSelection,
    },
    AcceptInvite {
        request_id: RequestId,
        room_id: String,
    },
    DeclineInvite {
        request_id: RequestId,
        room_id: String,
    },
    StartDirectMessage {
        request_id: RequestId,
        user_id: String,
    },
    JoinRoom {
        request_id: RequestId,
        room_id: String,
    },
    LeaveRoom {
        request_id: RequestId,
        room_id: String,
    },
    ForgetRoom {
        request_id: RequestId,
        room_id: String,
    },
    SetTag {
        request_id: RequestId,
        room_id: String,
        tag: RoomTagKind,
        order: Option<f64>,
    },
    RemoveTag {
        request_id: RequestId,
        room_id: String,
        tag: RoomTagKind,
    },
    PinEvent {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    UnpinEvent {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    RefreshPinnedEvents {
        request_id: RequestId,
        room_id: String,
    },
    QueryDirectory {
        request_id: RequestId,
        query: DirectoryQuery,
    },
    PreviewJoinTarget {
        request_id: RequestId,
        /// `#alias:server` or `!id:server`.
        room_id_or_alias: String,
        /// Servers to try when the homeserver does not already know the room.
        via_servers: Vec<String>,
    },
    DismissDirectoryPreview {
        request_id: RequestId,
    },
    JoinDirectoryRoom {
        request_id: RequestId,
        /// `#alias:server` or `!id:server`.
        room_id_or_alias: String,
        /// Servers to try when the homeserver does not already know the room.
        via_servers: Vec<String>,
    },
    LoadRoomSettings {
        request_id: RequestId,
        room_id: String,
    },
    QueryMentionCandidates {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: String,
        surface: koushi_state::MentionSurface,
        query: String,
    },
    ReshareRoomKey {
        request_id: RequestId,
        room_id: String,
    },
    /// Temporary dangerous encryption-debug control (issue #538): rotate the
    /// outbound Megolm session and confirm the fresh session is at index 0.
    ForceNewOutboundSession {
        request_id: RequestId,
        room_id: String,
    },
    /// Temporary dangerous encryption-debug control (issue #538): share the
    /// current outbound session's index-0 room key to every eligible
    /// recipient device (claiming missing Olm sessions).
    ShareIndex0RoomKey {
        request_id: RequestId,
        room_id: String,
    },
    /// Temporary dangerous encryption-debug control (issue #541): resend
    /// index-0 recovery material for the current outbound session to the
    /// immutable original recipient ledger.
    ResendIndex0RoomKey {
        request_id: RequestId,
        room_id: String,
    },
    UpdateRoomSetting {
        request_id: RequestId,
        room_id: String,
        change: RoomSettingChange,
    },
    ModerateRoomMember {
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        action: RoomModerationAction,
        reason: Option<String>,
    },
    UpdateRoomMemberRole {
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        power_level: i64,
    },
    SelectSpace {
        request_id: RequestId,
        space_id: Option<String>,
    },
    ReorderSpaces {
        request_id: RequestId,
        space_ids: Vec<String>,
    },
    /// User-intent lane: room selection is request-id correlated and must be
    /// routed through the reliable command path, not a drop-on-full background
    /// queue.
    SelectRoom {
        request_id: RequestId,
        room_id: String,
    },
    MarkRoomAsRead {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    MarkRoomAsUnread {
        request_id: RequestId,
        room_id: String,
        unread: bool,
    },
    SetRoomNotificationMode {
        request_id: RequestId,
        room_id: String,
        mode: koushi_state::RoomNotificationMode,
    },
    ReportContent {
        request_id: RequestId,
        room_id: String,
        event_id: String,
        reason: Option<String>,
    },
    ReportRoom {
        request_id: RequestId,
        room_id: String,
        reason: String,
    },
}

impl fmt::Debug for RoomCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateRoom {
                request_id,
                options,
                ..
            } => formatter
                .debug_struct("CreateRoom")
                .field("request_id", request_id)
                .field("name", &"RoomName(..)")
                .field("encrypted", &options.encrypted)
                .field("visibility", &options.visibility)
                .field("has_topic", &options.topic.is_some())
                .field("has_alias_localpart", &options.alias_localpart.is_some())
                .field("has_parent_space", &options.parent_space.is_some())
                .finish(),
            Self::CreatePublicDirectoryRoom { request_id, .. } => formatter
                .debug_struct("CreatePublicDirectoryRoom")
                .field("request_id", request_id)
                .field("name", &"RoomName(..)")
                .field("alias_localpart", &"RoomAliasLocalpart(..)")
                .finish(),
            Self::CreateSpace { request_id, .. } => formatter
                .debug_struct("CreateSpace")
                .field("request_id", request_id)
                .field("name", &"RoomName(..)")
                .finish(),
            Self::SetSpaceChild { request_id, .. } => formatter
                .debug_struct("SetSpaceChild")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("child_room_id", &"RoomId(..)")
                .field("via_server", &"ServerName(..)")
                .finish(),
            Self::InviteUser { request_id, .. } => formatter
                .debug_struct("InviteUser")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .finish(),
            Self::LoadSpaceMembers {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("LoadSpaceMembers")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("generation", generation)
                .finish(),
            Self::InviteUserToSpace {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("InviteUserToSpace")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .finish(),
            Self::CancelSpaceInvite {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("CancelSpaceInvite")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .finish(),
            Self::InviteTargets {
                request_id,
                user_ids,
                scope,
                ..
            } => formatter
                .debug_struct("InviteTargets")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("user_count", &user_ids.len())
                .field("scope", scope)
                .finish(),
            Self::AcceptInvite { request_id, .. } => formatter
                .debug_struct("AcceptInvite")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::DeclineInvite { request_id, .. } => formatter
                .debug_struct("DeclineInvite")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::StartDirectMessage { request_id, .. } => formatter
                .debug_struct("StartDirectMessage")
                .field("request_id", request_id)
                .field("user_id", &"UserId(..)")
                .finish(),
            Self::JoinRoom { request_id, .. } => formatter
                .debug_struct("JoinRoom")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::LeaveRoom { request_id, .. } => formatter
                .debug_struct("LeaveRoom")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::ForgetRoom { request_id, .. } => formatter
                .debug_struct("ForgetRoom")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::SetTag {
                request_id,
                tag,
                order,
                ..
            } => formatter
                .debug_struct("SetTag")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("tag", tag)
                .field("order", order)
                .finish(),
            Self::RemoveTag {
                request_id, tag, ..
            } => formatter
                .debug_struct("RemoveTag")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("tag", tag)
                .finish(),
            Self::PinEvent { request_id, .. } => formatter
                .debug_struct("PinEvent")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::UnpinEvent { request_id, .. } => formatter
                .debug_struct("UnpinEvent")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::RefreshPinnedEvents { request_id, .. } => formatter
                .debug_struct("RefreshPinnedEvents")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::QueryDirectory { request_id, query } => formatter
                .debug_struct("QueryDirectory")
                .field("request_id", request_id)
                .field("term", &query.term.as_ref().map(|_| "DirectoryQuery(..)"))
                .field(
                    "server_name",
                    &query.server_name.as_ref().map(|_| "ServerName(..)"),
                )
                .field("limit", &query.limit)
                .field("since", &query.since.as_ref().map(|_| "PageToken(..)"))
                .finish(),
            Self::PreviewJoinTarget {
                request_id,
                via_servers,
                ..
            } => formatter
                .debug_struct("PreviewJoinTarget")
                .field("request_id", request_id)
                .field("room_id_or_alias", &"RoomIdOrAlias(..)")
                .field("via_server_count", &via_servers.len())
                .finish(),
            Self::DismissDirectoryPreview { request_id } => formatter
                .debug_struct("DismissDirectoryPreview")
                .field("request_id", request_id)
                .finish(),
            Self::JoinDirectoryRoom { request_id, .. } => formatter
                .debug_struct("JoinDirectoryRoom")
                .field("request_id", request_id)
                .field("alias", &"RoomAlias(..)")
                .field("via_server", &"ServerName(..)")
                .finish(),
            Self::LoadRoomSettings { request_id, .. } => formatter
                .debug_struct("LoadRoomSettings")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::QueryMentionCandidates {
                request_id,
                surface,
                ..
            } => formatter
                .debug_struct("QueryMentionCandidates")
                .field("request_id", request_id)
                .field("surface", surface)
                .finish(),
            Self::ReshareRoomKey { request_id, .. } => formatter
                .debug_struct("ReshareRoomKey")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::ForceNewOutboundSession { request_id, .. } => formatter
                .debug_struct("ForceNewOutboundSession")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::ShareIndex0RoomKey { request_id, .. } => formatter
                .debug_struct("ShareIndex0RoomKey")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::ResendIndex0RoomKey { request_id, .. } => formatter
                .debug_struct("ResendIndex0RoomKey")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::UpdateRoomSetting {
                request_id, change, ..
            } => formatter
                .debug_struct("UpdateRoomSetting")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("change", change)
                .finish(),
            Self::ModerateRoomMember {
                request_id, action, ..
            } => formatter
                .debug_struct("ModerateRoomMember")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("target_user_id", &"UserId(..)")
                .field("action", action)
                .field("reason", &"ModerationReason(..)")
                .finish(),
            Self::UpdateRoomMemberRole {
                request_id,
                power_level,
                ..
            } => formatter
                .debug_struct("UpdateRoomMemberRole")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("target_user_id", &"UserId(..)")
                .field("power_level", power_level)
                .finish(),
            Self::SelectSpace {
                request_id,
                space_id,
            } => formatter
                .debug_struct("SelectSpace")
                .field("request_id", request_id)
                .field("space_id", &space_id.as_ref().map(|_| "RoomId(..)"))
                .finish(),
            Self::ReorderSpaces { request_id, .. } => formatter
                .debug_struct("ReorderSpaces")
                .field("request_id", request_id)
                .field("space_ids", &"Vec<RoomId>(..)")
                .finish(),
            Self::SelectRoom { request_id, .. } => formatter
                .debug_struct("SelectRoom")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::MarkRoomAsRead {
                request_id,
                room_id: _,
                ..
            } => formatter
                .debug_struct("MarkRoomAsRead")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::MarkRoomAsUnread {
                request_id,
                room_id: _,
                unread,
            } => formatter
                .debug_struct("MarkRoomAsUnread")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("unread", unread)
                .finish(),
            Self::SetRoomNotificationMode {
                request_id,
                room_id: _,
                mode,
            } => formatter
                .debug_struct("SetRoomNotificationMode")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("mode", mode)
                .finish(),
            Self::ReportContent { request_id, .. } => formatter
                .debug_struct("ReportContent")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .field("reason", &"ReportReason(..)")
                .finish(),
            Self::ReportRoom { request_id, .. } => formatter
                .debug_struct("ReportRoom")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("reason", &"ReportReason(..)")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CoreCommand, test_support::fake_rid};
    use super::*;

    #[test]
    fn pin_event_debug_redacts_room_and_event_ids() {
        let pin = RoomCommand::PinEvent {
            request_id: fake_rid(11),
            room_id: "!room:example.invalid".to_owned(),
            event_id: "$event:example.invalid".to_owned(),
        };
        let unpin = RoomCommand::UnpinEvent {
            request_id: fake_rid(12),
            room_id: "!room:example.invalid".to_owned(),
            event_id: "$event:example.invalid".to_owned(),
        };

        for debug in [format!("{pin:?}"), format!("{unpin:?}")] {
            assert!(debug.contains("RoomId(..)"), "{debug}");
            assert!(debug.contains("EventId(..)"), "{debug}");
            assert!(!debug.contains("!room:example.invalid"), "{debug}");
            assert!(!debug.contains("$event:example.invalid"), "{debug}");
        }
    }

    #[test]
    fn set_room_notification_mode_debug_redacts_room_id() {
        let command = RoomCommand::SetRoomNotificationMode {
            request_id: fake_rid(13),
            room_id: "!room:example.invalid".to_owned(),
            mode: koushi_state::RoomNotificationMode::Mute,
        };
        let debug = format!("{command:?}");
        assert!(debug.contains("SetRoomNotificationMode"), "{debug}");
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(!debug.contains("!room:example.invalid"), "{debug}");
    }

    #[test]
    fn directory_commands_debug_redacts_query_alias_and_server() {
        let query = RoomCommand::QueryDirectory {
            request_id: fake_rid(15),
            query: DirectoryQuery {
                term: Some("private search".to_owned()),
                server_name: Some("example.invalid".to_owned()),
                limit: Some(10),
                since: Some("opaque-page-token".to_owned()),
            },
        };
        let join_request_id = fake_rid(14);
        let join = RoomCommand::JoinDirectoryRoom {
            request_id: join_request_id,
            room_id_or_alias: "#private-room:example.invalid".to_owned(),
            via_servers: vec!["example.invalid".to_owned()],
        };
        let create_request_id = fake_rid(15);
        let create_public = RoomCommand::CreatePublicDirectoryRoom {
            request_id: create_request_id,
            name: "Private Public Room Name".to_owned(),
            alias_localpart: "private-public-alias".to_owned(),
        };

        assert_eq!(
            CoreCommand::Room(RoomCommand::JoinDirectoryRoom {
                request_id: join_request_id,
                room_id_or_alias: "#private-room:example.invalid".to_owned(),
                via_servers: vec!["example.invalid".to_owned()],
            })
            .request_id(),
            join_request_id
        );
        assert_eq!(
            CoreCommand::Room(RoomCommand::CreatePublicDirectoryRoom {
                request_id: create_request_id,
                name: "Private Public Room Name".to_owned(),
                alias_localpart: "private-public-alias".to_owned(),
            })
            .request_id(),
            create_request_id
        );
        for debug in [
            format!("{query:?}"),
            format!("{join:?}"),
            format!("{create_public:?}"),
        ] {
            assert!(!debug.contains("private search"), "{debug}");
            assert!(!debug.contains("#private-room:example.invalid"), "{debug}");
            assert!(!debug.contains("Private Public Room Name"), "{debug}");
            assert!(!debug.contains("private-public-alias"), "{debug}");
            assert!(!debug.contains("example.invalid"), "{debug}");
            assert!(!debug.contains("opaque-page-token"), "{debug}");
        }
    }

    #[test]
    fn mention_candidate_command_is_correlated_and_redacts_private_values() {
        let request_id = fake_rid(151);
        let command = RoomCommand::QueryMentionCandidates {
            request_id,
            account_key: AccountKey("@private-account:example.invalid".to_owned()),
            room_id: "!private-room:example.invalid".to_owned(),
            surface: koushi_state::MentionSurface::Thread,
            query: "Private Person Alias".to_owned(),
        };

        assert_eq!(
            CoreCommand::Room(RoomCommand::QueryMentionCandidates {
                request_id,
                account_key: AccountKey("@private-account:example.invalid".to_owned()),
                room_id: "!private-room:example.invalid".to_owned(),
                surface: koushi_state::MentionSurface::Thread,
                query: "Private Person Alias".to_owned(),
            })
            .request_id(),
            request_id
        );
        let debug = format!("{command:?}");
        assert!(debug.contains("QueryMentionCandidates"), "{debug}");
        assert!(debug.contains("Thread"), "{debug}");
        for private_value in [
            "@private-account:example.invalid",
            "!private-room:example.invalid",
            "Private Person Alias",
        ] {
            assert!(!debug.contains(private_value), "{debug}");
        }
    }

    #[test]
    fn room_management_commands_debug_redacts_room_user_and_settings_values() {
        use koushi_state::{RoomJoinRule, RoomModerationAction, RoomSettingChange};

        let load_request_id = fake_rid(16);
        let load = RoomCommand::LoadRoomSettings {
            request_id: load_request_id,
            room_id: "!private-room:example.invalid".to_owned(),
        };
        let update_request_id = fake_rid(17);
        let update = RoomCommand::UpdateRoomSetting {
            request_id: update_request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            change: RoomSettingChange::Name(Some("Private Room Name".to_owned())),
        };
        let moderation_request_id = fake_rid(18);
        let moderation = RoomCommand::ModerateRoomMember {
            request_id: moderation_request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            target_user_id: "@private-target:example.invalid".to_owned(),
            action: RoomModerationAction::Ban,
            reason: Some("Private moderation reason".to_owned()),
        };
        let role_request_id = fake_rid(19);
        let role = RoomCommand::UpdateRoomMemberRole {
            request_id: role_request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            target_user_id: "@private-target:example.invalid".to_owned(),
            power_level: 50,
        };

        assert_eq!(CoreCommand::Room(load).request_id(), load_request_id);
        assert_eq!(
            CoreCommand::Room(RoomCommand::UpdateRoomSetting {
                request_id: update_request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                change: RoomSettingChange::JoinRule(RoomJoinRule::Public),
            })
            .request_id(),
            update_request_id
        );
        assert_eq!(
            CoreCommand::Room(RoomCommand::ModerateRoomMember {
                request_id: moderation_request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                target_user_id: "@private-target:example.invalid".to_owned(),
                action: RoomModerationAction::Kick,
                reason: None,
            })
            .request_id(),
            moderation_request_id
        );
        assert_eq!(
            CoreCommand::Room(RoomCommand::UpdateRoomMemberRole {
                request_id: role_request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                target_user_id: "@private-target:example.invalid".to_owned(),
                power_level: 50,
            })
            .request_id(),
            role_request_id
        );

        for debug in [
            format!(
                "{:?}",
                RoomCommand::LoadRoomSettings {
                    request_id: fake_rid(20),
                    room_id: "!private-room:example.invalid".to_owned(),
                }
            ),
            format!("{update:?}"),
            format!("{moderation:?}"),
            format!("{role:?}"),
        ] {
            assert!(debug.contains("RoomId(..)"), "{debug}");
            assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
            assert!(
                !debug.contains("@private-target:example.invalid"),
                "{debug}"
            );
            assert!(!debug.contains("Private Room Name"), "{debug}");
            assert!(!debug.contains("Private moderation reason"), "{debug}");
        }
    }
}
