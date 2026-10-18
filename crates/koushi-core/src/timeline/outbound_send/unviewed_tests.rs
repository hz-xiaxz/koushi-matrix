//! Direct operations must not acquire display actors or settle a GUI composer.
use std::{collections::HashMap, sync::Arc, time::Duration};

use futures_util::StreamExt;
use koushi_protocol::{
    command::TimelineCommand,
    event::{CoreEvent, TimelineEvent},
    ids::{AccountKey, TimelineKey},
};
use koushi_sdk::MatrixClientSession;
use koushi_state::{ComposerDocument, SessionAuthenticationMethod, SessionInfo};
use matrix_sdk::{
    ruma::{event_id, room_id},
    test_utils::mocks::MatrixMockServer,
};
use tokio::sync::mpsc;

use super::super::test_support::{fake_rid, live_tail_test_manager};
use super::run_global_send_completion_observer;

#[tokio::test]
async fn direct_send_completes_in_unviewed_room_without_composer_actions() {
    for reply in [false, true] {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room = server
            .sync_joined_room(&client, room_id!("!unviewed:example.invalid"))
            .await;
        server.mock_room_state_encryption().plain().mount().await;
        let mut expected_body =
            serde_json::json!({ "body": "Synthetic background message", "msgtype": "m.text" });
        if reply {
            expected_body["m.relates_to"] =
                serde_json::json!({ "m.in_reply_to": { "event_id": "$original:example.invalid" } });
        }
        server
            .mock_room_send()
            .body_matches_partial_json(expected_body)
            .ok(event_id!("$sent:example.invalid"))
            .expect(1)
            .mount()
            .await;
        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client.clone(),
            SessionInfo {
                homeserver: server.server().uri(),
                user_id: client.user_id().unwrap().to_string(),
                device_id: client.device_id().unwrap().to_string(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            },
        ));
        let key = TimelineKey::room(
            AccountKey(session.info.user_id.clone()),
            room.room_id().to_string(),
        );
        let mut manager = live_tail_test_manager(HashMap::new());
        manager.session = Some(session);
        let (action_tx, mut action_rx) = mpsc::channel(32);
        manager.action_tx = action_tx;
        let mut events = manager.event_tx.subscribe();
        let observer = tokio::spawn(run_global_send_completion_observer(
            client.send_queue().subscribe(),
            Arc::clone(&manager.send_completion),
            manager.terminal_ingress.clone(),
            Some(client),
        ));
        let request_id = fake_rid(9100);
        let document = ComposerDocument::from_plain_text("Synthetic background message");
        let command = if reply {
            TimelineCommand::SendReply {
                request_id,
                key: key.clone(),
                transaction_id: "background-reply".into(),
                in_reply_to_event_id: "$original:example.invalid".into(),
                document,
            }
        } else {
            TimelineCommand::SendText {
                request_id,
                key: key.clone(),
                transaction_id: "background-text".into(),
                document,
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        manager.handle_command(command).await;
        assert!(
            events.try_recv().is_err(),
            "send must not fail admission without a display actor"
        );
        if let Some(completion) =
            tokio::time::timeout_at(deadline, manager.send_enqueue_workers.tasks.next())
                .await
                .unwrap()
        {
            manager
                .handle_send_enqueue_worker_completion(completion)
                .await;
        }
        let terminal = tokio::time::timeout_at(deadline, manager.terminal_rx.recv())
            .await
            .unwrap()
            .unwrap();
        manager.handle_send_terminal_handoff(terminal).await;
        assert!(
            matches!(events.try_recv().unwrap(), CoreEvent::Timeline(TimelineEvent::SendCompleted { request_id: completed, event_id, .. }) if completed == request_id && event_id == "$sent:example.invalid")
        );
        assert!(
            manager.timelines.is_empty(),
            "send must not create a display subscription"
        );
        assert!(
            action_rx.try_recv().is_err(),
            "direct send must leave GUI composer untouched"
        );
        observer.abort();
        let _ = observer.await;
    }
}

#[tokio::test]
async fn direct_send_rejects_foreign_missing_left_and_leaving_rooms() {
    use super::super::residency::RoomLeaveState;
    use koushi_protocol::failure::{CoreFailure, TimelineFailureKind};
    use matrix_sdk_test::LeftRoomBuilder;

    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let joined = server
        .sync_joined_room(&client, room_id!("!joined:example.invalid"))
        .await;
    server
        .sync_room(
            &client,
            LeftRoomBuilder::new(room_id!("!left:example.invalid")),
        )
        .await;
    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: server.server().uri(),
            user_id: client.user_id().unwrap().to_string(),
            device_id: client.device_id().unwrap().to_string(),
            authentication_method: SessionAuthenticationMethod::Unknown,
        },
    ));
    let account = AccountKey(session.info.user_id.clone());
    let mut manager = live_tail_test_manager(HashMap::new());
    manager.session = Some(session);
    manager.room_leave_states.insert(
        joined.room_id().to_owned(),
        RoomLeaveState::PendingLeftObservation,
    );
    let (action_tx, mut actions) = mpsc::channel(32);
    manager.action_tx = action_tx;
    let mut events = manager.event_tx.subscribe();
    for (key, expected) in [
        (
            TimelineKey::room(
                AccountKey("@foreign:example.invalid".into()),
                joined.room_id().to_string(),
            ),
            TimelineFailureKind::InvalidSendTarget,
        ),
        (
            TimelineKey::room(account.clone(), "!missing:example.invalid"),
            TimelineFailureKind::NotFound,
        ),
        (
            TimelineKey::room(account.clone(), "!left:example.invalid"),
            TimelineFailureKind::Forbidden,
        ),
        (
            TimelineKey::room(account, joined.room_id().to_string()),
            TimelineFailureKind::Forbidden,
        ),
    ] {
        manager
            .handle_command(TimelineCommand::SendText {
                request_id: fake_rid(9200),
                key,
                transaction_id: "rejected-send".into(),
                document: ComposerDocument::from_plain_text("Synthetic rejected message"),
            })
            .await;
        assert!(
            matches!(events.try_recv().unwrap(), CoreEvent::OperationFailed { failure: CoreFailure::TimelineOperationFailed { kind }, .. } if kind == expected)
        );
        assert!(manager.send_enqueue_workers.tasks.is_empty());
    }
    assert!(actions.try_recv().is_err());
    assert!(manager.timelines.is_empty());
}
