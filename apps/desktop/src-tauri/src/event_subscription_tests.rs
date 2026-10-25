use super::*;

#[tokio::test]
async fn cached_command_connection_does_not_keep_a_broadcast_receiver() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(AccountRuntimeManager::new(
        StoreActor::with_backend(
            koushi_core::store::TestCredentialStoreBackend::in_memory(),
            directory.path(),
        ),
        SettingsStore::new(directory.path()),
        Arc::new(|| Arc::new(NativeArtifactRegistry::new())),
    ));
    let (_ready, restore_ready) = tokio::sync::watch::channel(true);
    let selected = SelectedCoreConnection {
        runtime: Arc::clone(&runtime),
        connections: Arc::new(TokioMutex::new(HashMap::new())),
        restore_ready,
    };
    let connection = selected.lock().await;
    let first = connection.next_request_id();
    let second = connection.next_request_id();
    assert_eq!(first.connection_id, second.connection_id);
    assert!(second.sequence > first.sequence);
    drop(connection);
    // These are the connections used by account snapshot watchers.
    let watchers = runtime.tab_connections_without_events();
    assert_eq!(watchers.len(), 1);
    assert!(!watchers[0].1.event_receiver_diagnostics().subscribed);
    let cached = selected.cached_event_receiver_diagnostics().await;
    assert!(!cached[0].1.unwrap().subscribed);
    let observed = runtime.event_queue_diagnostics();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].1.receiver_count, 0);
    drop(watchers);
    selected.clear_cached_connections().await;
    runtime.shutdown_all().await;
}
