use super::*;
#[test]
fn diagnostic_snapshot_maps_structured_snapshot_to_camel_case_frontend_contract() {
    let snapshot = koushi_diagnostics::DiagnosticSnapshot {
        records: vec![koushi_diagnostics::DiagnosticRecord {
            timestamp_ms: 42,
            event: koushi_diagnostics::DiagnosticEvent::new(
                koushi_diagnostics::DiagnosticLevel::Debug,
                "desktop.timeline",
                "submit",
            )
            .field(koushi_diagnostics::DiagnosticField::token(
                "operation",
                "send_reaction",
            )),
        }],
        dropped_records: 7,
    };
    let json = serde_json::to_value(map_snapshot(
        snapshot,
        koushi_core::SlidingSyncDiagnosticsSnapshot::default(),
    ))
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "entries": [{
                "timestampMs": 42,
                "source": "desktop.timeline",
                "message": "stage=submit operation=send_reaction"
            }],
            "droppedEntries": 7,
            "slidingSync": {
                "discoveryState": "not_started",
                "advertised": false,
                "discoverySource": "unknown",
                "lastProbeAgeBucket": "never",
                "lastHttpStatusClass": "unknown",
                "requestSchema": "element_x_all_rooms",
                "engine": "SyncService",
                "sdkSlidingSyncVersion": "unknown",
                "roomListSharePos": true,
                "encryptionSharePos": false,
                "encryptionConnectionProfile": "sdk_default_encryption",
                "encryptionExtensionProfile": "e2ee_to_device",
                "provisionalEncryptionStarted": false,
                "provisionalFirstResponseSeen": false,
                "provisionalStoppedBeforeFirstResponse": false,
                "provisionalToNormalHandoffBucket": "never",
                "lifecycle": "stopped",
                "connectivityProven": false,
                "committedGeneration": 0,
                "lastSuccessAgeBucket": "never",
                "consecutiveFailureCount": 0,
                "lastFailureOrigin": "none",
                "lastFailureKind": "none",
                "lastFailureStage": "none",
                "lastHttpErrorSource": "none",
                "lastHttpStatus": "none",
                "lastMatrixErrorKind": "none",
                "lastFailureRetryability": "none",
                "roomListTaskRunning": false,
                "encryptionTaskRunning": false,
                "posPresent": false,
                "directAccountDataSource": "unavailable",
                "directMappedRoomCount": 0,
                "directTargetCount": 0,
                "projectedDmCount": 0,
                "explicitDmCount": 0,
                "fallbackDmCount": 0,
                "directNonDmCount": 0,
                "directInvalidEntryCount": 0,
                "directEventWakeCount": 0,
                "directEventAppliedCount": 0,
                "directEventStreamRunning": false
            }
        })
    );
}
#[test]
fn diagnostic_snapshot_serialization_excludes_synthetic_private_values() {
    let snapshot = koushi_diagnostics::DiagnosticSnapshot {
        records: vec![koushi_diagnostics::DiagnosticRecord {
            timestamp_ms: 42,
            event: koushi_diagnostics::DiagnosticEvent::new(
                koushi_diagnostics::DiagnosticLevel::Debug,
                "desktop.search",
                "submit",
            )
            .field(koushi_diagnostics::DiagnosticField::count(
                "query_bytes",
                23,
            ))
            .field(koushi_diagnostics::DiagnosticField::count(
                "query_chars",
                17,
            )),
        }],
        dropped_records: 0,
    };
    let serialized = serde_json::to_string(&map_snapshot(
        snapshot,
        koushi_core::SlidingSyncDiagnosticsSnapshot::default(),
    ))
    .unwrap();
    for forbidden in [
        "!room:synthetic.invalid",
        "@user:synthetic.invalid",
        "$event:synthetic.invalid",
        "/Users/alice/private",
        "secret message",
        "synthetic search query",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized diagnostics leaked {forbidden}"
        );
    }
    assert!(serialized.contains("query_bytes"));
    assert!(serialized.contains("query_chars"));
}
fn memory_audit_fixture() -> crate::process_memory::MemoryAuditSample {
    crate::process_memory::MemoryAuditSample {
        uptime_ms: Some(9_000),
        native_sample_duration_us: 12,
        native: crate::process_memory::NativeProcessMemory {
            source: "proc_self_status",
            status: "available",
            resident_bytes: Some(4096),
            ..crate::process_memory::NativeProcessMemory::unavailable()
        },
        allocator: crate::process_memory::AllocatorMemory::unavailable(),
    }
}

#[test]
fn diagnostic_snapshot_exports_current_media_memory_summaries() {
    let _guard = koushi_diagnostics::test_support::lock();
    let exported = snapshot_with_media_memory_summaries(
        koushi_core::renderable_thumbnail::RenderableThumbnailCacheStats {
            entry_count: 3,
            retained_bytes: 300,
            lease_charged_bytes: 120,
            high_water_entry_count: 5,
            high_water_bytes: 500,
            eviction_count: 2,
            clear_count: 1,
            oversize_rejection_count: 4,
        },
        koushi_core::media_preparation::MediaPreparationStats {
            source_count: 2,
            source_bytes: 200,
            variant_count: 3,
            source_backed_variant_count: 2,
            variant_bytes: 80,
            selected_count: 2,
            high_water_source_count: 4,
            high_water_source_bytes: 400,
            high_water_variant_count: 6,
            high_water_variant_bytes: 160,
        },
        &memory_audit_fixture(),
        koushi_core::SlidingSyncDiagnosticsSnapshot::default(),
    );
    let thumbnail = exported
        .entries
        .iter()
        .rev()
        .find(|entry| entry.source == "core.renderable_thumbnail")
        .expect("renderable-thumbnail summary must be exported");
    assert!(thumbnail.message.contains("stage=summary"));
    assert!(thumbnail.message.contains("entry_count=3"));
    assert!(thumbnail.message.contains("max_entry_count=256"));
    assert!(thumbnail.message.contains("max_bytes=33554432"));
    assert!(thumbnail.message.contains("lease_charged_bytes=120"));
    assert!(thumbnail.message.contains("max_lease_bytes=100663296"));
    assert!(thumbnail.message.contains("cache_scope=named_caches_only"));
    let media = exported
        .entries
        .iter()
        .rev()
        .find(|entry| entry.source == "core.media_preparation")
        .expect("media-preparation summary must be exported");
    assert!(media.message.contains("stage=summary"));
    assert!(media.message.contains("source_backed_variant_count=2"));
    assert!(media.message.contains("cache_scope=named_caches_only"));
    let details = koushi_diagnostics::test_support::detail_snapshot();
    let summaries = ["core.renderable_thumbnail", "core.media_preparation"].map(|source| {
        details
            .records
            .iter()
            .rev()
            .find(|record| record.event.source == source && record.event.stage == "summary")
            .expect("media summary diagnostic must be present")
    });
    for record in summaries {
        assert!(record.event.fields.iter().all(|field| matches!(
            field.value,
            koushi_diagnostics::DiagnosticValue::Count(_)
                | koushi_diagnostics::DiagnosticValue::Token(_)
        )));
    }
    let serialized = serde_json::to_string(&exported).unwrap();
    for forbidden in [
        "!room:synthetic.invalid",
        "@user:synthetic.invalid",
        "$event:synthetic.invalid",
        "/Users/alice/private",
        "secret message",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn diagnostic_snapshot_exports_the_process_memory_audit() {
    let _guard = koushi_diagnostics::test_support::lock();
    let exported = snapshot_with_media_memory_summaries(
        koushi_core::renderable_thumbnail::RenderableThumbnailCacheStats::default(),
        koushi_core::media_preparation::MediaPreparationStats::default(),
        &memory_audit_fixture(),
        koushi_core::SlidingSyncDiagnosticsSnapshot::default(),
    );
    let audit = exported
        .entries
        .iter()
        .rev()
        .find(|entry| entry.source == "desktop.process_memory")
        .expect("process memory audit must be exported");
    assert!(audit.message.contains("stage=audit"));
    assert!(audit.message.contains("memory_audit_schema=1"));
    assert!(audit.message.contains("resident_bytes=4096"));
    assert!(audit.message.contains("renderer_gpu_footprint=unavailable"));
    assert!(
        audit
            .message
            .contains("native_memory_scope=desktop_process_only")
    );
    assert!(audit.message.contains("allocator_in_use_bytes=unavailable"));
    let record = koushi_diagnostics::test_support::detail_snapshot()
        .records
        .into_iter()
        .rev()
        .find(|record| record.event.source == "desktop.process_memory")
        .expect("process memory audit must be in the detail ring");
    assert!(record.event.fields.iter().all(|field| matches!(
        field.value,
        koushi_diagnostics::DiagnosticValue::Count(_)
            | koushi_diagnostics::DiagnosticValue::Token(_)
    )));
}
