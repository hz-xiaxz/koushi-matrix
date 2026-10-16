use super::*;

#[tokio::test]
async fn interrupted_persistence_never_allows_the_worker_to_continue() {
    let poisoned = AtomicBool::new(false);
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(1),
        persist(&poisoned, std::future::pending()),
    )
    .await;
    assert!(result.is_err());
    assert!(poisoned.load(Ordering::Acquire));
}

#[tokio::test]
async fn only_confirmed_success_keeps_storage_usable() {
    let poisoned = AtomicBool::new(false);
    assert_eq!(persist(&poisoned, async { Ok(()) }).await, Ok(()));
    assert!(!poisoned.load(Ordering::Acquire));
    assert_eq!(
        persist(&poisoned, async { Err(Failure::Storage) }).await,
        Err(Failure::Storage)
    );
    assert!(poisoned.load(Ordering::Acquire));
}
