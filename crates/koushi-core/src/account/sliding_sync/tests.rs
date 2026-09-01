use koushi_key::StoredMatrixSession;
use koushi_sdk::PersistableMatrixSession;
use koushi_state::{
    AppAction, AuthFailureKind, LoginRequest, SlidingSyncAdmission, SlidingSyncCapabilityResult,
    SlidingSyncPositiveEvidence,
};

use super::{
    record_sliding_sync_capability_persistence, sliding_sync_revalidation_completion_is_current,
};
use crate::account::actor::AccountMessage;
use crate::account::test_support::{
    inspect_session_runtime, recv_account_action_with_sliding_sync_effects, spawn_actor_with_dirs,
    spawn_named_quarantine_password_server_with_controls, test_request_id,
};
use crate::command::AccountCommand;

use koushi_protocol::event::CoreEvent;

use koushi_protocol::failure::CoreFailure;
use koushi_protocol::ids::{RequestId, RuntimeConnectionId};

use crate::store::CredentialStoreBackend;
use crate::store::session_key_id_from_info;

use tempfile::tempdir;

#[test]
fn sliding_sync_evidence_persistence_diagnostic_is_private_and_closed() {
    let output = std::process::Command::new(
        std::env::current_exe().expect("current test executable should be available"),
    )
    .args([
        "--exact",
        "account::sliding_sync::tests::sliding_sync_evidence_persistence_diagnostic_child",
        "--ignored",
        "--nocapture",
    ])
    .output()
    .expect("sliding sync persistence diagnostic child should run");
    assert!(output.status.success(), "child failed: {output:?}");
    assert!(output.stderr.is_empty(), "diagnostics must stay buffered");

    let stdout = String::from_utf8(output.stdout).expect("child stdout should be utf8");
    let snapshot: serde_json::Value = serde_json::from_str(
        stdout
            .lines()
            .find(|line| line.starts_with('{'))
            .expect("child should print one JSON snapshot"),
    )
    .expect("child output should be a JSON snapshot");
    let matching = snapshot["records"]
        .as_array()
        .expect("diagnostic records")
        .iter()
        .filter(|record| {
            record["event"]["source"] == "core.sliding_sync_capability"
                && record["event"]["stage"] == "positive_evidence_persistence"
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 2);
    let outcomes = matching
        .iter()
        .map(|record| record["event"]["fields"][0]["value"]["value"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(outcomes, vec![Some("saved"), Some("failed")]);
    assert!(matching.iter().all(|record| {
        record["event"]["fields"]
            .as_array()
            .is_some_and(|fields| fields.len() == 1 && fields[0]["key"] == "outcome")
    }));
}

#[test]
#[ignore]
fn sliding_sync_evidence_persistence_diagnostic_child() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    record_sliding_sync_capability_persistence("saved");
    record_sliding_sync_capability_persistence("failed");
    println!(
        "{}",
        serde_json::to_string(&koushi_diagnostics::test_support::detail_snapshot())
            .expect("diagnostic snapshot should serialize")
    );
}

#[test]
fn sliding_sync_revalidation_completion_requires_the_exact_active_request() {
    let active = Some((7, 12));

    assert!(sliding_sync_revalidation_completion_is_current(
        active, 7, 12
    ));
    assert!(!sliding_sync_revalidation_completion_is_current(
        active, 7, 11
    ));
    assert!(!sliding_sync_revalidation_completion_is_current(
        active, 6, 12
    ));
    assert!(!sliding_sync_revalidation_completion_is_current(
        None, 7, 12
    ));
}

#[tokio::test]
async fn unsupported_password_login_never_installs_or_persists_the_session() {
    let homeserver = spawn_unsupported_quarantine_password_server();
    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let (handle, mut action_rx, mut event_rx) =
        spawn_actor_with_dirs(cred_dir.path(), data_dir.path());
    let request_id = test_request_id();
    assert!(
        handle
            .send(AccountMessage::Command(AccountCommand::LoginPassword {
                request_id,
                request: LoginRequest {
                    homeserver,
                    username: "fixture-user".to_owned(),
                    password: koushi_state::AuthSecret::new("synthetic-password"),
                    device_display_name: None,
                },
                platform: koushi_state::DisplayPlatform::Linux,
            }))
            .await
    );
    assert!(matches!(
        action_rx.recv().await.as_deref(),
        Some([AppAction::SlidingSyncCapabilityCheckStarted {
            admission: SlidingSyncAdmission::NewLogin { .. },
            ..
        }])
    ));
    assert!(matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::SlidingSyncCapabilityCheckCompleted {
            result: SlidingSyncCapabilityResult::Unsupported,
            ..
        }]
    ));
    assert!(matches!(
        action_rx.recv().await.as_deref(),
        Some([AppAction::LoginFailed { .. }])
    ));
    assert_eq!(
        inspect_session_runtime(&handle).await,
        (false, false, false, false)
    );
    let backend =
        CredentialStoreBackend::FileDir(crate::store::FileCredentialStore::new(cred_dir.path()));
    assert!(backend.load_last_session().expect("last pointer").is_none());
    assert!(
        backend
            .load_saved_sessions()
            .expect("saved sessions")
            .sessions()
            .is_empty()
    );
    assert!(matches!(
        event_rx.recv().await,
        Ok(CoreEvent::OperationFailed {
            request_id: failed_request_id,
            failure: CoreFailure::AccountOperationFailed {
                kind: AuthFailureKind::Unsupported,
            },
        }) if failed_request_id == request_id
    ));
    let _ = handle.send(AccountMessage::Shutdown).await;
}

#[tokio::test]
async fn unsupported_restore_preserves_persisted_session_and_positive_evidence() {
    let homeserver = spawn_unsupported_quarantine_password_server();
    let login = koushi_sdk::login_with_password_with_store(
        &LoginRequest {
            homeserver,
            username: "fixture-user".to_owned(),
            password: koushi_state::AuthSecret::new("synthetic-password"),
            device_display_name: None,
        },
        None,
    )
    .await
    .expect("fixture login");
    let expected_info = login.info.clone();
    let key_id = session_key_id_from_info(&login.info);
    let evidence = SlidingSyncPositiveEvidence { observed_at_ms: 7 };
    let stored = StoredMatrixSession::new(
        login
            .persistable_session()
            .expect("persistable")
            .with_sliding_sync_positive_evidence(evidence.clone())
            .to_json()
            .expect("json"),
    );
    drop(login);

    let cred_dir = tempdir().expect("tempdir");
    let data_dir = tempdir().expect("tempdir");
    let backend =
        CredentialStoreBackend::FileDir(crate::store::FileCredentialStore::new(cred_dir.path()));
    backend
        .save_matrix_session(&key_id, &stored)
        .expect("session seed");
    backend.remember_saved_session(&key_id).expect("index seed");
    backend.save_last_session(&key_id).expect("pointer seed");

    let (handle, mut action_rx, _event_rx) =
        spawn_actor_with_dirs(cred_dir.path(), data_dir.path());
    assert!(
        handle
            .send(AccountMessage::Command(
                AccountCommand::RestoreLastSession {
                    request_id: test_request_id(),
                },
            ))
            .await
    );
    assert!(matches!(
        action_rx.recv().await.as_deref(),
        Some([AppAction::SlidingSyncCapabilityCheckStarted {
            admission: SlidingSyncAdmission::StoredSessionRestore { .. },
            positive_evidence: Some(saved),
            ..
        }]) if saved == &evidence
    ));
    assert!(matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::SlidingSyncCapabilityCheckCompleted {
            result: SlidingSyncCapabilityResult::Unsupported,
            ..
        }]
    ));
    assert_eq!(
        inspect_session_runtime(&handle).await,
        (false, false, false, false)
    );
    let persisted = backend
        .load_matrix_session(&key_id)
        .expect("preserved session");
    let reopened =
        PersistableMatrixSession::from_json(persisted.as_str()).expect("preserved session JSON");
    assert_eq!(reopened.info, expected_info);
    assert_eq!(reopened.sliding_sync_positive_evidence(), Some(evidence));
    assert!(backend.load_last_session().expect("last pointer").is_some());

    assert!(
        handle
            .send(AccountMessage::Command(
                AccountCommand::RetrySlidingSyncCapability {
                    request_id: RequestId {
                        connection_id: RuntimeConnectionId(1),
                        sequence: 2,
                    },
                },
            ))
            .await
    );
    assert!(matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::SlidingSyncCapabilityRetryAccepted { .. }]
    ));
    assert!(matches!(
        action_rx.recv().await.as_deref(),
        Some([AppAction::SlidingSyncCapabilityCheckStarted {
            admission: SlidingSyncAdmission::StoredSessionRestore { .. },
            ..
        }])
    ));
    assert!(matches!(
        recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx)
            .await
            .as_slice(),
        [AppAction::SlidingSyncCapabilityCheckCompleted {
            result: SlidingSyncCapabilityResult::Unsupported,
            ..
        }]
    ));

    handle
        .send(AccountMessage::Command(AccountCommand::ResetLocalData {
            request_id: RequestId {
                connection_id: RuntimeConnectionId(1),
                sequence: 3,
            },
        }))
        .await;
    assert!(matches!(
        action_rx.recv().await.as_deref(),
        Some([
            AppAction::ResetLocalDataCompleted { request_id: 3 },
            AppAction::LogoutFinished,
        ])
    ));
    assert!(koushi_key::is_missing_credential_error(
        &backend
            .load_matrix_session(&key_id)
            .expect_err("blocked session persistence should be deleted")
    ));
    let _ = handle.send(AccountMessage::Shutdown).await;
}

fn spawn_unsupported_quarantine_password_server() -> String {
    spawn_named_quarantine_password_server_with_controls(
        "@fixture-user:example.invalid",
        "FIXTUREDEVICE",
        None,
        None,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}
