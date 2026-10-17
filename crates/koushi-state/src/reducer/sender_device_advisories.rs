//! Reducer for sender device advisories (#1494).
//!
//! State-machine contract: docs/architecture/state-machine.md, "Sender Device
//! Advisories". Read-only with respect to trust: nothing here verifies, pins,
//! or changes sending policy.

use std::collections::BTreeSet;

use crate::{
    effect::{AppEffect, UiEvent},
    state::AppState,
};

use super::is_session_ready;

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(
        UiEvent::SenderDeviceAdvisoriesChanged,
    )]
}

pub(crate) fn handle_updated(
    state: &mut AppState,
    room_id: String,
    user_ids: BTreeSet<String>,
) -> Vec<AppEffect> {
    if !is_session_ready(state) || !state.rooms.iter().any(|room| room.room_id == room_id) {
        return Vec::new();
    }
    let rooms = &mut state.sender_device_advisories.rooms;
    if user_ids.is_empty() {
        return if rooms.remove(&room_id).is_some() {
            changed()
        } else {
            Vec::new()
        };
    }
    if rooms.get(&room_id) == Some(&user_ids) {
        return Vec::new();
    }
    rooms.insert(room_id, user_ids);
    changed()
}

pub(crate) fn handle_cleared(state: &mut AppState, room_id: &str) -> Vec<AppEffect> {
    if state
        .sender_device_advisories
        .rooms
        .remove(room_id)
        .is_some()
    {
        changed()
    } else {
        Vec::new()
    }
}
