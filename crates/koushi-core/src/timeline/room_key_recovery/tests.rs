use std::time::Duration;

use crate::executor;
use koushi_protocol::command::TimelineCommand;
use koushi_protocol::event::{
    CoreEvent, RoomKeyRequestStage, RoomKeyRequestStateDto, RoomKeyRequestWithheldCode,
    TimelineEvent,
};

use koushi_protocol::ids::TimelineKey;

use koushi_diagnostics::DiagnosticValue;

use super::super::diagnostics::{
    decrypt_retry_backup_result_for_error, record_decrypt_retry_backup_lookup,
    record_decrypt_retry_device_request, record_decrypt_retry_request,
    record_decrypt_retry_settled,
};
use super::super::item_projection::{
    key_request_stage_token, key_request_withheld_code_token, withheld_update_should_publish,
};
use super::{
    DecryptRetryBackupResult, DecryptRetryBackupState, DecryptRetryController,
    DecryptRetryDeviceResult, DecryptRetryFailure, DecryptRetryReason, DecryptRetrySettledResult,
    decrypt_retry_backup_state_for, decrypt_retry_settlement_operation,
    next_decrypt_retry_operation,
};

#[test]
fn decrypt_retry_diagnostics_are_fixed_token_and_private_data_free() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    let operation = 48_217;

    record_decrypt_retry_request(
        operation,
        1,
        DecryptRetryReason::MissingRoomKey,
        DecryptRetryBackupState::Available,
        Duration::ZERO,
    );
    record_decrypt_retry_backup_lookup(operation, DecryptRetryBackupResult::Found, Duration::ZERO);
    record_decrypt_retry_device_request(
        operation,
        DecryptRetryDeviceResult::Failed,
        Some(DecryptRetryFailure::Forbidden),
        Duration::ZERO,
    );
    record_decrypt_retry_settled(
        operation,
        DecryptRetrySettledResult::StillMissing,
        Duration::ZERO,
    );

    let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
    let records = diagnostics
        .records
        .iter()
        .filter(|record| {
            record.event.source == "core.decrypt_retry"
                && record.event.fields.iter().any(|field| {
                    field.key == "operation"
                        && field.value == DiagnosticValue::Correlation(operation)
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .map(|record| (record.event.stage, &record.event.fields))
            .collect::<Vec<_>>(),
        vec![
            ("request", &records[0].event.fields),
            ("backup_lookup", &records[1].event.fields),
            ("device_request", &records[2].event.fields),
            ("settled", &records[3].event.fields),
        ]
    );
    for record in &records {
        assert_eq!(record.event.source, "core.decrypt_retry");
        assert!(record.event.fields.iter().any(|field| {
            field.key == "operation" && field.value == DiagnosticValue::Correlation(operation)
        }));
    }
    assert!(records[0].event.fields.iter().any(|field| {
        field.key == "reason" && field.value == DiagnosticValue::Token("missing_room_key")
    }));
    assert!(
        records[1].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("found")
        })
    );
    assert!(
        records[2].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("failed")
        })
    );
    assert!(records[2].event.fields.iter().any(|field| {
        field.key == "failure" && field.value == DiagnosticValue::Token("forbidden")
    }));
    assert!(records[3].event.fields.iter().any(|field| {
        field.key == "result" && field.value == DiagnosticValue::Token("still_missing")
    }));

    let serialized = serde_json::to_string(&records).expect("serialize diagnostics");
    for forbidden in [
        "!synthetic-room:example.invalid",
        "$synthetic-event:example.invalid",
        "@synthetic-user:example.invalid",
        "SYNTHETICDEVICE",
        "synthetic-session-id",
        "synthetic message body",
        "https://private.example.invalid",
        "/Users/member/private/store",
        "private-token",
        "recovery-key",
        "backup-version",
        "raw SDK error",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "diagnostic leaked {forbidden}"
        );
    }
}

#[test]
fn decrypt_retry_controller_fences_deadline_settlement_and_replacement() {
    let mut controller = DecryptRetryController::default();
    let admitted_at = executor::Instant::now();
    let (first, replaced, coalesced) = controller.admit("$event-a:test", 7, admitted_at);
    assert!(replaced.is_none());
    assert!(!coalesced);
    assert!(first.deadline > admitted_at);
    assert!(controller.is_current(first.operation, 7));
    let (same, replaced, coalesced) =
        controller.admit("$event-a:test", 7, executor::Instant::now());
    assert!(coalesced);
    assert!(replaced.is_none());
    assert_eq!(same.operation, first.operation);

    assert!(
        controller
            .settle_if_current(first.operation, 8, DecryptRetrySettledResult::Decrypted)
            .is_none()
    );
    assert!(
        controller
            .settle_if_current(
                first.operation.wrapping_add(1),
                7,
                DecryptRetrySettledResult::Timeout
            )
            .is_none()
    );
    assert!(controller.is_current(first.operation, 7));

    let (second, replaced, coalesced) =
        controller.admit("$event-b:test", 7, executor::Instant::now());
    assert!(!coalesced);
    assert_eq!(
        replaced.map(|pending| pending.operation),
        Some(first.operation)
    );
    assert!(!controller.is_current(first.operation, 7));
    assert!(controller.is_current(second.operation, 7));

    assert!(
        controller
            .settle_if_current(second.operation, 8, DecryptRetrySettledResult::Decrypted)
            .is_none()
    );
    let settled = controller
        .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Decrypted)
        .expect("current operation settles exactly once");
    assert_eq!(settled.pending.operation, second.operation);
    assert!(matches!(
        settled.result,
        DecryptRetrySettledResult::Decrypted
    ));
    assert!(!controller.is_current(second.operation, 7));
    assert!(
        controller
            .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Timeout)
            .is_none()
    );
}

#[test]
fn room_key_request_state_tokens_are_closed_and_serde_stable() {
    // Every internal stage literal maps to a closed wire token, and the
    // DTO serializes with the exact tokens the TypeScript union declares.
    let stage_cases = [
        ("sent", "sent"),
        ("automatic", "automatic"),
        ("still_waiting", "still_waiting"),
        ("withheld", "withheld"),
        ("decryption_recovered", "decryption_recovered"),
        ("send_failed", "send_failed"),
    ];
    for (literal, wire) in stage_cases {
        let serialized = serde_json::to_string(&key_request_stage_token(literal)).unwrap();
        assert_eq!(serialized, format!("\"{wire}\""));
    }
    let code_cases = [
        ("blacklisted", "blacklisted"),
        ("unverified", "unverified"),
        ("unauthorised", "unauthorised"),
        ("unavailable", "unavailable"),
    ];
    for (literal, wire) in code_cases {
        let serialized = serde_json::to_string(&key_request_withheld_code_token(literal)).unwrap();
        assert_eq!(serialized, format!("\"{wire}\""));
    }
    // Unknown / custom codes carry no specific copy: they map to None.
    assert!(key_request_withheld_code_token("custom").is_none());
    let dto = RoomKeyRequestStateDto {
        stage: key_request_stage_token("withheld"),
        withheld_code: key_request_withheld_code_token("unavailable"),
    };
    assert_eq!(
        serde_json::to_string(&dto).unwrap(),
        "{\"stage\":\"withheld\",\"withheldCode\":\"unavailable\"}"
    );
}

#[test]
fn withheld_update_guard_allows_typed_code_and_never_regresses_terminal_stages() {
    // Stage settled withheld by a diff without a code still gains it.
    assert!(withheld_update_should_publish(
        "withheld",
        None,
        "unavailable"
    ));
    // A different typed code replaces the previous one.
    assert!(withheld_update_should_publish(
        "withheld",
        Some("unverified"),
        "blacklisted"
    ));
    // Duplicate observation of the same code is idempotent.
    assert!(!withheld_update_should_publish(
        "withheld",
        Some("unavailable"),
        "unavailable"
    ));
    // Non-withheld pending stages accept the refusal.
    assert!(withheld_update_should_publish("sent", None, "unavailable"));
    assert!(withheld_update_should_publish(
        "still_waiting",
        None,
        "unavailable"
    ));
    // Terminal stages are never regressed by a late observation.
    assert!(!withheld_update_should_publish(
        "decryption_recovered",
        None,
        "unavailable"
    ));
    assert!(!withheld_update_should_publish(
        "send_failed",
        None,
        "unavailable"
    ));
}

#[test]
fn room_key_request_state_changed_debug_redacts_identifiers() {
    let event = CoreEvent::Room(
        koushi_protocol::event::RoomEvent::RoomKeyRequestStateChanged {
            key: TimelineKey::room(
                koushi_protocol::ids::AccountKey("@secret-account:example.invalid".to_owned()),
                "!secret-room:example.invalid",
            ),
            event_id: "$secret-event:example.invalid".to_owned(),
            request_id: None,
            stage: RoomKeyRequestStage::Withheld,
            withheld_code: Some(RoomKeyRequestWithheldCode::Unverified),
        },
    );
    let rendered = format!("{event:?}");
    assert!(!rendered.contains("secret-account"));
    assert!(!rendered.contains("secret-room"));
    assert!(!rendered.contains("secret-event"));
    assert!(rendered.contains("withheld"));
}

#[test]
fn decrypt_retry_diff_settlement_requires_current_generation_and_matching_event() {
    let mut controller = DecryptRetryController::default();
    let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 8, "$event:test"),
        None
    );
    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 7, "$other:test"),
        None
    );
    assert_eq!(
        decrypt_retry_settlement_operation(&controller, 7, "$event:test"),
        Some(pending.operation)
    );
}

#[test]
fn decrypt_retry_timeout_message_settles_current_operation_once() {
    let mut controller = DecryptRetryController::default();
    let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

    let settled = controller
        .settle_timeout_if_current(pending.operation, 7)
        .expect("current timeout settles");
    assert!(matches!(settled.result, DecryptRetrySettledResult::Timeout));
    assert!(
        controller
            .settle_timeout_if_current(pending.operation, 7)
            .is_none()
    );
}

#[test]
fn decrypt_retry_backup_state_only_reports_available_for_ready_local_recovery() {
    assert_eq!(
        decrypt_retry_backup_state_for(
            koushi_sdk::MatrixSecureBackupLocalState::Enabled,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        )
        .token(),
        "available"
    );
    for state in [
        (
            koushi_sdk::MatrixSecureBackupLocalState::Unknown,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        ),
        (
            koushi_sdk::MatrixSecureBackupLocalState::Enabled,
            koushi_sdk::MatrixSecureBackupRecoveryState::Unknown,
        ),
        (
            koushi_sdk::MatrixSecureBackupLocalState::Downloading,
            koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
        ),
    ] {
        assert_eq!(
            decrypt_retry_backup_state_for(state.0, state.1).token(),
            "unknown"
        );
    }
}

#[test]
fn decrypt_retry_operation_sequence_is_process_wide_and_monotonic() {
    let first = next_decrypt_retry_operation();
    let second = next_decrypt_retry_operation();
    assert!(second > first);
}

#[test]
fn decrypt_retry_backup_failures_keep_typed_private_kinds() {
    for (kind, expected) in [
        (
            koushi_sdk::E2eeTrustFailureKind::Network,
            DecryptRetryBackupResult::Network,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Forbidden,
            DecryptRetryBackupResult::Forbidden,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::InvalidBackup,
            DecryptRetryBackupResult::InvalidBackup,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Timeout,
            DecryptRetryBackupResult::Timeout,
        ),
        (
            koushi_sdk::E2eeTrustFailureKind::Sdk,
            DecryptRetryBackupResult::Sdk,
        ),
    ] {
        assert!(matches!(
            decrypt_retry_backup_result_for_error(&koushi_sdk::E2eeTrustError::Classified(
                kind
            )),
            result if result.token() == expected.token()
        ));
    }
}

#[test]
fn decrypt_retry_diagnostics_use_only_the_planned_outcome_tokens() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    let operation = 48_218;

    record_decrypt_retry_request(
        operation,
        2,
        DecryptRetryReason::MissingRoomKey,
        DecryptRetryBackupState::Available,
        Duration::ZERO,
    );
    for result in [
        DecryptRetryBackupResult::Found,
        DecryptRetryBackupResult::NotFound,
        DecryptRetryBackupResult::Network,
        DecryptRetryBackupResult::Forbidden,
        DecryptRetryBackupResult::InvalidBackup,
        DecryptRetryBackupResult::Timeout,
        DecryptRetryBackupResult::Sdk,
    ] {
        record_decrypt_retry_backup_lookup(operation, result, Duration::ZERO);
    }
    record_decrypt_retry_device_request(
        operation,
        DecryptRetryDeviceResult::Sent,
        None,
        Duration::ZERO,
    );
    for failure in [
        DecryptRetryFailure::Network,
        DecryptRetryFailure::Forbidden,
        DecryptRetryFailure::Timeout,
        DecryptRetryFailure::Sdk,
    ] {
        record_decrypt_retry_device_request(
            operation,
            DecryptRetryDeviceResult::Failed,
            Some(failure),
            Duration::ZERO,
        );
    }
    for result in [
        DecryptRetrySettledResult::Decrypted,
        DecryptRetrySettledResult::StillMissing,
        DecryptRetrySettledResult::Withheld,
        DecryptRetrySettledResult::Malformed,
        DecryptRetrySettledResult::Timeout,
        DecryptRetrySettledResult::Superseded,
    ] {
        record_decrypt_retry_settled(operation, result, Duration::ZERO);
    }

    let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
    let tokens = diagnostics
        .records
        .iter()
        .filter(|record| {
            record.event.source == "core.decrypt_retry"
                && record.event.fields.iter().any(|field| {
                    field.key == "operation"
                        && field.value == DiagnosticValue::Correlation(operation)
                })
        })
        .flat_map(|record| record.event.fields.iter())
        .filter_map(|field| match field.value {
            DiagnosticValue::Token(token) => Some((field.key, token)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [
        ("backup_state", "available"),
        ("result", "found"),
        ("result", "not_found"),
        ("result", "network"),
        ("result", "forbidden"),
        ("result", "invalid_backup"),
        ("result", "timeout"),
        ("result", "sdk"),
        ("failure", "network"),
        ("failure", "forbidden"),
        ("failure", "timeout"),
        ("failure", "sdk"),
        ("result", "decrypted"),
        ("result", "still_missing"),
        ("result", "withheld"),
        ("result", "malformed"),
        ("result", "superseded"),
    ] {
        assert!(
            tokens.contains(&expected),
            "missing fixed token {expected:?}"
        );
    }
}
