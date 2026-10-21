use super::*;

fn sample(stage: NavigationStage) -> NavigationDiagnostic {
    NavigationDiagnostic {
        cause: NavigationCause::SelectRoom,
        origin: NavigationOrigin::RendererKeyboard,
        stage,
        request_connection: Some(3),
        request_sequence: Some(7),
        previous_account_alias: Some(1),
        previous_room_alias: Some(2),
        requested_room_alias: Some(4),
        committed_room_alias: Some(4),
        old_room_present: Some(true),
        old_dm: Some(false),
        new_dm: Some(true),
        list_authoritative: Some(true),
        ..NavigationDiagnostic::default()
    }
}

#[test]
fn navigation_ring_evicts_oldest_and_reports_loss() {
    let ledger = NavigationDiagnosticLedger::new(256);
    for _ in 0..300 {
        ledger.record(sample(NavigationStage::Committed));
    }
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.records.len(), 256);
    assert_eq!(snapshot.evicted_records, 44);
    assert_eq!(snapshot.records.first().unwrap().sequence, 45);
    assert_eq!(snapshot.records.last().unwrap().sequence, 300);
    assert!(
        snapshot
            .records
            .windows(2)
            .all(|pair| pair[0].elapsed_ms <= pair[1].elapsed_ms)
    );
}

#[test]
fn navigation_reset_retains_monotonic_sequences_and_counts_explicit_resets() {
    let ledger = NavigationDiagnosticLedger::new(2);
    ledger.record(sample(NavigationStage::Admitted));
    ledger.reset();
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.reset_count, 1);
    assert!(snapshot.records.is_empty());
    ledger.record(sample(NavigationStage::Completed));
    assert_eq!(ledger.snapshot().records[0].sequence, 2);
}

#[test]
fn navigation_zero_capacity_tracks_every_eviction() {
    let ledger = NavigationDiagnosticLedger::new(0);
    ledger.record(sample(NavigationStage::NoOp));
    assert!(ledger.snapshot().records.is_empty());
    assert_eq!(ledger.snapshot().evicted_records, 1);
}

#[test]
fn navigation_survives_unrelated_general_diagnostic_noise_and_is_exported() {
    let _guard = test_support::lock();
    reset_navigation();
    record_navigation(sample(NavigationStage::Committed));
    let before = navigation_snapshot();
    for _ in 0..10_001 {
        record(DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "test",
            "general_noise",
        ));
    }
    assert_eq!(navigation_snapshot(), before);
    assert_eq!(snapshot().navigation, before);
}

#[test]
fn navigation_serialization_has_only_closed_labels_and_numeric_facts() {
    let ledger = NavigationDiagnosticLedger::new(2);
    ledger.record(sample(NavigationStage::Committed));
    let value = serde_json::to_value(ledger.snapshot()).unwrap();
    let record = &value["records"][0];
    assert_eq!(record["cause"], "select_room");
    assert_eq!(record["origin"], "renderer_keyboard");
    assert_eq!(record["stage"], "committed");
    assert_eq!(record["request_connection"], 3);
    for (key, value) in record.as_object().unwrap() {
        if ["cause", "origin", "stage"].contains(&key.as_str()) {
            assert!(value.is_string());
        } else {
            assert!(value.is_null() || value.is_boolean() || value.is_u64());
        }
    }
    let serialized = serde_json::to_string(&value).unwrap();
    for private in [
        "!private-room:example.test",
        "@private:example.test",
        "$private-event",
        "private-session",
        "private-token",
    ] {
        assert!(!serialized.contains(private));
    }
}

#[test]
fn navigation_capacity_cannot_exceed_process_retention_bound() {
    let ledger = NavigationDiagnosticLedger::new(2048);
    for _ in 0..300 {
        ledger.record(sample(NavigationStage::Committed));
    }
    assert_eq!(
        ledger.snapshot().records.len(),
        DEFAULT_NAVIGATION_DIAGNOSTIC_CAPACITY
    );
}

#[test]
fn navigation_concurrent_records_have_unique_ordered_sequences() {
    let ledger = std::sync::Arc::new(NavigationDiagnosticLedger::new(256));
    let workers = (0..4)
        .map(|_| {
            let ledger = std::sync::Arc::clone(&ledger);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    ledger.record(sample(NavigationStage::Committed));
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.evicted_records, 144);
    assert_eq!(snapshot.records.len(), 256);
    assert!(
        snapshot
            .records
            .windows(2)
            .all(|pair| pair[0].sequence + 1 == pair[1].sequence)
    );
}
