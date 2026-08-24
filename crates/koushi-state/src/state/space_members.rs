use std::fmt;

use serde::{Deserialize, Serialize};

use super::{AppState, ProfileState, RoomMemberRole, SessionState, errors::OperationFailureKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberMembership {
    SpaceJoined,
    SpaceInvited,
    ChildRoomOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpaceMembersCommandRejection {
    NoSelectedSpace,
    WrongSpace,
    StaleGeneration,
    InviteAlreadyInFlight,
    CancellationAlreadyInFlight,
    LoadBlockedByInvite,
    AlreadyJoined,
    AlreadyInvited,
    NotInvited,
    NotChildRoomOnly,
    RoleUpdateAlreadyInFlight,
    RoleNotEditable,
    RoleTargetInvalid,
    RoleOptionUnavailable,
    RoleRevisionMismatch,
    RoleCurrentPowerMismatch,
    RoleConfirmationRequired,
    RoleSessionRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMemberRoleOption {
    pub power_level: i64,
    pub role: RoomMemberRole,
    pub requires_confirmation: bool,
}

/// Closed, private-data-free failure kinds for a Space-member role update.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberRoleFailureKind {
    Forbidden,
    Stale,
    NotFound,
    Network,
    Timeout,
    Invalid,
    Sdk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberRoleUpdateOutcome {
    Succeeded,
    Failed(SpaceMemberRoleFailureKind),
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
    #[serde(default)]
    pub role_options: Vec<SpaceMemberRoleOption>,
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
            .field("role_option_count", &self.role_options.len())
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
    #[serde(default)]
    pub power_levels_revision: Option<String>,
    #[serde(default)]
    pub can_edit_roles: bool,
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
            .field(
                "power_levels_revision",
                &self.power_levels_revision.as_ref().map(|_| "EventId(..)"),
            )
            .field("can_edit_roles", &self.can_edit_roles)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMemberInviteOutcome {
    Invited,
    AlreadyInvited,
    AlreadyJoined,
    Cancelled,
    NotInvited,
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
    CancellingInvite {
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
    UpdatingRole {
        request_id: u64,
        space_id: String,
        user_id: String,
        generation: u64,
        #[serde(default)]
        expected_power_levels_revision: Option<String>,
        expected_power_level: i64,
        power_level: i64,
        confirmed: bool,
    },
    RoleUpdateFailed {
        request_id: u64,
        space_id: String,
        user_id: String,
        generation: u64,
        #[serde(default)]
        expected_power_levels_revision: Option<String>,
        expected_power_level: i64,
        power_level: i64,
        #[serde(default)]
        sent_revision: Option<String>,
        #[serde(rename = "failureKind")]
        kind: SpaceMemberRoleFailureKind,
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
            Self::CancellingInvite {
                request_id,
                generation,
                ..
            } => formatter
                .debug_struct("CancellingInvite")
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
            Self::UpdatingRole {
                request_id,
                generation,
                expected_power_level,
                power_level,
                ..
            } => formatter
                .debug_struct("UpdatingRole")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .field("expected_power_level", expected_power_level)
                .field("power_level", power_level)
                .finish(),
            Self::RoleUpdateFailed {
                request_id,
                generation,
                expected_power_level,
                power_level,
                kind,
                ..
            } => formatter
                .debug_struct("RoleUpdateFailed")
                .field("request_id", request_id)
                .field("space_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .field("generation", generation)
                .field("expected_power_level", expected_power_level)
                .field("power_level", power_level)
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
    #[serde(default)]
    pub power_levels_revision: Option<String>,
    #[serde(default)]
    pub can_edit_roles: bool,
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
            power_levels_revision: None,
            can_edit_roles: false,
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
        power_levels_revision: projection.power_levels_revision,
        can_edit_roles: projection.can_edit_roles,
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
            || entry.avatar_url != resolved.avatar_url
        {
            entry.display_label = resolved.display_label;
            entry.original_display_label = resolved.original_display_label;
            entry.avatar_url = resolved.avatar_url;
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
    if entry.avatar_url.is_none() {
        entry.avatar_url = profiles
            .users
            .get(&entry.user_id)
            .and_then(|profile| profile.avatar.as_ref())
            .map(|avatar| avatar.mxc_uri.clone())
            .filter(|mxc_uri| !mxc_uri.trim().is_empty());
    }
    entry
}

/// Admit a Space-member command against the state snapshot that is about to
/// be used for an external SDK operation. Callers must perform this check
/// before reducing an optimistic action or invoking the SDK.
pub fn admit_space_member_invite(
    state: &SpaceMembersState,
    space_id: &str,
    user_id: &str,
    generation: u64,
) -> Result<(), SpaceMembersCommandRejection> {
    let Some(selected_space_id) = state.selected_space_id.as_deref() else {
        return Err(SpaceMembersCommandRejection::NoSelectedSpace);
    };
    if selected_space_id != space_id {
        return Err(SpaceMembersCommandRejection::WrongSpace);
    }
    if state.generation != generation {
        return Err(SpaceMembersCommandRejection::StaleGeneration);
    }
    if matches!(
        state.operation,
        SpaceMembersOperationState::Inviting { .. }
            | SpaceMembersOperationState::CancellingInvite { .. }
            | SpaceMembersOperationState::UpdatingRole { .. }
            | SpaceMembersOperationState::RoleUpdateFailed { .. }
    ) {
        return Err(SpaceMembersCommandRejection::InviteAlreadyInFlight);
    }
    if state
        .space_joined
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        return Err(SpaceMembersCommandRejection::AlreadyJoined);
    }
    if state
        .space_invited
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        return Err(SpaceMembersCommandRejection::AlreadyInvited);
    }
    if !state
        .child_room_only
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        return Err(SpaceMembersCommandRejection::NotChildRoomOnly);
    }
    Ok(())
}

pub fn admit_space_member_cancellation(
    state: &SpaceMembersState,
    space_id: &str,
    user_id: &str,
    generation: u64,
) -> Result<(), SpaceMembersCommandRejection> {
    let Some(selected_space_id) = state.selected_space_id.as_deref() else {
        return Err(SpaceMembersCommandRejection::NoSelectedSpace);
    };
    if selected_space_id != space_id {
        return Err(SpaceMembersCommandRejection::WrongSpace);
    }
    if state.generation != generation {
        return Err(SpaceMembersCommandRejection::StaleGeneration);
    }
    let cancellation_context_is_retryable =
        matches!(&state.operation, SpaceMembersOperationState::Idle)
            || matches!(
                &state.operation,
                SpaceMembersOperationState::Failed {
                    space_id: failed_space_id,
                    user_id: Some(failed_user_id),
                    generation: failed_generation,
                    ..
                } if failed_space_id == space_id
                    && failed_user_id == user_id
                    && *failed_generation == generation
            );
    if !cancellation_context_is_retryable {
        return Err(SpaceMembersCommandRejection::CancellationAlreadyInFlight);
    }
    if !state
        .space_invited
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        return Err(SpaceMembersCommandRejection::NotInvited);
    }
    Ok(())
}

pub fn admit_space_member_role(
    state: &AppState,
    space_id: &str,
    user_id: &str,
    generation: u64,
    expected_power_levels_revision: Option<&str>,
    expected_power_level: i64,
    power_level: i64,
    confirmed: bool,
) -> Result<(), SpaceMembersCommandRejection> {
    if !matches!(state.session, SessionState::Ready(_)) {
        return Err(SpaceMembersCommandRejection::RoleSessionRequired);
    }
    if state.navigation.active_space_id.as_deref() != Some(space_id)
        || state.space_members.selected_space_id.as_deref() != Some(space_id)
    {
        return Err(SpaceMembersCommandRejection::WrongSpace);
    }
    if state.space_members.generation != generation {
        return Err(SpaceMembersCommandRejection::StaleGeneration);
    }
    let retry_matches = match &state.space_members.operation {
        SpaceMembersOperationState::Idle => true,
        SpaceMembersOperationState::RoleUpdateFailed {
            space_id: failed_space_id,
            user_id: failed_user_id,
            generation: failed_generation,
            power_level: failed_new_power,
            ..
        } => {
            failed_space_id == space_id
                && failed_user_id == user_id
                && *failed_generation == generation
                && *failed_new_power == power_level
        }
        _ => false,
    };
    if !retry_matches {
        return Err(SpaceMembersCommandRejection::RoleUpdateAlreadyInFlight);
    }
    if !state.space_members.can_edit_roles {
        return Err(SpaceMembersCommandRejection::RoleNotEditable);
    }
    if state.space_members.power_levels_revision.as_deref() != expected_power_levels_revision {
        return Err(SpaceMembersCommandRejection::RoleRevisionMismatch);
    }
    let Some(target) = state
        .space_members
        .space_joined
        .iter()
        .find(|entry| entry.user_id == user_id)
    else {
        return Err(SpaceMembersCommandRejection::RoleTargetInvalid);
    };
    let own_user_id = match &state.session {
        SessionState::Ready(info) => Some(info.user_id.as_str()),
        _ => None,
    };
    if own_user_id == Some(user_id)
        || target.membership != SpaceMemberMembership::SpaceJoined
        || target.power_level.is_none()
    {
        return Err(SpaceMembersCommandRejection::RoleTargetInvalid);
    }
    if target.power_level != Some(expected_power_level) {
        return Err(SpaceMembersCommandRejection::RoleCurrentPowerMismatch);
    }
    let Some(option) = target
        .role_options
        .iter()
        .find(|option| option.power_level == power_level)
    else {
        return Err(SpaceMembersCommandRejection::RoleOptionUnavailable);
    };
    if option.power_level == expected_power_level {
        return Err(SpaceMembersCommandRejection::RoleOptionUnavailable);
    }
    if option.requires_confirmation && !confirmed {
        return Err(SpaceMembersCommandRejection::RoleConfirmationRequired);
    }
    Ok(())
}

pub fn admit_space_members_load(
    state: &AppState,
    space_id: &str,
    generation: u64,
) -> Result<(), SpaceMembersCommandRejection> {
    let Some(active_space_id) = state.navigation.active_space_id.as_deref() else {
        return Err(SpaceMembersCommandRejection::NoSelectedSpace);
    };
    if active_space_id != space_id {
        return Err(SpaceMembersCommandRejection::WrongSpace);
    }

    if let Some(selected_space_id) = state.space_members.selected_space_id.as_deref() {
        if selected_space_id != space_id {
            return Err(SpaceMembersCommandRejection::WrongSpace);
        }
        if state.space_members.generation != generation {
            return Err(SpaceMembersCommandRejection::StaleGeneration);
        }
    }
    if matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Inviting { .. }
            | SpaceMembersOperationState::CancellingInvite { .. }
            | SpaceMembersOperationState::UpdatingRole { .. }
    ) {
        return Err(SpaceMembersCommandRejection::LoadBlockedByInvite);
    }
    Ok(())
}

pub fn sort_entries(entries: &mut [SpaceMemberEntry]) {
    entries.sort_by(|left, right| {
        left.display_label
            .cmp(&right.display_label)
            .then_with(|| left.user_id.cmp(&right.user_id))
    });
}
