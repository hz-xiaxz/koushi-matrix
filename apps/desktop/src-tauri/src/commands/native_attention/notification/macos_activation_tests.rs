use super::macos::*;

#[test]
fn macos_activation_registry_keeps_more_than_four_clicks_and_releases_cleared_targets() {
    let mut registry = MacActivationRegistry::default();
    let mut receivers = Vec::new();
    for index in 0..5 {
        let (cancel, receiver) = tokio::sync::watch::channel(None);
        assert!(
            registry
                .insert(format!("banner-{index}"), "account-a".into(), false, cancel)
                .is_none()
        );
        receivers.push(receiver);
    }
    assert_eq!(registry.len(), 5);
    assert!(receivers.iter().all(|receiver| receiver.borrow().is_none()));
    let cancelled = registry.clear("account-a", false);
    assert_eq!(cancelled.len(), 5);
    for sender in cancelled {
        let _ = sender.send(Some(false));
    }
    assert!(
        receivers
            .iter()
            .all(|receiver| *receiver.borrow() == Some(false))
    );
    // Grace-period targets remain tracked so a later privacy clear can override it.
    for sender in registry.clear("account-a", true) {
        let _ = sender.send(Some(true));
    }
    assert!(
        receivers
            .iter()
            .all(|receiver| *receiver.borrow() == Some(true))
    );
}

#[test]
fn macos_activation_registry_evicts_oldest_at_its_bound() {
    let mut registry = MacActivationRegistry::default();
    for index in 0..MAX_PENDING_MACOS_ACTIVATIONS {
        let (cancel, _receiver) = tokio::sync::watch::channel(None);
        assert!(
            registry
                .insert(format!("banner-{index}"), "account-a".into(), false, cancel)
                .is_none()
        );
    }
    let (cancel, _receiver) = tokio::sync::watch::channel(None);
    let evicted = registry.insert("new-banner".to_owned(), "account-a".into(), false, cancel);
    assert_eq!(
        evicted.as_ref().map(|pending| pending.id.as_str()),
        Some("banner-0")
    );
    assert_eq!(registry.len(), MAX_PENDING_MACOS_ACTIVATIONS);
}

#[test]
fn account_clear_preserves_other_accounts_and_newer_notifications() {
    let mut registry = MacActivationRegistry::default();
    let (a, receiver_a) = tokio::sync::watch::channel(None);
    let (b, receiver_b) = tokio::sync::watch::channel(None);
    registry.insert("a".into(), "account-a".into(), false, a);
    registry.insert("b".into(), "account-b".into(), false, b);
    let cleared = registry.clear("account-a", false);
    let (new, receiver_new) = tokio::sync::watch::channel(None);
    registry.insert("new".into(), "account-a".into(), false, new);
    for sender in cleared {
        let _ = sender.send(Some(false));
    }
    assert_eq!(*receiver_a.borrow(), Some(false));
    assert_eq!(*receiver_b.borrow(), None);
    assert_eq!(*receiver_new.borrow(), None);
}

#[test]
fn read_clear_waits_until_five_seconds_but_privacy_clear_is_immediate() {
    use std::time::Duration;
    assert_eq!(clear_delay(Duration::ZERO, false), Duration::from_secs(5));
    assert_eq!(
        clear_delay(Duration::from_millis(4999), false),
        Duration::from_millis(1)
    );
    assert_eq!(clear_delay(Duration::from_secs(5), false), Duration::ZERO);
    assert_eq!(clear_delay(Duration::from_secs(7), false), Duration::ZERO);
    assert_eq!(clear_delay(Duration::ZERO, true), Duration::ZERO);
}

#[test]
fn privacy_invalidation_is_never_downgraded_by_a_later_read_clear() {
    let mut registry = MacActivationRegistry::default();
    registry.clear("account-a", true);
    registry.clear("account-a", false);
    assert_eq!(registry.stale_clear_mode("account-a", 0, 0), Some(true));
    assert_eq!(registry.stale_clear_mode("account-b", 0, 0), None);
    let (pre_invalidated, receiver) = tokio::sync::watch::channel(Some(true));
    send_clear_mode(&pre_invalidated, false);
    assert_eq!(*receiver.borrow(), Some(true));
    let (sender, receiver) = tokio::sync::watch::channel(None);
    send_clear_mode(&sender, true);
    send_clear_mode(&sender, false);
    assert_eq!(*receiver.borrow(), Some(true));
    let (sender, receiver) = tokio::sync::watch::channel(None);
    send_clear_mode(&sender, false);
    send_clear_mode(&sender, true);
    assert_eq!(*receiver.borrow(), Some(true));
}
