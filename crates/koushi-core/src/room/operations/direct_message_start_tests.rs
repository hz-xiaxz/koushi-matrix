//! #1518: `StartDirectMessage` runs inside the Rust-owned basic-operation slot,
//! so the renderer reads its pending state from the snapshot, and every exit
//! settles the slot.

use std::{sync::Arc, time::Duration};

use koushi_protocol::command::{DirectMessageMissingPolicy, RoomCommand};
use koushi_protocol::event::{CoreEvent, RoomEvent};
use koushi_sdk::MatrixClientSession;
use koushi_state::{AppAction, BasicOperationRequest, SessionInfo};
use matrix_sdk::{Client, test_utils::mocks::MatrixMockServer};
use tokio::sync::{broadcast, mpsc};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path_regex},
};

use crate::room::actor::{RoomActor, RoomActorHandle, RoomMessage, make_request_id};

const TARGET: &str = "@member-1:example.test";

async fn actor_with_session(
    server: &MatrixMockServer,
    client: Client,
) -> (
    RoomActorHandle,
    mpsc::Receiver<Vec<AppAction>>,
    broadcast::Receiver<CoreEvent>,
) {
    let session = Arc::new(MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: server.uri(),
            user_id: client.user_id().unwrap().to_string(),
            device_id: client.device_id().unwrap().to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    ));
    let (action_tx, action_rx) = mpsc::channel(64);
    let (event_tx, event_rx) = broadcast::channel(64);
    let handle = RoomActor::spawn(
        action_tx,
        event_tx,
        crate::SlidingSyncDiagnostics::default(),
    );
    assert!(
        handle
            .send(RoomMessage::SessionEstablished { session })
            .await
    );
    (handle, action_rx, event_rx)
}

async fn actions_until(
    action_rx: &mut mpsc::Receiver<Vec<AppAction>>,
    done: impl Fn(&AppAction) -> bool,
) -> Vec<AppAction> {
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(batch) = action_rx.recv().await {
            let finished = batch.iter().any(&done);
            seen.extend(batch);
            if finished {
                return;
            }
        }
    })
    .await
    .expect("expected reducer actions");
    seen
}

fn requested(actions: &[AppAction], expected: u64) -> bool {
    actions.iter().any(|action| {
        matches!(
            action,
            AppAction::BasicOperationRequested {
                request_id,
                request: BasicOperationRequest::StartDirectMessage { user_id },
            } if *request_id == expected && user_id == TARGET
        )
    })
}

#[tokio::test]
async fn a_reported_missing_direct_message_settles_the_pending_start() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let (handle, mut action_rx, mut event_rx) = actor_with_session(&server, client).await;

    assert!(
        handle
            .send(RoomMessage::Command(RoomCommand::StartDirectMessage {
                request_id: make_request_id(31),
                user_id: TARGET.to_owned(),
                missing: DirectMessageMissingPolicy::Report,
            }))
            .await
    );

    let actions = actions_until(&mut action_rx, |action| {
        matches!(
            action,
            AppAction::BasicOperationSucceeded { request_id: 31 }
        )
    })
    .await;
    assert!(requested(&actions, 31));
    let event = tokio::time::timeout(Duration::from_secs(10), event_rx.recv())
        .await
        .expect("missing event")
        .expect("event channel");
    assert!(matches!(
        event,
        CoreEvent::Room(RoomEvent::DirectMessageMissing { .. })
    ));
}

#[tokio::test]
async fn a_failed_direct_message_start_settles_the_pending_start() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/_matrix/client/v3/createRoom"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "errcode": "M_UNKNOWN",
            "error": "synthetic failure"
        })))
        .mount(server.server())
        .await;
    let (handle, mut action_rx, _event_rx) = actor_with_session(&server, client).await;

    assert!(
        handle
            .send(RoomMessage::Command(RoomCommand::StartDirectMessage {
                request_id: make_request_id(32),
                user_id: TARGET.to_owned(),
                missing: DirectMessageMissingPolicy::Create,
            }))
            .await
    );

    let actions = actions_until(&mut action_rx, |action| {
        matches!(
            action,
            AppAction::BasicOperationFailed { request_id: 32, .. }
        )
    })
    .await;
    assert!(requested(&actions, 32));
}
