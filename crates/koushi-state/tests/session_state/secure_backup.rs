use super::support::session_info;
use koushi_state::{
    AppAction, AppEffect, AppState, PendingKeyCountBucket, SecureBackupGateFailureKind,
    SecureBackupGateState, SessionState, UiEvent, encrypted_messaging_is_admitted, reduce,
};

#[test]
fn secure_backup_gate_is_closed_until_authoritative_ready_and_can_degrade() {
    let mut state = AppState {
        session: SessionState::Ready(session_info()),
        secure_backup_gate: SecureBackupGateState::Checking,
        ..AppState::default()
    };

    let effects = reduce(
        &mut state,
        AppAction::SecureBackupGateChanged(SecureBackupGateState::ExistingBackupNeedsRecovery {
            failure: None,
        }),
    );
    assert!(effects.contains(&AppEffect::EmitUiEvent(UiEvent::SessionChanged)));
    assert!(matches!(
        state.secure_backup_gate,
        SecureBackupGateState::ExistingBackupNeedsRecovery { failure: None }
    ));

    reduce(
        &mut state,
        AppAction::SecureBackupGateChanged(SecureBackupGateState::Ready),
    );
    assert_eq!(state.secure_backup_gate, SecureBackupGateState::Ready);

    reduce(
        &mut state,
        AppAction::SecureBackupGateChanged(SecureBackupGateState::DegradedRetrying {
            failure: SecureBackupGateFailureKind::Network,
        }),
    );
    assert!(matches!(
        state.secure_backup_gate,
        SecureBackupGateState::DegradedRetrying {
            failure: SecureBackupGateFailureKind::Network
        }
    ));
    assert_eq!(state.session, SessionState::Ready(session_info()));
}

#[test]
fn signed_out_state_ignores_secure_backup_updates() {
    let mut state = AppState::default();
    let before = state.clone();

    let effects = reduce(
        &mut state,
        AppAction::SecureBackupGateChanged(SecureBackupGateState::Ready),
    );

    assert!(effects.is_empty());
    assert_eq!(state, before);
}

#[test]
fn every_non_ready_backup_state_keeps_combined_encrypted_admission_closed() {
    let non_ready = vec![
        SecureBackupGateState::Inactive,
        SecureBackupGateState::Checking,
        SecureBackupGateState::ExistingBackupNeedsRecovery { failure: None },
        SecureBackupGateState::SecureStorageIncomplete,
        SecureBackupGateState::SetupRequired,
        SecureBackupGateState::ExplicitlyDisabledRequiresSetup,
        SecureBackupGateState::CreatingBackup,
        SecureBackupGateState::RecoveryKeyDeliveryRequired,
        SecureBackupGateState::UploadingExistingKeys {
            pending: koushi_state::PendingKeyCountBucket::TwoToTen,
        },
        SecureBackupGateState::DegradedRetrying {
            failure: SecureBackupGateFailureKind::Network,
        },
        SecureBackupGateState::BlockedFailed {
            failure: SecureBackupGateFailureKind::Sdk,
        },
    ];
    for gate in non_ready {
        let state = AppState {
            session: SessionState::Ready(session_info()),
            secure_backup_gate: gate.clone(),
            ..AppState::default()
        };
        assert!(
            !encrypted_messaging_is_admitted(&state),
            "non-ready backup state admitted encrypted sending: {gate:?}"
        );
    }

    let ready = AppState {
        session: SessionState::Ready(session_info()),
        secure_backup_gate: SecureBackupGateState::Ready,
        ..AppState::default()
    };
    assert!(encrypted_messaging_is_admitted(&ready));

    let unverified = AppState {
        session: SessionState::Locked(session_info()),
        secure_backup_gate: SecureBackupGateState::Ready,
        ..AppState::default()
    };
    assert!(!encrypted_messaging_is_admitted(&unverified));
}

#[test]
fn duplicate_ready_is_quiet_and_degradation_preserves_a_nonempty_draft() {
    let mut state = AppState {
        session: SessionState::Ready(session_info()),
        secure_backup_gate: SecureBackupGateState::Ready,
        ..AppState::default()
    };
    state
        .composer_drafts
        .set_room_draft("!synthetic:example.invalid".to_owned(), "unsent draft");
    let draft_before = state.composer_drafts.clone();

    assert!(
        reduce(
            &mut state,
            AppAction::SecureBackupGateChanged(SecureBackupGateState::Ready),
        )
        .is_empty()
    );
    reduce(
        &mut state,
        AppAction::SecureBackupGateChanged(SecureBackupGateState::DegradedRetrying {
            failure: SecureBackupGateFailureKind::RateLimited,
        }),
    );
    assert_eq!(state.composer_drafts, draft_before);
}

#[test]
fn secure_backup_gate_wire_is_closed_privacy_safe_and_legacy_defaults_inactive() {
    let cases = vec![
        (SecureBackupGateState::Checking, "checking"),
        (
            SecureBackupGateState::ExistingBackupNeedsRecovery { failure: None },
            "existingBackupNeedsRecovery",
        ),
        (
            SecureBackupGateState::ExistingBackupNeedsRecovery {
                failure: Some(SecureBackupGateFailureKind::InvalidRecoveryKey),
            },
            "existingBackupNeedsRecovery",
        ),
        (
            SecureBackupGateState::SecureStorageIncomplete,
            "secureStorageIncomplete",
        ),
        (SecureBackupGateState::SetupRequired, "setupRequired"),
        (
            SecureBackupGateState::ExplicitlyDisabledRequiresSetup,
            "explicitlyDisabledRequiresSetup",
        ),
        (SecureBackupGateState::CreatingBackup, "creatingBackup"),
        (
            SecureBackupGateState::RecoveryKeyDeliveryRequired,
            "recoveryKeyDeliveryRequired",
        ),
        (
            SecureBackupGateState::UploadingExistingKeys {
                pending: PendingKeyCountBucket::TwoToTen,
            },
            "uploadingExistingKeys",
        ),
        (
            SecureBackupGateState::DegradedRetrying {
                failure: SecureBackupGateFailureKind::Network,
            },
            "degradedRetrying",
        ),
        (
            SecureBackupGateState::BlockedFailed {
                failure: SecureBackupGateFailureKind::Forbidden,
            },
            "blockedFailed",
        ),
        (SecureBackupGateState::Ready, "ready"),
    ];
    for (gate, kind) in cases {
        let value = serde_json::to_value(&gate).expect("gate serializes");
        assert_eq!(
            value.get("kind").and_then(serde_json::Value::as_str),
            Some(kind)
        );
        let restored: SecureBackupGateState =
            serde_json::from_value(value).expect("gate round trips");
        assert_eq!(restored, gate);
    }

    let state = AppState {
        session: SessionState::Ready(session_info()),
        secure_backup_gate: SecureBackupGateState::ExistingBackupNeedsRecovery {
            failure: Some(SecureBackupGateFailureKind::InvalidRecoveryKey),
        },
        ..AppState::default()
    };
    let mut legacy = serde_json::to_value(&state).expect("state serializes");
    legacy
        .as_object_mut()
        .expect("state object")
        .remove("secure_backup_gate");
    let restored: AppState = serde_json::from_value(legacy).expect("legacy state restores");
    assert_eq!(restored.secure_backup_gate, SecureBackupGateState::Inactive);

    let serialized = serde_json::to_string(&state.secure_backup_gate).unwrap();
    let debug = format!("{:?}", state.secure_backup_gate);
    for private in [
        "EsT1 RcVy KeyM ater",
        "backup-version-1",
        "!room:example.invalid",
        "raw sdk error",
    ] {
        assert!(!serialized.contains(private));
        assert!(!debug.contains(private));
    }
}
