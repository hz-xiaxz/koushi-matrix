use serde::Serialize;
use tauri::State;

use crate::CoreRuntimeState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendDiagnosticLogEntry {
    timestamp_ms: u64,
    source: &'static str,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendDiagnosticLogSnapshot {
    entries: Vec<FrontendDiagnosticLogEntry>,
    dropped_entries: u64,
    sliding_sync: koushi_core::SlidingSyncDiagnosticsSnapshot,
}

fn map_snapshot(
    snapshot: koushi_diagnostics::DiagnosticSnapshot,
    sliding_sync: koushi_core::SlidingSyncDiagnosticsSnapshot,
) -> FrontendDiagnosticLogSnapshot {
    FrontendDiagnosticLogSnapshot {
        entries: snapshot
            .records
            .into_iter()
            .map(|record| FrontendDiagnosticLogEntry {
                timestamp_ms: record.timestamp_ms,
                source: record.event.source,
                message: koushi_diagnostics::format_event(&record.event),
            })
            .collect(),
        dropped_entries: snapshot.dropped_records,
        sliding_sync,
    }
}

fn snapshot_with_media_memory_summaries(
    thumbnail_stats: koushi_core::renderable_thumbnail::RenderableThumbnailCacheStats,
    media_stats: koushi_core::media_preparation::MediaPreparationStats,
    sliding_sync: koushi_core::SlidingSyncDiagnosticsSnapshot,
) -> FrontendDiagnosticLogSnapshot {
    koushi_core::renderable_thumbnail::record_renderable_thumbnail_summary(thumbnail_stats);
    koushi_core::media_preparation::record_media_preparation_summary(media_stats);
    map_snapshot(koushi_diagnostics::snapshot(), sliding_sync)
}

#[tauri::command]
pub async fn get_diagnostic_snapshot(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDiagnosticLogSnapshot, String> {
    Ok(snapshot_with_media_memory_summaries(
        koushi_core::renderable_thumbnail::renderable_thumbnail_cache_stats(),
        state.runtime.media_preparation().stats().await,
        state.runtime.sliding_sync_diagnostics(),
    ))
}

#[cfg(test)]
mod tests {
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
    fn diagnostic_snapshot_command_is_registered_in_generate_handler() {
        let source = include_str!("../lib.rs");
        assert!(source.contains("commands::diagnostics::get_diagnostic_snapshot"));
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

    #[test]
    fn diagnostic_snapshot_exports_current_media_memory_summaries() {
        let _guard = koushi_diagnostics::test_support::lock();
        let before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        let exported = snapshot_with_media_memory_summaries(
            koushi_core::renderable_thumbnail::RenderableThumbnailCacheStats {
                entry_count: 3,
                retained_bytes: 300,
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
        let media = exported
            .entries
            .iter()
            .rev()
            .find(|entry| entry.source == "core.media_preparation")
            .expect("media-preparation summary must be exported");
        assert!(media.message.contains("stage=summary"));
        assert!(media.message.contains("source_backed_variant_count=2"));

        let details = koushi_diagnostics::test_support::detail_snapshot();
        let summaries = &details.records[before..];
        assert_eq!(summaries.len(), 2);
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
}
