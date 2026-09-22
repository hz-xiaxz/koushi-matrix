use crate::state::{SESSION_STATUS_FRESHNESS_MS, session_status_failure_backoff_ms};
use crate::{
    AppEffect, AppState, CurrentSessionStatusDetails, CurrentSessionStatusFailureKind,
    CurrentSessionStatusState, SessionState, SessionStatusRefreshTrigger,
};

fn last_known_details(state: &CurrentSessionStatusState) -> Option<CurrentSessionStatusDetails> {
    match state {
        CurrentSessionStatusState::Ready { details, .. } => Some(details.clone()),
        CurrentSessionStatusState::Checking {
            last_known_details, ..
        }
        | CurrentSessionStatusState::Failed {
            last_known_details, ..
        } => last_known_details.clone(),
        CurrentSessionStatusState::Idle => None,
    }
}

pub(crate) fn consecutive_failures(state: &CurrentSessionStatusState) -> u32 {
    match state {
        CurrentSessionStatusState::Checking {
            consecutive_failures,
            ..
        }
        | CurrentSessionStatusState::Failed {
            consecutive_failures,
            ..
        } => *consecutive_failures,
        CurrentSessionStatusState::Idle | CurrentSessionStatusState::Ready { .. } => 0,
    }
}

/// #982: a full inspection costs several remote round trips, so an automatic
/// trigger only runs it when the last known status is stale or its failure
/// cooldown has expired. `Manual` is the user asking for a check now and always
/// bypasses this.
fn automatic_refresh_is_due(status: &CurrentSessionStatusState, now_ms: u64) -> bool {
    match status {
        // No check yet: app/session start and account switch both land here.
        CurrentSessionStatusState::Idle => true,
        CurrentSessionStatusState::Checking { .. } => false,
        CurrentSessionStatusState::Ready { details, .. } => {
            now_ms.saturating_sub(details.checked_at_ms) >= SESSION_STATUS_FRESHNESS_MS
        }
        CurrentSessionStatusState::Failed {
            checked_at_ms,
            consecutive_failures,
            ..
        } => {
            now_ms.saturating_sub(*checked_at_ms)
                >= session_status_failure_backoff_ms(*consecutive_failures)
        }
    }
}

pub(super) fn handle_refresh_requested(
    state: &mut AppState,
    request_id: u64,
    trigger: SessionStatusRefreshTrigger,
    now_ms: u64,
) -> Vec<AppEffect> {
    if !matches!(state.session, SessionState::Ready(_))
        || matches!(
            state.current_session_status,
            CurrentSessionStatusState::Checking { .. }
        )
    {
        return Vec::new();
    }
    if !matches!(trigger, SessionStatusRefreshTrigger::Manual)
        && !automatic_refresh_is_due(&state.current_session_status, now_ms)
    {
        // Serve the last known status unchanged.
        return Vec::new();
    }
    let last_known_details = last_known_details(&state.current_session_status);
    let consecutive_failures = consecutive_failures(&state.current_session_status);
    state.current_session_status = CurrentSessionStatusState::Checking {
        request_id,
        trigger,
        last_known_details,
        consecutive_failures,
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
    let last_known_details = last_known_details(&state.current_session_status);
    let consecutive_failures =
        consecutive_failures(&state.current_session_status).saturating_add(1);
    state.current_session_status = CurrentSessionStatusState::Failed {
        request_id,
        kind,
        checked_at_ms,
        last_known_details,
        consecutive_failures,
    };
    Vec::new()
}

pub(super) fn reset(state: &mut AppState) {
    state.current_session_status = CurrentSessionStatusState::Idle;
}
