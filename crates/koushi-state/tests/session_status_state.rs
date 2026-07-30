use koushi_state::{
    AppAction, AppEffect, AppState, CurrentSessionBackupState, CurrentSessionStatusDetails,
    CurrentSessionStatusFailureKind, CurrentSessionStatusState, CurrentSessionSyncState,
    CurrentSessionVerification, OwnIdentityVerification, SessionAuthenticationMethod, SessionInfo,
    SessionState, SessionStatusRefreshTrigger, reduce,
};

fn ready_state() -> AppState {
    AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@user:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
        }),
        ..AppState::default()
    }
}

fn details(
    is_cross_signed_by_owner: bool,
    own_identity: OwnIdentityVerification,
) -> CurrentSessionStatusDetails {
    CurrentSessionStatusDetails::new(
        Some("Koushi on Linux".to_owned()),
        "DEVICE".to_owned(),
        SessionAuthenticationMethod::OAuth,
        CurrentSessionSyncState::Running,
        is_cross_signed_by_owner,
        own_identity,
        CurrentSessionBackupState::Ready,
        1_234,
    )
}

#[test]
fn refresh_enters_checking_and_emits_one_correlated_effect() {
    let mut state = ready_state();

    let effects = reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshRequested {
            request_id: 7,
            trigger: SessionStatusRefreshTrigger::Open,
        },
    );

    assert_eq!(
        state.current_session_status,
        CurrentSessionStatusState::Checking {
            request_id: 7,
            trigger: SessionStatusRefreshTrigger::Open,
        }
    );
    assert_eq!(
        effects,
        vec![AppEffect::RefreshCurrentSessionStatus {
            request_id: 7,
            trigger: SessionStatusRefreshTrigger::Open,
        }]
    );
}

#[test]
fn duplicate_refresh_is_rejected_while_checking() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshRequested {
            request_id: 7,
            trigger: SessionStatusRefreshTrigger::Open,
        },
    );

    let effects = reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshRequested {
            request_id: 8,
            trigger: SessionStatusRefreshTrigger::Manual,
        },
    );

    assert!(effects.is_empty());
    assert!(matches!(
        state.current_session_status,
        CurrentSessionStatusState::Checking { request_id: 7, .. }
    ));
}

#[test]
fn correlated_completion_settles_ready_and_derives_verified_once_in_rust() {
    let mut state = ready_state();
    state.current_session_status = CurrentSessionStatusState::Checking {
        request_id: 7,
        trigger: SessionStatusRefreshTrigger::Manual,
    };

    reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshed {
            request_id: 7,
            details: details(true, OwnIdentityVerification::Verified),
        },
    );

    let CurrentSessionStatusState::Ready {
        request_id,
        details,
    } = &state.current_session_status
    else {
        panic!("expected ready status");
    };
    assert_eq!(*request_id, 7);
    assert_eq!(details.verification, CurrentSessionVerification::Verified);
}

#[test]
fn either_missing_trust_fact_derives_unverified() {
    assert_eq!(
        details(false, OwnIdentityVerification::Verified).verification,
        CurrentSessionVerification::Unverified
    );
    assert_eq!(
        details(true, OwnIdentityVerification::Unverified).verification,
        CurrentSessionVerification::Unverified
    );
}

#[test]
fn failed_refresh_replaces_prior_ready_status() {
    let mut state = ready_state();
    state.current_session_status = CurrentSessionStatusState::Ready {
        request_id: 6,
        details: details(true, OwnIdentityVerification::Verified),
    };
    reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshRequested {
            request_id: 7,
            trigger: SessionStatusRefreshTrigger::Manual,
        },
    );

    reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshFailed {
            request_id: 7,
            kind: CurrentSessionStatusFailureKind::Sdk,
            checked_at_ms: 1_235,
        },
    );

    assert_eq!(
        state.current_session_status,
        CurrentSessionStatusState::Failed {
            request_id: 7,
            kind: CurrentSessionStatusFailureKind::Sdk,
            checked_at_ms: 1_235,
        }
    );
}

#[test]
fn stale_completion_cannot_replace_the_current_request() {
    let mut state = ready_state();
    state.current_session_status = CurrentSessionStatusState::Checking {
        request_id: 8,
        trigger: SessionStatusRefreshTrigger::Manual,
    };

    let effects = reduce(
        &mut state,
        AppAction::CurrentSessionStatusRefreshed {
            request_id: 7,
            details: details(true, OwnIdentityVerification::Verified),
        },
    );

    assert!(effects.is_empty());
    assert!(matches!(
        state.current_session_status,
        CurrentSessionStatusState::Checking { request_id: 8, .. }
    ));
}

#[test]
fn logout_resets_current_session_status() {
    let mut state = ready_state();
    state.current_session_status = CurrentSessionStatusState::Ready {
        request_id: 7,
        details: details(true, OwnIdentityVerification::Verified),
    };

    reduce(&mut state, AppAction::LogoutRequested);

    assert_eq!(
        state.current_session_status,
        CurrentSessionStatusState::Idle
    );
}
