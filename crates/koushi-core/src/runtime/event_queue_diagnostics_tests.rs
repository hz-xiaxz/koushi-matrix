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
        active.event_rx.try_recv().unwrap();
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
