use std::collections::VecDeque;

use super::*;

struct ScriptedSource {
    state: koushi_state::AppState,
    events: VecDeque<(Result<CoreEvent, EventStreamLag>, Option<SubmissionId>)>,
    pending_on_empty: bool,
}

impl SubmissionEventSource for ScriptedSource {
    fn snapshot(&self) -> koushi_state::AppState {
        self.state.clone()
    }

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + '_>> {
        if let Some((event, accepted_id)) = self.events.pop_front() {
            if let Some(accepted_id) = accepted_id {
                self.state
                    .timeline
                    .submission_registry
                    .accepted_submission_ids
                    .push_back(accepted_id);
            }
            Box::pin(async move { event })
        } else if self.pending_on_empty {
            Box::pin(std::future::pending())
        } else {
            Box::pin(async { Err(EventStreamLag { skipped: 0 }) })
        }
    }
}

struct DraftAcceptanceSource {
    state: koushi_state::AppState,
    target: koushi_state::ComposerTarget,
    submitted_revision: koushi_state::ComposerDraftRevision,
    pending_acceptance: bool,
    terminal_lag: Option<EventStreamLag>,
}

impl SubmissionEventSource for DraftAcceptanceSource {
    fn snapshot(&self) -> koushi_state::AppState {
        self.state.clone()
    }

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + '_>> {
        if self.pending_acceptance {
            self.pending_acceptance = false;
            match &self.target {
                koushi_state::ComposerTarget::Main { room_id } => {
                    let _ = self
                        .state
                        .composer_drafts
                        .advance_room_revision(room_id, self.submitted_revision);
                }
                koushi_state::ComposerTarget::Thread {
                    room_id,
                    root_event_id,
                } => {
                    let _ = self.state.composer_drafts.advance_thread_revision(
                        room_id,
                        root_event_id,
                        self.submitted_revision,
                    );
                }
            }
            if let Some(lag) = self.terminal_lag.take() {
                Box::pin(async move { Err(lag) })
            } else {
                Box::pin(async { Ok(accepted(SubmissionId::new("draft-accept"), 99)) })
            }
        } else {
            Box::pin(std::future::pending())
        }
    }
}

fn accepted(id: SubmissionId, sequence: u64) -> CoreEvent {
    CoreEvent::Timeline(TimelineEvent::SubmissionAccepted {
        request_id: request_id(sequence),
        key: build_timeline_key(AccountKey("@u:test".to_owned()), "!r:test".to_owned()),
        submission_id: id,
        transaction_id: "txn".to_owned(),
    })
}

fn request_id(sequence: u64) -> RequestId {
    RequestId {
        connection_id: koushi_core::RuntimeConnectionId(1),
        sequence,
    }
}

fn media_send_queued(request_id: RequestId, transaction_id: &str) -> CoreEvent {
    CoreEvent::Timeline(TimelineEvent::MediaSendQueued {
        request_id,
        key: build_timeline_key(AccountKey("@u:test".to_owned()), "!r:test".to_owned()),
        transaction_id: transaction_id.to_owned(),
    })
}

#[tokio::test]
async fn composer_acceptance_wait_is_target_keyed_after_ui_switch() {
    let targets = [
        koushi_state::ComposerTarget::Main {
            room_id: "!room-a:test".to_owned(),
        },
        koushi_state::ComposerTarget::Thread {
            room_id: "!room-a:test".to_owned(),
            root_event_id: "$root:test".to_owned(),
        },
    ];

    for target in targets {
        let mut state = koushi_state::AppState::default();
        state.timeline.room_id = Some("!room-b:test".to_owned());
        let expected_revision =
            next_composer_draft_acceptance_revision(&state, &target, 4.into()).expect("revision");
        let mut source = DraftAcceptanceSource {
            state,
            target: target.clone(),
            submitted_revision: 4.into(),
            pending_acceptance: true,
            terminal_lag: None,
        };

        assert_eq!(
            wait_for_composer_draft_acceptance(
                &mut source,
                request_id(99),
                &target,
                expected_revision,
                Duration::from_secs(1),
            )
            .await,
            Ok(expected_revision)
        );
    }
}

#[tokio::test]
async fn composer_acceptance_wait_reconciles_terminal_snapshot_after_stream_failure() {
    for skipped in [0, 3] {
        let target = koushi_state::ComposerTarget::Main {
            room_id: "!room-a:test".to_owned(),
        };
        let state = koushi_state::AppState::default();
        let expected_revision =
            next_composer_draft_acceptance_revision(&state, &target, 7.into()).expect("revision");
        let mut source = DraftAcceptanceSource {
            state,
            target: target.clone(),
            submitted_revision: 7.into(),
            pending_acceptance: true,
            terminal_lag: Some(EventStreamLag { skipped }),
        };

        assert_eq!(
            wait_for_composer_draft_acceptance(
                &mut source,
                request_id(99),
                &target,
                expected_revision,
                Duration::from_secs(1),
            )
            .await,
            Ok(expected_revision)
        );
    }
}

#[tokio::test]
async fn composer_acceptance_wait_stops_on_correlated_command_rejection() {
    let target = koushi_state::ComposerTarget::Main {
        room_id: "!room-a:test".to_owned(),
    };
    let rejected_request_id = request_id(99);
    let mut source = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([(
            Ok(CoreEvent::OperationFailed {
                request_id: rejected_request_id,
                failure: koushi_core::CoreFailure::SessionRequired,
            }),
            None,
        )]),
        pending_on_empty: true,
    };

    assert_eq!(
        wait_for_composer_draft_acceptance(
            &mut source,
            rejected_request_id,
            &target,
            1.into(),
            Duration::from_secs(1),
        )
        .await,
        Err("composer draft acceptance was rejected".to_owned())
    );
}

#[tokio::test]
async fn composer_acceptance_wait_stops_only_on_the_correlated_keyed_slash_rejection() {
    // Issue #450: the schedule waiter must ignore unrelated keyed
    // rejections and terminate on the matching request id.
    let target = koushi_state::ComposerTarget::Main {
        room_id: "!room-a:test".to_owned(),
    };
    let expected_request_id = request_id(42);
    let mut source = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([
            (
                Ok(CoreEvent::Room(
                    koushi_core::event::RoomEvent::ComposerSlashCommandRejected {
                        key: koushi_core::TimelineKey::room(
                            koushi_core::AccountKey("@a:test".to_owned()),
                            "!room-a:test",
                        ),
                        request_id: request_id(7),
                    },
                )),
                None,
            ),
            (
                Ok(CoreEvent::Room(
                    koushi_core::event::RoomEvent::ComposerSlashCommandRejected {
                        key: koushi_core::TimelineKey::room(
                            koushi_core::AccountKey("@a:test".to_owned()),
                            "!room-a:test",
                        ),
                        request_id: expected_request_id,
                    },
                )),
                None,
            ),
        ]),
        pending_on_empty: true,
    };

    assert_eq!(
        wait_for_composer_draft_acceptance(
            &mut source,
            expected_request_id,
            &target,
            1.into(),
            Duration::from_secs(1),
        )
        .await,
        Err("composer draft acceptance was rejected".to_owned())
    );
    // Both events were consumed: the unrelated keyed rejection was skipped
    // (continue) and the matching one terminated the wait. If the waiter
    // terminated on ANY keyed rejection, this assertion fails.
    assert!(
        source.events.is_empty(),
        "waiter must consume the unrelated rejection before the matching one"
    );
}

#[tokio::test]
async fn waits_for_global_reducer_acceptance_after_active_room_switch() {
    let expected = SubmissionId::new("expected");
    let mut switched_state = koushi_state::AppState::default();
    switched_state.timeline.room_id = Some("!room-b:test".to_owned());
    let mut source = ScriptedSource {
        state: switched_state,
        events: VecDeque::from([
            (Ok(accepted(SubmissionId::new("other"), 1)), None),
            (Ok(accepted(expected.clone(), 2)), None),
            (
                Ok(accepted(SubmissionId::new("after-accept"), 3)),
                Some(expected.clone()),
            ),
        ]),
        pending_on_empty: false,
    };
    let result = wait_for_submission_outcome(&mut source, &expected, Duration::from_secs(1))
        .await
        .expect("accepted");
    assert_eq!(result.0, SubmissionOutcome::Accepted);
}

#[tokio::test]
async fn matching_rejection_disconnect_lag_and_timeout_are_typed() {
    let expected = SubmissionId::new("expected");
    let rejected = CoreEvent::Timeline(TimelineEvent::SubmissionRejected {
        request_id: RequestId {
            connection_id: koushi_core::RuntimeConnectionId(1),
            sequence: 1,
        },
        key: build_timeline_key(AccountKey("@u:test".to_owned()), "!r:test".to_owned()),
        submission_id: expected.clone(),
        kind: koushi_core::TimelineFailureKind::NotSubscribed,
    });
    let mut source = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([(Ok(rejected), None)]),
        pending_on_empty: false,
    };
    assert!(matches!(
        wait_for_submission_outcome(&mut source, &expected, Duration::from_secs(1)).await,
        Ok((
            SubmissionOutcome::Rejected {
                kind: koushi_core::TimelineFailureKind::NotSubscribed
            },
            None
        ))
    ));
    let mut disconnected = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::new(),
        pending_on_empty: false,
    };
    assert_eq!(
        wait_for_submission_outcome(&mut disconnected, &expected, Duration::from_secs(1)).await,
        Err(SubmissionFailure::Disconnected)
    );
    let mut lagged = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([(Err(EventStreamLag { skipped: 1 }), None)]),
        pending_on_empty: false,
    };
    assert_eq!(
        wait_for_submission_outcome(&mut lagged, &expected, Duration::from_secs(1)).await,
        Err(SubmissionFailure::Lagged)
    );
    let mut timed_out = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::new(),
        pending_on_empty: true,
    };
    assert_eq!(
        wait_for_submission_outcome(&mut timed_out, &expected, Duration::from_millis(1)).await,
        Err(SubmissionFailure::Timeout)
    );
}

#[tokio::test]
async fn prepared_media_wait_ignores_unrelated_queue_event_until_matching_admission() {
    let expected_request = RequestId {
        connection_id: koushi_core::RuntimeConnectionId(1),
        sequence: 8,
    };
    let unrelated_request = RequestId {
        connection_id: koushi_core::RuntimeConnectionId(1),
        sequence: 7,
    };
    let mut source = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([
            (Ok(media_send_queued(unrelated_request, "other")), None),
            (Ok(media_send_queued(expected_request, "expected")), None),
        ]),
        pending_on_empty: false,
    };

    assert_eq!(
        wait_for_prepared_media_queue(
            &mut source,
            expected_request,
            "expected",
            Duration::from_secs(1),
        )
        .await,
        Ok(())
    );
}

#[tokio::test]
async fn prepared_media_queue_wait_returns_matching_failure_before_cleanup() {
    let request_id = RequestId {
        connection_id: koushi_core::RuntimeConnectionId(1),
        sequence: 8,
    };
    let mut source = ScriptedSource {
        state: koushi_state::AppState::default(),
        events: VecDeque::from([(
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure: koushi_core::CoreFailure::TimelineOperationFailed {
                    kind: koushi_core::TimelineFailureKind::Network,
                },
            }),
            None,
        )]),
        pending_on_empty: false,
    };

    let failure =
        wait_for_prepared_media_queue(&mut source, request_id, "expected", Duration::from_secs(1))
            .await
            .expect_err("matching failure must be terminal");
    assert!(failure.starts_with("prepared upload send failed"));
}
