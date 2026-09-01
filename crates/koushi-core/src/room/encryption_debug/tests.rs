use super::EncryptionDebugTestControl;

use koushi_protocol::command::RoomCommand;

use koushi_protocol::event::{
    CoreEvent, EncryptionDebugOperationOutcome as CoreEncryptionDebugOutcome, RoomEvent,
};

use crate::room::actor::make_request_id;
use crate::room::actor::{RoomActor, RoomMessage};

use koushi_sdk::MatrixClientSession;

use koushi_state::EncryptionDebugOperationKind;
use koushi_state::SessionInfo;

use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot};

#[cfg(any(test, feature = "test-hooks"))]
#[tokio::test]
async fn resend_actor_rejects_duplicate_and_correlates_one_terminal_each() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let _room = server
        .sync_joined_room(
            &client,
            matrix_sdk::ruma::room_id!("!resend:example.invalid"),
        )
        .await;
    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        client,
        SessionInfo {
            homeserver: server.uri(),
            user_id: "@actor:example.invalid".to_owned(),
            device_id: "ACTOR".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    ));
    let (action_tx, _action_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );
    handle
        .send(RoomMessage::SessionEstablished {
            session: session.clone(),
        })
        .await;
    assert!(
        handle
            .install_known_rooms_for_test(BTreeSet::from(["!resend:example.invalid".to_owned()]))
            .await
    );

    let (reached_tx, reached_rx) = oneshot::channel();
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        handle.install_encryption_debug_test_control(EncryptionDebugTestControl {
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
            reached: reached_tx,
            completion: completion_rx,
        })
    );
    let first = make_request_id(101);
    let second = make_request_id(102);
    handle
        .send(RoomMessage::Command(RoomCommand::ResendIndex0RoomKey {
            request_id: first,
            room_id: "!resend:example.invalid".to_owned(),
        }))
        .await;
    reached_rx.await.expect("first resend reached test seam");
    handle
        .send(RoomMessage::Command(RoomCommand::ResendIndex0RoomKey {
            request_id: second,
            room_id: "!resend:example.invalid".to_owned(),
        }))
        .await;

    let duplicate = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("duplicate terminal timeout")
        .expect("duplicate terminal event");
    assert!(matches!(
        duplicate,
        CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
            request_id,
            outcome: CoreEncryptionDebugOutcome::Failed,
            ..
        }) if request_id == second
    ));

    completion_tx
        .send(CoreEncryptionDebugOutcome::Completed)
        .expect("complete first resend");
    let completed = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("completion terminal timeout")
        .expect("completion terminal event");
    match completed {
        CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
            request_id,
            outcome: CoreEncryptionDebugOutcome::Completed,
            ..
        }) if request_id == first => {}
        other => panic!("unexpected first terminal event: {other:?}"),
    }
    assert!(
        event_rx.try_recv().is_err(),
        "one terminal event per request"
    );

    let (reached_tx, reached_rx) = oneshot::channel();
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        handle.install_encryption_debug_test_control(EncryptionDebugTestControl {
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
            reached: reached_tx,
            completion: completion_rx,
        })
    );
    let teardown_request = make_request_id(103);
    handle
        .send(RoomMessage::Command(RoomCommand::ResendIndex0RoomKey {
            request_id: teardown_request,
            room_id: "!resend:example.invalid".to_owned(),
        }))
        .await;
    reached_rx.await.expect("teardown resend reached test seam");
    let (ack_tx, ack_rx) = oneshot::channel();
    handle
        .send(RoomMessage::SessionCleared { ack: ack_tx })
        .await;
    completion_tx
        .send(CoreEncryptionDebugOutcome::Completed)
        .expect("complete teardown resend after cancellation");
    ack_rx.await.expect("session clear acknowledgement");
    let teardown = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("teardown terminal timeout")
        .expect("teardown terminal event");
    assert!(matches!(
        teardown,
        CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
            request_id,
            outcome: CoreEncryptionDebugOutcome::CancelledStale,
            ..
        }) if request_id == teardown_request
    ));
    assert!(
        event_rx.try_recv().is_err(),
        "late completion must be dropped"
    );

    assert!(handle.send(RoomMessage::Shutdown).await);
    handle.join().await;
}

#[cfg(any(test, feature = "test-hooks"))]
#[tokio::test]
async fn authoritative_room_removal_cancels_resend_and_rejects_replacement() {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let _room = server
        .sync_joined_room(
            &client,
            matrix_sdk::ruma::room_id!("!removed:example.invalid"),
        )
        .await;
    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        client,
        SessionInfo {
            homeserver: server.uri(),
            user_id: "@actor:example.invalid".to_owned(),
            device_id: "ACTOR".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    ));
    let (action_tx, _action_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = broadcast::channel(16);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );
    handle
        .send(RoomMessage::SessionEstablished { session })
        .await;
    assert!(
        handle
            .install_known_rooms_for_test(BTreeSet::from(["!removed:example.invalid".to_owned()]))
            .await
    );

    let (reached_tx, reached_rx) = oneshot::channel();
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        handle.install_encryption_debug_test_control(EncryptionDebugTestControl {
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
            reached: reached_tx,
            completion: completion_rx,
        })
    );
    let request_id = make_request_id(104);
    handle
        .send(RoomMessage::Command(RoomCommand::ResendIndex0RoomKey {
            request_id,
            room_id: "!removed:example.invalid".to_owned(),
        }))
        .await;
    reached_rx.await.expect("resend reached test seam");
    assert!(handle.install_known_rooms_for_test(BTreeSet::new()).await);
    handle
        .send(RoomMessage::AuthoritativeRoomsRemoved {
            room_ids: BTreeSet::from(["!removed:example.invalid".to_owned()]),
        })
        .await;
    completion_tx
        .send(CoreEncryptionDebugOutcome::Completed)
        .expect("cancelled resend must still settle its join");

    let cancelled = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("removal cancellation timeout")
        .expect("removal cancellation event");
    assert!(matches!(
        cancelled,
        CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
            request_id: got,
            outcome: CoreEncryptionDebugOutcome::CancelledStale,
            ..
        }) if got == request_id
    ));

    let replacement = make_request_id(105);
    handle
        .send(RoomMessage::Command(RoomCommand::ResendIndex0RoomKey {
            request_id: replacement,
            room_id: "!removed:example.invalid".to_owned(),
        }))
        .await;
    let rejected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("replacement rejection timeout")
        .expect("replacement rejection event");
    assert!(matches!(
        rejected,
        CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
            request_id: got,
            outcome: CoreEncryptionDebugOutcome::Failed,
            ..
        }) if got == replacement
    ));
    assert!(
        event_rx.try_recv().is_err(),
        "room removal emits one terminal event"
    );

    assert!(handle.send(RoomMessage::Shutdown).await);
    handle.join().await;
}
