//! Runtime core tests: request-id correlation, local rejection, batching, and
//! slow-consumer recovery.

use std::time::Duration;

use koushi_core::command::{AppCommand, CoreCommand, RoomCommand};
use koushi_core::event::CoreEvent;
use koushi_core::executor;
use koushi_core::runtime::{CommandSubmitError, CoreRuntime};
use koushi_state::{AppAction, AuthDiscoveryState, SessionState, SettingsPatch, ThreadListOrder};

mod support;

#[tokio::test]
async fn frontend_neutral_consumer_converges_and_shuts_down_without_tauri() {
    let data_dir = tempfile::tempdir().expect("runtime data dir");
    let runtime = CoreRuntime::start_with_data_dir(data_dir.path().to_owned());
    let mut connection = runtime.attach();
    let initial = connection.versioned_snapshot();
    let request_id = connection.next_request_id();

    connection
        .command(CoreCommand::App(AppCommand::UpdateSettings {
            request_id,
            patch: SettingsPatch {
                thread_list_order: Some(ThreadListOrder::RootChronology),
                ..SettingsPatch::default()
            },
        }))
        .await
        .expect("submit typed Core command");

    executor::timeout(Duration::from_secs(1), async {
        loop {
            match connection.recv_event().await.expect("Core event") {
                CoreEvent::StateChanged(snapshot)
                    if snapshot.settings.values.thread_list_order
                        == ThreadListOrder::RootChronology =>
                {
                    break;
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("typed command should converge through Core event");

    let current = connection.versioned_snapshot();
    assert!(current.generation > initial.generation);
    assert_eq!(
        current.state.settings.values.thread_list_order,
        ThreadListOrder::RootChronology
    );
    let independent_consumer = runtime.attach();
    assert_eq!(independent_consumer.versioned_snapshot(), current);

    drop(independent_consumer);
    drop(connection);
    executor::timeout(Duration::from_secs(1), runtime.shutdown())
        .await
        .expect("Core runtime should complete awaited shutdown");
}

#[tokio::test]
async fn mismatched_request_id_fails_locally_without_publishing() {
    let runtime = CoreRuntime::start();
    let intruder = runtime.attach();
    let mut observer = runtime.attach();

    let foreign_id = observer.next_request_id();
    let result = intruder
        .command(CoreCommand::Room(RoomCommand::JoinRoom {
            request_id: foreign_id,
            room_id: "!room:example.test".to_owned(),
        }))
        .await;
    assert_eq!(result, Err(CommandSubmitError::InvalidRequestId));

    // No CoreEvent may be published with the forged RequestId.
    let outcome = executor::timeout(Duration::from_millis(100), observer.recv_event()).await;
    assert!(
        outcome.is_err(),
        "no event should be published for a rejected submission"
    );
}

#[tokio::test]
async fn result_events_correlate_in_submission_order() {
    let runtime = CoreRuntime::start();
    let mut connection = runtime.attach();

    let first = connection.next_request_id();
    let second = connection.next_request_id();
    assert_ne!(first, second);

    for request_id in [first, second] {
        connection
            .command(CoreCommand::Room(RoomCommand::JoinRoom {
                request_id,
                room_id: "!room:example.test".to_owned(),
            }))
            .await
            .expect("submit");
    }

    let mut seen = Vec::new();
    while seen.len() < 2 {
        if let CoreEvent::OperationFailed { request_id, .. } =
            connection.recv_event().await.expect("event")
        {
            seen.push(request_id);
        }
    }
    assert_eq!(seen, vec![first, second], "events must be ordered");
}

#[tokio::test]
async fn reducer_actions_coalesce_into_single_state_changed() {
    let runtime = CoreRuntime::start();
    let mut connection = runtime.attach();

    runtime
        .inject_actions(vec![
            AppAction::AppStarted,
            AppAction::RestoreSessionFailed {
                message: "synthetic".to_owned(),
            },
            AppAction::LoginDiscoveryRequested {
                homeserver: "https://example.test".to_owned(),
            },
        ])
        .await;

    let mut state_changed_count = 0;
    let mut last = None;
    // Drain everything emitted within a quiet period.
    while let Ok(Ok(event)) =
        executor::timeout(Duration::from_millis(200), connection.recv_event()).await
    {
        if let CoreEvent::StateChanged(snapshot) = event {
            state_changed_count += 1;
            last = Some(snapshot);
        }
    }

    assert_eq!(
        state_changed_count, 1,
        "one batch of actions must coalesce into exactly one StateChanged"
    );
    let last = last.expect("snapshot");
    // The final state reflects the LAST action in the batch.
    assert!(matches!(last.auth, AuthDiscoveryState::Discovering { .. }));
    assert_eq!(connection.snapshot(), last);
}

#[tokio::test]
async fn slow_consumer_observes_lag_and_recovers_via_snapshot() {
    let runtime = CoreRuntime::start_with_event_capacity(4);
    let pump = runtime.attach();
    let mut slow = runtime.attach();

    // Overflow the slow consumer's bounded queue.
    for _ in 0..32 {
        let request_id = pump.next_request_id();
        pump.command(CoreCommand::Room(RoomCommand::JoinRoom {
            request_id,
            room_id: "!room:example.test".to_owned(),
        }))
        .await
        .expect("submit");
    }
    runtime.inject_actions(vec![AppAction::AppStarted]).await;
    executor::sleep(Duration::from_millis(100)).await;

    let first = slow.recv_event().await;
    assert!(first.is_err(), "slow consumer must observe the lag marker");

    // Recovery path: latest-wins snapshot is intact and current.
    assert!(matches!(
        slow.snapshot().session,
        SessionState::Restoring | SessionState::SignedOut
    ));

    drop(slow);
    drop(pump);
    executor::timeout(Duration::from_secs(1), runtime.shutdown())
        .await
        .expect("lag recovery consumers should release runtime shutdown");
}
