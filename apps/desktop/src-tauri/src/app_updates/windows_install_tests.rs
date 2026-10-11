use super::*;

#[test]
fn notification_candidate_cannot_download() {
    let mut lifecycle = Lifecycle::new(DesktopUpdateState::Available {
        version: "1.0.1".into(),
        generation: 0,
        notification_only: true,
        release_url: Some("https://example.invalid/release".into()),
    });
    lifecycle.pending = Some(PendingUpdate {
        version: "1.0.1".into(),
        notification_only: true,
        release_url: None,
        update: (),
        bytes: None,
    });
    assert!(lifecycle.begin_download(0).is_err());
}

#[test]
fn verified_install_transfer_stops_admission_and_only_happens_once() {
    let mut lifecycle = Lifecycle::new(DesktopUpdateState::Ready {
        version: "1.0.1".into(),
    });
    lifecycle.pending = Some(PendingUpdate {
        version: "1.0.1".into(),
        notification_only: false,
        release_url: None,
        update: (),
        bytes: Some(vec![1]),
    });
    let pending = lifecycle.take_install_for_shutdown(|| true).unwrap();
    assert_eq!(pending.bytes, Some(vec![1]));
    assert!(lifecycle.stopping);
    assert!(lifecycle.claim_work().is_none());
    assert!(lifecycle.take_install_for_shutdown(|| true).is_err());
}

#[test]
fn concurrent_quit_retains_artifact_without_installing() {
    let mut lifecycle = Lifecycle::new(DesktopUpdateState::Ready {
        version: "1.0.1".into(),
    });
    lifecycle.pending = Some(PendingUpdate {
        version: "1.0.1".into(),
        notification_only: false,
        release_url: None,
        update: (),
        bytes: Some(vec![1]),
    });
    assert!(lifecycle.take_install_for_shutdown(|| false).is_err());
    assert!(lifecycle.pending.is_some());
    assert!(matches!(lifecycle.state, DesktopUpdateState::Ready { .. }));
}

#[test]
fn actual_shutdown_claim_fences_install_and_quit_in_both_orders() {
    use std::sync::atomic::AtomicU8;
    for quit_first in [false, true] {
        let stage = AtomicU8::new(crate::QuitStage::Idle.repr());
        let mut lifecycle = Lifecycle::new(DesktopUpdateState::Ready {
            version: "1.0.1".into(),
        });
        lifecycle.pending = Some(PendingUpdate {
            version: "1.0.1".into(),
            notification_only: false,
            release_url: None,
            update: (),
            bytes: Some(vec![1]),
        });
        if quit_first {
            assert!(crate::claim_core_shutdown(&stage));
        }
        let install = lifecycle.take_install_for_shutdown(|| crate::claim_core_shutdown(&stage));
        assert_eq!(install.is_err(), quit_first);
        assert!(!crate::claim_core_shutdown(&stage));
        assert_eq!(lifecycle.pending.is_some(), quit_first);
    }
}
