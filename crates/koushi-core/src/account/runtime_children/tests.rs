use std::time::Duration;

use koushi_protocol::SessionKeyId;

use koushi_state::{AppAction, LoginRequest};

use tokio::sync::mpsc;

use super::{next_read_persistence_session_generation, run_read_persistence_worker};
use crate::account::actor::AccountMessage;
use crate::account::test_support::{
    acknowledge_next_verified_projection, assert_no_logout_finished, configure_verified_trust,
    recv_account_action_with_sliding_sync_effects, recv_probe_with_sliding_sync_effects,
    shutdown_and_ack, spawn_actor_with_dirs, spawn_quarantine_password_server, test_request_id,
};
use koushi_protocol::command::AccountCommand;

use crate::executor;
use koushi_protocol::event::{AccountEvent, CoreEvent};

use koushi_protocol::ids::RequestId;

use crate::store::CredentialStoreBackend;
use crate::store::StoreActor;

use crate::timeline::{ReadPersistenceIngress, ReadPersistenceRequest};

use tempfile::tempdir;

#[tokio::test]
async fn shutdown_quiesces_provisional_tasks_and_releases_session_without_logout_terminal() {
    let homeserver = spawn_quarantine_password_server();
    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let (handle, mut action_rx, mut event_rx) =
        spawn_actor_with_dirs(cred_dir.path(), data_dir.path());
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    handle
        .send(AccountMessage::AttachLifecycleProbe { probe_tx })
        .await;
    handle
        .send(AccountMessage::Command(AccountCommand::LoginPassword {
            request_id: test_request_id(),
            request: LoginRequest {
                homeserver,
                username: "fixture-user".to_owned(),
                password: koushi_state::AuthSecret::new("synthetic-password"),
                device_display_name: None,
            },
            platform: koushi_state::DisplayPlatform::Linux,
        }))
        .await;
    while !matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::LoginSucceeded { .. }]
    ) {}
    shutdown_and_ack(&handle).await;
    let tokens: Vec<_> = std::iter::from_fn(|| probe_rx.try_recv().ok()).collect();
    assert!(tokens.contains(&"trust_observer_terminated"));
    assert!(tokens.contains(&"provisional_encryption_sync_terminated"));
    assert!(tokens.contains(&"current_session_released"));
    assert_no_logout_finished(&mut action_rx);
    while let Ok(event) = event_rx.try_recv() {
        assert!(!matches!(
            event,
            CoreEvent::Account(AccountEvent::LoggedOut { .. })
        ));
    }
}

#[tokio::test]
async fn shutdown_quiesces_promoted_children_before_releasing_session() {
    let homeserver = spawn_quarantine_password_server();
    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let (handle, mut action_rx, _event_rx) =
        spawn_actor_with_dirs(cred_dir.path(), data_dir.path());
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    handle
        .send(AccountMessage::AttachLifecycleProbe { probe_tx })
        .await;
    configure_verified_trust(&handle).await;
    handle
        .send(AccountMessage::Command(AccountCommand::LoginPassword {
            request_id: test_request_id(),
            request: LoginRequest {
                homeserver,
                username: "fixture-user".to_owned(),
                password: koushi_state::AuthSecret::new("synthetic-password"),
                device_display_name: None,
            },
            platform: koushi_state::DisplayPlatform::Linux,
        }))
        .await;
    acknowledge_next_verified_projection(&handle, &mut action_rx).await;
    while probe_rx.try_recv().is_ok() {}
    shutdown_and_ack(&handle).await;
    let tokens: Vec<_> = std::iter::from_fn(|| probe_rx.try_recv().ok()).collect();
    assert!(tokens.contains(&"trust_observer_terminated"));
    assert!(tokens.contains(&"shutdown_stop_sync_actor"));
    assert!(tokens.contains(&"shutdown_clear_room_session"));
    assert_eq!(tokens.last(), Some(&"current_session_released"));
}

#[tokio::test]
async fn shutdown_aborts_pending_teardown_retry_and_releases_held_sessions_without_terminal() {
    let first_homeserver = spawn_quarantine_password_server();
    let second_homeserver = spawn_quarantine_password_server();
    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let (handle, mut action_rx, _event_rx) =
        spawn_actor_with_dirs(cred_dir.path(), data_dir.path());
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    handle
        .send(AccountMessage::AttachLifecycleProbe { probe_tx })
        .await;
    let request_id = test_request_id();
    handle
        .send(AccountMessage::Command(AccountCommand::LoginPassword {
            request_id,
            request: LoginRequest {
                homeserver: first_homeserver,
                username: "fixture-user".to_owned(),
                password: koushi_state::AuthSecret::new("synthetic-password"),
                device_display_name: None,
            },
            platform: koushi_state::DisplayPlatform::Linux,
        }))
        .await;
    while !matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::LoginSucceeded { .. }]
    ) {}
    handle
        .send(AccountMessage::ConfigureCloseStoreResults {
            results: vec![false; 8],
        })
        .await;
    handle
        .send(AccountMessage::Command(AccountCommand::LoginPassword {
            request_id: RequestId {
                connection_id: koushi_protocol::ids::RuntimeConnectionId(4),
                sequence: 2,
            },
            request: LoginRequest {
                homeserver: second_homeserver,
                username: "replacement".to_owned(),
                password: koushi_state::AuthSecret::new("synthetic-password"),
                device_display_name: None,
            },
            platform: koushi_state::DisplayPlatform::Linux,
        }))
        .await;
    recv_probe_with_sliding_sync_effects(
        &handle,
        &mut action_rx,
        &mut probe_rx,
        "session_store_close_retrying",
    )
    .await;
    shutdown_and_ack(&handle).await;
    let tokens: Vec<_> = std::iter::from_fn(|| probe_rx.try_recv().ok()).collect();
    assert!(tokens.contains(&"teardown_retry_terminated"));
    assert!(tokens.contains(&"pending_teardown_sessions_released"));
    assert_no_logout_finished(&mut action_rx);
}

#[tokio::test]
async fn read_persistence_worker_saves_latest_snapshot_and_joins_after_channel_close() {
    use crate::read_state::{ReadStateEngine, ReadStateKey, ReadTarget, ReadWaiterId};

    fn snapshot(event_id: &str) -> crate::read_state::ReadPersistenceSnapshot {
        let mut engine = ReadStateEngine::new(1);
        engine.admit(
            1,
            ReadStateKey::PublicUnthreaded {
                room_id: "!worker-room:example.test".to_owned(),
            },
            ReadTarget::new(event_id.to_owned()),
            ReadWaiterId::new(1),
        );
        engine.persistence_snapshot()
    }

    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let key_id = SessionKeyId {
        homeserver: "https://example.test".to_owned(),
        user_id: "@worker:example.test".to_owned(),
        device_id: "WORKER".to_owned(),
    };
    let store = StoreActor::with_backend(
        CredentialStoreBackend::FileDir(crate::store::FileCredentialStore::new(cred_dir.path())),
        data_dir.path(),
    );
    store
        .account_store_config(&key_id)
        .expect("seed unlock secret");
    let (ingress, requests) = ReadPersistenceIngress::channel();
    let worker_store = store.clone();
    let worker_key_id = key_id.clone();
    let mut worker = executor::spawn(run_read_persistence_worker(
        worker_store,
        worker_key_id,
        7,
        requests,
    ));
    ingress.publish(ReadPersistenceRequest::new(7, 1, snapshot("$first")));
    let latest = snapshot("$latest");
    ingress.publish(ReadPersistenceRequest::new(7, 2, latest.clone()));
    drop(ingress);

    executor::timeout(Duration::from_secs(1), &mut worker)
        .await
        .expect("closed persistence channel must join within the shutdown bound")
        .expect("persistence worker task");
    assert_eq!(
        store
            .load_read_state_outbox(&key_id)
            .expect("load saved latest snapshot"),
        latest
    );
}

#[test]
fn read_persistence_session_generation_survives_actor_recreation() {
    let first_actor_generation = next_read_persistence_session_generation();
    let recreated_actor_generation = next_read_persistence_session_generation();

    assert!(
        recreated_actor_generation > first_actor_generation,
        "a recreated AccountActor must not reuse a process-local outbox generation"
    );
}
