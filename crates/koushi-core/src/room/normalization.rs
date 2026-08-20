// ---------------------------------------------------------------------------
// Normalization helpers: auth snapshot → state DTOs
// ---------------------------------------------------------------------------

/// Convert `MatrixRoomListSnapshot` spaces into `SpaceSummary` values with
/// child room id lists. Homeservers may sync one side of the Matrix space
/// relationship before the other, so the projection uses both the space's
/// `m.space.child` state and rooms' `m.space.parent` state.
fn normalize_spaces(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<SpaceSummary> {
    snapshot
        .spaces
        .iter()
        .map(|space| {
            let child_room_ids = normalize_space_child_room_ids(snapshot, space);
            SpaceSummary {
                space_id: space.space_id.clone(),
                display_name: space.display_name.clone(),
                avatar: avatar_from_mxc_uri(space.avatar_mxc_uri.as_deref()),
                child_room_ids,
            }
        })
        .collect()
}

fn normalize_space_child_room_ids(
    snapshot: &koushi_sdk::MatrixRoomListSnapshot,
    space: &koushi_sdk::MatrixRoomListSpace,
) -> Vec<String> {
    let mut child_room_ids = BTreeSet::new();
    child_room_ids.extend(space.child_room_ids.iter().cloned());
    child_room_ids.extend(
        snapshot
            .rooms
            .iter()
            .filter(|room| room.parent_space_ids.iter().any(|id| id == &space.space_id))
            .map(|room| room.room_id.clone()),
    );
    child_room_ids.into_iter().collect()
}

/// Convert `MatrixRoomListSnapshot` rooms into `RoomSummary` values.
fn normalize_rooms(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<RoomSummary> {
    let mut rooms: Vec<RoomSummary> = snapshot
        .rooms
        .iter()
        .map(|room| {
            let display_label = room
                .display_name
                .trim()
                .is_empty()
                .then(|| room.room_id.clone())
                .unwrap_or_else(|| room.display_name.trim().to_owned());
            RoomSummary {
                room_id: room.room_id.clone(),
                display_name: room.display_name.clone(),
                display_label: display_label.clone(),
                original_display_label: display_label,
                avatar: avatar_from_mxc_uri(room.avatar_mxc_uri.as_deref()),
                is_dm: room.is_dm,
                dm_user_ids: room.dm_user_ids.clone(),
                tags: normalize_room_tags(&room.tags),
                unread_count: room.unread_count,
                notification_count: room.notification_count,
                highlight_count: room.highlight_count,
                marked_unread: room.marked_unread,
                recency_stamp: room.recency_stamp,
                conversation_activity: room.conversation_activity.map(|activity| {
                    koushi_state::ConversationActivity {
                        timestamp_ms: activity.timestamp_ms,
                        source: match activity.source {
                            koushi_sdk::MatrixConversationActivitySource::Message => {
                                koushi_state::ConversationActivitySource::Message
                            }
                            koushi_sdk::MatrixConversationActivitySource::EncryptedMessage => {
                                koushi_state::ConversationActivitySource::EncryptedMessage
                            }
                            koushi_sdk::MatrixConversationActivitySource::ThreadReply => {
                                koushi_state::ConversationActivitySource::ThreadReply
                            }
                        },
                    }
                }),
                latest_event: room.latest_event.as_ref().map(|event| {
                    koushi_state::RoomLatestEventSummary {
                        event_id: event.event_id.clone(),
                        relation_type: event.relation_type.clone(),
                        relation_event_id: event.relation_event_id.clone(),
                        sender_id: event.sender_id.clone(),
                        sender_label: event.sender_label.clone(),
                        sender_avatar: avatar_from_mxc_uri(event.sender_avatar_mxc_uri.as_deref()),
                        preview: event.preview.clone(),
                        timestamp_ms: event.timestamp_ms,
                    }
                }),
                parent_space_ids: normalize_room_parent_space_ids(snapshot, room),
                dm_space_ids: Vec::new(),
                is_encrypted: room.is_encrypted,
                joined_members: room.joined_members,
            }
        })
        .collect();
    let space_members: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        snapshot
            .spaces
            .iter()
            .map(|s| {
                (
                    s.space_id.clone(),
                    s.member_user_ids.iter().cloned().collect(),
                )
            })
            .collect();
    assign_dm_space_ids(&mut rooms, &space_members);
    rooms
}

fn normalize_room_parent_space_ids(
    snapshot: &koushi_sdk::MatrixRoomListSnapshot,
    room: &koushi_sdk::MatrixRoomListRoom,
) -> Vec<String> {
    let mut parent_space_ids: BTreeSet<String> = room.parent_space_ids.iter().cloned().collect();
    parent_space_ids.extend(
        snapshot
            .spaces
            .iter()
            .filter(|space| space.child_room_ids.iter().any(|id| id == &room.room_id))
            .map(|space| space.space_id.clone()),
    );
    parent_space_ids.into_iter().collect()
}

/// Populate `dm_space_ids` on each `RoomSummary` in `rooms`.
///
/// For each DM room, `dm_space_ids` is set to the sorted list of space IDs
/// (keys of `space_members`) whose member set contains at least one of
/// `room.dm_user_ids`. Non-DM rooms always get an empty `dm_space_ids`.
///
/// The result is deterministically ordered because `space_members` is a
/// `BTreeMap` and iteration yields keys in ascending order.
pub fn assign_dm_space_ids(
    rooms: &mut [koushi_state::RoomSummary],
    space_members: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    for room in rooms.iter_mut() {
        if !room.is_dm {
            room.dm_space_ids = Vec::new();
            continue;
        }
        room.dm_space_ids = space_members
            .iter()
            .filter(|(_space_id, members)| room.dm_user_ids.iter().any(|uid| members.contains(uid)))
            .map(|(space_id, _)| space_id.clone())
            .collect();
    }
}

fn normalize_room_tags(tags: &MatrixRoomTags) -> RoomTags {
    RoomTags {
        favourite: tags.favourite.as_ref().map(|info| RoomTagInfo {
            order: info.order.clone(),
        }),
        low_priority: tags.low_priority.as_ref().map(|info| RoomTagInfo {
            order: info.order.clone(),
        }),
    }
}

fn normalize_user_profiles(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<UserProfile> {
    snapshot
        .user_profiles
        .iter()
        .map(|profile| {
            let display_label = profile
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|display_name| !display_name.is_empty())
                .unwrap_or(profile.user_id.as_str())
                .to_owned();
            UserProfile {
                user_id: profile.user_id.clone(),
                display_name: profile.display_name.clone(),
                display_label: display_label.clone(),
                original_display_label: display_label,
                mention_search_terms: user_profile_mention_search_terms(
                    &profile.user_id,
                    profile.display_name.as_deref(),
                ),
                avatar: avatar_from_mxc_uri(profile.avatar_mxc_uri.as_deref()),
            }
        })
        .collect()
}
fn user_profile_mention_search_terms(user_id: &str, display_name: Option<&str>) -> Vec<String> {
    let mut terms = Vec::new();
    if let Some(display_name) = display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        terms.push(display_name.to_owned());
    }
    if !terms.iter().any(|term| term == user_id) {
        terms.push(user_id.to_owned());
    }
    terms
}
fn normalize_invites(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<InvitePreview> {
    snapshot
        .invites
        .iter()
        .map(|invite| InvitePreview {
            room_id: invite.room_id.clone(),
            display_name: invite.display_name.clone(),
            avatar: avatar_from_mxc_uri(invite.avatar_mxc_uri.as_deref()),
            topic: invite.topic.clone(),
            inviter_display_name: invite.inviter_display_name.clone(),
            inviter_user_id: invite.inviter_user_id.clone(),
            is_dm: invite.is_dm,
        })
        .collect()
}

fn room_member_summary_from_sdk(member: MatrixRoomMemberSummary) -> RoomMemberSummary {
    let display_label = member
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .unwrap_or(member.user_id.as_str())
        .to_owned();
    RoomMemberSummary {
        user_id: member.user_id,
        display_name: member.display_name,
        display_label: display_label.clone(),
        original_display_label: display_label,
        avatar_url: member.avatar_url,
        power_level: member.power_level,
        role: room_member_role_from_sdk(member.role),
        user_trust: member.user_trust.map(user_trust_state_from_sdk),
    }
}

fn user_trust_state_from_sdk(state: MatrixUserTrustState) -> UserTrustState {
    match state {
        MatrixUserTrustState::Unverified => UserTrustState::Unverified,
        MatrixUserTrustState::Verified => UserTrustState::Verified,
        MatrixUserTrustState::IdentityReset => UserTrustState::IdentityReset,
    }
}

fn room_member_role_from_sdk(role: MatrixRoomMemberRole) -> RoomMemberRole {
    match role {
        MatrixRoomMemberRole::Creator => RoomMemberRole::Creator,
        MatrixRoomMemberRole::Administrator => RoomMemberRole::Administrator,
        MatrixRoomMemberRole::Moderator => RoomMemberRole::Moderator,
        MatrixRoomMemberRole::User => RoomMemberRole::User,
    }
}

fn room_join_rule_from_sdk(join_rule: MatrixRoomJoinRule) -> RoomJoinRule {
    match join_rule {
        MatrixRoomJoinRule::Public => RoomJoinRule::Public,
        MatrixRoomJoinRule::Invite => RoomJoinRule::Invite,
        MatrixRoomJoinRule::Knock => RoomJoinRule::Knock,
        MatrixRoomJoinRule::Restricted => RoomJoinRule::Restricted,
        MatrixRoomJoinRule::Private => RoomJoinRule::Private,
    }
}

fn room_join_rule_to_sdk(join_rule: RoomJoinRule) -> MatrixRoomJoinRule {
    match join_rule {
        RoomJoinRule::Public => MatrixRoomJoinRule::Public,
        RoomJoinRule::Invite => MatrixRoomJoinRule::Invite,
        RoomJoinRule::Knock => MatrixRoomJoinRule::Knock,
        RoomJoinRule::Restricted => MatrixRoomJoinRule::Restricted,
        RoomJoinRule::Private => MatrixRoomJoinRule::Private,
    }
}

fn room_history_visibility_from_sdk(
    history_visibility: MatrixRoomHistoryVisibility,
) -> RoomHistoryVisibility {
    match history_visibility {
        MatrixRoomHistoryVisibility::WorldReadable => RoomHistoryVisibility::WorldReadable,
        MatrixRoomHistoryVisibility::Shared => RoomHistoryVisibility::Shared,
        MatrixRoomHistoryVisibility::Invited => RoomHistoryVisibility::Invited,
        MatrixRoomHistoryVisibility::Joined => RoomHistoryVisibility::Joined,
    }
}

fn room_history_visibility_to_sdk(
    history_visibility: RoomHistoryVisibility,
) -> MatrixRoomHistoryVisibility {
    match history_visibility {
        RoomHistoryVisibility::WorldReadable => MatrixRoomHistoryVisibility::WorldReadable,
        RoomHistoryVisibility::Shared => MatrixRoomHistoryVisibility::Shared,
        RoomHistoryVisibility::Invited => MatrixRoomHistoryVisibility::Invited,
        RoomHistoryVisibility::Joined => MatrixRoomHistoryVisibility::Joined,
    }
}

fn room_permission_facts_from_sdk(permissions: MatrixRoomPermissionFacts) -> RoomPermissionFacts {
    RoomPermissionFacts {
        can_edit_settings: permissions.can_edit_settings,
        can_edit_roles: permissions.can_edit_roles,
        can_invite: permissions.can_invite,
        can_kick: permissions.can_kick,
        can_ban: permissions.can_ban,
        can_unban: permissions.can_unban,
    }
}

fn room_setting_change_to_sdk(change: RoomSettingChange) -> MatrixRoomSettingChange {
    match change {
        RoomSettingChange::Name(name) => MatrixRoomSettingChange::Name(name),
        RoomSettingChange::Topic(topic) => MatrixRoomSettingChange::Topic(topic),
        RoomSettingChange::AvatarUrl(avatar_url) => MatrixRoomSettingChange::AvatarUrl(avatar_url),
        RoomSettingChange::JoinRule(join_rule) => {
            MatrixRoomSettingChange::JoinRule(room_join_rule_to_sdk(join_rule))
        }
        RoomSettingChange::HistoryVisibility(history_visibility) => {
            MatrixRoomSettingChange::HistoryVisibility(room_history_visibility_to_sdk(
                history_visibility,
            ))
        }
    }
}

fn room_moderation_action_to_sdk(action: RoomModerationAction) -> MatrixRoomModerationAction {
    match action {
        RoomModerationAction::Kick => MatrixRoomModerationAction::Kick,
        RoomModerationAction::Ban => MatrixRoomModerationAction::Ban,
        RoomModerationAction::Unban => MatrixRoomModerationAction::Unban,
    }
}

fn room_moderation_allowed(
    permissions: &RoomPermissionFacts,
    action: RoomModerationAction,
) -> bool {
    match action {
        RoomModerationAction::Kick => permissions.can_kick,
        RoomModerationAction::Ban => permissions.can_ban,
        RoomModerationAction::Unban => permissions.can_unban,
    }
}

fn avatar_from_mxc_uri(mxc_uri: Option<&str>) -> Option<AvatarImage> {
    mxc_uri.map(|mxc_uri| AvatarImage {
        mxc_uri: mxc_uri.to_owned(),
        thumbnail: AvatarThumbnailState::NotRequested,
    })
}

fn sdk_room_tag_kind(tag: RoomTagKind) -> MatrixRoomTagKind {
    match tag {
        RoomTagKind::Favourite => MatrixRoomTagKind::Favourite,
        RoomTagKind::LowPriority => MatrixRoomTagKind::LowPriority,
    }
}

fn room_tag_info_from_order(order: Option<f64>) -> RoomTagInfo {
    RoomTagInfo {
        order: order.map(|order| order.to_string()),
    }
}

fn operation_failure_kind(kind: RoomFailureKind) -> OperationFailureKind {
    match kind {
        RoomFailureKind::Forbidden => OperationFailureKind::Forbidden,
        RoomFailureKind::Network => OperationFailureKind::Network,
        RoomFailureKind::NotFound => OperationFailureKind::NotFound,
        RoomFailureKind::Sdk => OperationFailureKind::Sdk,
    }
}

enum InviteTargetOutcome {
    Invited,
    AlreadyInSpace,
    Failed,
}

async fn invite_target_to_space_if_needed(
    session: &MatrixClientSession,
    space_id: &str,
    user_id: &str,
) -> InviteTargetOutcome {
    match koushi_sdk::room_has_active_member_no_sync(session, space_id, user_id).await {
        Ok(true) => return InviteTargetOutcome::AlreadyInSpace,
        Ok(false) => {}
        Err(_error) => return InviteTargetOutcome::Failed,
    }

    match koushi_sdk::invite_user_to_room(session, space_id, user_id).await {
        Ok(()) => InviteTargetOutcome::Invited,
        Err(_error) => InviteTargetOutcome::Failed,
    }
}
