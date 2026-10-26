use super::CoreRuntime;
use koushi_protocol::event::{CoreEvent, SyncEvent};

#[tokio::test]
async fn event_queue_diagnostics_observe_idle_receiver_without_consuming_or_subscribing() {
    let (runtime, _stores) = CoreRuntime::start_isolated_with_event_capacity(4);
    let idle = runtime.attach();
    let mut active = runtime.attach();
    for _ in 0..3 {
        runtime
            .event_tx
            .send(CoreEvent::Sync(SyncEvent::Running))
            .unwrap();
        active.recv_event().await.unwrap();
    }
    let before = runtime.event_queue_diagnostics();
    assert_eq!(before.retained_events, 3);
    assert_eq!(before.receiver_count, 2);
    assert_eq!(before.capacity_events, 4);
    assert!(before.inline_slot_bytes_lower_bound > 0);
    assert_eq!(
        idle.event_receiver_diagnostics()
            .unread_position_distance_events,
        3
    );
    assert_eq!(runtime.event_queue_diagnostics(), before);
    assert_eq!(
        idle.event_receiver_diagnostics()
            .unread_position_distance_events,
        3
    );
    drop(idle);
    assert_eq!(runtime.event_queue_diagnostics().retained_events, 0);
    drop(active);
    runtime.shutdown().await;
}

#[tokio::test]
async fn event_queue_reports_retained_count_separately_from_overwritten_receiver_positions() {
    let (runtime, _stores) = CoreRuntime::start_isolated_with_event_capacity(3);
    let idle = runtime.attach();
    for _ in 0..10 {
        runtime
            .event_tx
            .send(CoreEvent::Sync(SyncEvent::Running))
            .unwrap();
    }
    let queue = runtime.event_queue_diagnostics();
    assert_eq!(queue.capacity_events, 4);
    assert_eq!(queue.retained_events, 4);
    assert_eq!(
        idle.event_receiver_diagnostics()
            .unread_position_distance_events,
        10
    );
    drop(idle);
    runtime.shutdown().await;
}

#[tokio::test]
async fn command_and_snapshot_connections_do_not_retain_delivered_events() {
    let (runtime, _stores) = CoreRuntime::start_isolated_with_event_capacity(4);
    let mut command = runtime.attach_without_events();
    let snapshot_watcher = runtime.attach_without_events();
    let mut active = runtime.attach();
    assert_eq!(runtime.event_queue_diagnostics().receiver_count, 1);
    for _ in 0..3 {
        // Eager attachment must receive even before its first read.
        runtime
            .event_tx
            .send(CoreEvent::Sync(SyncEvent::Running))
            .unwrap();
        assert_eq!(runtime.event_queue_diagnostics().retained_events, 1);
        assert!(matches!(
            active.recv_event().await,
            Ok(CoreEvent::Sync(SyncEvent::Running))
        ));
        assert_eq!(runtime.event_queue_diagnostics().retained_events, 0);
    }
    assert!(!snapshot_watcher.event_receiver_diagnostics().subscribed);
    assert!(!command.event_receiver_diagnostics().subscribed);
    assert!(matches!(
        super::CoreConnection::recv_raw_event(&mut command.event_rx).await,
        Err(tokio::sync::broadcast::error::RecvError::Closed)
    ));
    assert!(matches!(
        command.recv_event().await,
        Err(super::EventStreamLag { skipped: 0 })
    ));
    assert_eq!(runtime.event_queue_diagnostics().receiver_count, 1);
    drop((command, snapshot_watcher, active));
    runtime.shutdown().await;
}

#[tokio::test]
async fn non_event_connection_submits_commands_and_observes_watch_updates() {
    use koushi_protocol::command::{AppCommand, CoreCommand};
    let (runtime, _stores) = CoreRuntime::start_isolated_with_event_capacity(4);
    let command = runtime.attach_without_events();
    let mut watcher = runtime.attach_without_events();
    let mut display = command.snapshot().settings.values.display;
    display.code_block_wrap = !display.code_block_wrap;
    let expected_wrap = display.code_block_wrap;
    command
        .command_with_admission(CoreCommand::App(AppCommand::UpdateSettings {
            request_id: command.next_request_id(),
            patch: koushi_state::SettingsPatch {
                scope: Some(koushi_state::SettingsPatchScope::App),
                display: Some(display),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let snapshot = watcher.next_versioned_snapshot().await.unwrap();
            if snapshot.state.settings.values.display.code_block_wrap == expected_wrap {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(runtime.event_queue_diagnostics().receiver_count, 0);
    assert_eq!(runtime.event_queue_diagnostics().retained_events, 0);
    drop((command, watcher));
    runtime.shutdown().await;
}

#[tokio::test]
async fn non_event_connection_never_lazily_subscribes_for_an_outcome_waiter() {
    use super::request_outcome::{
        OutcomeCorrelation, RequestOutcomeError, RequestOutcomeExpectation,
    };
    let (runtime, _stores) = CoreRuntime::start_isolated_with_event_capacity(4);
    let mut connection = runtime.attach_without_events();
    let request_id = connection.next_request_id();
    let baseline = connection.state_generation();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        connection.wait_for_request_outcome(
            OutcomeCorrelation::Request(request_id),
            RequestOutcomeExpectation::OidcAuthorization { request_id },
            baseline,
            tokio::time::Instant::now() + std::time::Duration::from_secs(60),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, Err(RequestOutcomeError::Disconnected)));
    assert_eq!(runtime.event_queue_diagnostics().receiver_count, 0);
    drop(connection);
    runtime.shutdown().await;
}
