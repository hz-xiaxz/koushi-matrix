use crate::room_projection::{
    matrix_public_room_from_chunk, matrix_room, matrix_room_operation_failure_kind,
    matrix_room_settings_snapshot, non_empty_name, room_settings_snapshot_with_change,
    room_settings_snapshot_with_member_power_level, sdk_history_visibility,
    sdk_join_rule_for_update,
};
use crate::{MatrixClientSession, MatrixRoomMemberSummary, MatrixRoomTagKind};
use koushi_diagnostics::{DiagnosticEvent, DiagnosticLevel};
#[cfg(test)]
use koushi_state::SessionInfo;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MatrixRoomOperationError {
    #[error("Matrix room id is invalid")]
    InvalidRoomId,
    #[error("Matrix room alias is invalid")]
    InvalidRoomAlias,
    #[error("Matrix room setting is invalid")]
    InvalidRoomSetting,
    #[error("Matrix event id is invalid")]
    InvalidEventId,
    #[error("Matrix user id is invalid")]
    InvalidUserId,
    #[error("Matrix server name is invalid")]
    InvalidServerName,
    #[error("Matrix room is not available")]
    RoomUnavailable,
    #[error("Matrix room operation failed: {0}")]
    Sdk(MatrixRoomOperationFailureKind),
}

impl MatrixRoomOperationError {
    pub fn failure_kind(&self) -> Option<MatrixRoomOperationFailureKind> {
        match self {
            Self::Sdk(kind) => Some(*kind),
            Self::InvalidRoomId
            | Self::InvalidRoomAlias
            | Self::InvalidRoomSetting
            | Self::InvalidEventId
            | Self::InvalidUserId
            | Self::InvalidServerName
            | Self::RoomUnavailable => None,
        }
    }

    pub(super) fn from_sdk_error(error: matrix_sdk::Error) -> Self {
        Self::Sdk(matrix_room_operation_failure_kind(&error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixRoomOperationFailureKind {
    AuthenticationRequired,
    Encryption,
    Forbidden,
    Http,
    Store,
    SecureBackupRequired,
    WrongRoomState,
    Sdk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixSpaceInviteCancellationOutcome {
    Cancelled,
    NotInvited,
}

impl fmt::Display for MatrixRoomOperationFailureKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::AuthenticationRequired => "authentication_required",
            Self::Encryption => "encryption",
            Self::Forbidden => "forbidden",
            Self::Http => "http",
            Self::Store => "store",
            Self::SecureBackupRequired => "secure_backup_required",
            Self::WrongRoomState => "wrong_room_state",
            Self::Sdk => "sdk",
        };
        formatter.write_str(label)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixPublicRoomDirectoryQuery {
    pub term: Option<String>,
    pub server_name: Option<String>,
    pub limit: Option<u32>,
    pub since: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixPublicRoomDirectoryResult {
    pub rooms: Vec<MatrixPublicRoomDirectoryRoom>,
    pub next_batch: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixPublicRoomDirectoryRoom {
    pub room_id: String,
    pub canonical_alias: Option<String>,
    /// Matrix `room_type`, e.g. `m.space`. Absent for an ordinary room.
    pub room_type: Option<String>,
    /// Empty when the directory entry has no name; the caller decides how to
    /// label it, because a room and a space need different fallbacks.
    pub name: String,
    pub topic: Option<String>,
    pub avatar_url: Option<String>,
    pub joined_members: u64,
    pub world_readable: bool,
    pub guest_can_join: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixCreateRoomOptions {
    pub name: String,
    pub topic: Option<String>,
    pub alias_localpart: Option<String>,
    pub encrypted: bool,
    pub visibility: MatrixCreateRoomVisibility,
    pub parent_space: Option<MatrixCreateRoomParentSpace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatrixCreateRoomVisibility {
    Private,
    Public,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixCreateRoomParentSpace {
    pub space_id: String,
    pub via_server: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixRoomSettingsSnapshot {
    pub room_id: String,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub avatar_url: Option<String>,
    pub canonical_alias: Option<String>,
    pub alternate_aliases: Vec<String>,
    pub join_rule: MatrixRoomJoinRule,
    pub history_visibility: MatrixRoomHistoryVisibility,
    pub permissions: MatrixRoomPermissionFacts,
    pub members: Vec<MatrixRoomMemberSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatrixRoomMemberRole {
    Creator,
    Administrator,
    Moderator,
    User,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixUserTrustState {
    Unverified,
    Verified,
    IdentityReset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixRoomJoinRule {
    Public,
    Invite,
    Knock,
    Restricted,
    Private,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixRoomHistoryVisibility {
    WorldReadable,
    Shared,
    Invited,
    Joined,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MatrixRoomPermissionFacts {
    pub can_edit_settings: bool,
    pub can_edit_roles: bool,
    pub can_invite: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub can_unban: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatrixRoomSettingChange {
    Name(Option<String>),
    Topic(Option<String>),
    AvatarUrl(Option<String>),
    JoinRule(MatrixRoomJoinRule),
    HistoryVisibility(MatrixRoomHistoryVisibility),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixRoomModerationAction {
    Kick,
    Ban,
    Unban,
}

pub async fn room_can_send_text_message(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<bool, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    if room.state() != matrix_sdk::RoomState::Joined {
        return Ok(false);
    }

    let power_levels = room.power_levels_or_default().await;
    Ok(power_levels.user_can_send_message(
        room.own_user_id(),
        matrix_sdk::ruma::events::MessageLikeEventType::RoomMessage,
    ))
}

pub async fn get_room_settings_snapshot(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<MatrixRoomSettingsSnapshot, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    Ok(matrix_room_settings_snapshot(&room).await)
}

pub async fn update_room_setting(
    session: &MatrixClientSession,
    room_id: &str,
    change: MatrixRoomSettingChange,
) -> Result<MatrixRoomSettingsSnapshot, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let snapshot = matrix_room_settings_snapshot(&room).await;
    match &change {
        MatrixRoomSettingChange::Name(name) => {
            room.set_name(name.clone().unwrap_or_default())
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
        MatrixRoomSettingChange::Topic(topic) => {
            room.set_room_topic(topic.as_deref().unwrap_or_default())
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
        MatrixRoomSettingChange::AvatarUrl(Some(avatar_url)) => {
            let avatar_url = matrix_sdk::ruma::OwnedMxcUri::from(avatar_url.clone());
            room.set_avatar_url(avatar_url.as_ref(), None)
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
        MatrixRoomSettingChange::AvatarUrl(None) => {
            room.remove_avatar()
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
        MatrixRoomSettingChange::JoinRule(join_rule) => {
            let join_rule = sdk_join_rule_for_update(*join_rule)?;
            room.privacy_settings()
                .update_join_rule(join_rule)
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
        MatrixRoomSettingChange::HistoryVisibility(history_visibility) => {
            room.privacy_settings()
                .update_room_history_visibility(sdk_history_visibility(*history_visibility))
                .await
                .map_err(MatrixRoomOperationError::from_sdk_error)?;
        }
    }

    Ok(room_settings_snapshot_with_change(snapshot, &change))
}

pub async fn moderate_room_member(
    session: &MatrixClientSession,
    room_id: &str,
    target_user_id: &str,
    action: MatrixRoomModerationAction,
    reason: Option<&str>,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let target_user_id = matrix_sdk::ruma::UserId::parse(target_user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;

    match action {
        MatrixRoomModerationAction::Kick => room.kick_user(&target_user_id, reason).await,
        MatrixRoomModerationAction::Ban => room.ban_user(&target_user_id, reason).await,
        MatrixRoomModerationAction::Unban => room.unban_user(&target_user_id, reason).await,
    }
    .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn update_room_member_power_level(
    session: &MatrixClientSession,
    room_id: &str,
    target_user_id: &str,
    power_level: i64,
) -> Result<MatrixRoomSettingsSnapshot, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let target_user_id = matrix_sdk::ruma::UserId::parse(target_user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;
    let power_level = matrix_sdk::ruma::Int::try_from(power_level)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomSetting)?;

    room.update_power_levels(vec![(target_user_id.as_ref(), power_level)])
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;

    let target_user_id_ref: &matrix_sdk::ruma::UserId = target_user_id.as_ref();
    Ok(room_settings_snapshot_with_member_power_level(
        matrix_room_settings_snapshot(&room).await,
        target_user_id_ref.as_str(),
        power_level.into(),
    ))
}

pub async fn create_room(
    session: &MatrixClientSession,
    options: MatrixCreateRoomOptions,
) -> Result<String, MatrixRoomOperationError> {
    let request = create_room_request(options)?;
    let room = session
        .client()
        .create_room(request)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(room.room_id().to_string())
}

pub async fn create_public_directory_room(
    session: &MatrixClientSession,
    name: &str,
    alias_localpart: &str,
) -> Result<String, MatrixRoomOperationError> {
    create_room(
        session,
        MatrixCreateRoomOptions {
            name: name.to_owned(),
            topic: None,
            alias_localpart: Some(alias_localpart.to_owned()),
            encrypted: false,
            visibility: MatrixCreateRoomVisibility::Public,
            parent_space: None,
        },
    )
    .await
}

pub(super) fn create_room_request(
    options: MatrixCreateRoomOptions,
) -> Result<matrix_sdk::ruma::api::client::room::create_room::v3::Request, MatrixRoomOperationError>
{
    let mut request = matrix_sdk::ruma::api::client::room::create_room::v3::Request::new();
    request.name = non_empty_name(&options.name);
    request.topic = options
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        .map(ToOwned::to_owned);

    let is_public = matches!(options.visibility, MatrixCreateRoomVisibility::Public);
    if is_public {
        let alias_localpart = options
            .alias_localpart
            .as_deref()
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .ok_or(MatrixRoomOperationError::InvalidRoomAlias)?;
        validate_alias_localpart(alias_localpart)?;
        request.room_alias_name = Some(alias_localpart.to_owned());
        request.visibility = matrix_sdk::ruma::api::client::room::Visibility::Public;
        request.preset =
            Some(matrix_sdk::ruma::api::client::room::create_room::v3::RoomPreset::PublicChat);
    }

    if options.encrypted && !is_public {
        request.initial_state.push(
            matrix_sdk::ruma::events::InitialStateEvent::with_empty_state_key(
                matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent::with_recommended_defaults(),
            )
            .to_raw_any(),
        );
    }

    if let Some(parent_space) = options.parent_space {
        let parent_space_id = matrix_sdk::ruma::OwnedRoomId::try_from(parent_space.space_id)
            .map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
        let via_server = matrix_sdk::ruma::OwnedServerName::try_from(parent_space.via_server)
            .map_err(|_| MatrixRoomOperationError::InvalidServerName)?;
        let mut parent_content =
            matrix_sdk::ruma::events::space::parent::SpaceParentEventContent::new(vec![via_server]);
        parent_content.canonical = true;
        request.initial_state.push(
            matrix_sdk::ruma::events::InitialStateEvent::new(
                parent_space_id.clone(),
                parent_content,
            )
            .to_raw_any(),
        );

        if !is_public {
            request.room_version = Some(matrix_sdk::ruma::RoomVersionId::V9);
            request.initial_state.push(
                matrix_sdk::ruma::events::InitialStateEvent::with_empty_state_key(
                    matrix_sdk::ruma::events::room::join_rules::RoomJoinRulesEventContent::restricted(
                        vec![
                            matrix_sdk::ruma::events::room::join_rules::AllowRule::room_membership(
                                parent_space_id,
                            ),
                        ],
                    ),
                )
                .to_raw_any(),
            );
            request.initial_state.push(
                matrix_sdk::ruma::events::InitialStateEvent::with_empty_state_key(
                    matrix_sdk::ruma::events::room::history_visibility::RoomHistoryVisibilityEventContent::new(
                        matrix_sdk::ruma::events::room::history_visibility::HistoryVisibility::Invited,
                    ),
                )
                .to_raw_any(),
            );
        }
    }

    Ok(request)
}

fn validate_alias_localpart(alias_localpart: &str) -> Result<(), MatrixRoomOperationError> {
    if alias_localpart.starts_with('#') || alias_localpart.contains(':') {
        return Err(MatrixRoomOperationError::InvalidRoomAlias);
    }
    Ok(())
}

pub async fn create_space(
    session: &MatrixClientSession,
    name: &str,
) -> Result<String, MatrixRoomOperationError> {
    let mut creation_content =
        matrix_sdk::ruma::api::client::room::create_room::v3::CreationContent::new();
    creation_content.room_type = Some(matrix_sdk::ruma::room::RoomType::Space);

    let mut request = matrix_sdk::ruma::api::client::room::create_room::v3::Request::new();
    request.name = non_empty_name(name);
    request.creation_content = Some(
        matrix_sdk::ruma::serde::Raw::new(&creation_content)
            .map_err(|_| MatrixRoomOperationError::Sdk(MatrixRoomOperationFailureKind::Sdk))?,
    );

    let room = session
        .client()
        .create_room(request)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(room.room_id().to_string())
}

pub async fn invite_user_to_room(
    session: &MatrixClientSession,
    room_id: &str,
    user_id: &str,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;
    room.invite_user_by_id(&user_id)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn cancel_space_invite(
    session: &MatrixClientSession,
    space_id: &str,
    user_id: &str,
) -> Result<MatrixSpaceInviteCancellationOutcome, MatrixRoomOperationError> {
    let room = matrix_room(session, space_id)?;
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;
    let invited_members = room
        .members_no_sync(matrix_sdk::RoomMemberships::INVITE)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    if !invited_members
        .iter()
        .any(|member| member.user_id().as_str() == user_id.as_str())
    {
        return Ok(MatrixSpaceInviteCancellationOutcome::NotInvited);
    }
    room.kick_user(&user_id, None)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(MatrixSpaceInviteCancellationOutcome::Cancelled)
}

pub async fn room_has_active_member_no_sync(
    session: &MatrixClientSession,
    room_id: &str,
    user_id: &str,
) -> Result<bool, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;
    let members = room
        .members_no_sync(matrix_sdk::RoomMemberships::ACTIVE)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(members
        .iter()
        .any(|member| member.user_id().as_str() == user_id.as_str()))
}

pub async fn start_direct_message(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<String, MatrixRoomOperationError> {
    koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "sdk.room_operation",
        "start_dm_started",
    ));
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixRoomOperationError::InvalidUserId)?;
    // Get-or-create (#368): reuse the existing joined DM whose only direct
    // target is this user. Unconditional create_dm minted a duplicate DM room
    // on every repeated "Send message".
    if let Some(room) = session.client().get_dm_room(&user_id) {
        koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
            DiagnosticLevel::Info,
            "sdk.room_operation",
            "start_dm_reused",
        ));
        return Ok(room.room_id().to_string());
    }
    koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "sdk.room_operation",
        "start_dm_create_started",
    ));
    let room = match session.client().create_dm(&user_id).await {
        Ok(room) => room,
        Err(error) => {
            koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
                DiagnosticLevel::Warn,
                "sdk.room_operation",
                "start_dm_create_failed",
            ));
            return Err(MatrixRoomOperationError::from_sdk_error(error));
        }
    };
    koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "sdk.room_operation",
        "start_dm_create_completed",
    ));
    Ok(room.room_id().to_string())
}

#[cfg(test)]
mod start_direct_message_tests {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_test::JoinedRoomBuilder;
    use serde_json::json;

    use super::{MatrixClientSession, SessionInfo};

    async fn session_for(server: &MatrixMockServer) -> MatrixClientSession {
        let client = server.client_builder().build().await;
        let info = SessionInfo {
            homeserver: server.server().uri(),
            user_id: client
                .user_id()
                .expect("mock client has a user id")
                .to_string(),
            device_id: client
                .device_id()
                .expect("mock client has a device id")
                .to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        };
        MatrixClientSession { client, info }
    }

    #[tokio::test]
    async fn start_direct_message_reuses_the_existing_joined_dm_room() {
        let server = MatrixMockServer::new().await;
        let session = session_for(&server).await;
        let target = matrix_sdk::ruma::user_id!("@dm-target:example.org");
        let dm_room_id = matrix_sdk::ruma::room_id!("!existing-dm:example.org");

        // Seed a joined room marked as the DM with the target through
        // m.direct. No createRoom endpoint is mounted, so an accidental
        // create_dm would fail the call instead of silently passing.
        server
            .mock_sync()
            .ok_and_run(&session.client(), |builder| {
                builder.add_custom_global_account_data(json!({
                    "type": "m.direct",
                    "content": { target: [dm_room_id] }
                }));
                builder.add_joined_room(JoinedRoomBuilder::new(dm_room_id));
            })
            .await;

        let started = super::start_direct_message(&session, target.as_str())
            .await
            .expect("existing DM must be reused without a create call");
        assert_eq!(started, dm_room_id.to_string());
    }

    #[tokio::test]
    async fn start_direct_message_creates_a_room_only_when_no_dm_exists() {
        let server = MatrixMockServer::new().await;
        let session = session_for(&server).await;
        let target = matrix_sdk::ruma::user_id!("@fresh-target:example.org");

        server.mock_create_room().ok().mock_once().mount().await;

        let started = super::start_direct_message(&session, target.as_str())
            .await
            .expect("a missing DM must be created");
        // The prebuilt createRoom mock answers with this fixed room id.
        assert_eq!(started, "!room:example.org");
    }
}

pub async fn join_room_by_id(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<String, MatrixRoomOperationError> {
    koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "sdk.room_operation",
        "join_started",
    ));
    let room_id = matrix_sdk::ruma::RoomId::parse(room_id)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
    let room = match session.client().join_room_by_id(&room_id).await {
        Ok(room) => room,
        Err(error) => {
            koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
                DiagnosticLevel::Warn,
                "sdk.room_operation",
                "join_failed",
            ));
            return Err(MatrixRoomOperationError::from_sdk_error(error));
        }
    };
    koushi_diagnostics::record_and_stderr(DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "sdk.room_operation",
        "join_completed",
    ));
    Ok(room.room_id().to_string())
}

pub async fn query_public_room_directory(
    session: &MatrixClientSession,
    query: MatrixPublicRoomDirectoryQuery,
) -> Result<MatrixPublicRoomDirectoryResult, MatrixRoomOperationError> {
    let mut filter = matrix_sdk::ruma::directory::Filter::new();
    filter.generic_search_term = query.term;

    let mut request =
        matrix_sdk::ruma::api::client::directory::get_public_rooms_filtered::v3::Request::new();
    request.filter = filter;
    request.limit = query.limit.map(Into::into);
    request.since = query.since;
    request.server = query
        .server_name
        .map(matrix_sdk::ruma::OwnedServerName::try_from)
        .transpose()
        .map_err(|_| MatrixRoomOperationError::InvalidServerName)?;

    let response = session
        .client()
        .public_rooms_filtered(request)
        .await
        .map_err(|error| MatrixRoomOperationError::from_sdk_error(error.into()))?;

    Ok(MatrixPublicRoomDirectoryResult {
        rooms: response
            .chunk
            .into_iter()
            .map(matrix_public_room_from_chunk)
            .collect(),
        next_batch: response.next_batch,
    })
}

/// Coarse joinability of a previewed room, for deciding what to offer.
///
/// The exact join rule is server policy; the GUI only needs to know whether a
/// plain Join is expected to work, so restricted/knock variants collapse here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixPreviewJoinability {
    /// Anyone may join.
    Open,
    /// An invite (or a knock) is required first.
    InviteOnly,
    /// Joining depends on membership of another room.
    Restricted,
    /// The server did not report a join rule.
    Unknown,
}

/// Membership the current account already has in a previewed room.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixPreviewMembership {
    Joined,
    Invited,
    /// Not a member, or the room is unknown to this account.
    None,
}

/// A private-data-minimized preview of a room the user has not joined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixRoomPreview {
    pub room_id: String,
    pub canonical_alias: Option<String>,
    /// Matrix `room_type`, e.g. `m.space`. Absent for an ordinary room.
    pub room_type: Option<String>,
    /// Empty when the room has no name; the caller supplies the fallback.
    pub name: String,
    pub topic: Option<String>,
    pub joined_members: u64,
    pub joinability: MatrixPreviewJoinability,
    pub membership: MatrixPreviewMembership,
}

/// Project an SDK room preview into the private-data-minimized DTO.
pub(super) fn matrix_room_preview_from_sdk(
    preview: matrix_sdk::room_preview::RoomPreview,
) -> MatrixRoomPreview {
    use matrix_sdk::ruma::room::JoinRuleSummary;

    let joinability = match preview.join_rule {
        Some(JoinRuleSummary::Public) => MatrixPreviewJoinability::Open,
        Some(JoinRuleSummary::Invite | JoinRuleSummary::Knock | JoinRuleSummary::Private) => {
            MatrixPreviewJoinability::InviteOnly
        }
        Some(JoinRuleSummary::Restricted(_) | JoinRuleSummary::KnockRestricted(_)) => {
            MatrixPreviewJoinability::Restricted
        }
        _ => MatrixPreviewJoinability::Unknown,
    };
    let membership = match preview.state {
        Some(matrix_sdk::RoomState::Joined) => MatrixPreviewMembership::Joined,
        Some(matrix_sdk::RoomState::Invited) => MatrixPreviewMembership::Invited,
        _ => MatrixPreviewMembership::None,
    };
    MatrixRoomPreview {
        room_id: preview.room_id.to_string(),
        canonical_alias: preview.canonical_alias.map(|alias| alias.to_string()),
        room_type: preview.room_type.map(|room_type| room_type.to_string()),
        name: preview.name.unwrap_or_default(),
        topic: preview.topic,
        joined_members: preview.num_joined_members,
        joinability,
        membership,
    }
}

pub async fn preview_join_target(
    session: &MatrixClientSession,
    target: &MatrixJoinTarget,
) -> Result<MatrixRoomPreview, MatrixRoomOperationError> {
    let (room_or_alias, via) = resolve_join_target(target)?;
    let preview = session
        .client()
        .get_room_preview(room_or_alias.as_ref(), via)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(matrix_room_preview_from_sdk(preview))
}

/// A room to join, as named by a directory result or a `matrix.to` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixJoinTarget {
    /// `#alias:server` or `!id:server`.
    pub room_id_or_alias: String,
    /// Servers to try when the homeserver does not already know the room.
    pub via_servers: Vec<String>,
}

/// Resolve a join target into the ids the SDK join call needs.
///
/// A room id is a legitimate join target, not a malformed alias: links carry
/// ids far more often than aliases. Every `via` server is kept, because for an
/// id target they are the only way the homeserver can locate a room it has
/// never seen - dropping all but the first silently breaks federated joins.
pub(super) fn resolve_join_target(
    target: &MatrixJoinTarget,
) -> Result<
    (
        matrix_sdk::ruma::OwnedRoomOrAliasId,
        Vec<matrix_sdk::ruma::OwnedServerName>,
    ),
    MatrixRoomOperationError,
> {
    let room_or_alias = matrix_sdk::ruma::RoomOrAliasId::parse(&target.room_id_or_alias)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomAlias)?;
    let via = target
        .via_servers
        .iter()
        .map(|server| {
            matrix_sdk::ruma::OwnedServerName::try_from(server.as_str())
                .map_err(|_| MatrixRoomOperationError::InvalidServerName)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((room_or_alias, via))
}

pub async fn join_room_target(
    session: &MatrixClientSession,
    target: &MatrixJoinTarget,
) -> Result<String, MatrixRoomOperationError> {
    let (room_or_alias, via) = resolve_join_target(target)?;
    let room = session
        .client()
        .join_room_by_id_or_alias(room_or_alias.as_ref(), &via)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(room.room_id().to_string())
}

pub async fn leave_room(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<String, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let room_id = room.room_id().to_string();
    room.leave()
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(room_id)
}

pub async fn forget_room(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<String, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let room_id = room.room_id().to_string();
    room.forget()
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(room_id)
}

pub async fn set_space_child(
    session: &MatrixClientSession,
    space_id: &str,
    child_room_id: &str,
    via_server: &str,
) -> Result<(), MatrixRoomOperationError> {
    let space = matrix_room(session, space_id)?;
    let child_room_id = matrix_sdk::ruma::OwnedRoomId::try_from(child_room_id)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
    let via_server = matrix_sdk::ruma::OwnedServerName::try_from(via_server)
        .map_err(|_| MatrixRoomOperationError::InvalidServerName)?;
    let content =
        matrix_sdk::ruma::events::space::child::SpaceChildEventContent::new(vec![via_server]);

    space
        .send_state_event_for_key(&child_room_id, content)
        .await
        .map(|_| ())
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub fn room_id_server_name(room_id: &str) -> Result<String, MatrixRoomOperationError> {
    let room_id = matrix_sdk::ruma::RoomId::parse(room_id)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
    room_id
        .server_name()
        .map(ToString::to_string)
        .ok_or(MatrixRoomOperationError::InvalidRoomId)
}

pub async fn set_room_tag(
    session: &MatrixClientSession,
    room_id: &str,
    tag: MatrixRoomTagKind,
    order: Option<f64>,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    match tag {
        MatrixRoomTagKind::Favourite => room.set_is_favourite(true, order).await,
        MatrixRoomTagKind::LowPriority => room.set_is_low_priority(true, order).await,
    }
    .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn remove_room_tag(
    session: &MatrixClientSession,
    room_id: &str,
    tag: MatrixRoomTagKind,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    match tag {
        MatrixRoomTagKind::Favourite => room.set_is_favourite(false, None).await,
        MatrixRoomTagKind::LowPriority => room.set_is_low_priority(false, None).await,
    }
    .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn pin_event(
    session: &MatrixClientSession,
    room_id: &str,
    event_id: &str,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let event_id = matrix_sdk::ruma::EventId::parse(event_id)
        .map_err(|_| MatrixRoomOperationError::InvalidEventId)?;
    room.pin_event(&event_id)
        .await
        .map(|_| ())
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn unpin_event(
    session: &MatrixClientSession,
    room_id: &str,
    event_id: &str,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let event_id = matrix_sdk::ruma::EventId::parse(event_id)
        .map_err(|_| MatrixRoomOperationError::InvalidEventId)?;
    room.unpin_event(&event_id)
        .await
        .map(|_| ())
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn mark_room_as_read(
    session: &MatrixClientSession,
    room_id: &str,
    event_id: &str,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let event_id = matrix_sdk::ruma::EventId::parse(event_id)
        .map_err(|_| MatrixRoomOperationError::InvalidEventId)?;
    let receipts = matrix_sdk::room::Receipts::new()
        .fully_read_marker(event_id.clone())
        .private_read_receipt(event_id);
    room.send_multiple_receipts(receipts)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

pub async fn mark_room_as_unread(
    session: &MatrixClientSession,
    room_id: &str,
    unread: bool,
) -> Result<(), MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    room.set_unread_flag(unread)
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)
}

const ROOM_NOTIFICATION_RULE_ID_PREFIX: &str = "org.matrix.desktop.notify.room.";

fn room_notification_rule_id(room_id: &matrix_sdk::ruma::RoomId) -> String {
    format!("{ROOM_NOTIFICATION_RULE_ID_PREFIX}{room_id}")
}

/// Sets the per-room notification mode by manipulating app-owned push rules.
///
/// - `All`: removes any app-owned override/underride rule for the room.
/// - `Mentions`: adds an underride rule with empty actions so generic message
///   rules are suppressed but mention/highlight rules still fire.
/// - `Mute`: adds an override rule with empty actions so all notifications for
///   the room are suppressed.
pub async fn set_room_notification_mode(
    session: &MatrixClientSession,
    room_id: &str,
    mode: koushi_state::RoomNotificationMode,
) -> Result<(), MatrixRoomOperationError> {
    use matrix_sdk::ruma::{
        RoomId,
        api::client::push::{delete_pushrule, set_pushrule},
        push::{
            EventMatchConditionData, NewConditionalPushRule, NewPushRule, PushCondition, RuleKind,
        },
    };

    let room_id = RoomId::parse(room_id).map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
    let rule_id = room_notification_rule_id(&room_id);
    let client = session.client();

    // Remove any previous app-owned rule for this room. Missing-rule errors are
    // ignored so the operation is idempotent.
    for kind in [RuleKind::Override, RuleKind::Underride] {
        let delete_request = delete_pushrule::v3::Request::new(kind, rule_id.clone());
        let _ = client.send(delete_request).await;
    }

    if mode != koushi_state::RoomNotificationMode::All {
        let actions = Vec::new();
        let conditions = vec![PushCondition::EventMatch(EventMatchConditionData::new(
            "room_id".to_owned(),
            room_id.to_string(),
        ))];
        let new_rule = NewConditionalPushRule::new(rule_id, conditions, actions);
        let new_push_rule = match mode {
            koushi_state::RoomNotificationMode::Mentions => NewPushRule::Underride(new_rule),
            koushi_state::RoomNotificationMode::Mute => NewPushRule::Override(new_rule),
            koushi_state::RoomNotificationMode::All => unreachable!(),
        };
        let set_request = set_pushrule::v3::Request::new(new_push_rule);
        client.send(set_request).await.map_err(|error| {
            MatrixRoomOperationError::from_sdk_error(matrix_sdk::Error::Http(Box::new(error)))
        })?;
    }

    Ok(())
}

pub async fn load_pinned_event_ids(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<Vec<String>, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    let pinned = room
        .load_pinned_events()
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?
        .unwrap_or_default();
    Ok(pinned
        .into_iter()
        .map(|event_id| event_id.to_string())
        .collect())
}

/// Whether the room's current membership is joined (issue #538: the actor
/// re-checks this when settling a manual encryption-debug completion, so a
/// completion that lands after the user left the room fails closed).
pub async fn room_is_joined(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<bool, MatrixRoomOperationError> {
    let room = matrix_room(session, room_id)?;
    Ok(room.state() == matrix_sdk_base::RoomState::Joined)
}

#[cfg(test)]
mod tests {
    use super::{
        MatrixCreateRoomOptions, MatrixCreateRoomParentSpace, MatrixCreateRoomVisibility,
        MatrixJoinTarget, MatrixPreviewJoinability, MatrixPreviewMembership,
        MatrixPublicRoomDirectoryQuery, MatrixPublicRoomDirectoryRoom, MatrixRoomHistoryVisibility,
        MatrixRoomJoinRule, MatrixRoomMemberRole, MatrixRoomModerationAction,
        MatrixRoomOperationError, MatrixRoomPermissionFacts, MatrixRoomSettingChange,
        MatrixRoomSettingsSnapshot, create_public_directory_room, create_room_request,
        get_room_settings_snapshot, join_room_target, matrix_room_preview_from_sdk,
        moderate_room_member, query_public_room_directory, resolve_join_target,
        update_room_member_power_level, update_room_setting,
    };

    use crate::room_projection::{
        matrix_public_room_from_chunk, matrix_room_member_role, room_settings_snapshot_with_change,
        room_settings_snapshot_with_member_power_level,
    };

    #[test]
    fn mark_room_as_read_sends_read_marker_with_private_receipt() {
        let source = include_str!("room_operations.rs");
        let body = crate::test_source::item_body(source, "pub async fn mark_room_as_read");

        assert!(
            body.contains("send_multiple_receipts"),
            "mark_room_as_read must persist the read marker and unread-count receipt through one SDK request"
        );
        assert!(
            body.contains("fully_read_marker"),
            "mark_room_as_read must update the user's fully-read marker"
        );
        assert!(
            body.contains("private_read_receipt"),
            "mark_room_as_read must reset unread counts without publishing a public read receipt"
        );
        assert!(
            !body.contains("send_single_receipt(ReceiptType::FullyRead"),
            "fully-read alone must not be treated as sufficient to clear persistent unread counts"
        );
    }
    #[test]
    fn cancel_space_invite_validates_invite_membership_before_kicking() {
        let _cancelled = super::MatrixSpaceInviteCancellationOutcome::Cancelled;
        let _not_invited = super::MatrixSpaceInviteCancellationOutcome::NotInvited;
        let source = include_str!("room_operations.rs");
        let body = crate::test_source::item_body(source, "pub async fn cancel_space_invite");
        let invite_lookup = body
            .find("members_no_sync(matrix_sdk::RoomMemberships::INVITE)")
            .expect("cancellation must load current INVITE membership");
        let not_invited = body
            .find("MatrixSpaceInviteCancellationOutcome::NotInvited")
            .expect("cancellation must have a no-op NotInvited outcome");
        let kick = body
            .find(".kick_user(")
            .expect("cancellation must use the Matrix kick transport");

        assert!(invite_lookup < not_invited);
        assert!(not_invited < kick);
        assert!(body.contains("MatrixSpaceInviteCancellationOutcome::Cancelled"));
    }
    #[test]
    fn create_room_request_projects_space_room_options() {
        let request = create_room_request(MatrixCreateRoomOptions {
            name: "Synthetic Ops".to_owned(),
            topic: Some("Deployment notes".to_owned()),
            alias_localpart: None,
            encrypted: true,
            visibility: MatrixCreateRoomVisibility::Private,
            parent_space: Some(MatrixCreateRoomParentSpace {
                space_id: "!space:example.invalid".to_owned(),
                via_server: "example.invalid".to_owned(),
            }),
        })
        .expect("request should build");

        assert_eq!(request.name.as_deref(), Some("Synthetic Ops"));
        assert_eq!(request.topic.as_deref(), Some("Deployment notes"));
        assert_eq!(
            request
                .room_version
                .as_ref()
                .map(|version| version.as_str()),
            Some("9")
        );
        let initial_state = initial_state_json(&request);
        assert!(initial_state.iter().any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("m.room.encryption")
        }));
        assert!(initial_state.iter().any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("m.space.parent")
                && event.get("state_key").and_then(serde_json::Value::as_str)
                    == Some("!space:example.invalid")
        }));
        let join_rules = initial_state
            .iter()
            .find(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("m.room.join_rules")
            })
            .expect("join rules");
        assert_eq!(
            join_rules
                .get("content")
                .and_then(|content| content.get("join_rule"))
                .and_then(serde_json::Value::as_str),
            Some("restricted")
        );
        let history_visibility = initial_state
            .iter()
            .find(|event| {
                event.get("type").and_then(serde_json::Value::as_str)
                    == Some("m.room.history_visibility")
            })
            .expect("history visibility");
        assert_eq!(
            history_visibility
                .get("content")
                .and_then(|content| content.get("history_visibility"))
                .and_then(serde_json::Value::as_str),
            Some("invited")
        );
    }
    #[test]
    fn create_room_request_projects_public_alias_without_encryption() {
        let request = create_room_request(MatrixCreateRoomOptions {
            name: "Synthetic Public".to_owned(),
            topic: None,
            alias_localpart: Some("synthetic-public".to_owned()),
            encrypted: true,
            visibility: MatrixCreateRoomVisibility::Public,
            parent_space: None,
        })
        .expect("request should build");

        assert_eq!(request.room_alias_name.as_deref(), Some("synthetic-public"));
        assert_eq!(
            request.visibility,
            matrix_sdk::ruma::api::client::room::Visibility::Public
        );
        let initial_state = initial_state_json(&request);
        assert!(!initial_state.iter().any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("m.room.encryption")
        }));
    }
    fn initial_state_json(
        request: &matrix_sdk::ruma::api::client::room::create_room::v3::Request,
    ) -> Vec<serde_json::Value> {
        request
            .initial_state
            .iter()
            .map(|event| {
                serde_json::from_str::<serde_json::Value>(event.json().get())
                    .expect("initial state event JSON")
            })
            .collect()
    }
    #[test]
    fn room_tag_operations_use_sdk_tag_methods() {
        let source = include_str!("room_operations.rs");

        assert!(source.contains("set_is_favourite(true"));
        assert!(source.contains("set_is_favourite(false"));
        assert!(source.contains("set_is_low_priority(true"));
        assert!(source.contains("set_is_low_priority(false"));
    }
    #[test]
    fn pin_operations_use_sdk_pinned_event_methods() {
        let source = include_str!("room_operations.rs");
        let pin_body = crate::test_source::item_body(source, "pub async fn pin_event");
        let unpin_body = crate::test_source::item_body(source, "pub async fn unpin_event");

        assert!(pin_body.contains(".pin_event(&event_id)"));
        assert!(unpin_body.contains(".unpin_event(&event_id)"));
    }
    #[test]
    fn join_target_accepts_a_room_id_because_links_carry_ids_more_often_than_aliases() {
        let target = MatrixJoinTarget {
            room_id_or_alias: "!room:example.invalid".to_owned(),
            via_servers: Vec::new(),
        };

        let (resolved, _via) = resolve_join_target(&target).expect("room id is a join target");

        assert_eq!(resolved.as_str(), "!room:example.invalid");
    }
    #[test]
    fn join_target_keeps_every_via_server_so_a_federated_room_stays_reachable() {
        let target = MatrixJoinTarget {
            room_id_or_alias: "!room:example.invalid".to_owned(),
            via_servers: vec!["first.invalid".to_owned(), "second.invalid".to_owned()],
        };

        let (_resolved, via) = resolve_join_target(&target).expect("room id is a join target");

        // For an id target these are the only routing hints the homeserver has.
        let names = via.iter().map(|server| server.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["first.invalid", "second.invalid"]);
    }
    #[test]
    fn join_target_still_accepts_an_alias() {
        let target = MatrixJoinTarget {
            room_id_or_alias: "#room:example.invalid".to_owned(),
            via_servers: Vec::new(),
        };

        let (resolved, via) = resolve_join_target(&target).expect("alias is a join target");

        assert_eq!(resolved.as_str(), "#room:example.invalid");
        assert!(via.is_empty());
    }
    #[test]
    fn join_target_rejects_input_that_is_neither_a_room_id_nor_an_alias() {
        let target = MatrixJoinTarget {
            room_id_or_alias: "room-without-a-sigil".to_owned(),
            via_servers: Vec::new(),
        };

        assert!(matches!(
            resolve_join_target(&target),
            Err(MatrixRoomOperationError::InvalidRoomAlias)
        ));
    }
    #[test]
    fn join_target_rejects_an_unusable_via_server_instead_of_dropping_it() {
        let target = MatrixJoinTarget {
            room_id_or_alias: "!room:example.invalid".to_owned(),
            via_servers: vec!["not a server name".to_owned()],
        };

        assert!(matches!(
            resolve_join_target(&target),
            Err(MatrixRoomOperationError::InvalidServerName)
        ));
    }
    fn sdk_room_preview(
        join_rule: Option<matrix_sdk::ruma::room::JoinRuleSummary>,
        state: Option<matrix_sdk::RoomState>,
    ) -> matrix_sdk::room_preview::RoomPreview {
        matrix_sdk::room_preview::RoomPreview {
            room_id: matrix_sdk::ruma::room_id!("!previewed:example.invalid").to_owned(),
            canonical_alias: None,
            name: None,
            topic: None,
            avatar_url: None,
            num_joined_members: 7,
            num_active_members: None,
            room_type: Some(matrix_sdk::ruma::room::RoomType::Space),
            join_rule,
            is_world_readable: None,
            state,
            is_direct: None,
            heroes: None,
        }
    }
    #[test]
    fn preview_reports_an_invite_only_room_as_not_plainly_joinable() {
        let preview = matrix_room_preview_from_sdk(sdk_room_preview(
            Some(matrix_sdk::ruma::room::JoinRuleSummary::Invite),
            None,
        ));

        // Offering a plain Join here would produce a silent forbidden failure.
        assert_eq!(preview.joinability, MatrixPreviewJoinability::InviteOnly);
        assert_eq!(preview.membership, MatrixPreviewMembership::None);
    }
    #[test]
    fn preview_reports_existing_membership_so_the_gui_navigates_instead_of_joining() {
        let preview = matrix_room_preview_from_sdk(sdk_room_preview(
            Some(matrix_sdk::ruma::room::JoinRuleSummary::Public),
            Some(matrix_sdk::RoomState::Joined),
        ));

        assert_eq!(preview.membership, MatrixPreviewMembership::Joined);
        assert_eq!(preview.joinability, MatrixPreviewJoinability::Open);
    }
    #[test]
    fn preview_keeps_the_room_type_and_leaves_an_unnamed_room_unlabelled() {
        let preview = matrix_room_preview_from_sdk(sdk_room_preview(None, None));

        assert_eq!(preview.room_type.as_deref(), Some("m.space"));
        assert_eq!(preview.name, "");
        // No join rule reported is not the same as "anyone may join".
        assert_eq!(preview.joinability, MatrixPreviewJoinability::Unknown);
        assert_eq!(preview.joined_members, 7);
    }
    #[test]
    fn directory_chunk_keeps_the_room_type_so_a_space_is_distinguishable() {
        let mut chunk = matrix_sdk::ruma::directory::PublicRoomsChunk::from(
            matrix_sdk::ruma::directory::PublicRoomsChunkInit {
                num_joined_members: matrix_sdk::ruma::UInt::from(3u32),
                room_id: matrix_sdk::ruma::room_id!("!space:example.invalid").to_owned(),
                world_readable: true,
                guest_can_join: false,
            },
        );
        chunk.room_type = Some(matrix_sdk::ruma::room::RoomType::Space);

        let room = matrix_public_room_from_chunk(chunk);

        // Without this the entry is indistinguishable from an ordinary room.
        assert_eq!(room.room_type.as_deref(), Some("m.space"));
    }
    #[test]
    fn directory_chunk_without_a_name_stays_empty_rather_than_claiming_to_be_a_room() {
        let chunk = matrix_sdk::ruma::directory::PublicRoomsChunk::from(
            matrix_sdk::ruma::directory::PublicRoomsChunkInit {
                num_joined_members: matrix_sdk::ruma::UInt::from(0u32),
                room_id: matrix_sdk::ruma::room_id!("!unnamed:example.invalid").to_owned(),
                world_readable: false,
                guest_can_join: false,
            },
        );

        let room = matrix_public_room_from_chunk(chunk);

        // A hardcoded "Public room" would mislabel an unnamed space.
        assert_eq!(room.name, "");
        assert_eq!(room.room_type, None);
    }
    #[test]
    fn directory_operations_use_public_room_and_alias_join_apis() {
        let _query = MatrixPublicRoomDirectoryQuery {
            term: Some("synthetic".to_owned()),
            server_name: Some("example.invalid".to_owned()),
            limit: Some(10),
            since: None,
        };
        let _room = MatrixPublicRoomDirectoryRoom {
            room_id: "!room:example.invalid".to_owned(),
            canonical_alias: Some("#room:example.invalid".to_owned()),
            room_type: None,
            name: "Synthetic Room".to_owned(),
            topic: None,
            avatar_url: None,
            joined_members: 1,
            world_readable: true,
            guest_can_join: false,
        };
        let _query_fn = query_public_room_directory;
        let _join_fn = join_room_target;
        let _create_public_fn = create_public_directory_room;
    }
    #[test]
    fn room_management_wrappers_use_settings_privacy_and_moderation_apis() {
        let _snapshot = MatrixRoomSettingsSnapshot {
            room_id: "!room:example.invalid".to_owned(),
            name: Some("Synthetic Room".to_owned()),
            topic: Some("Synthetic topic".to_owned()),
            avatar_url: None,
            canonical_alias: None,
            alternate_aliases: Vec::new(),
            join_rule: MatrixRoomJoinRule::Invite,
            history_visibility: MatrixRoomHistoryVisibility::Shared,
            permissions: MatrixRoomPermissionFacts {
                can_edit_settings: true,
                can_edit_roles: true,
                can_invite: true,
                can_kick: true,
                can_ban: true,
                can_unban: false,
            },
            members: vec![super::MatrixRoomMemberSummary {
                user_id: "@member:example.invalid".to_owned(),
                display_name: Some("Synthetic Member".to_owned()),
                avatar_url: None,
                power_level: Some(50),
                role: MatrixRoomMemberRole::Moderator,
                user_trust: None,
            }],
        };
        let _change = MatrixRoomSettingChange::JoinRule(MatrixRoomJoinRule::Public);
        let _moderation = MatrixRoomModerationAction::Kick;
        let _snapshot_fn = get_room_settings_snapshot;
        let _update_fn = update_room_setting;
        let _moderate_fn = moderate_room_member;
        let _role_fn = update_room_member_power_level;

        let source = include_str!("room_operations.rs");
        assert!(source.contains(".set_name("));
        assert!(source.contains(".set_room_topic("));
        assert!(source.contains(".set_avatar_url("));
        assert!(source.contains(".remove_avatar("));
        assert!(source.contains(".privacy_settings()"));
        assert!(source.contains(".update_join_rule("));
        assert!(source.contains(".update_room_history_visibility("));
        assert!(source.contains(".kick_user("));
        assert!(source.contains(".ban_user("));
        assert!(source.contains(".unban_user("));
        assert!(source.contains(".update_power_levels("));
        assert!(source.contains(".user_can_invite(own_user_id)"));
    }
    #[test]
    fn room_setting_update_projects_the_sent_change_into_the_success_snapshot() {
        let original = MatrixRoomSettingsSnapshot {
            room_id: "!room:example.invalid".to_owned(),
            name: Some("Original Room".to_owned()),
            topic: Some("Original topic".to_owned()),
            avatar_url: Some("mxc://example.invalid/original".to_owned()),
            canonical_alias: None,
            alternate_aliases: Vec::new(),
            join_rule: MatrixRoomJoinRule::Invite,
            history_visibility: MatrixRoomHistoryVisibility::Shared,
            permissions: MatrixRoomPermissionFacts {
                can_edit_settings: true,
                can_edit_roles: true,
                can_invite: true,
                can_kick: true,
                can_ban: true,
                can_unban: true,
            },
            members: vec![],
        };

        assert_eq!(
            room_settings_snapshot_with_change(
                original.clone(),
                &MatrixRoomSettingChange::Topic(Some("Updated topic".to_owned())),
            )
            .topic
            .as_deref(),
            Some("Updated topic")
        );
        assert_eq!(
            room_settings_snapshot_with_change(
                original.clone(),
                &MatrixRoomSettingChange::Name(None),
            )
            .name,
            None
        );
        assert_eq!(
            room_settings_snapshot_with_change(
                original.clone(),
                &MatrixRoomSettingChange::AvatarUrl(None),
            )
            .avatar_url,
            None
        );
        assert_eq!(
            room_settings_snapshot_with_change(
                original.clone(),
                &MatrixRoomSettingChange::JoinRule(MatrixRoomJoinRule::Public),
            )
            .join_rule,
            MatrixRoomJoinRule::Public
        );
        assert_eq!(
            room_settings_snapshot_with_change(
                original,
                &MatrixRoomSettingChange::HistoryVisibility(MatrixRoomHistoryVisibility::Joined,),
            )
            .history_visibility,
            MatrixRoomHistoryVisibility::Joined
        );
    }
    #[test]
    fn room_member_power_level_projection_updates_role_in_success_snapshot() {
        let original = MatrixRoomSettingsSnapshot {
            room_id: "!room:example.invalid".to_owned(),
            name: Some("Original Room".to_owned()),
            topic: Some("Original topic".to_owned()),
            avatar_url: None,
            canonical_alias: None,
            alternate_aliases: Vec::new(),
            join_rule: MatrixRoomJoinRule::Invite,
            history_visibility: MatrixRoomHistoryVisibility::Shared,
            permissions: MatrixRoomPermissionFacts {
                can_edit_settings: true,
                can_edit_roles: true,
                can_invite: true,
                can_kick: true,
                can_ban: true,
                can_unban: true,
            },
            members: vec![super::MatrixRoomMemberSummary {
                user_id: "@member:example.invalid".to_owned(),
                display_name: Some("Synthetic Member".to_owned()),
                avatar_url: None,
                power_level: Some(0),
                role: MatrixRoomMemberRole::User,
                user_trust: None,
            }],
        };

        let updated =
            room_settings_snapshot_with_member_power_level(original, "@member:example.invalid", 50);
        let member = updated.members.first().expect("member summary");
        assert_eq!(member.power_level, Some(50));
        assert_eq!(member.role, MatrixRoomMemberRole::Moderator);
        assert_eq!(
            matrix_room_member_role(Some(100)),
            MatrixRoomMemberRole::Administrator
        );
        assert_eq!(matrix_room_member_role(None), MatrixRoomMemberRole::Creator);
    }
}
