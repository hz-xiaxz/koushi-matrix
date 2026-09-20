//! Issue #961: the selected Space's advertised children.
//!
//! Scoped like the Space member projection: a response is admitted only for the
//! Space that is selected now and the generation it was requested under, so a
//! slow `/hierarchy` response can never paint another Space's rooms.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, OperationFailureKind, SpaceChildSummary, SpaceChildrenLoadState,
        SpaceChildrenState,
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

fn admits(state: &AppState, space_id: &str, generation: u64) -> bool {
    state.space_children.selected_space_id.as_deref() == Some(space_id)
        && state.space_children.generation == generation
}
