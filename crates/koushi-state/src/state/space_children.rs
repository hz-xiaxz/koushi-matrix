//! Issue #961: every child room a Space advertises, with the viewer's
//! membership beside it.
//!
//! The room list only ever carried rooms the account has joined, so a Space's
//! own room list could not show what else is in the Space. This slice holds the
//! Space's advertised children — the `/hierarchy` projection — so the sidebar
//! and the Space info panel can show a joined room, an invitation and a room
//! the account has not joined in one list, without the frontend inferring any
//! of it.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{AvatarImage, errors::OperationFailureKind};

/// The viewer's relationship to a Space child room.
///
/// `Unknown` is the honest answer for a child the server did not describe:
/// a room the account may not see is reported as such, never probed around.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceChildMembership {
    Joined,
    Invited,
    Knocked,
    Left,
    Banned,
    NotJoined,
    #[default]
    Unknown,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceChildSummary {
    pub room_id: String,
    /// Display name as the server computed it, or the room ID when the server
    /// described nothing.
    pub display_name: String,
    #[serde(default)]
    pub avatar: Option<AvatarImage>,
    pub membership: SpaceChildMembership,
    /// Whether the join rule the server reported permits an attempt to join.
    /// A room the account is invited to counts: accepting is a join.
    #[serde(default)]
    pub can_join: bool,
    #[serde(default)]
    pub is_space: bool,
    #[serde(default)]
    pub joined_members: u64,
}

impl fmt::Debug for SpaceChildSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaceChildSummary")
            .field("room_id", &"RoomId(..)")
            .field("display_name", &"RoomName(..)")
            .field("avatar", &self.avatar.as_ref().map(|_| "AvatarImage(..)"))
            .field("membership", &self.membership)
            .field("can_join", &self.can_join)
            .field("is_space", &self.is_space)
            .field("joined_members", &self.joined_members)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SpaceChildrenLoadState {
    #[default]
    Idle,
    Loading,
    Failed {
        failure: OperationFailureKind,
    },
}

/// The advertised children of the currently selected Space.
///
/// Scoped to one Space at a time, like the Space member projection: selecting
/// another Space bumps the generation and clears what the previous one loaded,
/// so a late response can never paint the wrong Space's rooms.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceChildrenState {
    #[serde(default)]
    pub selected_space_id: Option<String>,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub children: Vec<SpaceChildSummary>,
    #[serde(default)]
    pub load: SpaceChildrenLoadState,
}

impl SpaceChildrenState {
    /// The children of `space_id`, or nothing when another Space is selected.
    pub fn children_for(&self, space_id: &str) -> &[SpaceChildSummary] {
        match self.selected_space_id.as_deref() {
            Some(selected) if selected == space_id => &self.children,
            _ => &[],
        }
    }
}
