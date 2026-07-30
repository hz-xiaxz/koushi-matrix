use crate::{
    AppEffect, AppState, CurrentSessionStatusDetails, CurrentSessionStatusFailureKind,
    CurrentSessionStatusState, SessionState, SessionStatusRefreshTrigger,
};

pub(super) fn handle_refresh_requested(
    state: &mut AppState,
    request_id: u64,
    trigger: SessionStatusRefreshTrigger,
) -> Vec<AppEffect> {
    if !matches!(state.session, SessionState::Ready(_))
        || matches!(
            state.current_session_status,
            CurrentSessionStatusState::Checking { .. }
        )
    {
        return Vec::new();
    }
    state.current_session_status = CurrentSessionStatusState::Checking {
        request_id,
        trigger,
    };
    vec![AppEffect::RefreshCurrentSessionStatus {
        request_id,
        trigger,
    }]
}

pub(super) fn handle_refreshed(
    state: &mut AppState,
    request_id: u64,
    details: CurrentSessionStatusDetails,
) -> Vec<AppEffect> {
    if !matches!(
        state.current_session_status,
        CurrentSessionStatusState::Checking {
            request_id: active_request_id,
            ..
        } if active_request_id == request_id
    ) {
        return Vec::new();
    }
    state.current_session_status = CurrentSessionStatusState::Ready {
        request_id,
        details,
    };
    Vec::new()
}

pub(super) fn handle_refresh_failed(
    state: &mut AppState,
    request_id: u64,
    kind: CurrentSessionStatusFailureKind,
    checked_at_ms: u64,
) -> Vec<AppEffect> {
    if !matches!(
        state.current_session_status,
        CurrentSessionStatusState::Checking {
            request_id: active_request_id,
            ..
        } if active_request_id == request_id
    ) {
        return Vec::new();
    }
    state.current_session_status = CurrentSessionStatusState::Failed {
        request_id,
        kind,
        checked_at_ms,
    };
    Vec::new()
}

pub(super) fn reset(state: &mut AppState) {
    state.current_session_status = CurrentSessionStatusState::Idle;
}
