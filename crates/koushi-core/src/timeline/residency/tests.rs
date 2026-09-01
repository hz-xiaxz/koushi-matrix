use std::collections::{BTreeSet, HashMap};

use std::sync::Arc;

use tokio::sync::mpsc;

use koushi_protocol::command::TimelineCommand;

#[cfg(any(test, feature = "test-hooks"))]
use koushi_protocol::ids::AccountKey;
use koushi_protocol::ids::{TimelineKey, TimelineKind};

use super::super::actor::{TimelineActorHandle, TimelineActorMessage};
use super::super::test_support::{fake_rid, live_tail_test_manager};
use super::{
    MembershipOperationGate, SubscriptionReconcileTrigger, TimelineSubscriptionResidencyHandle,
};

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn begin_operation_rejects_when_message_receiver_is_closed() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let handle = TimelineSubscriptionResidencyHandle {
        tx,
        gate: MembershipOperationGate::new(),
    };

    assert!(handle.begin_operation().is_none());
    assert_eq!(handle.gate_snapshot(), (true, 0));
}

#[tokio::test]
async fn lease_and_release_room_leases_refcount_by_room() {
    let mut manager = live_tail_test_manager(HashMap::new());
    let room_a = matrix_sdk::ruma::room_id!("!a:test").to_owned();

    manager.lease_room(room_a.clone());
    manager.lease_room(room_a.clone());
    assert_eq!(manager.subscribed_room_leases.get(&room_a), Some(&2));

    // Removing one of two leases keeps the room subscribed.
    assert!(!manager.release_room_lease(&room_a));
    assert_eq!(manager.subscribed_room_leases.get(&room_a), Some(&1));
    assert!(manager.room_is_leased("!a:test"));

    // Removing the final lease drops the room.
    assert!(manager.release_room_lease(&room_a));
    assert!(!manager.subscribed_room_leases.contains_key(&room_a));
    assert!(!manager.room_is_leased("!a:test"));
}

#[tokio::test]
async fn reconcile_subscriptions_uses_session_residency_not_leases() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_ui::room_list_service::RoomListService;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let room_list = Arc::new(RoomListService::new(client.clone()).await.unwrap());

    let mut manager = live_tail_test_manager(HashMap::new());
    manager.room_list_service = Some(room_list.clone());

    let room_a = matrix_sdk::ruma::room_id!("!a:test").to_owned();
    let room_b = matrix_sdk::ruma::room_id!("!b:test").to_owned();

    // Actor leases are resource bookkeeping; residency is the desired set.
    manager
        .session_subscribed_rooms
        .extend([room_a.clone(), room_b.clone()]);
    manager.lease_room(room_a.clone());
    manager.lease_room(room_a.clone());
    manager.lease_room(room_b.clone());
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::RoomSelected)
        .await;

    // The live set matches the deduplicated lease set exactly; a second
    // reconcile of the same set is a true no-op.
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a.clone(), room_b.clone()])
    );
    let generation_before = room_list.subscription_generation();
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::ThreadOpened)
        .await;
    assert_eq!(
        room_list.subscription_generation(),
        generation_before,
        "identical session-resident set must be a true no-op"
    );

    // Releasing B's actor lease does not remove its session residency.
    assert!(manager.release_room_lease(&room_b));
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::ThreadOpened)
        .await;
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a, room_b])
    );
}

#[tokio::test]
async fn unsubscribe_releases_the_room_lease_only_at_zero() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_ui::room_list_service::RoomListService;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let room_list = Arc::new(RoomListService::new(client.clone()).await.unwrap());

    let mut manager = live_tail_test_manager(HashMap::new());
    manager.room_list_service = Some(room_list.clone());
    let room_a = matrix_sdk::ruma::room_id!("!a:test").to_owned();
    let key_room = TimelineKey::room(AccountKey("@a:test".to_owned()), "!a:test");
    let key_thread = TimelineKey {
        account_key: AccountKey("@a:test".to_owned()),
        kind: TimelineKind::Thread {
            room_id: "!a:test".to_owned(),
            root_event_id: "$root:test".to_owned(),
        },
    };

    // One lease per live TimelineKey; both keys carry placeholder actors.
    manager.session_subscribed_rooms.insert(room_a.clone());
    manager.lease_room(room_a.clone());
    manager.lease_room(room_a.clone());
    manager
        .timelines
        .insert(key_room.clone(), placeholder_actor_handle());
    manager
        .timelines
        .insert(key_thread.clone(), placeholder_actor_handle());
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::RoomSelected)
        .await;

    // Removing one TimelineKey (with an actor removed) keeps the room
    // subscribed through the remaining lease.
    manager
        .handle_command(TimelineCommand::Unsubscribe {
            request_id: fake_rid(60_001),
            key: key_room.clone(),
        })
        .await;
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a.clone()]),
        "one remaining lease must keep the room subscribed"
    );

    // Removing the final TimelineKey drops its actor lease but retains the room.
    manager
        .handle_command(TimelineCommand::Unsubscribe {
            request_id: fake_rid(60_002),
            key: key_thread,
        })
        .await;
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a.clone()]),
        "the final actor lease removal must retain session residency"
    );

    // A duplicate unsubscribe for an already-removed key (no actor to
    // remove) must not release another surface's lease.
    manager.lease_room(room_a.clone());
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::RoomSelected)
        .await;
    manager
        .handle_command(TimelineCommand::Unsubscribe {
            request_id: fake_rid(60_003),
            key: key_room.clone(),
        })
        .await;
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a.clone()]),
        "an unsubscribe without an actor must not release the lease"
    );
}

#[tokio::test]
async fn retained_room_actor_receives_generation_update_on_set_change() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_ui::room_list_service::RoomListService;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let room_list = Arc::new(RoomListService::new(client.clone()).await.unwrap());

    let mut manager = live_tail_test_manager(HashMap::new());
    manager.room_list_service = Some(room_list.clone());
    let room_a = matrix_sdk::ruma::room_id!("!a:test").to_owned();
    let room_b = matrix_sdk::ruma::room_id!("!b:test").to_owned();
    let key_a = TimelineKey::room(AccountKey("@a:test".to_owned()), "!a:test");

    // Retained Room actor A holds a live channel so the ordered
    // generation-update message is observable.
    let (tx, mut rx) = mpsc::channel(8);
    manager.timelines.insert(
        key_a.clone(),
        TimelineActorHandle {
            tx,
            control_tx: None,
            thread_summary_projection:
                crate::timeline::actor::ThreadSummaryProjectionIngress::channel().0,
            position_rx: None,
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        },
    );
    manager
        .session_subscribed_rooms
        .extend([room_a.clone(), room_b.clone()]);
    manager.lease_room(room_a.clone());
    manager.lease_room(room_b.clone());
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::RoomSelected)
        .await;
    let generation_after_first = room_list.subscription_generation().get();
    // Drain the update sent by the first (also non-noop) reconcile.
    while rx.try_recv().is_ok() {}

    // Removing B from session residency changes the set; retained actor A
    // must be told its expected generation advanced.
    manager.session_subscribed_rooms.remove(&room_b);
    assert!(manager.release_room_lease(&room_b));
    manager
        .reconcile_subscriptions(SubscriptionReconcileTrigger::RoomSelected)
        .await;
    let generation_after_change = room_list.subscription_generation().get();
    assert_ne!(generation_after_first, generation_after_change);

    let update = rx
        .try_recv()
        .expect("retained actor A must receive a generation update");
    match update {
        TimelineActorMessage::UpdateSubscriptionGeneration(generation) => {
            assert_eq!(generation, generation_after_change);
        }
        _ => panic!("expected UpdateSubscriptionGeneration"),
    }
}

fn placeholder_actor_handle() -> TimelineActorHandle {
    let (tx, _rx) = mpsc::channel(1);
    TimelineActorHandle {
        tx,
        control_tx: None,
        thread_summary_projection: crate::timeline::actor::ThreadSummaryProjectionIngress::channel(
        )
        .0,
        position_rx: None,
        task: None,
        auxiliary_tasks: Vec::new(),
        subscription_generation: None,
        enqueue_context: None,
    }
}

#[tokio::test]
async fn sync_started_reconciles_the_full_session_residency_set_once() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_ui::room_list_service::RoomListService;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let room_list = Arc::new(RoomListService::new(client.clone()).await.unwrap());

    let mut manager = live_tail_test_manager(HashMap::new());
    let room_a = matrix_sdk::ruma::room_id!("!a:test").to_owned();
    let room_b = matrix_sdk::ruma::room_id!("!b:test").to_owned();
    manager
        .session_subscribed_rooms
        .extend([room_a.clone(), room_b.clone()]);
    manager.lease_room(room_a.clone());
    manager.lease_room(room_b.clone());

    manager.handle_sync_started(room_list.clone(), 1).await;
    manager
        .room_subscription_checkpoint_task
        .take()
        .map(|task| task.abort());

    // The full deduplicated session-resident set is reconciled once.
    assert_eq!(
        room_list.active_room_subscriptions(),
        BTreeSet::from([room_a, room_b])
    );
}
