use koushi_state::{PendingKeyCountBucket, SecureBackupGateFailureKind, SecureBackupGateState};

use super::{
    E2eeTrustError, MatrixSecureBackupInspection, MatrixSecureBackupLocalState,
    MatrixSecureBackupRecoveryState, MatrixSecureBackupServerState, MatrixSecureBackupState,
    MatrixSecureBackupStateObservation, MatrixSecureBackupTrustState,
    MatrixSecureBackupUploadState, SecureBackupStateStream, classify_secure_backup_upload,
};

#[test]
fn secure_backup_upload_snapshot_classifies_without_waiting_for_settlement() {
    use matrix_sdk::encryption::backups::UploadState;
    use matrix_sdk_base::crypto::store::types::RoomKeyCounts;

    assert_eq!(
        classify_secure_backup_upload(
            Ok(RoomKeyCounts {
                total: 125,
                backed_up: 20,
            }),
            UploadState::Uploading(RoomKeyCounts {
                total: 125,
                backed_up: 20,
            }),
        ),
        MatrixSecureBackupUploadState::Pending(PendingKeyCountBucket::OverOneHundred)
    );
    assert_eq!(
        classify_secure_backup_upload(
            Ok(RoomKeyCounts {
                total: 125,
                backed_up: 125,
            }),
            UploadState::Done,
        ),
        MatrixSecureBackupUploadState::Settled
    );
    assert_eq!(
        classify_secure_backup_upload(
            Ok(RoomKeyCounts {
                total: 125,
                backed_up: 20,
            }),
            UploadState::Error,
        ),
        MatrixSecureBackupUploadState::Failed
    );
}

fn inspection(
    server: MatrixSecureBackupServerState,
    local: MatrixSecureBackupLocalState,
    recovery: MatrixSecureBackupRecoveryState,
    upload: MatrixSecureBackupUploadState,
    trust: MatrixSecureBackupTrustState,
) -> MatrixSecureBackupInspection {
    MatrixSecureBackupInspection {
        server,
        local,
        recovery,
        upload,
        trust,
        recovery_key_delivery_pending: false,
        local_cross_signing_complete: true,
    }
}

#[test]
fn secure_backup_inspection_classifies_required_cartesian_cases() {
    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Trusted,
        )
        .recommended_gate_state(),
        SecureBackupGateState::Ready
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Disabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Unknown,
            MatrixSecureBackupTrustState::Unknown,
        )
        .recommended_gate_state(),
        SecureBackupGateState::ExistingBackupNeedsRecovery { failure: None }
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Absent,
            MatrixSecureBackupLocalState::Disabled,
            MatrixSecureBackupRecoveryState::Unknown,
            MatrixSecureBackupUploadState::Unknown,
            MatrixSecureBackupTrustState::Unknown,
        )
        .recommended_gate_state(),
        SecureBackupGateState::SetupRequired
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Absent,
            MatrixSecureBackupLocalState::Disabled,
            MatrixSecureBackupRecoveryState::Disabled,
            MatrixSecureBackupUploadState::Unknown,
            MatrixSecureBackupTrustState::Unknown,
        )
        .recommended_gate_state(),
        SecureBackupGateState::ExplicitlyDisabledRequiresSetup
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Unknown,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Trusted,
        )
        .recommended_gate_state(),
        SecureBackupGateState::Checking
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Mismatch,
        )
        .recommended_gate_state(),
        SecureBackupGateState::ExistingBackupNeedsRecovery {
            failure: Some(SecureBackupGateFailureKind::BackupKeyMismatch),
        }
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Incomplete,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Trusted,
        )
        .recommended_gate_state(),
        SecureBackupGateState::SecureStorageIncomplete
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Failed,
            MatrixSecureBackupTrustState::Trusted,
        )
        .recommended_gate_state(),
        SecureBackupGateState::DegradedRetrying {
            failure: SecureBackupGateFailureKind::Network,
        }
    );
}

#[test]
fn pending_recovery_key_delivery_survives_inspection_and_keeps_gate_closed() {
    let mut inspection = inspection(
        MatrixSecureBackupServerState::Present,
        MatrixSecureBackupLocalState::Enabled,
        MatrixSecureBackupRecoveryState::Enabled,
        MatrixSecureBackupUploadState::Settled,
        MatrixSecureBackupTrustState::Trusted,
    );
    inspection.recovery_key_delivery_pending = true;

    assert_eq!(
        inspection.recommended_gate_state(),
        koushi_state::SecureBackupGateState::RecoveryKeyDeliveryRequired
    );
}

#[test]
fn secure_backup_inspection_requires_typed_trust_evidence() {
    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Unknown,
        )
        .recommended_gate_state(),
        SecureBackupGateState::Checking
    );

    assert_eq!(
        inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Settled,
            MatrixSecureBackupTrustState::Mismatch,
        )
        .recommended_gate_state(),
        SecureBackupGateState::ExistingBackupNeedsRecovery {
            failure: Some(SecureBackupGateFailureKind::BackupKeyMismatch),
        }
    );
}

#[test]
fn secure_backup_state_observation_is_public_and_private_data_free() {
    let state = MatrixSecureBackupState {
        backup: MatrixSecureBackupLocalState::Enabled,
        recovery: MatrixSecureBackupRecoveryState::Enabled,
    };
    let serialized = serde_json::to_string(&state).expect("state is serializable");
    let debug = format!("{state:?}");

    assert!(serialized.contains("backup"));
    assert!(serialized.contains("recovery"));
    for forbidden in [
        "backup-version-42",
        "recovery-key-secret",
        "@alice:example.invalid",
        "!room:example.invalid",
        "/tmp/recovery-key.txt",
        "raw SDK failure",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized state leaked {forbidden}"
        );
        assert!(!debug.contains(forbidden), "debug state leaked {forbidden}");
    }

    let _observation: Option<MatrixSecureBackupStateObservation> = None;
    let _stream: Option<SecureBackupStateStream> = None;
    let _observe: fn(&super::MatrixClientSession) -> MatrixSecureBackupStateObservation =
        super::MatrixClientSession::observe_secure_backup_state;
}

#[test]
fn secure_backup_inspection_has_no_secret_or_identifier_surface() {
    let inspection = inspection(
        MatrixSecureBackupServerState::Present,
        MatrixSecureBackupLocalState::Enabled,
        MatrixSecureBackupRecoveryState::Enabled,
        MatrixSecureBackupUploadState::Pending(PendingKeyCountBucket::One),
        MatrixSecureBackupTrustState::Trusted,
    );
    let serialized = serde_json::to_string(&inspection).expect("inspection is serializable");
    let debug = format!("{inspection:?}");
    for forbidden in [
        "backup-version-42",
        "recovery-key-secret",
        "@alice:example.invalid",
        "!room:example.invalid",
        "/tmp/recovery-key.txt",
        "raw SDK failure",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized inspection leaked {forbidden}"
        );
        assert!(
            !debug.contains(forbidden),
            "debug inspection leaked {forbidden}"
        );
    }
    assert!(!serialized.contains("version"));
    assert!(!debug.contains("version"));

    let error = E2eeTrustError::Sdk("raw SDK failure with a recovery-key-secret".to_owned());
    assert!(!format!("{error:?}").contains("raw SDK failure"));
    assert!(!format!("{error:?}").contains("recovery-key-secret"));
}

/// Right after a restart the persisted delivery marker may be read before the
/// local backup and secret storage settle. The lost-key reset is offered
/// (`RecoveryKeyDeliveryRequired`) only once `reset_key()` can succeed.
#[test]
fn pending_delivery_offers_the_reset_only_when_the_reset_can_succeed() {
    let settled = || {
        let mut inspection = inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            MatrixSecureBackupRecoveryState::Enabled,
            MatrixSecureBackupUploadState::Unknown,
            MatrixSecureBackupTrustState::Trusted,
        );
        inspection.recovery_key_delivery_pending = true;
        inspection
    };
    assert!(settled().recovery_key_reset_is_possible());
    assert_eq!(
        settled().recommended_gate_state(),
        SecureBackupGateState::RecoveryKeyDeliveryRequired
    );

    for local in [
        MatrixSecureBackupLocalState::Unknown,
        MatrixSecureBackupLocalState::Enabling,
        MatrixSecureBackupLocalState::Resuming,
        MatrixSecureBackupLocalState::Downloading,
    ] {
        let unsettled = MatrixSecureBackupInspection { local, ..settled() };
        assert!(!unsettled.recovery_key_reset_is_possible());
        assert_eq!(
            unsettled.recommended_gate_state(),
            SecureBackupGateState::Checking,
            "{local:?}"
        );
    }
    let untrusted = MatrixSecureBackupInspection {
        trust: MatrixSecureBackupTrustState::Unknown,
        ..settled()
    };
    assert!(!untrusted.recovery_key_reset_is_possible());
    assert_eq!(
        untrusted.recommended_gate_state(),
        SecureBackupGateState::Checking
    );
    let disabled = MatrixSecureBackupInspection {
        local: MatrixSecureBackupLocalState::Disabled,
        ..settled()
    };
    assert_eq!(
        disabled.recommended_gate_state(),
        SecureBackupGateState::ExistingBackupNeedsRecovery { failure: None }
    );
}

/// NEW-1: `enable()` created and enabled the backup but secret storage was
/// never created (network error or the app was killed). Nothing can be lost:
/// the backup key and complete cross-signing keys are local, so the lost-key
/// reset (`reset_key()`) is offered and admitted to finish the missing step.
#[test]
fn pending_delivery_without_secret_storage_offers_the_reset() {
    let interrupted = |recovery| {
        let mut inspection = inspection(
            MatrixSecureBackupServerState::Present,
            MatrixSecureBackupLocalState::Enabled,
            recovery,
            MatrixSecureBackupUploadState::Unknown,
            MatrixSecureBackupTrustState::Trusted,
        );
        inspection.recovery_key_delivery_pending = true;
        inspection
    };
    for recovery in [
        MatrixSecureBackupRecoveryState::Unknown,
        MatrixSecureBackupRecoveryState::Disabled,
    ] {
        assert!(
            interrupted(recovery).recovery_key_reset_is_possible(),
            "{recovery:?}"
        );
        assert_eq!(
            interrupted(recovery).recommended_gate_state(),
            SecureBackupGateState::RecoveryKeyDeliveryRequired,
            "{recovery:?}"
        );

        // Incomplete local cross-signing keys would be lost by a new store.
        let missing_keys = MatrixSecureBackupInspection {
            local_cross_signing_complete: false,
            ..interrupted(recovery)
        };
        assert!(
            !missing_keys.recovery_key_reset_is_possible(),
            "{recovery:?}"
        );
        assert_ne!(
            missing_keys.recommended_gate_state(),
            SecureBackupGateState::RecoveryKeyDeliveryRequired,
            "{recovery:?}"
        );
    }

    // Secret storage exists but secrets are missing locally: keep refusing.
    let incomplete = interrupted(MatrixSecureBackupRecoveryState::Incomplete);
    assert!(!incomplete.recovery_key_reset_is_possible());
    assert_eq!(
        incomplete.recommended_gate_state(),
        SecureBackupGateState::SecureStorageIncomplete
    );
}
