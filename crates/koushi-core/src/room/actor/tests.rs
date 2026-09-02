use super::make_request_id;
use super::{RoomActor, RoomActorHandle, RoomMessage};

use koushi_protocol::command::RoomCommand;

use crate::executor;
use koushi_protocol::event::RoomEvent;

use koushi_sdk::MatrixClientSession;

use koushi_state::SessionInfo;
use koushi_state::{AppAction, RoomListSource};

#[cfg(any(test, feature = "test-hooks"))]
use std::sync::{Mutex, atomic::AtomicUsize};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

#[tokio::test]
async fn room_actor_shutdown_aborts_when_its_mailbox_cannot_accept_shutdown() {
    let (tx, _rx) = mpsc::channel(1);
    tx.send(RoomMessage::Shutdown).await.expect("fill mailbox");
    let (timeline_residency, _timeline_residency_rx) = watch::channel(None);
    let (session, _session_rx) = watch::channel(None);
    let mut handle = RoomActorHandle {
        tx,
        timeline_residency,
        session,
        #[cfg(any(test, feature = "test-hooks"))]
        room_operation_test_control: Arc::new(Mutex::new(None)),
        #[cfg(any(test, feature = "test-hooks"))]
        room_operation_test_reached_count: Arc::new(AtomicUsize::new(0)),
        task: Some(executor::spawn(std::future::pending())),
    };

    assert!(
        !handle
            .shutdown_with_timeouts(Duration::from_millis(10), Duration::from_millis(10))
            .await
    );
    assert!(handle.task.is_none());
}

#[tokio::test]
async fn select_space_projects_action() {
    let (action_tx, mut action_rx) = mpsc::channel(16);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );

    handle
        .send(RoomMessage::Command(RoomCommand::SelectSpace {
            request_id: make_request_id(1),
            space_id: Some("!space:example.test".to_owned()),
        }))
        .await;

    let actions = action_rx.recv().await.expect("actions");
    assert!(
        matches!(
            actions.as_slice(),
            [AppAction::SelectSpace {
                space_id: Some(id)
            }] if id == "!space:example.test"
        ),
        "expected SelectSpace action, got {actions:?}"
    );
}

#[tokio::test]
async fn reorder_spaces_projects_action() {
    let (action_tx, mut action_rx) = mpsc::channel(16);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );

    handle
        .send(RoomMessage::Command(RoomCommand::ReorderSpaces {
            request_id: make_request_id(1),
            space_ids: vec![
                "!space-b:example.test".to_owned(),
                "!space-a:example.test".to_owned(),
            ],
        }))
        .await;

    let actions = action_rx.recv().await.expect("actions");
    assert!(
        matches!(
            actions.as_slice(),
            [AppAction::ReorderSpaces { space_ids }]
                if space_ids == &vec![
                        "!space-b:example.test".to_owned(),
                        "!space-a:example.test".to_owned()
                ]
        ),
        "expected ReorderSpaces action, got {actions:?}"
    );
}

#[tokio::test]
async fn select_room_projects_action() {
    let (action_tx, mut action_rx) = mpsc::channel(16);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );

    handle
        .send(RoomMessage::Command(RoomCommand::SelectRoom {
            request_id: make_request_id(2),
            room_id: "!room:example.test".to_owned(),
        }))
        .await;

    let actions = action_rx.recv().await.expect("actions");
    assert!(
        matches!(
            actions.as_slice(),
            [AppAction::SelectRoom { room_id }] if room_id == "!room:example.test"
        ),
        "expected SelectRoom action, got {actions:?}"
    );
}

#[test]
fn room_event_carries_request_id() {
    let request_id = make_request_id(10);
    let event = RoomEvent::RoomCreated {
        request_id,
        room_id: "!room:example.test".to_owned(),
    };
    match event {
        RoomEvent::RoomCreated {
            request_id: ev_id, ..
        } => assert_eq!(ev_id, request_id),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn session_lifecycle_messages_without_session_complete_cleanly() {
    let (action_tx, _action_rx) = mpsc::channel(16);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );

    // No session, no observation loop: both must be no-ops, and the
    // actor task must still exit on Shutdown.
    let (stop_ack_tx, stop_ack_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::StopSyncObservation {
                backend_generation: 1,
                ack: stop_ack_tx,
            })
            .await
    );
    stop_ack_rx.await.expect("stop acknowledgement");
    let (ack_tx, _ack_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::SessionCleared { ack: ack_tx })
            .await
    );
    assert!(handle.send(RoomMessage::Shutdown).await);
    tokio::time::timeout(std::time::Duration::from_secs(5), handle.join())
        .await
        .expect("actor task must exit after Shutdown");
}

#[tokio::test]
async fn stale_stop_generation_does_not_stop_replacement_observation() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: server.uri(),
            user_id: "@observer:example.invalid".to_owned(),
            device_id: "OBSERVER".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    ));
    let first_service = Arc::new(
        matrix_sdk_ui::room_list_service::RoomListService::new(client.clone())
            .await
            .expect("first room-list service"),
    );
    let replacement_service = Arc::new(
        matrix_sdk_ui::room_list_service::RoomListService::new(client)
            .await
            .expect("replacement room-list service"),
    );
    let (action_tx, _action_rx) = mpsc::channel(16);
    let (event_tx, _event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );

    assert!(
        handle
            .send(RoomMessage::SyncStarted {
                session: session.clone(),
                room_list_service: first_service,
                source: RoomListSource::Live,
                backend_generation: 1,
            })
            .await
    );
    assert!(
        handle
            .send(RoomMessage::SyncStarted {
                session,
                room_list_service: replacement_service,
                source: RoomListSource::Live,
                backend_generation: 2,
            })
            .await
    );

    let (stale_ack_tx, stale_ack_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::StopSyncObservation {
                backend_generation: 1,
                ack: stale_ack_tx,
            })
            .await
    );
    stale_ack_rx.await.expect("stale stop acknowledgement");

    let (inspect_tx, inspect_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::InspectObservationGeneration {
                response: inspect_tx,
            })
            .await
    );
    assert_eq!(inspect_rx.await.expect("observation generation"), Some(2));

    let (active_ack_tx, active_ack_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::StopSyncObservation {
                backend_generation: 2,
                ack: active_ack_tx,
            })
            .await
    );
    active_ack_rx.await.expect("active stop acknowledgement");

    let (inspect_tx, inspect_rx) = oneshot::channel();
    assert!(
        handle
            .send(RoomMessage::InspectObservationGeneration {
                response: inspect_tx,
            })
            .await
    );
    assert_eq!(inspect_rx.await.expect("stopped observation"), None);
    assert!(handle.send(RoomMessage::Shutdown).await);
    handle.join().await;
}
