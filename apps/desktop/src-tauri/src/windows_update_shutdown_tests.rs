use super::*;
use std::sync::Mutex;

#[tokio::test]
async fn windows_install_requires_updater_then_successful_core_cleanup() {
    let order = Mutex::new(Vec::new());
    let result = prepare_windows_update_install(
        async {
            order.lock().unwrap().push("updater");
        },
        async {
            order.lock().unwrap().push("core");
            CoreExitOutcome::Completed
        },
        || {
            order.lock().unwrap().push("install");
            Box::pin(async { Ok(()) })
        },
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(*order.lock().unwrap(), vec!["updater", "core", "install"]);
}

#[tokio::test]
async fn failed_or_timed_out_core_never_invokes_installer() {
    for outcome in [CoreExitOutcome::Failed, CoreExitOutcome::TimedOut] {
        let result = prepare_windows_update_install(async {}, async { outcome }, || {
            panic!("installer must not run after unsuccessful cleanup")
        })
        .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn installer_failure_after_core_cleanup_is_terminal() {
    assert!(
        prepare_windows_update_install(
            async {},
            async { CoreExitOutcome::Completed },
            || Box::pin(async { Err(()) })
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn coordinator_retains_and_awaits_stalled_installer() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(prepare_windows_update_install(
        async {},
        async { CoreExitOutcome::Completed },
        || {
            Box::pin(async move {
                started_tx.send(()).unwrap();
                finish_rx.await.unwrap();
                Ok(())
            })
        },
    ));
    started_rx.await.unwrap();
    assert!(!owner.is_finished());
    finish_tx.send(()).unwrap();
    assert!(owner.await.unwrap().is_ok());
}
