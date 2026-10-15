//! Issue #961: the selected Space's advertised children.
//!
//! Scoped like the Space member projection: a response is admitted only for the
//! Space that is selected now and the generation it was requested under, so a
//! slow `/hierarchy` response can never paint another Space's rooms.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, OperationFailureKind, SpaceChildMembership, SpaceChildSuggestedEditState,
        SpaceChildSummary, SpaceChildrenLoadState, SpaceChildrenState, SuggestedRoomCandidate,
        SuggestedRoomJoinOutcome, SuggestedRoomJoinResult, SuggestedRoomsOffer,
    },
};

use super::is_session_ready;

/// A newly selected Space owns a fresh generation and none of the previous
/// Space's children.
pub(crate) fn handle_selected(state: &mut AppState, selected_space_id: Option<String>) -> bool {
    if state.space_children.selected_space_id == selected_space_id {
        return false;
    }
    // #1410: the offer and the recent joins are not about the selection.
    let suggested_offer = std::mem::take(&mut state.space_children.suggested_offer);
    let recently_joined_room_ids =
        std::mem::take(&mut state.space_children.recently_joined_room_ids);
    state.space_children = SpaceChildrenState {
        selected_space_id,
        generation: state.space_children.generation.wrapping_add(1),
        children: Vec::new(),
        load: SpaceChildrenLoadState::Idle,
        can_edit_suggested: false,
        suggested_edit: SpaceChildSuggestedEditState::Idle,
        suggested_offer,
        recently_joined_room_ids,
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
    can_edit_suggested: bool,
) -> Vec<AppEffect> {
    if !admits(state, &space_id, generation) {
        return Vec::new();
    }
    state.space_children.children = children;
    state.space_children.can_edit_suggested = can_edit_suggested;
    state.space_children.load = SpaceChildrenLoadState::Idle;
    offer_suggested_rooms(state, &space_id);
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

/// Issue #1410: this client joined `room_id`; a Space among these may offer
/// its suggested rooms on its first children load.
pub(crate) fn record_room_joined(state: &mut AppState, room_id: &str) {
    state
        .space_children
        .recently_joined_room_ids
        .record(room_id);
}

/// Issue #1410: make the one-time offer for `space_id` when it was joined
/// here recently and suggests at least one room the account can simply join.
/// The recent-join entry is consumed either way, so later loads never offer.
fn offer_suggested_rooms(state: &mut AppState, space_id: &str) {
    if state.space_children.suggested_offer != SuggestedRoomsOffer::None
        || !state.space_children.recently_joined_room_ids.take(space_id)
    {
        return;
    }
    let rooms: Vec<SuggestedRoomCandidate> = state
        .space_children
        .children
        .iter()
        .filter(|child| {
            child.suggested
                && child.can_join
                && !child.is_space
                && child.membership == SpaceChildMembership::NotJoined
                && child.joined_members > 0
                && !state.rooms.iter().any(|room| room.room_id == child.room_id)
                && !state
                    .invites
                    .iter()
                    .any(|invite| invite.room_id == child.room_id)
        })
        .map(|child| SuggestedRoomCandidate {
            room_id: child.room_id.clone(),
            display_name: child.display_name.clone(),
            display_name_placeholder: child.display_name_placeholder.clone(),
        })
        .collect();
    if rooms.is_empty() {
        return;
    }
    state.space_children.suggested_offer = SuggestedRoomsOffer::Offered {
        space_id: space_id.to_owned(),
        rooms,
    };
}

/// Issue #1410: admit a suggested toggle for a loaded child of the selected
/// Space, one at a time.
pub(crate) fn handle_suggested_update_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    child_room_id: String,
    suggested: bool,
) -> Vec<AppEffect> {
    let children = &state.space_children;
    if !is_session_ready(state)
        || children.selected_space_id.as_deref() != Some(space_id.as_str())
        || !children.can_edit_suggested
        || matches!(
            children.suggested_edit,
            SpaceChildSuggestedEditState::Saving { .. }
        )
        || !children
            .children
            .iter()
            .any(|child| child.room_id == child_room_id)
    {
        return Vec::new();
    }
    state.space_children.suggested_edit = SpaceChildSuggestedEditState::Saving {
        request_id,
        child_room_id,
        suggested,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_suggested_update_settled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    child_room_id: String,
    suggested: bool,
    failure: Option<OperationFailureKind>,
) -> Vec<AppEffect> {
    let matches_in_flight = matches!(
        &state.space_children.suggested_edit,
        SpaceChildSuggestedEditState::Saving {
            request_id: current,
            child_room_id: current_child,
            suggested: current_suggested,
        } if *current == request_id
            && *current_child == child_room_id
            && *current_suggested == suggested
    );
    if !matches_in_flight
        || state.space_children.selected_space_id.as_deref() != Some(space_id.as_str())
    {
        return Vec::new();
    }
    state.space_children.suggested_edit = match failure {
        None => {
            if let Some(child) = state
                .space_children
                .children
                .iter_mut()
                .find(|child| child.room_id == child_room_id)
            {
                child.suggested = suggested;
            }
            SpaceChildSuggestedEditState::Idle
        }
        Some(failure) => SpaceChildSuggestedEditState::Failed {
            child_room_id,
            suggested,
            failure,
        },
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_suggested_rooms_join_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    let SuggestedRoomsOffer::Offered {
        space_id: offered_space_id,
        rooms,
    } = &state.space_children.suggested_offer
    else {
        return Vec::new();
    };
    if *offered_space_id != space_id {
        return Vec::new();
    }
    state.space_children.suggested_offer = SuggestedRoomsOffer::Joining {
        request_id,
        space_id,
        rooms: rooms.clone(),
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_suggested_rooms_join_settled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    outcomes: Vec<SuggestedRoomJoinOutcome>,
) -> Vec<AppEffect> {
    let SuggestedRoomsOffer::Joining {
        request_id: current,
        space_id: joining_space_id,
        rooms,
    } = &state.space_children.suggested_offer
    else {
        return Vec::new();
    };
    if *current != request_id || *joining_space_id != space_id {
        return Vec::new();
    }
    // Every offered room gets a result; a room Core did not report is not
    // claimed as joined.
    let results: Vec<SuggestedRoomJoinResult> = rooms
        .iter()
        .map(|room| SuggestedRoomJoinResult {
            room_id: room.room_id.clone(),
            display_name: room.display_name.clone(),
            display_name_placeholder: room.display_name_placeholder.clone(),
            failure: outcomes
                .iter()
                .find(|outcome| outcome.room_id == room.room_id)
                .map_or(Some(OperationFailureKind::Sdk), |outcome| outcome.failure),
        })
        .collect();
    state.space_children.suggested_offer = if results.iter().all(|result| result.failure.is_none())
    {
        SuggestedRoomsOffer::None
    } else {
        SuggestedRoomsOffer::Completed { space_id, results }
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
}

pub(crate) fn handle_suggested_rooms_offer_dismissed(
    state: &mut AppState,
    space_id: String,
) -> Vec<AppEffect> {
    match &state.space_children.suggested_offer {
        SuggestedRoomsOffer::Offered {
            space_id: offered, ..
        }
        | SuggestedRoomsOffer::Completed {
            space_id: offered, ..
        } if *offered == space_id => {
            state.space_children.suggested_offer = SuggestedRoomsOffer::None;
            vec![AppEffect::EmitUiEvent(UiEvent::SpaceChildrenChanged)]
        }
        _ => Vec::new(),
    }
}

/// Issue #1410: the account is no longer in `space_id`; an offer for it that is
/// not joining is withdrawn. Returns whether anything changed.
pub(crate) fn withdraw_suggested_offer_for_left_space(state: &mut AppState) -> bool {
    let Some(space_id) = state.space_children.suggested_offer.space_id() else {
        return false;
    };
    if matches!(
        state.space_children.suggested_offer,
        SuggestedRoomsOffer::Joining { .. }
    ) || state.spaces.iter().any(|space| space.space_id == space_id)
    {
        return false;
    }
    state.space_children.suggested_offer = SuggestedRoomsOffer::None;
    true
}

/// Issue #1410: session cleanup drops the offer and the recent joins. Returns
/// whether an offer was visible.
pub(crate) fn clear_session_suggestions(space_children: &mut SpaceChildrenState) -> bool {
    space_children.recently_joined_room_ids = Default::default();
    space_children.can_edit_suggested = false;
    space_children.suggested_edit = SpaceChildSuggestedEditState::Idle;
    std::mem::take(&mut space_children.suggested_offer) != SuggestedRoomsOffer::None
}

fn admits(state: &AppState, space_id: &str, generation: u64) -> bool {
    state.space_children.selected_space_id.as_deref() == Some(space_id)
        && state.space_children.generation == generation
}
