use std::collections::HashMap;

use std::sync::Arc;

use std::time::Duration;

use koushi_sdk::{
    MatrixClientSession, MatrixOutboundGroupSessionToken, MatrixRoomKeyReshareTarget,
};

use tokio::sync::{mpsc, oneshot};

use crate::account_work::{AccountWorkKind, AccountWorkScheduler};

use crate::command::TimelineCommand;
use crate::event::{
    CoreEvent, RoomKeyRequestStage, RoomKeyRequestStateDto, RoomKeyRequestWithheldCode,
    TimelineEvent,
};
use crate::executor;

#[cfg(any(test, feature = "test-hooks"))]
use crate::ids::AccountKey;
use crate::ids::TimelineKey;

use koushi_diagnostics::DiagnosticValue;

use super::super::diagnostics::{
    decrypt_retry_backup_result_for_error, record_decrypt_retry_backup_lookup,
    record_decrypt_retry_device_request, record_decrypt_retry_request,
    record_decrypt_retry_settled, record_room_key_reshare,
};
use super::super::item_projection::{
    key_request_stage_token, key_request_withheld_code_token, withheld_update_should_publish,
};
use super::super::manager::{TimelineManagerActor, TimelineManagerControl, TimelineMessage};
use super::super::outbound_send::{
    TimelineSendCompletionDelivery, TimelineSendTerminalAdmission, TimelineSendTerminalHandoff,
};
use super::super::test_support::{fake_rid, live_tail_test_manager, test_timeline_actor_handle};
use super::{
    DecryptRetryBackupResult, DecryptRetryBackupState, DecryptRetryController,
    DecryptRetryDeviceResult, DecryptRetryFailure, DecryptRetryReason, DecryptRetrySettledResult,
    ROOM_KEY_RESHARE_ATTEMPTS, RoomKeyReshareCompletion, RoomKeyReshareSchedule,
    RoomKeyReshareTaskSlot, RoomKeyReshareTestSignals, decrypt_retry_backup_state_for,
    decrypt_retry_settlement_operation, next_decrypt_retry_operation,
    spawn_delayed_timeline_message, spawn_room_key_reshare_task_with_operation,
};

#[tokio::test(start_paused = true)]
async fn room_key_reshare_wakes_only_at_the_three_bounded_delays() {
    let (tx, mut rx) = mpsc::channel(3);
    let tasks = ROOM_KEY_RESHARE_ATTEMPTS
        .iter()
        .map(|attempt| spawn_delayed_timeline_message(tx.clone(), attempt.delay, attempt.number))
        .collect::<Vec<_>>();

    for (advance, expected) in [(3, 1), (2, 2), (10, 3)] {
        tokio::time::advance(Duration::from_secs(advance)).await;
        assert_eq!(rx.recv().await, Some(expected));
    }
    assert!(rx.try_recv().is_err());
    for task in tasks {
        task.await.expect("timer task completed");
    }
}

#[tokio::test(start_paused = true)]
async fn delayed_room_key_reshare_wake_is_cancellable() {
    let (tx, mut rx) = mpsc::channel(1);
    let task = spawn_delayed_timeline_message(tx, Duration::from_secs(3), ());
    task.abort();

    tokio::time::advance(Duration::from_secs(3)).await;

    assert!(rx.try_recv().is_err());
    assert!(task.await.expect_err("aborted timer").is_cancelled());
}

async fn assert_room_key_reshare_slot_released(scheduler: &AccountWorkScheduler) {
    let permit = tokio::time::timeout(
        Duration::from_secs(1),
        scheduler.acquire(AccountWorkKind::RoomKeyReshare),
    )
    .await
    .expect("room-key reshare work must release its scheduler slot");
    drop(permit);
}

#[tokio::test]
async fn room_key_reshare_waiter_does_not_block_manager_terminal_progress() {
    let fixture = room_key_reshare_fixture().await;
    let key = fixture.key.clone();
    let mut manager =
        live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
    manager.session = Some(fixture.session.clone());
    let generation = manager
        .timeline_actor_generations
        .activate_after_quiescence(&key)
        .await
        .generation;
    install_room_key_reshare_schedule(&mut manager, &key, fixture.token.clone());
    let scheduler = manager.account_work.clone();
    let _interactive = scheduler.begin_interactive(AccountWorkKind::MessageSend);
    let terminal_ingress = manager.terminal_ingress.clone();
    let mut event_rx = manager.event_tx.subscribe();
    let (control_tx, control_rx) = mpsc::channel(1);
    manager.control_rx = Some(control_rx);
    let manager_tx = manager.msg_tx.clone();
    let manager_task = executor::spawn(manager.run());

    manager_tx
        .send(TimelineMessage::RunRoomKeyReshare {
            key: key.clone(),
            actor_generation: generation,
            expected_session: fixture.token.clone(),
            target: MatrixRoomKeyReshareTarget::OwnOtherDevices,
            attempt: 1,
        })
        .await
        .expect("reshare wake must enter the manager mailbox");
    let (processed_tx, processed_rx) = oneshot::channel();
    manager_tx
        .send(TimelineMessage::TestLiveTailDispatchState {
            key: key.clone(),
            epoch: 0,
            response: processed_tx,
        })
        .await
        .expect("manager probe must enter after the reshare wake");
    tokio::time::timeout(Duration::from_secs(1), processed_rx)
        .await
        .expect("manager must keep polling while reshare waits")
        .expect("manager probe response");

    let terminal_request = fake_rid(92_000);
    assert!(matches!(
        terminal_ingress.admit(TimelineSendTerminalHandoff {
            submission_id: None,
            action: None,
            completion: Some(TimelineSendCompletionDelivery {
                request_id: terminal_request,
                key: key.clone(),
                transaction_id: "reshare-terminal-progress".to_owned(),
                event_id: "$reshare-terminal-progress:test".to_owned(),
                diagnostic_correlation: None,
            }),
            failure: None,
        }),
        TimelineSendTerminalAdmission::Accepted
    ));
    let delivered = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let CoreEvent::Timeline(TimelineEvent::SendCompleted {
                request_id,
                key: completed_key,
                ..
            }) = event_rx.recv().await.expect("manager event stream")
                && request_id == terminal_request
                && completed_key == key
            {
                break;
            }
        }
    })
    .await;
    assert!(
        delivered.is_ok(),
        "the manager must deliver a correlated send terminal while reshare waits"
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    control_tx
        .send(TimelineManagerControl::Shutdown {
            acknowledged: shutdown_tx,
        })
        .await
        .expect("manager shutdown control");
    tokio::time::timeout(Duration::from_secs(1), shutdown_rx)
        .await
        .expect("manager shutdown must not wait for the reshare permit")
        .expect("manager shutdown acknowledgement");
    manager_task.await.expect("manager task");
    drop(_interactive);
    assert_room_key_reshare_slot_released(&scheduler).await;
}

#[tokio::test]
async fn room_key_reshare_completion_is_exactly_once_and_stale_inputs_are_inert() {
    let fixture = room_key_reshare_fixture().await;
    let diagnostic_lock = koushi_diagnostics::test_support::lock();
    let mut manager = live_tail_test_manager(HashMap::from([(
        fixture.key.clone(),
        test_timeline_actor_handle(),
    )]));
    let generation = manager
        .timeline_actor_generations
        .activate_after_quiescence(&fixture.key)
        .await
        .generation;
    install_pending_room_key_reshare_worker(
        &mut manager,
        &fixture.key,
        generation,
        fixture.token.clone(),
        1,
    );
    let detail_start = koushi_diagnostics::test_support::detail_snapshot()
        .records
        .len();
    manager
        .handle_room_key_reshare_completed(
            fixture.key.clone(),
            generation,
            fixture.token.clone(),
            MatrixRoomKeyReshareTarget::OwnOtherDevices,
            1,
            RoomKeyReshareCompletion::Sent {
                request_count: 1,
                recipient_count: 1,
                failed_recipient_count: 0,
            },
        )
        .await;
    let detail_after_first = koushi_diagnostics::test_support::detail_snapshot()
        .records
        .iter()
        .skip(detail_start)
        .filter(|record| record.event.source == "core.room_key_reshare")
        .count();
    manager
        .handle_room_key_reshare_completed(
            fixture.key.clone(),
            generation,
            fixture.token.clone(),
            MatrixRoomKeyReshareTarget::OwnOtherDevices,
            1,
            RoomKeyReshareCompletion::Sent {
                request_count: 1,
                recipient_count: 1,
                failed_recipient_count: 0,
            },
        )
        .await;
    let detail_after_duplicate = koushi_diagnostics::test_support::detail_snapshot()
        .records
        .iter()
        .skip(detail_start)
        .filter(|record| record.event.source == "core.room_key_reshare")
        .count();
    assert_eq!(detail_after_first, 1);
    assert_eq!(detail_after_duplicate, detail_after_first);

    for (label, key, stale_generation, stale_token) in [
        (
            "key",
            TimelineKey::room(AccountKey("@stale:test".to_owned()), "!stale:test"),
            generation,
            fixture.token.clone(),
        ),
        (
            "generation",
            fixture.key.clone(),
            generation + 1,
            fixture.token.clone(),
        ),
        (
            "token",
            fixture.key.clone(),
            generation,
            fixture.other_token.clone(),
        ),
    ] {
        let mut stale_manager = live_tail_test_manager(HashMap::from([(
            fixture.key.clone(),
            test_timeline_actor_handle(),
        )]));
        let current_generation = stale_manager
            .timeline_actor_generations
            .activate_after_quiescence(&fixture.key)
            .await
            .generation;
        install_pending_room_key_reshare_worker(
            &mut stale_manager,
            &fixture.key,
            current_generation,
            fixture.token.clone(),
            1,
        );
        let stale_manager_detail_start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        stale_manager
            .handle_room_key_reshare_completed(
                key,
                stale_generation,
                stale_token,
                MatrixRoomKeyReshareTarget::OwnOtherDevices,
                1,
                RoomKeyReshareCompletion::Sent {
                    request_count: 1,
                    recipient_count: 1,
                    failed_recipient_count: 0,
                },
            )
            .await;
        assert!(
            stale_manager
                .send_enqueue_workers
                .room_key_reshares
                .get(&fixture.key)
                .and_then(|schedule| schedule.tasks[0].worker.as_ref())
                .is_some(),
            "stale {label} completion must not consume the active task"
        );
        assert_eq!(
            koushi_diagnostics::test_support::detail_snapshot()
                .records
                .iter()
                .skip(stale_manager_detail_start)
                .filter(|record| record.event.source == "core.room_key_reshare")
                .count(),
            0,
            "stale {label} completion must not record diagnostics"
        );
    }
    drop(diagnostic_lock);
}

#[tokio::test]
async fn room_key_reshare_replacement_unsubscribe_and_shutdown_abort_owned_work() {
    let fixture = room_key_reshare_fixture().await;

    for admitted in [false, true] {
        for cancellation in ["replacement", "unsubscribe", "shutdown"] {
            let mut manager = live_tail_test_manager(if cancellation == "unsubscribe" {
                HashMap::from([(fixture.key.clone(), test_timeline_actor_handle())])
            } else {
                HashMap::new()
            });
            let scheduler = manager.account_work.clone();
            let interactive =
                (!admitted).then(|| scheduler.begin_interactive(AccountWorkKind::MessageSend));
            let (mut completion_tx, acquire_started, permit_acquired) =
                install_controlled_room_key_reshare_worker(
                    &mut manager,
                    &fixture.key,
                    1,
                    fixture.token.clone(),
                    1,
                );

            acquire_started
                .await
                .expect("reshare worker must enter account-work acquire");
            if admitted {
                permit_acquired
                    .await
                    .expect("reshare worker must acquire its permit");
            }

            match cancellation {
                "replacement" => {
                    install_room_key_reshare_schedule(
                        &mut manager,
                        &fixture.key,
                        fixture.other_token.clone(),
                    );
                }
                "unsubscribe" => {
                    manager
                        .handle_command(TimelineCommand::Unsubscribe {
                            request_id: fake_rid(92_001),
                            key: fixture.key.clone(),
                        })
                        .await;
                }
                "shutdown" => {
                    let (control_tx, control_rx) = mpsc::channel(1);
                    manager.control_rx = Some(control_rx);
                    let task = executor::spawn(manager.run());
                    let (ack_tx, ack_rx) = oneshot::channel();
                    control_tx
                        .send(TimelineManagerControl::Shutdown {
                            acknowledged: ack_tx,
                        })
                        .await
                        .expect("shutdown control");
                    ack_rx
                        .await
                        .expect("shutdown must acknowledge after cancellation");
                    task.await.expect("shutdown manager task");
                }
                _ => unreachable!(),
            }

            completion_tx.closed().await;
            drop(interactive);
            assert_room_key_reshare_slot_released(&scheduler).await;
        }
    }
}

struct RoomKeyReshareFixture {
    _server: matrix_sdk::test_utils::mocks::MatrixMockServer,
    session: Arc<MatrixClientSession>,
    key: TimelineKey,
    token: MatrixOutboundGroupSessionToken,
    other_token: MatrixOutboundGroupSessionToken,
}

async fn room_key_reshare_fixture() -> RoomKeyReshareFixture {
    use matrix_sdk::ruma::{RoomVersionId, device_id, room_id, user_id};
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_test::{JoinedRoomBuilder, event_factory::EventFactory};
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path_regex},
    };

    let server = MatrixMockServer::new().await;
    server.mock_crypto_endpoints_preset().await;
    let alice_id = user_id!("@alice:example.org");
    let bob_id = user_id!("@bob:example.org");
    let alice_device = device_id!("ALICEDEVICE");
    let bob_device = device_id!("BOBDEVICE");
    let alice = server
        .client_builder_for_crypto_end_to_end(alice_id, alice_device)
        .build()
        .await;
    let bob = server
        .client_builder_for_crypto_end_to_end(bob_id, bob_device)
        .build()
        .await;
    server.exchange_e2ee_identities(&alice, &bob).await;

    let first_room = room_id!("!reshare-first:example.org");
    let second_room = room_id!("!reshare-second:example.org");
    server
        .mock_sync()
        .ok_and_run(&alice, |builder| {
            for room_id in [first_room, second_room] {
                let factory = EventFactory::new().sender(alice_id).room(room_id);
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id)
                        .add_state_event(factory.create(alice_id, RoomVersionId::V1))
                        .add_state_event(factory.room_encryption())
                        .add_state_event(factory.member(alice_id).into_raw())
                        .add_state_event(factory.member(bob_id).into_raw()),
                );
            }
        })
        .await;
    let factory = EventFactory::new().sender(alice_id).room(first_room);
    server
        .mock_get_members()
        .ok(vec![
            factory.member(alice_id).into_raw(),
            factory.member(bob_id).into_raw(),
        ])
        .mount()
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(
            r"^/_matrix/client/.*/sendToDevice/m.room.encrypted/.*",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(server.server())
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(
            r"^/_matrix/client/.*/rooms/.*/send/m.room.encrypted/.*",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "event_id": "$reshare-event:example.org" })),
        )
        .expect(2)
        .mount(server.server())
        .await;

    for room_id in [first_room, second_room] {
        alice
            .get_room(room_id)
            .expect("synthetic encrypted room")
            .send(
                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(
                    "synthetic reshare fixture",
                ),
            )
            .await
            .expect("synthetic encrypted send");
    }

    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        alice.clone(),
        koushi_state::SessionInfo {
            homeserver: server.uri(),
            user_id: alice_id.to_string(),
            device_id: alice_device.to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    ));
    let token = koushi_sdk::current_outbound_group_session_token(&session, first_room.as_str())
        .await
        .expect("first outbound session lookup")
        .expect("first outbound session");
    let other_token =
        koushi_sdk::current_outbound_group_session_token(&session, second_room.as_str())
            .await
            .expect("second outbound session lookup")
            .expect("second outbound session");
    RoomKeyReshareFixture {
        _server: server,
        session,
        key: TimelineKey::room(AccountKey(alice_id.to_string()), first_room.as_str()),
        token,
        other_token,
    }
}

fn install_room_key_reshare_schedule(
    manager: &mut TimelineManagerActor,
    key: &TimelineKey,
    token: MatrixOutboundGroupSessionToken,
) {
    let tasks = ROOM_KEY_RESHARE_ATTEMPTS
        .iter()
        .map(|attempt| RoomKeyReshareTaskSlot {
            attempt: attempt.number,
            delayed: executor::spawn(async {}),
            started: false,
            worker: None,
        })
        .collect();
    manager.send_enqueue_workers.room_key_reshares.insert(
        key.clone(),
        RoomKeyReshareSchedule {
            session: token,
            tasks,
        },
    );
}

fn install_pending_room_key_reshare_worker(
    manager: &mut TimelineManagerActor,
    key: &TimelineKey,
    generation: u64,
    token: MatrixOutboundGroupSessionToken,
    attempt: u8,
) {
    let worker = spawn_room_key_reshare_task_with_operation(
        manager.account_work.clone(),
        manager.msg_tx.clone(),
        key.clone(),
        generation,
        token.clone(),
        MatrixRoomKeyReshareTarget::OwnOtherDevices,
        attempt,
        Box::pin(std::future::pending()),
        None,
    );
    install_room_key_reshare_schedule(manager, key, token);
    let schedule = manager
        .send_enqueue_workers
        .room_key_reshares
        .get_mut(key)
        .expect("reshare schedule");
    let slot = schedule
        .tasks
        .iter_mut()
        .find(|slot| slot.attempt == attempt)
        .expect("reshare attempt slot");
    slot.started = true;
    slot.worker = Some(worker);
}

fn install_controlled_room_key_reshare_worker(
    manager: &mut TimelineManagerActor,
    key: &TimelineKey,
    generation: u64,
    token: MatrixOutboundGroupSessionToken,
    attempt: u8,
) -> (
    oneshot::Sender<RoomKeyReshareCompletion>,
    oneshot::Receiver<()>,
    oneshot::Receiver<()>,
) {
    let (completion_tx, completion_rx) = oneshot::channel();
    let (acquire_started_tx, acquire_started_rx) = oneshot::channel();
    let (permit_acquired_tx, permit_acquired_rx) = oneshot::channel();
    let worker = spawn_room_key_reshare_task_with_operation(
        manager.account_work.clone(),
        manager.msg_tx.clone(),
        key.clone(),
        generation,
        token.clone(),
        MatrixRoomKeyReshareTarget::OwnOtherDevices,
        attempt,
        Box::pin(async move {
            completion_rx
                .await
                .unwrap_or(RoomKeyReshareCompletion::SdkError)
        }),
        Some(RoomKeyReshareTestSignals {
            acquire_started: acquire_started_tx,
            permit_acquired: permit_acquired_tx,
        }),
    );
    install_room_key_reshare_schedule(manager, key, token);
    let schedule = manager
        .send_enqueue_workers
        .room_key_reshares
        .get_mut(key)
        .expect("reshare schedule");
    let slot = schedule
        .tasks
        .iter_mut()
        .find(|slot| slot.attempt == attempt)
        .expect("reshare attempt slot");
    slot.started = true;
    slot.worker = Some(worker);
    (completion_tx, acquire_started_rx, permit_acquired_rx)
}

#[test]
fn decrypt_retry_diagnostics_are_fixed_token_and_private_data_free() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    let operation = 48_217;

    record_decrypt_retry_request(
        operation,
        1,
        DecryptRetryReason::MissingRoomKey,
        DecryptRetryBackupState::Available,
        Duration::ZERO,
    );
    record_decrypt_retry_backup_lookup(operation, DecryptRetryBackupResult::Found, Duration::ZERO);
    record_decrypt_retry_device_request(
        operation,
        DecryptRetryDeviceResult::Failed,
        Some(DecryptRetryFailure::Forbidden),
        Duration::ZERO,
    );
    record_decrypt_retry_settled(
        operation,
        DecryptRetrySettledResult::StillMissing,
        Duration::ZERO,
    );

    let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
    let records = diagnostics
        .records
        .iter()
        .filter(|record| {
            record.event.source == "core.decrypt_retry"
                && record.event.fields.iter().any(|field| {
                    field.key == "operation"
                        && field.value == DiagnosticValue::Correlation(operation)
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .map(|record| (record.event.stage, &record.event.fields))
            .collect::<Vec<_>>(),
        vec![
            ("request", &records[0].event.fields),
            ("backup_lookup", &records[1].event.fields),
            ("device_request", &records[2].event.fields),
            ("settled", &records[3].event.fields),
        ]
    );
    for record in &records {
        assert_eq!(record.event.source, "core.decrypt_retry");
        assert!(record.event.fields.iter().any(|field| {
            field.key == "operation" && field.value == DiagnosticValue::Correlation(operation)
        }));
    }
    assert!(records[0].event.fields.iter().any(|field| {
        field.key == "reason" && field.value == DiagnosticValue::Token("missing_room_key")
    }));
    assert!(
        records[1].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("found")
        })
    );
    assert!(
        records[2].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("failed")
        })
    );
    assert!(records[2].event.fields.iter().any(|field| {
        field.key == "failure" && field.value == DiagnosticValue::Token("forbidden")
    }));
    assert!(records[3].event.fields.iter().any(|field| {
        field.key == "result" && field.value == DiagnosticValue::Token("still_missing")
    }));

    let serialized = serde_json::to_string(&records).expect("serialize diagnostics");
    for forbidden in [
        "!synthetic-room:example.invalid",
        "$synthetic-event:example.invalid",
        "@synthetic-user:example.invalid",
        "SYNTHETICDEVICE",
        "synthetic-session-id",
        "synthetic message body",
        "https://private.example.invalid",
        "/Users/member/private/store",
        "private-token",
        "recovery-key",
        "backup-version",
        "raw SDK error",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "diagnostic leaked {forbidden}"
        );
    }
}

#[test]
fn decrypt_retry_controller_fences_deadline_settlement_and_replacement() {
    let mut controller = DecryptRetryController::default();
    let admitted_at = executor::Instant::now();
    let (first, replaced, coalesced) = controller.admit("$event-a:test", 7, admitted_at);
    assert!(replaced.is_none());
    assert!(!coalesced);
    assert!(first.deadline > admitted_at);
    assert!(controller.is_current(first.operation, 7));
    let (same, replaced, coalesced) =
        controller.admit("$event-a:test", 7, executor::Instant::now());
    assert!(coalesced);
    assert!(replaced.is_none());
    assert_eq!(same.operation, first.operation);

    assert!(
        controller
            .settle_if_current(first.operation, 8, DecryptRetrySettledResult::Decrypted)
            .is_none()
    );
    assert!(
        controller
            .settle_if_current(
                first.operation.wrapping_add(1),
                7,
                DecryptRetrySettledResult::Timeout
            )
            .is_none()
    );
    assert!(controller.is_current(first.operation, 7));

    let (second, replaced, coalesced) =
        controller.admit("$event-b:test", 7, executor::Instant::now());
    assert!(!coalesced);
    assert_eq!(
        replaced.map(|pending| pending.operation),
        Some(first.operation)
    );
    assert!(!controller.is_current(first.operation, 7));
    assert!(controller.is_current(second.operation, 7));

    assert!(
        controller
            .settle_if_current(second.operation, 8, DecryptRetrySettledResult::Decrypted)
            .is_none()
    );
    let settled = controller
        .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Decrypted)
        .expect("current operation settles exactly once");
    assert_eq!(settled.pending.operation, second.operation);
    assert!(matches!(
        settled.result,
        DecryptRetrySettledResult::Decrypted
    ));
    assert!(!controller.is_current(second.operation, 7));
    assert!(
        controller
            .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Timeout)
            .is_none()
    );
}

#[test]
fn room_key_request_state_tokens_are_closed_and_serde_stable() {
    // Every internal stage literal maps to a closed wire token, and the
    // DTO serializes with the exact tokens the TypeScript union declares.
    let stage_cases = [
        ("sent", "sent"),
        ("automatic", "automatic"),
        ("still_waiting", "still_waiting"),
        ("withheld", "withheld"),
        ("decryption_recovered", "decryption_recovered"),
        ("send_failed", "send_failed"),
    ];
    for (literal, wire) in stage_cases {
        let serialized = serde_json::to_string(&key_request_stage_token(literal)).unwrap();
        assert_eq!(serialized, format!("\"{wire}\""));
    }
    let code_cases = [
        ("blacklisted", "blacklisted"),
        ("unverified", "unverified"),
        ("unauthorised", "unauthorised"),
        ("unavailable", "unavailable"),
    ];
    for (literal, wire) in code_cases {
        let serialized = serde_json::to_string(&key_request_withheld_code_token(literal)).unwrap();
        assert_eq!(serialized, format!("\"{wire}\""));
    }
    // Unknown / custom codes carry no specific copy: they map to None.
    assert!(key_request_withheld_code_token("custom").is_none());
    let dto = RoomKeyRequestStateDto {
        stage: key_request_stage_token("withheld"),
        withheld_code: key_request_withheld_code_token("unavailable"),
    };
    assert_eq!(
        serde_json::to_string(&dto).unwrap(),
        "{\"stage\":\"withheld\",\"withheldCode\":\"unavailable\"}"
    );
}

#[test]
fn withheld_update_guard_allows_typed_code_and_never_regresses_terminal_stages() {
    // Stage settled withheld by a diff without a code still gains it.
    assert!(withheld_update_should_publish(
        "withheld",
        None,
        "unavailable"
    ));
    // A different typed code replaces the previous one.
    assert!(withheld_update_should_publish(
        "withheld",
        Some("unverified"),
        "blacklisted"
    ));
    // Duplicate observation of the same code is idempotent.
    assert!(!withheld_update_should_publish(
        "withheld",
        Some("unavailable"),
        "unavailable"
    ));
    // Non-withheld pending stages accept the refusal.
    assert!(withheld_update_should_publish("sent", None, "unavailable"));
    assert!(withheld_update_should_publish(
        "still_waiting",
        None,
        "unavailable"
    ));
    // Terminal stages are never regressed by a late observation.
    assert!(!withheld_update_should_publish(
        "decryption_recovered",
        None,
        "unavailable"
    ));
    assert!(!withheld_update_should_publish(
        "send_failed",
        None,
        "unavailable"
    ));
}

#[test]
fn room_key_request_state_changed_debug_redacts_identifiers() {
    let event = CoreEvent::Room(crate::event::RoomEvent::RoomKeyRequestStateChanged {
        key: TimelineKey::room(
            crate::ids::AccountKey("@secret-account:example.invalid".to_owned()),
            "!secret-room:example.invalid",
        ),
        event_id: "$secret-event:example.invalid".to_owned(),
        request_id: None,
        stage: RoomKeyRequestStage::Withheld,
        withheld_code: Some(RoomKeyRequestWithheldCode::Unverified),
    });
    let rendered = format!("{event:?}");
    assert!(!rendered.contains("secret-account"));
    assert!(!rendered.contains("secret-room"));
    assert!(!rendered.contains("secret-event"));
    assert!(rendered.contains("withheld"));
}

#[test]
fn decrypt_retry_diff_settlement_requires_current_generation_and_matching_event() {
    let mut controller = DecryptRetryController::default();
    let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 8, "$event:test"),
        None
    );
    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 7, "$other:test"),
        None
    );
    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 7, "$event:test"),
        Some(pending.operation)
    );
}

#[test]
fn decrypt_retry_timeout_message_settles_current_operation_once() {
    let mut controller = DecryptRetryController::default();
    let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

    let settled = controller
        .settle_timeout_if_current(pending.operation, 7)
        .expect("current timeout settles");
    assert!(matches!(settled.result, DecryptRetrySettledResult::Timeout));
    assert!(
        controller
            .settle_timeout_if_current(pending.operation, 7)
            .is_none()
    );
}

#[test]
fn decrypt_retry_backup_state_only_reports_available_for_ready_local_recovery() {
    assert_eq!(
        decrypt_retry_backup_state_for(
            koushi_sdk::MatrixSecureBackupLocalState::Enabled,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        )
        .token(),
        "available"
    );
    for state in [
        (
            koushi_sdk::MatrixSecureBackupLocalState::Unknown,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        ),
        (
            koushi_sdk::MatrixSecureBackupLocalState::Enabled,
            koushi_sdk::MatrixSecureBackupRecoveryState::Unknown,
        ),
        (
            koushi_sdk::MatrixSecureBackupLocalState::Downloading,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        ),
    ] {
        assert_eq!(
            decrypt_retry_backup_state_for(state.0, state.1).token(),
            "unknown"
        );
    }
}

#[test]
fn decrypt_retry_operation_sequence_is_process_wide_and_monotonic() {
    let first = next_decrypt_retry_operation();
    let second = next_decrypt_retry_operation();
    assert!(second > first);
}

#[test]
fn decrypt_retry_backup_failures_keep_typed_private_kinds() {
    for (kind, expected) in [
        (
            koushi_sdk::E2eeTrustFailureKind::Network,
            DecryptRetryBackupResult::Network,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Forbidden,
            DecryptRetryBackupResult::Forbidden,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::InvalidBackup,
            DecryptRetryBackupResult::InvalidBackup,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Timeout,
            DecryptRetryBackupResult::Timeout,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Sdk,
            DecryptRetryBackupResult::Sdk,
        ),
    ] {
        assert!(matches!(
            decrypt_retry_backup_result_for_error(&koushi_sdk::E2eeTrustError::Classified(
                kind
            )),
            result if result.token() == expected.token()
        ));
    }
}

#[test]
fn decrypt_retry_diagnostics_use_only_the_planned_outcome_tokens() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    let operation = 48_218;

    record_decrypt_retry_request(
        operation,
        2,
        DecryptRetryReason::MissingRoomKey,
        DecryptRetryBackupState::Available,
        Duration::ZERO,
    );
    for result in [
        DecryptRetryBackupResult::Found,
        DecryptRetryBackupResult::NotFound,
        DecryptRetryBackupResult::Network,
        DecryptRetryBackupResult::Forbidden,
        DecryptRetryBackupResult::InvalidBackup,
        DecryptRetryBackupResult::Timeout,
        DecryptRetryBackupResult::Sdk,
    ] {
        record_decrypt_retry_backup_lookup(operation, result, Duration::ZERO);
    }
    record_decrypt_retry_device_request(
        operation,
        DecryptRetryDeviceResult::Sent,
        None,
        Duration::ZERO,
    );
    for failure in [
        DecryptRetryFailure::Network,
        DecryptRetryFailure::Forbidden,
        DecryptRetryFailure::Timeout,
        DecryptRetryFailure::Sdk,
    ] {
        record_decrypt_retry_device_request(
            operation,
            DecryptRetryDeviceResult::Failed,
            Some(failure),
            Duration::ZERO,
        );
    }
    for result in [
        DecryptRetrySettledResult::Decrypted,
        DecryptRetrySettledResult::StillMissing,
        DecryptRetrySettledResult::Withheld,
        DecryptRetrySettledResult::Malformed,
        DecryptRetrySettledResult::Timeout,
        DecryptRetrySettledResult::Superseded,
    ] {
        record_decrypt_retry_settled(operation, result, Duration::ZERO);
    }

    let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
    let tokens = diagnostics
        .records
        .iter()
        .filter(|record| {
            record.event.source == "core.decrypt_retry"
                && record.event.fields.iter().any(|field| {
                    field.key == "operation"
                        && field.value == DiagnosticValue::Correlation(operation)
                })
        })
        .flat_map(|record| record.event.fields.iter())
        .filter_map(|field| match field.value {
            DiagnosticValue::Token(token) => Some((field.key, token)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [
        ("backup_state", "available"),
        ("result", "found"),
        ("result", "not_found"),
        ("result", "network"),
        ("result", "forbidden"),
        ("result", "invalid_backup"),
        ("result", "timeout"),
        ("result", "sdk"),
        ("failure", "network"),
        ("failure", "forbidden"),
        ("failure", "timeout"),
        ("failure", "sdk"),
        ("result", "decrypted"),
        ("result", "still_missing"),
        ("result", "withheld"),
        ("result", "malformed"),
        ("result", "superseded"),
    ] {
        assert!(
            tokens.contains(&expected),
            "missing fixed token {expected:?}"
        );
    }
}

#[test]
fn room_key_reshare_diagnostics_include_attempt_target_and_result() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    let diagnostic_start = koushi_diagnostics::test_support::detail_snapshot()
        .records
        .len();

    record_room_key_reshare(
        "own_device_retry_1",
        "sent",
        1,
        MatrixRoomKeyReshareTarget::OwnOtherDevices,
        3,
        2,
        5,
        1,
    );

    let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
    let record = diagnostics.records[diagnostic_start..]
        .iter()
        .find(|record| {
            record.event.source == "core.room_key_reshare" && record.event.stage == "attempt"
        })
        .expect("room-key reshare diagnostic");
    for (key, value) in [
        ("attempt", DiagnosticValue::Count(1)),
        ("target", DiagnosticValue::Token("own_other_devices")),
        ("delay_seconds", DiagnosticValue::Count(3)),
        ("request_count", DiagnosticValue::Count(2)),
        ("recipient_count", DiagnosticValue::Count(5)),
    ] {
        assert!(
            record
                .event
                .fields
                .iter()
                .any(|field| { field.key == key && field.value == value }),
            "missing {key}"
        );
    }
    assert!(record.event.fields.iter().all(|field| {
        !matches!(
            field.key,
            "room_id"
                | "event_id"
                | "user_id"
                | "device_id"
                | "session_id"
                | "transaction_id"
                | "request_id"
                | "message"
                | "key"
                | "key_material"
        )
    }));
}
