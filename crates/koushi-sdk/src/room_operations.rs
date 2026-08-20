use super::*;

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

fn create_room_request(
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
fn matrix_room_preview_from_sdk(
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
fn resolve_join_target(
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
