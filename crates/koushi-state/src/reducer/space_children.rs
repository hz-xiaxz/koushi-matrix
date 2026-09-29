//! Issue #961: the selected Space's advertised children.
//!
//! Scoped like the Space member projection: a response is admitted only for the
//! Space that is selected now and the generation it was requested under, so a
//! slow `/hierarchy` response can never paint another Space's rooms.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, OperationFailureKind, SpaceChildMembership, SpaceChildSummary,
        SpaceChildrenLoadState, SpaceChildrenState,
    },
};

use super::is_session_ready;

/// A newly selected Space owns a fresh generation and none of the previous
/// Space's children.
pub(crate) fn handle_selected(state: &mut AppState, selected_space_id: Option<String>) -> bool {
    if state.space_children.selected_space_id == selected_space_id {
        return false;
    }
    state.space_children = SpaceChildrenState {
        selected_space_id,
        generation: state.space_children.generation.wrapping_add(1),
        children: Vec::new(),
        load: SpaceChildrenLoadState::Idle,
    };
    true
}

pub(crate) fn handle_load_requested(
    state: &mut AppState,
    space_id: String,
    generation: u64,
) -> Vec<AppEffect> {
    if !is_session_ready(state) || !admits(state, &space_id, generation) {
        return Vec::new();
    }
    state.space_children.load = SpaceChildrenLoadState::Loading;
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_loaded(
    state: &mut AppState,
    space_id: String,
    generation: u64,
    children: Vec<SpaceChildSummary>,
) -> Vec<AppEffect> {
    if !admits(state, &space_id, generation) {
        return Vec::new();
    }
    state.space_children.children = children;
    state.space_children.load = SpaceChildrenLoadState::Idle;
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_load_failed(
    state: &mut AppState,
    space_id: String,
    generation: u64,
    failure: OperationFailureKind,
) -> Vec<AppEffect> {
    if !admits(state, &space_id, generation) {
        return Vec::new();
    }
    // A failed refresh keeps whatever was already projected: a transient
    // /hierarchy failure must not empty a list the user is looking at.
    state.space_children.load = SpaceChildrenLoadState::Failed { failure };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

/// Issue #1062: the account left `room_id`, which may be a child of the
/// selected Space.
///
/// The cached `/hierarchy` projection predates the leave: it still calls the
/// room joined, counts the account among its members, and carries the
/// join-rule verdict of a joined room. Correct what this client knows for
/// certain right away (the account is no longer a member, so an emptied room
/// stops being offered) and ask for a fresh projection under a new generation,
/// which fences any response that was already in flight from before the leave.
/// `joined_members_before_leave` is the room list's server-reported count,
/// which included the account.
pub(crate) fn handle_room_left(
    state: &mut AppState,
    room_id: &str,
    joined_members_before_leave: Option<u64>,
) -> Vec<AppEffect> {
    let Some(space_id) = state.space_children.selected_space_id.clone() else {
        return Vec::new();
    };
    // The Space's own child list also counts: the first load may still be in
    // flight with nothing cached yet, and its pre-leave answer must be fenced.
    let advertised_by_space = state
        .spaces
        .iter()
        .find(|space| space.space_id == space_id)
        .is_some_and(|space| space.child_room_ids.iter().any(|child| child == room_id));
    let cached_child = state
        .space_children
        .children
        .iter_mut()
        .find(|child| child.room_id == room_id);
    if cached_child.is_none() && !advertised_by_space {
        return Vec::new();
    }
    if let Some(child) = cached_child
        && child.membership == SpaceChildMembership::Joined
    {
        child.membership = SpaceChildMembership::Left;
        child.can_join = false;
        child.joined_members = joined_members_before_leave
            .unwrap_or(child.joined_members)
            .saturating_sub(1);
    }
    state.space_children.generation = state.space_children.generation.wrapping_add(1);
    state.space_children.load = SpaceChildrenLoadState::Loading;
    vec![
        AppEffect::LoadSpaceChildren {
            space_id,
            generation: state.space_children.generation,
        },
        AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged),
    ]
}

fn admits(state: &AppState, space_id: &str, generation: u64) -> bool {
    state.space_children.selected_space_id.as_deref() == Some(space_id)
        && state.space_children.generation == generation
}
