use super::*;

#[tauri::command]
pub async fn retry_current_device_trust_discovery(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_retry_current_device_trust_discovery_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_own_user_sas(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    // The persistent CoreConnection request sequence is process-unique and
    // therefore owns the opaque verification flow identity across retries.
    let flow_id = request_id.sequence;
    submit_core_command(
        state.inner(),
        build_start_own_user_sas_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn mismatch_sas_verification(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_cancel_verification_command(request_id, flow_id, VerificationCancelReason::Mismatch),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_session_bootstrap(
    passphrase: Option<String>,
    recovery_key_destination_path: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    let flow_id = request_id.sequence;
    submit_core_command(
        state.inner(),
        build_start_session_bootstrap_command(
            request_id,
            flow_id,
            passphrase.map(AuthSecret::new),
            recovery_key_destination_path,
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn confirm_session_bootstrap_saved(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_confirm_session_bootstrap_saved_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn bootstrap_cross_signing(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_bootstrap_cross_signing_command(request_id, None),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn enable_key_backup(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_enable_key_backup_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn bootstrap_secure_backup(
    passphrase: Option<String>,
    recovery_key_destination_path: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_bootstrap_secure_backup_command(
            request_id,
            passphrase.map(AuthSecret::new),
            recovery_key_destination_path,
            false,
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn reenable_secure_backup(
    passphrase: Option<String>,
    recovery_key_destination_path: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons, MessageDialogKind};

    let (confirmation_tx, confirmation_rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(
            "Re-enabling Secure Backup changes the account-wide setting used by your other Matrix clients.\n\nSecure Backupを再有効化すると、他のMatrixクライアントも参照するアカウント全体の設定が変更されます。",
        )
        .title("Secure Backup")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Enable / 有効にする".to_owned(),
            "Cancel / キャンセル".to_owned(),
        ))
        .show(move |confirmed| {
            let _ = confirmation_tx.send(confirmed);
        });
    if !confirmation_rx.await.unwrap_or(false) {
        return current_snapshot(state.inner()).await;
    }

    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_bootstrap_secure_backup_command(
            request_id,
            passphrase.map(AuthSecret::new),
            recovery_key_destination_path,
            true,
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn recover_secure_backup(
    secret: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_recover_secure_backup_command(request_id, AuthSecret::new(secret)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn retry_secure_backup_inspection(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_retry_secure_backup_inspection_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn change_secure_backup_passphrase(
    old_secret: String,
    new_passphrase: String,
    recovery_key_destination_path: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_change_secure_backup_passphrase_command(
            request_id,
            AuthSecret::new(old_secret),
            AuthSecret::new(new_passphrase),
            recovery_key_destination_path,
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn export_room_keys(
    destination_path: String,
    passphrase: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_export_room_keys_command(request_id, destination_path, AuthSecret::new(passphrase)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn import_room_keys(
    source_path: String,
    passphrase: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_import_room_keys_command(request_id, source_path, AuthSecret::new(passphrase)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn accept_verification(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_accept_verification_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn confirm_sas_verification(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_confirm_sas_verification_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn cancel_verification(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_cancel_verification_command(request_id, flow_id, VerificationCancelReason::User),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn reset_identity(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_reset_identity_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn cancel_identity_reset(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_cancel_identity_reset_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn submit_identity_reset_password(
    flow_id: u64,
    password: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_submit_identity_reset_password_command(
            request_id,
            flow_id,
            AuthSecret::new(password),
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn submit_identity_reset_oauth(
    flow_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_submit_identity_reset_oauth_command(request_id, flow_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) fn build_bootstrap_cross_signing_command(
    request_id: koushi_core::RequestId,
    auth: Option<AuthSecret>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::BootstrapCrossSigning { request_id, auth })
}

pub(super) fn build_enable_key_backup_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::EnableKeyBackup {
        request_id,
        passphrase: None,
    })
}

pub(super) fn build_bootstrap_secure_backup_command(
    request_id: koushi_core::RequestId,
    passphrase: Option<AuthSecret>,
    recovery_key_destination_path: Option<String>,
    explicit_reenable_confirmed: bool,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::BootstrapSecureBackup {
        request_id,
        request: SecureBackupSetupRequest {
            passphrase,
            recovery_key_destination_path: recovery_key_destination_path.map(PathBuf::from),
            explicit_reenable_confirmed,
        },
    })
}

pub(super) fn build_recover_secure_backup_command(
    request_id: koushi_core::RequestId,
    secret: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::RecoverSecureBackup {
        request_id,
        request: RecoveryRequest { secret },
    })
}

pub(super) fn build_retry_secure_backup_inspection_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::RetrySecureBackupInspection { request_id })
}

pub(super) fn build_change_secure_backup_passphrase_command(
    request_id: koushi_core::RequestId,
    old_secret: AuthSecret,
    new_passphrase: AuthSecret,
    recovery_key_destination_path: Option<String>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ChangeSecureBackupPassphrase {
        request_id,
        request: SecureBackupPassphraseChangeRequest {
            old_secret,
            new_passphrase,
            recovery_key_destination_path: recovery_key_destination_path.map(PathBuf::from),
        },
    })
}

pub(super) fn build_export_room_keys_command(
    request_id: koushi_core::RequestId,
    destination_path: String,
    passphrase: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ExportRoomKeys {
        request_id,
        request: RoomKeyExportRequest {
            destination_path: PathBuf::from(destination_path),
            passphrase,
        },
    })
}

pub(super) fn build_import_room_keys_command(
    request_id: koushi_core::RequestId,
    source_path: String,
    passphrase: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ImportRoomKeys {
        request_id,
        request: RoomKeyImportRequest {
            source_path: PathBuf::from(source_path),
            passphrase,
        },
    })
}

pub(super) fn build_accept_verification_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::AcceptVerification {
        request_id,
        flow_id,
    })
}

pub(super) fn build_start_own_user_sas_command(request_id: RequestId, flow_id: u64) -> CoreCommand {
    CoreCommand::Account(AccountCommand::StartOwnUserSas {
        request_id,
        flow_id,
    })
}

pub(super) fn build_retry_current_device_trust_discovery_command(
    request_id: RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::RetryCurrentDeviceTrustDiscovery { request_id })
}

pub(super) fn build_start_session_bootstrap_command(
    request_id: RequestId,
    flow_id: u64,
    passphrase: Option<AuthSecret>,
    recovery_key_destination_path: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::StartSessionBootstrap {
        request_id,
        flow_id,
        auth: None,
        request: SecureBackupSetupRequest {
            passphrase,
            recovery_key_destination_path: Some(PathBuf::from(recovery_key_destination_path)),
            explicit_reenable_confirmed: false,
        },
    })
}

pub(super) fn build_confirm_session_bootstrap_saved_command(
    request_id: RequestId,
    flow_id: u64,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ConfirmSessionBootstrapSaved {
        request_id,
        flow_id,
    })
}

pub(super) fn build_confirm_sas_verification_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ConfirmSasVerification {
        request_id,
        flow_id,
    })
}

pub(super) fn build_cancel_verification_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
    reason: VerificationCancelReason,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::CancelVerification {
        request_id,
        flow_id,
        reason,
    })
}

pub(super) fn build_reset_identity_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ResetIdentity { request_id })
}

pub(super) fn build_cancel_identity_reset_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::CancelIdentityReset {
        request_id,
        flow_id,
    })
}

pub(super) fn build_submit_identity_reset_password_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
    password: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SubmitIdentityResetAuth {
        request_id,
        flow_id,
        request: IdentityResetAuthRequest::UiaaPassword { password },
    })
}

pub(super) fn build_submit_identity_reset_oauth_command(
    request_id: koushi_core::RequestId,
    flow_id: u64,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SubmitIdentityResetAuth {
        request_id,
        flow_id,
        request: IdentityResetAuthRequest::OAuthApproved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e2ee_trust_commands_route_to_account_state_machine() {
        match build_bootstrap_cross_signing_command(
            fake_request_id(25),
            Some(AuthSecret::new("cross-signing-password")),
        ) {
            CoreCommand::Account(AccountCommand::BootstrapCrossSigning { request_id, auth }) => {
                assert_eq!(request_id, fake_request_id(25));
                assert_eq!(
                    auth.expect("auth secret").expose_secret(),
                    "cross-signing-password"
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_enable_key_backup_command(fake_request_id(26)) {
            CoreCommand::Account(AccountCommand::EnableKeyBackup {
                request_id,
                passphrase,
            }) => {
                assert_eq!(request_id, fake_request_id(26));
                assert!(passphrase.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_accept_verification_command(fake_request_id(27), 72) {
            CoreCommand::Account(AccountCommand::AcceptVerification {
                request_id,
                flow_id,
            }) => {
                assert_eq!(request_id, fake_request_id(27));
                assert_eq!(flow_id, 72);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_confirm_sas_verification_command(fake_request_id(28), 73) {
            CoreCommand::Account(AccountCommand::ConfirmSasVerification {
                request_id,
                flow_id,
            }) => {
                assert_eq!(request_id, fake_request_id(28));
                assert_eq!(flow_id, 73);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_cancel_verification_command(
            fake_request_id(29),
            74,
            VerificationCancelReason::User,
        ) {
            CoreCommand::Account(AccountCommand::CancelVerification {
                request_id,
                flow_id,
                reason,
            }) => {
                assert_eq!(request_id, fake_request_id(29));
                assert_eq!(flow_id, 74);
                assert_eq!(reason, VerificationCancelReason::User);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_reset_identity_command(fake_request_id(30)) {
            CoreCommand::Account(AccountCommand::ResetIdentity { request_id }) => {
                assert_eq!(request_id, fake_request_id(30));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match build_cancel_identity_reset_command(fake_request_id(31), 75) {
            CoreCommand::Account(AccountCommand::CancelIdentityReset {
                request_id,
                flow_id,
            }) => {
                assert_eq!(request_id, fake_request_id(31));
                assert_eq!(flow_id, 75);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let password_command = build_submit_identity_reset_password_command(
            fake_request_id(32),
            76,
            AuthSecret::new("identity-reset-password"),
        );
        match &password_command {
            CoreCommand::Account(AccountCommand::SubmitIdentityResetAuth {
                request_id,
                flow_id,
                request: IdentityResetAuthRequest::UiaaPassword { password },
            }) => {
                assert_eq!(*request_id, fake_request_id(32));
                assert_eq!(*flow_id, 76);
                assert_eq!(password.expose_secret(), "identity-reset-password");
            }
            other => panic!("unexpected command: {other:?}"),
        }
        let debug = format!("{password_command:?}");
        assert!(!debug.contains("identity-reset-password"), "{debug}");

        match build_submit_identity_reset_oauth_command(fake_request_id(33), 77) {
            CoreCommand::Account(AccountCommand::SubmitIdentityResetAuth {
                request_id,
                flow_id,
                request: IdentityResetAuthRequest::OAuthApproved,
            }) => {
                assert_eq!(request_id, fake_request_id(33));
                assert_eq!(flow_id, 77);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn verification_gate_transport_builders_preserve_flow_and_redact_secrets() {
        assert!(matches!(
            super::build_start_own_user_sas_command(fake_request_id(40), 400),
            CoreCommand::Account(AccountCommand::StartOwnUserSas { flow_id: 400, .. })
        ));
        assert!(matches!(
            super::build_retry_current_device_trust_discovery_command(fake_request_id(41)),
            CoreCommand::Account(AccountCommand::RetryCurrentDeviceTrustDiscovery { .. })
        ));
        assert!(matches!(
            build_cancel_verification_command(
                fake_request_id(42),
                402,
                VerificationCancelReason::Mismatch
            ),
            CoreCommand::Account(AccountCommand::CancelVerification {
                flow_id: 402,
                reason: VerificationCancelReason::Mismatch,
                ..
            })
        ));
        let bootstrap = super::build_start_session_bootstrap_command(
            fake_request_id(43),
            403,
            Some(AuthSecret::new("private-passphrase")),
            "/private/recovery-key".to_owned(),
        );
        let debug = format!("{bootstrap:?}");
        assert!(debug.contains("StartSessionBootstrap"), "{debug}");
        assert!(!debug.contains("private-passphrase"), "{debug}");
        assert!(!debug.contains("/private/recovery-key"), "{debug}");
        assert!(matches!(
            super::build_confirm_session_bootstrap_saved_command(fake_request_id(44), 403),
            CoreCommand::Account(AccountCommand::ConfirmSessionBootstrapSaved { flow_id: 403, .. })
        ));
    }

    #[test]
    fn e2ee_trust_tauri_command_contracts_are_present() {
        let commands_source = commands_source();
        let lib_source = include_str!("../lib.rs");
        for (command_name, route_name, registration_name) in [
            (
                "pub async fn bootstrap_cross_signing",
                "build_bootstrap_cross_signing_command",
                "commands::e2ee::bootstrap_cross_signing",
            ),
            (
                "pub async fn enable_key_backup",
                "build_enable_key_backup_command",
                "commands::e2ee::enable_key_backup",
            ),
            (
                "pub async fn export_room_keys",
                "build_export_room_keys_command",
                "commands::e2ee::export_room_keys",
            ),
            (
                "pub async fn import_room_keys",
                "build_import_room_keys_command",
                "commands::e2ee::import_room_keys",
            ),
            (
                "pub async fn bootstrap_secure_backup",
                "build_bootstrap_secure_backup_command",
                "commands::e2ee::bootstrap_secure_backup",
            ),
            (
                "pub async fn reenable_secure_backup",
                "build_bootstrap_secure_backup_command",
                "commands::e2ee::reenable_secure_backup",
            ),
            (
                "pub async fn change_secure_backup_passphrase",
                "build_change_secure_backup_passphrase_command",
                "commands::e2ee::change_secure_backup_passphrase",
            ),
            (
                "pub async fn accept_verification",
                "build_accept_verification_command",
                "commands::e2ee::accept_verification",
            ),
            (
                "pub async fn confirm_sas_verification",
                "build_confirm_sas_verification_command",
                "commands::e2ee::confirm_sas_verification",
            ),
            (
                "pub async fn cancel_verification",
                "build_cancel_verification_command",
                "commands::e2ee::cancel_verification",
            ),
            (
                "pub async fn reset_identity",
                "build_reset_identity_command",
                "commands::e2ee::reset_identity",
            ),
            (
                "pub async fn cancel_identity_reset",
                "build_cancel_identity_reset_command",
                "commands::e2ee::cancel_identity_reset",
            ),
            (
                "pub async fn submit_identity_reset_password",
                "build_submit_identity_reset_password_command",
                "commands::e2ee::submit_identity_reset_password",
            ),
            (
                "pub async fn submit_identity_reset_oauth",
                "build_submit_identity_reset_oauth_command",
                "commands::e2ee::submit_identity_reset_oauth",
            ),
        ] {
            assert!(
                commands_source.contains(command_name),
                "Tauri command should expose {command_name}"
            );
            assert!(
                commands_source.contains(route_name),
                "Tauri command should route through {route_name}"
            );
            assert!(
                lib_source.contains(registration_name),
                "Tauri command should register {registration_name}"
            );
        }
    }
}
