//! Room-history export reducer (#59). See the "Room History Export" section of
//! `docs/architecture/state-machine.md`.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, RoomHistoryExportFailureKind, RoomHistoryExportProgress, RoomHistoryExportRange,
        RoomHistoryExportState,
    },
};

use super::is_session_ready;

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(UiEvent::RoomHistoryExportChanged)]
}

/// Start guard: a Ready session, a valid range, a known room, and no export in
/// flight. A terminal state from an earlier export is replaced.
pub(super) fn handle_requested(
    state: &mut AppState,
    request_id: u64,
    room_id: String,
    range: RoomHistoryExportRange,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || state.room_history_export.active_request_id().is_some()
        || !range.is_valid()
        || !state.rooms.iter().any(|room| room.room_id == room_id)
    {
        return Vec::new();
    }
    state.room_history_export = RoomHistoryExportState::Exporting {
        request_id,
        room_id,
        range,
        progress: RoomHistoryExportProgress::default(),
        cancel_requested: false,
    };
    changed()
}

pub(super) fn handle_progressed(
    state: &mut AppState,
    request_id: u64,
    next: RoomHistoryExportProgress,
) -> Vec<AppEffect> {
    let RoomHistoryExportState::Exporting {
        request_id: active,
        progress,
        ..
    } = &mut state.room_history_export
    else {
        return Vec::new();
    };
    if *active != request_id || *progress == next {
        return Vec::new();
    }
    *progress = next;
    changed()
}

pub(super) fn handle_cancel_requested(state: &mut AppState, request_id: u64) -> Vec<AppEffect> {
    let RoomHistoryExportState::Exporting {
        request_id: active,
        cancel_requested,
        ..
    } = &mut state.room_history_export
    else {
        return Vec::new();
    };
    if *active != request_id || *cancel_requested {
        return Vec::new();
    }
    *cancel_requested = true;
    changed()
}

pub(super) fn handle_completed(
    state: &mut AppState,
    request_id: u64,
    progress: RoomHistoryExportProgress,
) -> Vec<AppEffect> {
    let RoomHistoryExportState::Exporting {
        request_id: active,
        room_id,
        range,
        ..
    } = &state.room_history_export
    else {
        return Vec::new();
    };
    if *active != request_id {
        return Vec::new();
    }
    state.room_history_export = RoomHistoryExportState::Completed {
        request_id,
        room_id: room_id.clone(),
        range: range.clone(),
        progress,
    };
    changed()
}

pub(super) fn handle_cancelled(
    state: &mut AppState,
    request_id: u64,
    progress: RoomHistoryExportProgress,
) -> Vec<AppEffect> {
    let RoomHistoryExportState::Exporting {
        request_id: active,
        room_id,
        ..
    } = &state.room_history_export
    else {
        return Vec::new();
    };
    if *active != request_id {
        return Vec::new();
    }
    state.room_history_export = RoomHistoryExportState::Cancelled {
        request_id,
        room_id: room_id.clone(),
        progress,
    };
    changed()
}

pub(super) fn handle_failed(
    state: &mut AppState,
    request_id: u64,
    failure_kind: RoomHistoryExportFailureKind,
    progress: RoomHistoryExportProgress,
) -> Vec<AppEffect> {
    let RoomHistoryExportState::Exporting {
        request_id: active,
        room_id,
        ..
    } = &state.room_history_export
    else {
        return Vec::new();
    };
    if *active != request_id {
        return Vec::new();
    }
    state.room_history_export = RoomHistoryExportState::Failed {
        request_id,
        room_id: room_id.clone(),
        progress,
        failure_kind,
    };
    changed()
}
