#![recursion_limit = "512"]

use std::sync::Arc;

use koushi_core::room_subscription_residency_test_support::RoomSubscriptionResidencyHarness;
use koushi_core::{AccountKey, TimelineKey, TimelineKind};
use matrix_sdk::ruma::room_id;
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use matrix_sdk_ui::room_list_service::RoomListService;

async fn harness() -> (MatrixMockServer, RoomSubscriptionResidencyHarness) {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let service = Arc::new(
        RoomListService::new(client)
            .await
            .expect("room list service"),
    );
    (
        server,
        RoomSubscriptionResidencyHarness::with_room_list_service(service),
    )
}

fn room_key(room_id: &str) -> TimelineKey {
    TimelineKey::room(AccountKey("@resident:example.invalid".to_owned()), room_id)
}

fn thread_key(room_id: &str) -> TimelineKey {
    TimelineKey {
        account_key: AccountKey("@resident:example.invalid".to_owned()),
        kind: TimelineKind::Thread {
            room_id: room_id.to_owned(),
            root_event_id: "$root:example.invalid".to_owned(),
        },
    }
}

fn focused_key(room_id: &str) -> TimelineKey {
    TimelineKey {
        account_key: AccountKey("@resident:example.invalid".to_owned()),
        kind: TimelineKind::Focused {
            room_id: room_id.to_owned(),
            event_id: "$event:example.invalid".to_owned(),
        },
    }
}

#[tokio::test]
async fn room_subscription_residency_final_actor_unsubscribe_retains_room() {
    let (_server, mut harness) = harness().await;
    let room_id = room_id!("!resident-a:example.invalid").to_string();

    harness.admit_timeline_key(room_key(&room_id)).await;
    harness.admit_timeline_key(thread_key(&room_id)).await;
    harness.unsubscribe(room_key(&room_id)).await;
    harness.unsubscribe(thread_key(&room_id)).await;

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.actor_count, 0);
    assert_eq!(snapshot.lease_count, 0);
    assert_eq!(snapshot.desired_rooms, vec![room_id.clone()]);
    assert_eq!(snapshot.active_rooms, vec![room_id.clone()]);

    let extra_room_id = room_id!("!resident-extra:example.invalid").to_string();
    harness.admit_timeline_key(room_key(&extra_room_id)).await;

    let snapshot = harness.snapshot();
    let mut expected_rooms = vec![room_id, extra_room_id];
    expected_rooms.sort();
    assert_eq!(snapshot.desired_rooms, expected_rooms);
    assert_eq!(snapshot.active_rooms, expected_rooms);
}

#[tokio::test]
async fn room_subscription_residency_actor_build_failure_retains_admitted_room() {
    let (_server, mut harness) = harness().await;
    let room_id = room_id!("!resident-build-failure:example.invalid").to_string();

    harness.admit_build_failure(&room_id).await;

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.actor_count, 0);
    assert_eq!(snapshot.lease_count, 0);
    assert_eq!(snapshot.desired_rooms, vec![room_id.clone()]);
    assert_eq!(snapshot.active_rooms, vec![room_id]);
}

#[tokio::test]
async fn room_subscription_residency_has_no_count_or_lru_eviction() {
    let (_server, mut harness) = harness().await;
    let mut room_ids = Vec::new();
    let mut keys = Vec::new();
    for index in 0..140 {
        let room_id = format!("!resident-{index:03}:example.invalid");
        room_ids.push(room_id.clone());
        let key = room_key(&room_id);
        keys.push(key.clone());
        harness.admit_timeline_key(key).await;
    }
    for key in keys {
        harness.unsubscribe(key).await;
    }

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.actor_count, 0);
    assert_eq!(snapshot.lease_count, 0);
    assert_eq!(snapshot.desired_rooms, room_ids);
    assert_eq!(snapshot.active_rooms, snapshot.desired_rooms);

    let extra_room_id = room_id!("!resident-140:example.invalid").to_string();
    harness.admit_timeline_key(room_key(&extra_room_id)).await;

    let snapshot = harness.snapshot();
    let mut expected_rooms = room_ids;
    expected_rooms.push(extra_room_id);
    expected_rooms.sort();
    assert_eq!(snapshot.desired_rooms, expected_rooms);
    assert_eq!(snapshot.active_rooms, expected_rooms);
}

#[tokio::test]
async fn room_subscription_residency_room_thread_focused_share_one_room() {
    let (_server, mut harness) = harness().await;
    let room_id = room_id!("!resident-shared:example.invalid").to_string();

    harness.admit_timeline_key(room_key(&room_id)).await;
    let first = harness.snapshot();
    harness.admit_timeline_key(thread_key(&room_id)).await;
    harness.admit_timeline_key(focused_key(&room_id)).await;
    let after_admission = harness.snapshot();
    assert_eq!(after_admission.actor_count, 3);
    assert_eq!(after_admission.lease_count, 3);
    assert_eq!(after_admission.desired_rooms, vec![room_id.clone()]);
    assert_eq!(after_admission.active_rooms, vec![room_id.clone()]);
    assert_eq!(after_admission.sdk_generation, first.sdk_generation);

    harness.unsubscribe(room_key(&room_id)).await;
    harness.unsubscribe(thread_key(&room_id)).await;
    harness.unsubscribe(focused_key(&room_id)).await;

    let final_snapshot = harness.snapshot();
    assert_eq!(final_snapshot.actor_count, 0);
    assert_eq!(final_snapshot.lease_count, 0);
    assert_eq!(final_snapshot.desired_rooms, vec![room_id.clone()]);
    assert_eq!(final_snapshot.active_rooms, vec![room_id.clone()]);

    let extra_room_id = room_id!("!resident-extra:example.invalid").to_string();
    harness.admit_timeline_key(room_key(&extra_room_id)).await;

    let final_snapshot = harness.snapshot();
    let mut expected_rooms = vec![room_id, extra_room_id];
    expected_rooms.sort();
    assert_eq!(final_snapshot.desired_rooms, expected_rooms);
    assert_eq!(final_snapshot.active_rooms, expected_rooms);
}

#[tokio::test]
async fn room_subscription_residency_test_support_compiles() {
    let harness = RoomSubscriptionResidencyHarness::new();
    harness.compile_probe();
}
