use std::fmt;

use serde::{Deserialize, Serialize};

use super::{ProfileState, RoomMemberRole, errors::OperationFailureKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberMembership {
    SpaceJoined,
    SpaceInvited,
    ChildRoomOnly,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceMemberEntry {
    pub user_id: String,
    /// The non-empty room-scoped label observed by the SDK, if one existed.
    pub display_name: Option<String>,
    pub display_label: String,
    #[serde(default)]
    pub original_display_label: String,
    pub avatar_url: Option<String>,
    pub power_level: Option<i64>,
    pub role: RoomMemberRole,
    pub membership: SpaceMemberMembership,
    #[serde(default)]
    pub child_room_ids: Vec<String>,
    #[serde(default)]
    pub invite_pending: bool,
}

impl fmt::Debug for SpaceMemberEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaceMemberEntry")
            .field("user_id", &"UserId(..)")
            .field(
                "display_name",
                &self.display_name.as_ref().map(|_| "DisplayName(..)"),
            )
            .field("display_label", &"DisplayLabel(..)")
            .field("original_display_label", &"OriginalDisplayLabel(..)")
            .field(
                "avatar_url",
                &self.avatar_url.as_ref().map(|_| "MxcUri(..)"),
            )
            .field("power_level", &self.power_level)
            .field("role", &self.role)
            .field("membership", &self.membership)
            .field("child_room_count", &self.child_room_ids.len())
            .field("invite_pending", &self.invite_pending)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceMembersProjection {
    pub space_id: String,
    pub generation: u64,
    pub space_joined: Vec<SpaceMemberEntry>,
    pub space_invited: Vec<SpaceMemberEntry>,
    pub child_room_only: Vec<SpaceMemberEntry>,
    pub child_room_count: usize,
    pub complete_child_room_count: usize,
    pub incomplete_child_room_count: usize,
}

impl fmt::Debug for SpaceMembersProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaceMembersProjection")
            .field("space_id", &"RoomId(..)")
            .field("generation", &self.generation)
            .field("space_joined_count", &self.space_joined.len())
            .field("space_invited_count", &self.space_invited.len())
            .field("child_room_only_count", &self.child_room_only.len())
            .field("child_room_count", &self.child_room_count)
            .field("complete_child_room_count", &self.complete_child_room_count)
            .field(
                "incomplete_child_room_count",
                &self.incomplete_child_room_count,
            )
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberInviteOutcome {
    Invited,
    AlreadyInvited,
    AlreadyJoined,
    Failed(OperationFailureKind),
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SpaceMembersOperationState {
    #[serde(rename = "idle")]
    Idle,
    Loading {
        request_id: Option<u64>,
        space_id: String,
        generation: u64,
    },
    Inviting {
        request_id: u64,
        space_id: String,
        user_id: String,
        generation: u64,
    },
    Failed {
        request_id: u64,
        space_id: String,
        user_id: Option<String>,
        generation: u64,
        #[serde(rename = "failureKind")]
        kind: OperationFailureKind,
    },
}

impl Default for SpaceMembersOperationState {
    fn default() -> Self {
        Self::Idle
    }
}

impl fmt::Debug for SpaceMembersOperationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => formatter.write_str("Idle"),
            Self::Loading {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("Loading")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("generation", generation)
                .finish(),
            Self::Inviting {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("Inviting")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .finish(),
            Self::Failed {
                request_id,
                generation,
                kind,
                ..
            } => formatter
                .debug_struct("Failed")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .field("kind", kind)
                .finish(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceMembersState {
    pub selected_space_id: Option<String>,
    pub generation: u64,
    pub space_joined: Vec<SpaceMemberEntry>,
    pub space_invited: Vec<SpaceMemberEntry>,
    pub child_room_only: Vec<SpaceMemberEntry>,
    pub child_room_count: usize,
    pub complete_child_room_count: usize,
    pub incomplete_child_room_count: usize,
    pub operation: SpaceMembersOperationState,
}

impl Default for SpaceMembersState {
    fn default() -> Self {
        Self {
            selected_space_id: None,
            generation: 0,
            space_joined: Vec::new(),
            space_invited: Vec::new(),
            child_room_only: Vec::new(),
            child_room_count: 0,
            complete_child_room_count: 0,
            incomplete_child_room_count: 0,
            operation: SpaceMembersOperationState::Idle,
        }
    }
}

impl fmt::Debug for SpaceMembersState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaceMembersState")
            .field(
                "selected_space_id",
                &self.selected_space_id.as_ref().map(|_| "RoomId(..)"),
            )
            .field("generation", &self.generation)
            .field("space_joined_count", &self.space_joined.len())
            .field("space_invited_count", &self.space_invited.len())
            .field("child_room_only_count", &self.child_room_only.len())
            .field("child_room_count", &self.child_room_count)
            .field("complete_child_room_count", &self.complete_child_room_count)
            .field(
                "incomplete_child_room_count",
                &self.incomplete_child_room_count,
            )
            .field("operation", &self.operation)
            .finish()
    }
}

impl SpaceMembersState {
    pub fn is_incomplete(&self) -> bool {
        self.incomplete_child_room_count > 0
    }

    pub fn all_entries(&self) -> impl Iterator<Item = &SpaceMemberEntry> {
        self.space_joined
            .iter()
            .chain(self.space_invited.iter())
            .chain(self.child_room_only.iter())
    }
}

pub fn resolve_space_members_projection(
    projection: SpaceMembersProjection,
    profiles: &ProfileState,
) -> SpaceMembersProjection {
    SpaceMembersProjection {
        space_id: projection.space_id,
        generation: projection.generation,
        space_joined: projection
            .space_joined
            .into_iter()
            .map(|entry| resolve_entry(entry, profiles))
            .collect(),
        space_invited: projection
            .space_invited
            .into_iter()
            .map(|entry| resolve_entry(entry, profiles))
            .collect(),
        child_room_only: projection
            .child_room_only
            .into_iter()
            .map(|entry| resolve_entry(entry, profiles))
            .collect(),
        child_room_count: projection.child_room_count,
        complete_child_room_count: projection.complete_child_room_count,
        incomplete_child_room_count: projection.incomplete_child_room_count,
    }
}

pub fn refresh_space_member_display_projection(
    state: &mut SpaceMembersState,
    profiles: &ProfileState,
) -> bool {
    let mut changed = false;
    for entry in state
        .space_joined
        .iter_mut()
        .chain(state.space_invited.iter_mut())
        .chain(state.child_room_only.iter_mut())
    {
        let resolved = resolve_entry(entry.clone(), profiles);
        if entry.display_label != resolved.display_label
            || entry.original_display_label != resolved.original_display_label
        {
            entry.display_label = resolved.display_label;
            entry.original_display_label = resolved.original_display_label;
            changed = true;
        }
    }
    changed
}

fn resolve_entry(mut entry: SpaceMemberEntry, profiles: &ProfileState) -> SpaceMemberEntry {
    let cached = profiles.users.get(&entry.user_id).and_then(|profile| {
        profile
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    let input = super::profile::ProfileResolutionInput {
        local_alias: profiles
            .local_aliases
            .get(&entry.user_id)
            .map(String::as_str),
        relevant_room_label: matches!(entry.membership, SpaceMemberMembership::ChildRoomOnly)
            .then(|| entry.display_name.as_deref())
            .flatten(),
        space_room_label: matches!(
            entry.membership,
            SpaceMemberMembership::SpaceJoined | SpaceMemberMembership::SpaceInvited
        )
        .then(|| entry.display_name.as_deref())
        .flatten(),
        payload_label: None,
        cached_label: cached,
        local_homeserver_label: None,
    };
    let resolved = super::profile::resolve_people_label(input);
    entry.display_label = resolved.label;
    entry.original_display_label = entry
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(cached)
        .unwrap_or("Unknown user")
        .to_owned();
    entry
}

pub fn sort_entries(entries: &mut [SpaceMemberEntry]) {
    entries.sort_by(|left, right| {
        left.display_label
            .cmp(&right.display_label)
            .then_with(|| left.user_id.cmp(&right.user_id))
    });
}
