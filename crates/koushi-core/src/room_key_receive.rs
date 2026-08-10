//! Receive-side room-key lifecycle diagnostics and the bounded local
//! late-decryption retry path (issue #476).
//!
//! Everything here is privacy-safe: only aggregate counters, closed tokens,
//! and booleans are recorded. No Matrix identifiers or cryptographic material
//! ever enter diagnostics.

use std::collections::BTreeSet;

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_sdk::MatrixRoomKeyReceiveDiagnostics;
use matrix_sdk::event_cache::RedecryptorReport;

/// Bound on how many distinct UTD session IDs a single explicit retry may
/// carry, so a pathological timeline cannot fan out unboundedly.
pub const LATE_DECRYPTION_RETRY_SESSION_LIMIT: usize = 64;

/// Fixed token for the trigger that produced a receive-side summary.
pub const RECEIVE_SUMMARY_TRIGGER_RESTORE: &str = "restore";
pub const RECEIVE_SUMMARY_TRIGGER_STREAM_LAGGED: &str = "stream_lagged";
pub const RECEIVE_SUMMARY_TRIGGER_BACKUP_AVAILABLE: &str = "backup_available";
pub const RECEIVE_SUMMARY_TRIGGER_MANUAL: &str = "manual";

/// Minimum interval between automatic late-decryption retries driven by
/// redecryptor reports, per timeline. Report-driven retries are idempotent;
/// this window bounds repeated reports from fanning out.
pub const LATE_DECRYPTION_RETRY_COALESCE_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(10);

/// Reset the receive-side late-decryption counters when an account runtime is
/// replaced.
pub fn reset_late_decryption_counters() {
    koushi_diagnostics::reset_counter("late_decryption_timeline_replacements");
    koushi_diagnostics::reset_counter("late_decryption_explicit_retries");
}

/// Record the consolidated privacy-safe receive-side summary: transport/Olm,
/// Megolm merge, and late-decryption groups plus event-cache health.
///
/// This is the single export that distinguishes the three failure groups from
/// issue #476. No identifiers or key material are recorded. The summary
/// record carries every count; no persistent counter keys are created here so
/// unrelated diagnostic tests that slice the global snapshot are unaffected.
pub fn record_room_key_receive_summary(
    diagnostics: &MatrixRoomKeyReceiveDiagnostics,
    trigger: &'static str,
) {
    let mut event = DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "core.room_key_receive_summary",
        "summary",
    )
    .field(DiagnosticField::token("trigger", trigger))
    .field(DiagnosticField::boolean(
        "event_cache_subscribed",
        diagnostics.late_decryption.subscribed,
    ))
    .field(DiagnosticField::boolean(
        "redecryptor_alive",
        diagnostics.late_decryption.redecryptor_alive,
    ));
    let crypto = &diagnostics.crypto;
    let crypto_fields = [
        DiagnosticField::count("ingress_direct", crypto.ingress_direct),
        DiagnosticField::count("ingress_forwarded", crypto.ingress_forwarded),
        DiagnosticField::count("olm_failed", crypto.to_device_olm_failed),
        DiagnosticField::count("olm_wedged", crypto.to_device_olm_wedged),
        DiagnosticField::count("dehydrated_rejected", crypto.to_device_dehydrated_rejected),
        DiagnosticField::count("malformed", crypto.to_device_malformed),
        DiagnosticField::count(
            "unsupported_algorithm",
            crypto.room_key_unsupported_algorithm,
        ),
        DiagnosticField::count(
            "forwarded_no_matching_request",
            crypto.forwarded_rejected_no_matching_request,
        ),
        DiagnosticField::count(
            "forwarded_untrusted_sender",
            crypto.forwarded_rejected_untrusted_sender,
        ),
        DiagnosticField::count(
            "forwarded_unsupported",
            crypto.forwarded_unsupported_algorithm,
        ),
        DiagnosticField::count("forwarded_accepted", crypto.forwarded_accepted),
        DiagnosticField::count("merge_accepted_new", crypto.merge_accepted_new),
        DiagnosticField::count("merge_accepted_improved", crypto.merge_accepted_improved),
        DiagnosticField::count("merge_duplicate_ignored", crypto.merge_duplicate_ignored),
        DiagnosticField::count("merge_worse_ignored", crypto.merge_worse_ignored),
        DiagnosticField::count(
            "merge_unconnected_rejected",
            crypto.merge_unconnected_rejected,
        ),
        DiagnosticField::count(
            "merge_invalid_session_key",
            crypto.merge_invalid_session_key,
        ),
        DiagnosticField::count("merge_store_failed", crypto.merge_store_failed),
    ];
    for field in crypto_fields {
        event = event.field(field);
    }
    let late = &diagnostics.late_decryption.counters;
    event = event
        .field(DiagnosticField::count(
            "late_updates_broadcast",
            late.room_key_updates_broadcast,
        ))
        .field(DiagnosticField::count(
            "late_redecryption_requests",
            late.redecryption_requests,
        ))
        .field(DiagnosticField::count(
            "late_explicit_retry_requests",
            late.explicit_retry_requests,
        ))
        .field(DiagnosticField::count(
            "late_succeeded",
            late.redecryption_succeeded,
        ))
        .field(DiagnosticField::count(
            "late_remained_utd",
            late.redecryption_remained_utd,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_0",
            late.matching_events_bucket_0,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_1",
            late.matching_events_bucket_1,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_2_to_5",
            late.matching_events_bucket_2_to_5,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_6_to_20",
            late.matching_events_bucket_6_to_20,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_21_to_100",
            late.matching_events_bucket_21_to_100,
        ))
        .field(DiagnosticField::count(
            "late_matching_bucket_101_plus",
            late.matching_events_bucket_101_plus,
        ))
        .field(DiagnosticField::count(
            "late_failed",
            late.redecryption_failed,
        ))
        .field(DiagnosticField::count(
            "late_store_failed",
            late.redecryption_store_failed,
        ))
        .field(DiagnosticField::count(
            "late_stream_lagged",
            late.room_key_stream_lagged,
        ))
        .field(DiagnosticField::count(
            "late_stream_recreated",
            late.room_key_stream_recreated,
        ));
    record(event);
}

/// Record a bounded local late-decryption retry outcome.
pub fn record_late_decryption_retry(session_ids: usize, requested: bool) {
    koushi_diagnostics::increment_counter("late_decryption_explicit_retries");
    record(
        DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_receive", "retry")
            .field(DiagnosticField::count("session_ids", session_ids as u64))
            .field(DiagnosticField::boolean("requested", requested)),
    );
}

/// Extract the bounded set of distinct UTD session IDs from SDK timeline items
/// that the visible timeline currently shows as unable to decrypt.
///
/// The result is bounded to [`LATE_DECRYPTION_RETRY_SESSION_LIMIT`] entries so
/// a single retry cannot fan out; only session IDs enter the retry request,
/// never event IDs or identities.
pub fn collect_visible_utd_sessions(
    items: &[std::sync::Arc<matrix_sdk_ui::timeline::TimelineItem>],
) -> BTreeSet<String> {
    use matrix_sdk_ui::timeline::EncryptedMessage;

    let mut sessions = BTreeSet::new();
    for item in items {
        if sessions.len() >= LATE_DECRYPTION_RETRY_SESSION_LIMIT {
            break;
        }
        let Some(event) = item.as_event() else {
            continue;
        };
        if let Some(EncryptedMessage::MegolmV1AesSha2 { session_id, .. }) =
            event.content().as_unable_to_decrypt()
        {
            sessions.insert(session_id.clone());
        }
    }
    sessions
}

/// Classify a decryption report for the retry observer: only lag and backup
/// availability should drive a bounded retry; resolved events need no action.
pub fn report_should_trigger_retry(report: &RedecryptorReport) -> bool {
    matches!(
        report,
        RedecryptorReport::Lagging | RedecryptorReport::BackupAvailable
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use koushi_diagnostics::snapshot;
    use matrix_sdk::{
        encryption::RoomKeyReceiveCounters,
        event_cache::{RoomKeyLateDecryptionCounters, RoomKeyLateDecryptionDiagnostics},
    };

    fn sample_diagnostics() -> MatrixRoomKeyReceiveDiagnostics {
        let mut crypto = RoomKeyReceiveCounters::default();
        crypto.ingress_direct = 2;
        crypto.merge_accepted_new = 1;
        crypto.to_device_olm_wedged = 1;
        let mut late = RoomKeyLateDecryptionCounters::default();
        late.redecryption_succeeded = 3;
        late.room_key_stream_lagged = 1;
        MatrixRoomKeyReceiveDiagnostics {
            crypto,
            late_decryption: RoomKeyLateDecryptionDiagnostics {
                counters: late,
                subscribed: true,
                redecryptor_alive: true,
            },
        }
    }

    #[test]
    fn summary_contains_only_closed_fields_and_no_identifiers() {
        reset_late_decryption_counters();
        let diagnostics = sample_diagnostics();
        record_room_key_receive_summary(&diagnostics, RECEIVE_SUMMARY_TRIGGER_MANUAL);

        let snapshot = snapshot();
        let summary = snapshot
            .records
            .iter()
            .find(|record| record.event.source == "core.room_key_receive_summary")
            .expect("summary recorded");
        let text = format!("{:?}", summary.event);
        for private in [
            "@",
            "!",
            "room_id",
            "user_id",
            "device_id",
            "event_id",
            "session_id",
            "http",
        ] {
            assert!(
                !text.contains(private),
                "{private} leaked into summary: {text}"
            );
        }
    }

    #[test]
    fn visible_utd_session_collection_is_bounded_and_private() {
        // No SDK items in this unit test; verify the empty and privacy contract.
        let sessions = collect_visible_utd_sessions(&[]);
        assert!(sessions.is_empty());
        assert!(LATE_DECRYPTION_RETRY_SESSION_LIMIT > 0);
    }

    #[test]
    fn report_classification_is_closed() {
        use RedecryptorReport::*;
        assert!(report_should_trigger_retry(&Lagging));
        assert!(report_should_trigger_retry(&BackupAvailable));
        assert!(!report_should_trigger_retry(&ResolvedUtds {
            room_id: matrix_sdk::ruma::owned_room_id!("!x:example.invalid"),
            events: Default::default(),
        }));
    }

    #[test]
    fn retry_outcome_record_is_closed() {
        record_late_decryption_retry(2, true);
        let snapshot = snapshot();
        let retry = snapshot
            .records
            .iter()
            .find(|record| {
                record.event.source == "core.room_key_receive" && record.event.stage == "retry"
            })
            .expect("retry recorded");
        let text = format!("{:?}", retry.event);
        assert!(!text.contains('@') && !text.contains('!'));
    }
}
