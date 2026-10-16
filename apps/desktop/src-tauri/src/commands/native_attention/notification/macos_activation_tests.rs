use super::macos::*;

#[test]
fn macos_activation_registry_keeps_more_than_four_clicks_and_releases_cleared_targets() {
    let mut registry = MacActivationRegistry::default();
    let mut receivers = Vec::new();
    for index in 0..5 {
        let (cancel, receiver) = tokio::sync::oneshot::channel();
        assert!(registry.insert(format!("banner-{index}"), cancel).is_none());
        receivers.push(receiver);
    }
    assert_eq!(registry.len(), 5);
    assert!(receivers.iter_mut().all(|receiver| matches!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    )));

    let cancelled = registry.clear();
    assert_eq!(registry.len(), 0);
    for sender in cancelled {
        let _ = sender.send(());
    }
    assert!(
        receivers
            .iter_mut()
            .all(|receiver| receiver.try_recv().is_ok())
    );
}

#[test]
fn macos_activation_registry_evicts_oldest_at_its_bound() {
    let mut registry = MacActivationRegistry::default();
    for index in 0..MAX_PENDING_MACOS_ACTIVATIONS {
        let (cancel, _receiver) = tokio::sync::oneshot::channel();
        assert!(registry.insert(format!("banner-{index}"), cancel).is_none());
    }
    let (cancel, _receiver) = tokio::sync::oneshot::channel();
    let evicted = registry.insert("new-banner".to_owned(), cancel);
    assert_eq!(
        evicted.as_ref().map(|(id, _)| id.as_str()),
        Some("banner-0")
    );
    assert_eq!(registry.len(), MAX_PENDING_MACOS_ACTIVATIONS);
}
