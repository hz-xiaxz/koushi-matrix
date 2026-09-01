use super::{account_command_projected_action, secure_backup_setup_projection_failure};
use crate::{
    AccountCommand, CoreFailure, RequestId, RuntimeConnectionId, SecureBackupSetupRequest,
};
use koushi_state::{
    AppAction, AppState, SecureBackupGateState, SecureBackupSetupIntent, SecureBackupSetupState,
    SessionInfo, SessionState,
};

fn request(intent: SecureBackupSetupIntent) -> AccountCommand {
    AccountCommand::BootstrapSecureBackup {
        request_id: RequestId {
            connection_id: RuntimeConnectionId(1),
            sequence: 7,
        },
        request: SecureBackupSetupRequest {
            passphrase: None,
            recovery_key_destination_requested: true,
            intent,
        },
    }
}

fn ready_state(gate: SecureBackupGateState) -> AppState {
    let mut state = AppState::default();
    state.session = SessionState::Ready(SessionInfo {
        homeserver: "https://server.example.invalid".to_owned(),
        user_id: "@alice:example.invalid".to_owned(),
        device_id: "DEVICE".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    });
    state.secure_backup_gate = gate;
    state
}

#[test]
fn projected_secure_backup_action_carries_the_closed_intent() {
    let command = request(SecureBackupSetupIntent::Reenable { confirmed: true });
    assert_eq!(
        account_command_projected_action(&command),
        Some(AppAction::SecureBackupSetupRequested {
            request_id: 7,
            intent: SecureBackupSetupIntent::Reenable { confirmed: true },
        })
    );
}

#[test]
fn secure_backup_projection_gate_returns_typed_private_safe_failures() {
    let mut state = ready_state(SecureBackupGateState::ExplicitlyDisabledRequiresSetup);
    assert_eq!(
        secure_backup_setup_projection_failure(
            &state,
            &request(SecureBackupSetupIntent::Reenable { confirmed: false }),
        ),
        Some(CoreFailure::SecureBackupSetupConfirmationRequired)
    );
    assert_eq!(
        secure_backup_setup_projection_failure(
            &state,
            &request(SecureBackupSetupIntent::InitialSetup),
        ),
        Some(CoreFailure::SecureBackupSetupFailedNoOp)
    );

    state.secure_backup_gate = SecureBackupGateState::SetupRequired;
    assert_eq!(
        secure_backup_setup_projection_failure(
            &state,
            &request(SecureBackupSetupIntent::Reenable { confirmed: true }),
        ),
        Some(CoreFailure::SecureBackupSetupFailedNoOp)
    );

    state.e2ee_trust.key_management.secure_backup_setup =
        SecureBackupSetupState::SettingUp { request_id: 3 };
    assert_eq!(
        secure_backup_setup_projection_failure(
            &state,
            &request(SecureBackupSetupIntent::InitialSetup),
        ),
        Some(CoreFailure::SecureBackupSetupFailedNoOp)
    );
}
