use koushi_core::{
    SlidingSyncDiagnostics, SlidingSyncDiscoveryDiagnostic, SlidingSyncFailureDiagnostic,
    SlidingSyncFailureKind, SlidingSyncFailureOrigin, SlidingSyncFailureRetryability,
    SlidingSyncFailureStage, SlidingSyncHttpErrorSource, SlidingSyncHttpStatus,
    SlidingSyncLifecycle, SlidingSyncMatrixErrorKind, SlidingSyncProvisionalHandoffBucket,
    SlidingSyncSdkVersion,
};

#[test]
fn snapshot_is_latest_wins_and_tracks_actionable_sync_failure() {
    let diagnostics = SlidingSyncDiagnostics::default();

    diagnostics.record_discovery(SlidingSyncDiscoveryDiagnostic::supported());
    diagnostics.runtime_profile(SlidingSyncSdkVersion::Native);
    diagnostics.provisional_encryption_started();
    diagnostics.provisional_encryption_stopped();
    diagnostics.sync_started(7);
    diagnostics.sync_offline(SlidingSyncFailureDiagnostic {
        origin: SlidingSyncFailureOrigin::RoomList,
        kind: SlidingSyncFailureKind::Auth,
        stage: SlidingSyncFailureStage::RoomListSlidingSync,
        http_error_source: SlidingSyncHttpErrorSource::ServerResponse,
        http_status: SlidingSyncHttpStatus::BadRequest,
        matrix_error_kind: SlidingSyncMatrixErrorKind::BadJson,
        retryability: SlidingSyncFailureRetryability::Permanent,
    });

    let snapshot = diagnostics.snapshot();
    assert_eq!(snapshot.discovery_state.as_str(), "supported");
    assert_eq!(snapshot.lifecycle, SlidingSyncLifecycle::Reconnecting);
    assert_eq!(snapshot.committed_generation, 0);
    assert!(!snapshot.connectivity_proven);
    assert_eq!(snapshot.consecutive_failure_count, 1);
    assert_eq!(
        snapshot.last_failure_origin,
        SlidingSyncFailureOrigin::RoomList
    );
    assert_eq!(snapshot.last_failure_kind, SlidingSyncFailureKind::Auth);
    assert_eq!(
        snapshot.last_failure_stage,
        SlidingSyncFailureStage::RoomListSlidingSync
    );
    assert_eq!(
        snapshot.last_http_error_source,
        SlidingSyncHttpErrorSource::ServerResponse
    );
    assert_eq!(snapshot.last_http_status, SlidingSyncHttpStatus::BadRequest);
    assert_eq!(
        snapshot.last_matrix_error_kind,
        SlidingSyncMatrixErrorKind::BadJson
    );
    assert_eq!(
        snapshot.last_failure_retryability,
        SlidingSyncFailureRetryability::Permanent
    );
    assert!(!snapshot.room_list_task_running);
    assert!(!snapshot.encryption_task_running);
    assert_eq!(
        snapshot.sdk_sliding_sync_version,
        SlidingSyncSdkVersion::Native
    );
    assert!(snapshot.provisional_encryption_started);
    assert!(!snapshot.provisional_first_response_seen);
    assert!(snapshot.provisional_stopped_before_first_response);
    assert_eq!(
        snapshot.provisional_to_normal_handoff_bucket,
        SlidingSyncProvisionalHandoffBucket::Under100Milliseconds
    );
    assert!(snapshot.room_list_share_pos);
    assert!(!snapshot.encryption_share_pos);

    diagnostics.response_committed(7, true);
    let recovered = diagnostics.snapshot();
    assert_eq!(recovered.lifecycle, SlidingSyncLifecycle::Running);
    assert!(recovered.connectivity_proven);
    assert_eq!(recovered.committed_generation, 7);
    assert_eq!(recovered.consecutive_failure_count, 0);
    assert!(recovered.pos_present);
    assert!(recovered.room_list_task_running);
    assert!(recovered.encryption_task_running);
}

#[test]
fn snapshot_serialization_has_no_channel_for_private_values() {
    let diagnostics = SlidingSyncDiagnostics::default();
    diagnostics.record_discovery(SlidingSyncDiscoveryDiagnostic::supported());
    diagnostics.sync_started(23);
    diagnostics.sync_offline(SlidingSyncFailureDiagnostic {
        origin: SlidingSyncFailureOrigin::Encryption,
        kind: SlidingSyncFailureKind::Store,
        stage: SlidingSyncFailureStage::EncryptionLock,
        ..SlidingSyncFailureDiagnostic::default()
    });

    let serialized = serde_json::to_string(&diagnostics.snapshot()).unwrap();
    for forbidden in [
        "https://matrix.example.invalid",
        "@alice:example.invalid",
        "!room:example.invalid",
        "$event:example.invalid",
        "secret-access-token",
        "raw-pos-value",
        "/Users/alice/private",
    ] {
        assert!(!serialized.contains(forbidden), "leaked {forbidden}");
    }
    assert!(serialized.contains("encryption"));
    assert!(serialized.contains("encryption_lock"));
    assert!(serialized.contains("sync_failed_store"));
}
