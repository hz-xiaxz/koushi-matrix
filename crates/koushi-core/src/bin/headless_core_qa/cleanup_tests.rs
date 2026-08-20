use super::{
    QaE2eeLogoutBarrier, cleanup_all_owned_e2ee_participants,
    cleanup_owned_e2ee_lifecycle_best_effort,
};
use crate::contracts::{
    FirstEventSharedSnapshotPendingSource, FirstEventThenTerminalLagSource,
    IntervalQaSnapshotEventSource, RecordedOwnedE2eeCleanupOperation,
    ScriptedQaSnapshotEventSource, qa_logged_out_event, qa_operation_failed_event,
    qa_state_with_session, recording_owned_e2ee_cleanup_operations,
};
use crate::event_wait::{
    wait_for_logged_out, wait_for_operation_failed_and_signed_out, wait_for_signed_out_after_logout,
};
use crate::participants::{
    QaOwnedRuntimePhase, finish_e2ee_recipient_stage_with_owned_cleanup,
    retain_or_cleanup_e2ee_callers_after_stage,
};
use crate::registry::EVENT_TIMEOUT;
use crate::{
    AccountEvent, AccountKey, Arc, CoreEvent, CoreFailure, Duration, Mutex, RequestId, SessionState,
};

#[tokio::test]
async fn owned_e2ee_recipient_cleanup_runs_after_post_login_stage_failure() {
    let cleanup_attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_attempts = cleanup_attempts.clone();

    let result = finish_e2ee_recipient_stage_with_owned_cleanup(
        Err::<(), _>("injected post-login failure".to_owned()),
        Some("owned-recipient"),
        move |participant| {
            let cleanup_attempts = cleanup_attempts.clone();
            async move {
                assert_eq!(participant, "owned-recipient");
                cleanup_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        },
    )
    .await;

    assert_eq!(result.unwrap_err(), "injected post-login failure");
    assert_eq!(
        observed_attempts.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn borrowed_e2ee_stage_failure_runs_outer_caller_cleanup_path() {
    let cleanup_attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_attempts = cleanup_attempts.clone();

    let result = retain_or_cleanup_e2ee_callers_after_stage(
        Err::<(), _>("injected borrowed-stage failure".to_owned()),
        ("caller-a", "caller-b"),
        move |callers| {
            let cleanup_attempts = cleanup_attempts.clone();
            async move {
                assert_eq!(callers, ("caller-a", "caller-b"));
                cleanup_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        },
    )
    .await;

    assert_eq!(result.unwrap_err(), "injected borrowed-stage failure");
    assert_eq!(
        observed_attempts.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn owned_e2ee_cleanup_orders_each_ownership_phase() {
    let account_key = AccountKey("@owned:example.invalid".to_owned());
    let cases = [
        (
            QaOwnedRuntimePhase::LoginNotSubmitted,
            vec![
                RecordedOwnedE2eeCleanupOperation::DropConnection,
                RecordedOwnedE2eeCleanupOperation::ShutdownRuntime,
            ],
        ),
        (
            QaOwnedRuntimePhase::LoginSubmitted,
            vec![
                RecordedOwnedE2eeCleanupOperation::Logout(QaE2eeLogoutBarrier::AnyAccount),
                RecordedOwnedE2eeCleanupOperation::AuthoritativeLogoutBarrier(
                    QaE2eeLogoutBarrier::AnyAccount,
                ),
                RecordedOwnedE2eeCleanupOperation::DropConnection,
                RecordedOwnedE2eeCleanupOperation::ShutdownRuntime,
            ],
        ),
        (
            QaOwnedRuntimePhase::LoggedIn(account_key.clone()),
            vec![
                RecordedOwnedE2eeCleanupOperation::StopSync,
                RecordedOwnedE2eeCleanupOperation::Logout(QaE2eeLogoutBarrier::Exact(
                    account_key.clone(),
                )),
                RecordedOwnedE2eeCleanupOperation::AuthoritativeLogoutBarrier(
                    QaE2eeLogoutBarrier::Exact(account_key),
                ),
                RecordedOwnedE2eeCleanupOperation::DropConnection,
                RecordedOwnedE2eeCleanupOperation::ShutdownRuntime,
            ],
        ),
    ];

    for (phase, expected) in cases {
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut operations =
            recording_owned_e2ee_cleanup_operations("participant", false, &observed);

        cleanup_owned_e2ee_lifecycle_best_effort(
            &phase,
            &mut operations,
            "ownership phase cleanup",
        )
        .await
        .expect("phase cleanup should succeed");

        let actual = observed
            .lock()
            .expect("cleanup observation lock")
            .iter()
            .map(|(_, operation)| operation.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn borrowed_e2ee_recipient_is_not_cleaned_by_the_inner_stage() {
    let cleanup_attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_attempts = cleanup_attempts.clone();

    let result = finish_e2ee_recipient_stage_with_owned_cleanup(
        Err::<(), _>("injected borrowed-stage failure".to_owned()),
        None::<&'static str>,
        move |_| {
            let cleanup_attempts = cleanup_attempts.clone();
            async move {
                cleanup_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        },
    )
    .await;

    assert_eq!(result.unwrap_err(), "injected borrowed-stage failure");
    assert_eq!(
        observed_attempts.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn e2ee_multi_device_cleanup_attempts_every_owned_participant_after_one_failure() {
    let operations = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = operations.clone();
    let account_key = AccountKey("@owned:example.invalid".to_owned());

    let result = cleanup_all_owned_e2ee_participants(
        [
            Some((
                QaOwnedRuntimePhase::LoggedIn(account_key.clone()),
                recording_owned_e2ee_cleanup_operations("B3", true, &operations),
            )),
            Some((
                QaOwnedRuntimePhase::LoggedIn(account_key.clone()),
                recording_owned_e2ee_cleanup_operations("B2", false, &operations),
            )),
            Some((
                QaOwnedRuntimePhase::LoggedIn(account_key),
                recording_owned_e2ee_cleanup_operations("B", false, &operations),
            )),
        ],
        move |(phase, mut participant_operations)| async move {
            cleanup_owned_e2ee_lifecycle_best_effort(
                &phase,
                &mut participant_operations,
                "multi-device cleanup",
            )
            .await
        },
    )
    .await;

    assert_eq!(
        result.unwrap_err(),
        "E2EE cleanup failed for 1 owned recipient participant(s)"
    );
    let observed = observed.lock().expect("cleanup observation lock");
    for participant in ["B3", "B2", "B"] {
        let participant_operations = observed
            .iter()
            .filter_map(|(observed_participant, operation)| {
                (*observed_participant == participant).then_some(operation)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            participant_operations.last(),
            Some(&&RecordedOwnedE2eeCleanupOperation::ShutdownRuntime),
            "{participant} must reach ordered runtime shutdown"
        );
    }
}

#[tokio::test]
async fn logged_out_waiter_requires_event_and_signed_out_snapshot_in_either_order() {
    let request_id = RequestId {
        connection_id: koushi_core::ids::RuntimeConnectionId(1),
        sequence: 7,
    };
    let account_key = AccountKey("@logout-barrier:example.invalid".to_owned());
    let signed_out = qa_state_with_session(SessionState::SignedOut);
    let cases = [
        [
            (
                qa_logged_out_event(request_id, account_key.clone()),
                SessionState::LoggingOut,
            ),
            (
                CoreEvent::StateChanged(signed_out.clone()),
                SessionState::SignedOut,
            ),
        ],
        [
            (
                CoreEvent::StateChanged(signed_out.clone()),
                SessionState::SignedOut,
            ),
            (
                qa_logged_out_event(request_id, account_key.clone()),
                SessionState::SignedOut,
            ),
        ],
    ];

    for events in cases {
        let mut source = ScriptedQaSnapshotEventSource {
            events: events.into(),
            snapshot: qa_state_with_session(SessionState::LoggingOut),
            received: 0,
        };
        wait_for_logged_out(&mut source, request_id, &account_key, "logout barrier")
            .await
            .expect("both authoritative logout signals should satisfy the barrier");
        assert_eq!(
            source.received, 2,
            "neither event nor snapshot may complete the barrier alone"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn logout_waiters_observe_final_signed_out_snapshot_without_another_broadcast() {
    for keyed in [true, false] {
        let request_id = RequestId {
            connection_id: koushi_core::ids::RuntimeConnectionId(1),
            sequence: if keyed { 71 } else { 72 },
        };
        let account_key = AccountKey("@logout-final-snapshot:example.invalid".to_owned());
        let shared = Arc::new(Mutex::new(qa_state_with_session(SessionState::LoggingOut)));
        let mut source = FirstEventSharedSnapshotPendingSource {
            first_event: Some(qa_logged_out_event(request_id, account_key.clone())),
            snapshot: shared.clone(),
        };
        let signed_out_shared = shared.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            signed_out_shared
                .lock()
                .expect("shared QA snapshot lock should not be poisoned")
                .session = SessionState::SignedOut;
        });
        let started_at = tokio::time::Instant::now();

        if keyed {
            wait_for_logged_out(
                &mut source,
                request_id,
                &account_key,
                "keyed logout final snapshot",
            )
            .await
        } else {
            wait_for_signed_out_after_logout(
                &mut source,
                request_id,
                "keyless logout final snapshot",
            )
            .await
        }
        .expect("the final authoritative SignedOut snapshot should complete logout");

        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            EVENT_TIMEOUT
        );
    }
}

#[tokio::test]
async fn logout_waiters_observe_final_signed_out_snapshot_after_lag_or_close() {
    for (keyed, skipped) in [(true, 0), (false, 4)] {
        let request_id = RequestId {
            connection_id: koushi_core::ids::RuntimeConnectionId(1),
            sequence: if keyed { 73 } else { 74 },
        };
        let account_key = AccountKey("@logout-terminal-lag:example.invalid".to_owned());
        let mut source = FirstEventThenTerminalLagSource {
            first_event: Some(qa_logged_out_event(request_id, account_key.clone())),
            snapshot: qa_state_with_session(SessionState::LoggingOut),
            skipped,
        };

        if keyed {
            wait_for_logged_out(
                &mut source,
                request_id,
                &account_key,
                "keyed logout terminal snapshot",
            )
            .await
        } else {
            wait_for_signed_out_after_logout(
                &mut source,
                request_id,
                "keyless logout terminal snapshot",
            )
            .await
        }
        .expect("the terminal receive must recheck the authoritative SignedOut snapshot");
    }
}

#[tokio::test]
async fn logged_out_waiter_keeps_wrong_account_and_failure_terminal_and_private_safe() {
    let request_id = RequestId {
        connection_id: koushi_core::ids::RuntimeConnectionId(1),
        sequence: 8,
    };
    let account_key = AccountKey("@expected:example.invalid".to_owned());
    let mut wrong_account = ScriptedQaSnapshotEventSource {
        events: [(
            qa_logged_out_event(
                request_id,
                AccountKey("@unexpected:example.invalid".to_owned()),
            ),
            SessionState::SignedOut,
        )]
        .into(),
        snapshot: qa_state_with_session(SessionState::LoggingOut),
        received: 0,
    };
    let error = wait_for_logged_out(
        &mut wrong_account,
        request_id,
        &account_key,
        "logout barrier",
    )
    .await
    .expect_err("wrong account must fail immediately");
    assert!(error.contains("account_key mismatch"));
    assert!(!error.contains('@'));
    assert_eq!(wrong_account.received, 1);

    let mut failed = ScriptedQaSnapshotEventSource {
        events: [(
            CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::SessionRequired,
            },
            SessionState::SignedOut,
        )]
        .into(),
        snapshot: qa_state_with_session(SessionState::LoggingOut),
        received: 0,
    };
    let error = wait_for_logged_out(&mut failed, request_id, &account_key, "logout barrier")
        .await
        .expect_err("correlated failure must fail immediately");
    assert!(error.contains("SessionRequired"));
    assert_eq!(failed.received, 1);
}

#[tokio::test]
async fn operation_failed_signed_out_waiter_requires_both_signals_in_either_order() {
    let request_id = RequestId {
        connection_id: koushi_core::ids::RuntimeConnectionId(1),
        sequence: 10,
    };
    let signed_out = qa_state_with_session(SessionState::SignedOut);
    let cases = [
        [
            (
                qa_operation_failed_event(request_id),
                SessionState::Restoring,
            ),
            (
                CoreEvent::StateChanged(signed_out.clone()),
                SessionState::SignedOut,
            ),
        ],
        [
            (
                CoreEvent::StateChanged(signed_out.clone()),
                SessionState::SignedOut,
            ),
            (
                qa_operation_failed_event(request_id),
                SessionState::SignedOut,
            ),
        ],
    ];

    for events in cases {
        let mut source = ScriptedQaSnapshotEventSource {
            events: events.into(),
            snapshot: qa_state_with_session(SessionState::Restoring),
            received: 0,
        };
        let failure = wait_for_operation_failed_and_signed_out(
            &mut source,
            request_id,
            "restore cleanup barrier",
        )
        .await
        .expect("both authoritative cleanup signals should satisfy the barrier");
        assert_eq!(failure, CoreFailure::SessionNotFound);
        assert_eq!(
            source.received, 2,
            "neither failure nor SignedOut may complete the barrier alone"
        );
    }

    let mut succeeded = ScriptedQaSnapshotEventSource {
        events: [(
            CoreEvent::Account(AccountEvent::SessionRestored {
                request_id,
                account_key: AccountKey("@private:example.invalid".to_owned()),
            }),
            SessionState::SignedOut,
        )]
        .into(),
        snapshot: signed_out,
        received: 0,
    };
    let error = wait_for_operation_failed_and_signed_out(
        &mut succeeded,
        request_id,
        "restore cleanup barrier",
    )
    .await
    .expect_err("a same-request success terminal must fail immediately");
    assert!(error.contains("operation succeeded"));
    assert!(!error.contains('@'));
    assert_eq!(succeeded.received, 1);
}

#[tokio::test(start_paused = true)]
async fn operation_failed_signed_out_deadline_survives_unrelated_event_starvation() {
    let request_id = RequestId {
        connection_id: koushi_core::ids::RuntimeConnectionId(1),
        sequence: 11,
    };
    let mut source = IntervalQaSnapshotEventSource {
        interval: tokio::time::interval(Duration::from_secs(1)),
        snapshot: qa_state_with_session(SessionState::Restoring),
        first_event: Some(qa_operation_failed_event(request_id)),
    };
    let started_at = tokio::time::Instant::now();
    wait_for_operation_failed_and_signed_out(&mut source, request_id, "restore cleanup deadline")
        .await
        .expect_err("unrelated events must not restart the cleanup deadline");
    assert_eq!(
        tokio::time::Instant::now().duration_since(started_at),
        EVENT_TIMEOUT
    );
}
