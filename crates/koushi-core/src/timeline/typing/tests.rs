//! Room-scoped typing-lane tests (#1481).

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::executor;
use koushi_protocol::command::TimelineCommand;
use koushi_protocol::event::{CoreEvent, LiveSignalsEvent};
use koushi_protocol::failure::TimelineFailureKind;
use koushi_protocol::ids::{AccountKey, TimelineKey};

use super::super::actor::TimelineActorMessage;
use super::super::test_support::{
    fake_rid, focused_key, live_tail_test_manager, room_key, test_timeline_actor_handle, thread_key,
};
use super::{
    SyntheticTypingRequest, TYPING_LANE_MAX_ROOMS, TYPING_LANE_QUEUE_CAPACITY, TypingLane,
    TypingLaneSettlement,
};

fn lane() -> (TypingLane, mpsc::UnboundedReceiver<SyntheticTypingRequest>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (TypingLane::synthetic(tx, room_key().account_key.0), rx)
}

fn other_room_key() -> TimelineKey {
    TimelineKey::room(AccountKey("@a:test".to_owned()), "!other:test")
}

#[tokio::test]
async fn typing_notices_submit_in_room_order_one_at_a_time() {
    let (mut lane, mut rx) = lane();
    let key = room_key();
    lane.admit(fake_rid(1), key.clone(), true).unwrap();
    lane.admit(fake_rid(2), key.clone(), false).unwrap();
    lane.admit(fake_rid(3), key.clone(), true).unwrap();

    assert_eq!(
        lane.tasks.len(),
        1,
        "only one request is in flight per room"
    );
    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    let first = rx.try_recv().expect("first request reaches the network");
    assert!(first.is_typing);
    assert!(
        rx.try_recv().is_err(),
        "a newer request is not submitted before the older one settles"
    );

    first.response.send(true).unwrap();
    let completion = lane.tasks.next().await.expect("completion");
    let settlements = lane.settle(completion);
    assert!(matches!(
        settlements.as_slice(),
        [TypingLaneSettlement::Succeeded(request)] if request.request_id == fake_rid(1)
    ));

    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    let second = rx.try_recv().expect("second request reaches the network");
    assert!(!second.is_typing);
    second.response.send(true).unwrap();
    let completion = lane.tasks.next().await.expect("completion");
    assert!(matches!(
        lane.settle(completion).as_slice(),
        [TypingLaneSettlement::Succeeded(request)] if request.request_id == fake_rid(2)
    ));

    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    let third = rx.try_recv().expect("third request reaches the network");
    assert!(third.is_typing);
    third.response.send(true).unwrap();
    let completion = lane.tasks.next().await.expect("completion");
    assert!(matches!(
        lane.settle(completion).as_slice(),
        [TypingLaneSettlement::Succeeded(request)] if request.request_id == fake_rid(3)
    ));
    assert!(lane.is_idle());
}

#[tokio::test]
async fn independent_rooms_do_not_order_each_other() {
    let (mut lane, mut rx) = lane();
    lane.admit(fake_rid(1), room_key(), true).unwrap();
    lane.admit(fake_rid(2), other_room_key(), true).unwrap();
    assert_eq!(lane.tasks.len(), 2, "rooms progress independently");

    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    let mut seen = 0;
    while let Ok(request) = rx.try_recv() {
        assert!(request.is_typing);
        seen += 1;
    }
    assert_eq!(seen, 2, "both rooms submitted while neither settled");
}

#[tokio::test]
async fn room_queue_bound_rejects_explicitly() {
    let (mut lane, _rx) = lane();
    let key = room_key();
    lane.admit(fake_rid(1), key.clone(), true).unwrap();
    for sequence in 0..TYPING_LANE_QUEUE_CAPACITY {
        lane.admit(fake_rid(100 + sequence as u64), key.clone(), true)
            .expect("queued request is admitted below the bound");
    }
    assert_eq!(
        lane.admit(fake_rid(999), key.clone(), false),
        Err(TimelineFailureKind::QueueOverflow),
        "a request beyond the bound is rejected, never silently dropped"
    );
}

#[tokio::test]
async fn duplicate_settlement_is_inert() {
    let (mut lane, mut rx) = lane();
    lane.admit(fake_rid(1), room_key(), true).unwrap();
    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    rx.try_recv().unwrap().response.send(true).unwrap();

    let completion = lane.tasks.next().await.expect("completion");
    assert_eq!(lane.settle(completion.clone()).len(), 1);
    assert!(
        lane.settle(completion).is_empty(),
        "a duplicate completion cannot settle a newer request or emit twice"
    );
    assert!(lane.is_idle());
}

#[tokio::test]
async fn failed_typing_emits_one_correlated_failure() {
    let (mut lane, mut rx) = lane();
    lane.admit(fake_rid(1), room_key(), true).unwrap();
    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    rx.try_recv().unwrap().response.send(false).unwrap();

    let completion = lane.tasks.next().await.expect("completion");
    assert!(matches!(
        lane.settle(completion).as_slice(),
        [TypingLaneSettlement::Failed { request, kind: TimelineFailureKind::Sdk }]
            if request.request_id == fake_rid(1)
    ));
}

#[tokio::test]
async fn settle_all_settles_every_admitted_request_and_leaves_no_task() {
    let (mut lane, _rx) = lane();
    let key = room_key();
    for sequence in 1..=3 {
        lane.admit(fake_rid(sequence), key.clone(), sequence % 2 == 1)
            .unwrap();
    }
    let settlements = lane.settle_all();
    assert_eq!(settlements.len(), 3);
    assert!(settlements.iter().all(|settlement| matches!(
        settlement,
        TypingLaneSettlement::Failed {
            kind: TimelineFailureKind::Sdk,
            ..
        }
    )));
    assert!(lane.is_idle());
}

/// A held typing operation must not let a pending-send refresh wait behind it
/// (#1481). The refresh is delivered to the actor while the typing network
/// request is still unanswered; only then is the barrier released.
#[tokio::test]
async fn held_typing_does_not_block_pending_send_refresh() {
    let key = room_key();
    let (typing_tx, mut typing_rx) = mpsc::unbounded_channel();
    let (refresh_tx, mut refresh_rx) = mpsc::channel(4);
    let mut handle = test_timeline_actor_handle();
    let (actor_tx, mut actor_rx) = mpsc::channel(4);
    handle.tx = actor_tx;
    handle.task = Some(executor::spawn(async move {
        while let Some(message) = actor_rx.recv().await {
            if matches!(
                message,
                TimelineActorMessage::RefreshPendingSendProjection { .. }
            ) {
                let _ = refresh_tx.send(()).await;
            }
        }
    }));
    let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), handle)]));
    manager.typing_lane = TypingLane::synthetic(typing_tx, key.account_key.0.clone());
    let (event_tx, mut event_rx) = broadcast::channel(8);
    manager.event_tx = event_tx;

    manager
        .handle_command(TimelineCommand::SetTyping {
            request_id: fake_rid(1),
            key: key.clone(),
            is_typing: true,
        })
        .await;
    assert!(futures_util::poll!(manager.typing_lane.tasks.next()).is_pending());
    let held = typing_rx.try_recv().expect("typing request is in flight");
    assert!(held.is_typing);

    manager.refresh_pending_send_projection(&key).await;
    tokio::time::timeout(Duration::from_secs(1), refresh_rx.recv())
        .await
        .expect("pending-send refresh must not wait for the typing request")
        .expect("actor channel stays open");

    held.response.send(true).unwrap();
    let completion = manager
        .typing_lane
        .tasks
        .next()
        .await
        .expect("typing completion");
    manager.handle_typing_completion(completion);
    let mut saw_typing_set = false;
    while let Ok(event) = event_rx.try_recv() {
        if matches!(
            event,
            CoreEvent::LiveSignals(LiveSignalsEvent::TypingSet {
                is_typing: true,
                ..
            })
        ) {
            saw_typing_set = true;
        }
    }
    assert!(
        saw_typing_set,
        "the held typing request settles as TypingSet"
    );
}

/// Typing is room-scoped: room, thread, and focused views of one room share one
/// ordered lane.
#[tokio::test]
async fn thread_and_focused_views_share_the_room_lane() {
    let (mut lane, mut rx) = lane();
    lane.admit(fake_rid(1), room_key(), true).unwrap();
    lane.admit(fake_rid(2), thread_key(), true).unwrap();
    lane.admit(fake_rid(3), focused_key(), false).unwrap();
    assert_eq!(lane.tasks.len(), 1, "one room, one in-flight request");
    assert!(futures_util::poll!(lane.tasks.next()).is_pending());
    assert!(rx.try_recv().unwrap().is_typing);
    assert!(
        rx.try_recv().is_err(),
        "thread and focused views queue behind the room request"
    );
}

#[tokio::test]
async fn foreign_account_key_is_rejected() {
    let (mut lane, _rx) = lane();
    let foreign = TimelineKey::room(AccountKey("@b:test".to_owned()), "!r:test".to_owned());
    assert_eq!(
        lane.admit(fake_rid(1), foreign, true),
        Err(TimelineFailureKind::NotSubscribed),
        "a key for another account must not type on this session"
    );
}

#[tokio::test]
async fn session_room_bound_rejects_new_rooms() {
    let (mut lane, _rx) = lane();
    for index in 0..TYPING_LANE_MAX_ROOMS {
        let key = TimelineKey::room(
            AccountKey("@a:test".to_owned()),
            format!("!room{index}:test"),
        );
        lane.admit(fake_rid(index as u64 + 1), key, true)
            .expect("room below the session bound is admitted");
    }
    let extra = TimelineKey::room(AccountKey("@a:test".to_owned()), "!extra:test".to_owned());
    assert_eq!(
        lane.admit(fake_rid(9999), extra, true),
        Err(TimelineFailureKind::QueueOverflow),
        "the session-wide room map stays bounded"
    );
}

#[tokio::test]
async fn manager_shutdown_settles_admitted_typing_requests() {
    let key = room_key();
    let (typing_tx, _typing_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = broadcast::channel(8);
    let mut manager = live_tail_test_manager(HashMap::new());
    manager.typing_lane = TypingLane::synthetic(typing_tx, key.account_key.0.clone());
    manager.event_tx = event_tx;
    for sequence in 1..=3 {
        manager
            .handle_command(TimelineCommand::SetTyping {
                request_id: fake_rid(sequence),
                key: key.clone(),
                is_typing: sequence % 2 == 1,
            })
            .await;
    }
    manager.settle_typing_lane();

    let mut failed = 0;
    while let Ok(event) = event_rx.try_recv() {
        if matches!(event, CoreEvent::OperationFailed { .. }) {
            failed += 1;
        }
    }
    assert_eq!(failed, 3, "shutdown settles every admitted request once");
    assert!(manager.typing_lane.is_idle());
}

#[tokio::test]
async fn manager_drop_settles_admitted_typing_requests() {
    let key = room_key();
    let (typing_tx, _typing_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = broadcast::channel(8);
    {
        let mut manager = live_tail_test_manager(HashMap::new());
        manager.typing_lane = TypingLane::synthetic(typing_tx, key.account_key.0.clone());
        manager.event_tx = event_tx.clone();
        for sequence in 1..=2 {
            manager
                .handle_command(TimelineCommand::SetTyping {
                    request_id: fake_rid(sequence),
                    key: key.clone(),
                    is_typing: true,
                })
                .await;
        }
        drop(manager);
    }

    let mut failed = 0;
    while let Ok(event) = event_rx.try_recv() {
        if matches!(event, CoreEvent::OperationFailed { .. }) {
            failed += 1;
        }
    }
    assert_eq!(failed, 2, "drop fail-safe settles every admitted request");
}
