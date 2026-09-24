//! History export reducer. See the "History Export" section of
//! `docs/architecture/state-machine.md`.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, HistoryExportFailureKind, HistoryExportRange, HistoryExportRoom,
        HistoryExportRoomCounts, HistoryExportRoomFailureKind, HistoryExportRoomPhase,
        HistoryExportScope, HistoryExportState,
    },
};

use super::is_session_ready;

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(UiEvent::HistoryExportChanged)]
}

fn target_is_known(state: &AppState, scope: &HistoryExportScope) -> bool {
    match scope {
        HistoryExportScope::Room { room_id } => {
            state.rooms.iter().any(|room| &room.room_id == room_id)
        }
        HistoryExportScope::Space { space_id } => {
            state.spaces.iter().any(|space| &space.space_id == space_id)
        }
    }
}

/// Start guard: a Ready session, a valid range, a known room or Space, and no
/// export in flight. A settled export is replaced.
pub(super) fn handle_requested(
    state: &mut AppState,
    request_id: u64,
    scope: HistoryExportScope,
    range: HistoryExportRange,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || state.history_export.active_request_id().is_some()
        || !range.is_valid()
        || !target_is_known(state, &scope)
    {
        return Vec::new();
    }
    state.history_export = HistoryExportState::Preparing {
        request_id,
        scope,
        range,
        stop_requested: false,
    };
    changed()
}

/// Resume a settled export: same scope and range, new request.
pub(super) fn handle_retry_requested(
    state: &mut AppState,
    request_id: u64,
    target_request_id: u64,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    let (HistoryExportState::Completed {
        request_id: settled,
        scope,
        range,
        ..
    }
    | HistoryExportState::Stopped {
        request_id: settled,
        scope,
        range,
        ..
    }
    | HistoryExportState::Failed {
        request_id: settled,
        scope,
        range,
        ..
    }) = &state.history_export
    else {
        return Vec::new();
    };
    if *settled != target_request_id {
        return Vec::new();
    }
    state.history_export = HistoryExportState::Preparing {
        request_id,
        scope: scope.clone(),
        range: range.clone(),
        stop_requested: false,
    };
    changed()
}

pub(super) fn handle_prepared(
    state: &mut AppState,
    request_id: u64,
    rooms: Vec<HistoryExportRoom>,
) -> Vec<AppEffect> {
    let HistoryExportState::Preparing {
        request_id: active,
        scope,
        range,
        stop_requested,
    } = &state.history_export
    else {
        return Vec::new();
    };
    if *active != request_id {
        return Vec::new();
    }
    state.history_export = HistoryExportState::Running {
        request_id,
        scope: scope.clone(),
        range: range.clone(),
        rooms,
        stop_requested: *stop_requested,
    };
    changed()
}

fn running_room<'a>(
    state: &'a mut AppState,
    request_id: u64,
    room_id: &str,
) -> Option<&'a mut HistoryExportRoom> {
    let HistoryExportState::Running {
        request_id: active,
        rooms,
        ..
    } = &mut state.history_export
    else {
        return None;
    };
    if *active != request_id {
        return None;
    }
    rooms.iter_mut().find(|room| room.room_id == room_id)
}

/// Progress for an unsettled room. Phases only move forward and never settle
/// a room.
pub(super) fn handle_room_progressed(
    state: &mut AppState,
    request_id: u64,
    room_id: &str,
    phase: HistoryExportRoomPhase,
    counts: HistoryExportRoomCounts,
) -> Vec<AppEffect> {
    if phase.is_settled() {
        return Vec::new();
    }
    let Some(room) = running_room(state, request_id, room_id) else {
        return Vec::new();
    };
    if room.phase.is_settled()
        || phase < room.phase
        || (phase == room.phase && counts == room.counts)
    {
        return Vec::new();
    }
    room.phase = phase;
    room.counts = counts;
    changed()
}

pub(super) fn handle_room_settled(
    state: &mut AppState,
    request_id: u64,
    room_id: &str,
    phase: HistoryExportRoomPhase,
    counts: HistoryExportRoomCounts,
    failure_kind: Option<HistoryExportRoomFailureKind>,
) -> Vec<AppEffect> {
    if !matches!(
        phase,
        HistoryExportRoomPhase::Completed | HistoryExportRoomPhase::Failed
    ) {
        return Vec::new();
    }
    let Some(room) = running_room(state, request_id, room_id) else {
        return Vec::new();
    };
    if room.phase.is_settled() {
        return Vec::new();
    }
    room.phase = phase;
    room.counts = counts;
    room.failure_kind = (phase == HistoryExportRoomPhase::Failed)
        .then_some(failure_kind)
        .flatten();
    changed()
}

pub(super) fn handle_stop_requested(state: &mut AppState, request_id: u64) -> Vec<AppEffect> {
    let (HistoryExportState::Preparing {
        request_id: active,
        stop_requested,
        ..
    }
    | HistoryExportState::Running {
        request_id: active,
        stop_requested,
        ..
    }) = &mut state.history_export
    else {
        return Vec::new();
    };
    if *active != request_id || *stop_requested {
        return Vec::new();
    }
    *stop_requested = true;
    changed()
}

pub(super) enum Settlement {
    Completed,
    Stopped,
    Failed(HistoryExportFailureKind),
}

pub(super) fn handle_settled(
    state: &mut AppState,
    request_id: u64,
    settlement: Settlement,
) -> Vec<AppEffect> {
    let (scope, range, rooms) = match &mut state.history_export {
        HistoryExportState::Preparing {
            request_id: active,
            scope,
            range,
            ..
        } if *active == request_id => (scope.clone(), range.clone(), Vec::new()),
        HistoryExportState::Running {
            request_id: active,
            scope,
            range,
            rooms,
            ..
        } if *active == request_id => (scope.clone(), range.clone(), std::mem::take(rooms)),
        _ => return Vec::new(),
    };
    state.history_export = match settlement {
        Settlement::Completed => HistoryExportState::Completed {
            request_id,
            scope,
            range,
            rooms,
        },
        Settlement::Stopped => HistoryExportState::Stopped {
            request_id,
            scope,
            range,
            rooms,
        },
        Settlement::Failed(failure_kind) => HistoryExportState::Failed {
            request_id,
            scope,
            range,
            rooms,
            failure_kind,
        },
    };
    changed()
}
