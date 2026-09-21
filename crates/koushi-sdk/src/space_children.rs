//! Issue #961: a Space's advertised children, with the account's membership.
//!
//! The room list only carries joined rooms, so a Space's own room list could
//! never show what else the Space contains. The server answers that with
//! `/hierarchy`, which the SDK already wraps as the Element X-compatible
//! [`SpaceRoomList`]; this module drives it to completion once and projects the
//! result into Koushi's own vocabulary.
//!
//! A child the Space advertises may be absent from `/hierarchy` when it is an
//! invite-only room. If the sync client already knows that room (for example
//! because an invitation was received), use its local state to project the
//! name and membership. Only completely unknown rooms remain opaque.

use matrix_sdk::{RoomMemberships, RoomState};
use matrix_sdk_ui::spaces::{SpaceRoomList, room_list::SpaceRoomListPaginationState};
use ruma::{RoomId, events::room::join_rules::JoinRule, room::JoinRuleSummary};

use crate::{
    client_session::MatrixClientSession,
    room_operations::MatrixRoomOperationError,
    room_projection::{matrix_room, matrix_space_child_room_ids},
};

/// The account's relationship to a Space child room.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MatrixSpaceChildMembership {
    Joined,
    Invited,
    Knocked,
    Left,
    Banned,
    NotJoined,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixSpaceChildEntry {
    pub room_id: String,
    /// The child's canonical `m.room.name`, when the server reported one.
    pub raw_name: Option<String>,
    /// The name the server computed, or the room ID when it described nothing.
    pub display_name: String,
    pub avatar_mxc_uri: Option<String>,
    pub membership: MatrixSpaceChildMembership,
    pub can_join: bool,
    pub is_space: bool,
    pub joined_members: u64,
}

/// Every child `space_id` advertises, as the server is willing to describe it.
///
/// Pagination runs to the end before anything is projected: a half-read
/// hierarchy would show the user an arbitrary prefix of their Space and call it
/// the Space.
pub async fn matrix_space_children_projection(
    session: &MatrixClientSession,
    space_id: &str,
) -> Result<Vec<MatrixSpaceChildEntry>, MatrixRoomOperationError> {
    let space_room = matrix_room(session, space_id)?;
    let advertised_child_room_ids = matrix_space_child_room_ids(&space_room).await;

    let room_list = SpaceRoomList::new(session.client(), space_room.room_id().to_owned()).await;
    loop {
        room_list
            .paginate()
            .await
            .map_err(|_| MatrixRoomOperationError::RoomUnavailable)?;
        match room_list.pagination_state() {
            SpaceRoomListPaginationState::Idle { end_reached: true } => break,
            SpaceRoomListPaginationState::Idle { end_reached: false } => continue,
            SpaceRoomListPaginationState::Loading => continue,
        }
    }

    let mut entries = Vec::new();
    for room in room_list.rooms().await {
        let room_id = room.room_id.to_string();
        let joined_members = local_joined_member_count(
            session,
            &room_id,
            room.num_joined_members,
        )
        .await;
        entries.push(MatrixSpaceChildEntry {
            room_id,
            raw_name: room
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned),
            display_name: if room.display_name.trim().is_empty() {
                room.room_id.to_string()
            } else {
                room.display_name.trim().to_owned()
            },
            avatar_mxc_uri: room.avatar_url.as_ref().map(ToString::to_string),
            membership: membership_from_room_state(room.state),
            can_join: can_join(room.state, room.join_rule.as_ref()),
            is_space: room.room_type == Some(ruma::room::RoomType::Space),
            joined_members,
        });
    }

    // A child the Space advertises that the hierarchy did not describe is one
    // this account may not see. Report it as unknown rather than omitting it:
    // the Space does contain it.
    for room_id in advertised_child_room_ids {
        if entries.iter().any(|entry| entry.room_id == room_id) {
            continue;
        }

        // `/hierarchy` may omit an invite-only child, while the sliding-sync
        // client still has the invited room locally. Prefer that local state
        // so the user sees its name and can accept the invitation.
        if let Some(room) = RoomId::parse(&room_id)
            .ok()
            .and_then(|parsed_room_id| session.client().get_room(&parsed_room_id))
        {
            let raw_name = room
                .name()
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
            let display_name = room
                .cached_display_name()
                .map(|name| name.to_string())
                .filter(|name| !name.trim().is_empty())
                .or_else(|| raw_name.clone())
                .unwrap_or_else(|| room_id.clone());
            let state = room.state();
            let joined_members = local_joined_member_count(
                session,
                &room_id,
                room.joined_members_count(),
            )
            .await;
            let can_join = local_room_can_join(&room);
            entries.push(MatrixSpaceChildEntry {
                room_id,
                raw_name,
                display_name,
                avatar_mxc_uri: room.avatar_url().map(|url| url.to_string()),
                membership: membership_from_room_state(Some(state)),
                can_join,
                is_space: room.is_space(),
                joined_members,
            });
            continue;
        }

        entries.push(MatrixSpaceChildEntry {
            room_id: room_id.clone(),
            raw_name: None,
            display_name: room_id,
            avatar_mxc_uri: None,
            membership: MatrixSpaceChildMembership::Unknown,
            can_join: false,
            is_space: false,
            joined_members: 0,
        });
    }

    entries.sort_by(|left, right| left.room_id.cmp(&right.room_id));
    entries.dedup_by(|left, right| left.room_id == right.room_id);
    // A child with no current joined members is an abandoned room (for
    // example, a room whose last member left). Pending invitations remain
    // actionable even when the room has no joined members.
    entries.retain(space_child_is_visible);
    Ok(entries)
}

fn space_child_is_visible(entry: &MatrixSpaceChildEntry) -> bool {
    entry.joined_members > 0
        || matches!(
            entry.membership,
            MatrixSpaceChildMembership::Joined
                | MatrixSpaceChildMembership::Invited
                | MatrixSpaceChildMembership::Knocked
        )
}

async fn local_joined_member_count(
    session: &MatrixClientSession,
    room_id: &str,
    fallback: u64,
) -> u64 {
    let Some(room) = RoomId::parse(room_id)
        .ok()
        .and_then(|parsed_room_id| session.client().get_room(&parsed_room_id))
    else {
        return fallback;
    };
    room.members_no_sync(RoomMemberships::JOIN)
        .await
        .map(|members| members.len() as u64)
        .unwrap_or(fallback)
}

fn local_room_can_join(room: &matrix_sdk::Room) -> bool {
    match room.state() {
        RoomState::Joined | RoomState::Banned => false,
        // Accepting a pending invitation is a join.
        RoomState::Invited => true,
        _ => matches!(
            room.join_rule(),
            Some(JoinRule::Public)
                | Some(JoinRule::Restricted(_))
                | Some(JoinRule::KnockRestricted(_))
        ),
    }
}

fn membership_from_room_state(state: Option<RoomState>) -> MatrixSpaceChildMembership {
    match state {
        Some(RoomState::Joined) => MatrixSpaceChildMembership::Joined,
        Some(RoomState::Invited) => MatrixSpaceChildMembership::Invited,
        Some(RoomState::Knocked) => MatrixSpaceChildMembership::Knocked,
        Some(RoomState::Left) => MatrixSpaceChildMembership::Left,
        Some(RoomState::Banned) => MatrixSpaceChildMembership::Banned,
        // The client does not know the room at all, which for a room the
        // hierarchy described means the account is not in it.
        None => MatrixSpaceChildMembership::NotJoined,
    }
}

/// Whether a join attempt is worth offering.
///
/// Only rules that admit a plain join say yes. Knocking is a different request
/// than joining, and an invite-only room cannot be joined by asking, so neither
/// is presented as a join the user can make.
fn can_join(state: Option<RoomState>, join_rule: Option<&JoinRuleSummary>) -> bool {
    match state {
        Some(RoomState::Joined) | Some(RoomState::Banned) => false,
        // Accepting a pending invitation is a join.
        Some(RoomState::Invited) => true,
        _ => matches!(
            join_rule,
            Some(JoinRuleSummary::Public) | Some(JoinRuleSummary::Restricted(_))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        membership: MatrixSpaceChildMembership,
        can_join: bool,
        joined_members: u64,
    ) -> MatrixSpaceChildEntry {
        MatrixSpaceChildEntry {
            room_id: "!room:example.invalid".to_owned(),
            raw_name: Some("Room".to_owned()),
            display_name: "Room".to_owned(),
            avatar_mxc_uri: None,
            membership,
            can_join,
            is_space: false,
            joined_members,
        }
    }

    #[test]
    fn abandoned_empty_room_is_hidden() {
        assert!(!space_child_is_visible(&entry(
            MatrixSpaceChildMembership::NotJoined,
            false,
            0,
        )));
        assert!(!space_child_is_visible(&entry(
            MatrixSpaceChildMembership::Left,
            false,
            0,
        )));
    }

    #[test]
    fn actionable_or_nonempty_child_remains_visible() {
        assert!(!space_child_is_visible(&entry(
            MatrixSpaceChildMembership::NotJoined,
            true,
            0,
        )));
        assert!(space_child_is_visible(&entry(
            MatrixSpaceChildMembership::Invited,
            false,
            0,
        )));
        assert!(space_child_is_visible(&entry(
            MatrixSpaceChildMembership::Unknown,
            false,
            2,
        )));
        assert!(!space_child_is_visible(&entry(
            MatrixSpaceChildMembership::Unknown,
            true,
            0,
        )));
        assert!(!space_child_is_visible(&entry(
            MatrixSpaceChildMembership::Unknown,
            false,
            0,
        )));
    }
}
