use std::{path::PathBuf, sync::Arc};

use koushi_core::{
    AccountCommand, CoreCommand, CoreRuntime, NativeArtifactKind, NativeArtifactRegistry,
    RoomKeyExportRequest,
};
use koushi_state::AuthSecret;

#[tokio::test]
async fn rejected_command_releases_its_exact_native_artifact_registration() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let registry = Arc::new(NativeArtifactRegistry::new());
    let runtime = CoreRuntime::start_with_data_dir_and_native_artifact_port(
        data_dir.path().to_path_buf(),
        registry.clone(),
    );
    let connection = runtime.attach();
    let request_id = connection.next_request_id();
    connection
        .register_native_artifact(
            request_id,
            NativeArtifactKind::RoomKeyExportDestination,
            PathBuf::from("synthetic-export-path"),
        )
        .expect("register path");

    connection
        .command_with_admission(CoreCommand::Account(AccountCommand::ExportRoomKeys {
            request_id,
            request: RoomKeyExportRequest {
                passphrase: AuthSecret::new("synthetic-passphrase"),
            },
        }))
        .await
        .expect("local command admission");

    assert!(registry.is_empty());
    drop(connection);
    runtime.shutdown().await;
}

#[tokio::test]
async fn rejected_bootstrap_releases_its_recovery_key_destination() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let registry = Arc::new(NativeArtifactRegistry::new());
    let runtime = CoreRuntime::start_with_data_dir_and_native_artifact_port(
        data_dir.path().to_path_buf(),
        registry.clone(),
    );
    let connection = runtime.attach();
    let request_id = connection.next_request_id();
    connection
        .register_native_artifact(
            request_id,
            NativeArtifactKind::RecoveryKeyDestination,
            PathBuf::from("synthetic-recovery-key-path"),
        )
        .expect("register path");

    connection
        .command_with_admission(CoreCommand::Account(
            AccountCommand::StartSessionBootstrap {
                request_id,
                flow_id: 1,
                auth: None,
                request: koushi_protocol::SecureBackupSetupRequest {
                    passphrase: None,
                    recovery_key_destination_requested: true,
                    intent: koushi_state::SecureBackupSetupIntent::InitialSetup,
                },
            },
        ))
        .await
        .expect("local command admission");

    assert!(registry.is_empty());
    drop(connection);
    runtime.shutdown().await;
}
