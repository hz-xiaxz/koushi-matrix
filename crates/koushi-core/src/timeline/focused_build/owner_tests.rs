//! #1465: projection ownership of a pending focused build. Only the admitted
//! navigation owner of a focused key may take over the build's projection
//! correlation; renderer recovery/replay subscribes are observers.

use std::time::Duration;

use koushi_protocol::command::{InitialBackfillPolicy, TimelineCommand};
use koushi_protocol::event::CoreEvent;
use koushi_protocol::failure::{CoreFailure, TimelineFailureKind};
use koushi_protocol::ids::{RequestId, TimelineKey};
use tokio::sync::mpsc;

use crate::executor;

use super::super::navigation::{FocusedProjectionCommitted, NavigationProjectionIngress};
use super::super::test_support::{fake_rid, focused_key};
use super::gated_manager::{GatedFocusedBuildManager, spawn_gated_focused_build_manager};

struct Owned {
    manager: GatedFocusedBuildManager,
    ingress: NavigationProjectionIngress,
    commits: mpsc::UnboundedReceiver<FocusedProjectionCommitted>,
}

fn spawn() -> Owned {
    let (ingress, navigation_rx) = NavigationProjectionIngress::channel();
    let (commit_tx, commits) = mpsc::unbounded_channel();
    Owned {
        manager: spawn_gated_focused_build_manager(navigation_rx, commit_tx),
        ingress,
        commits,
    }
}

impl Owned {
    async fn subscribe(&self, request_id: RequestId, key: &TimelineKey) {
        self.manager
            .command(TimelineCommand::Subscribe {
                request_id,
                key: key.clone(),
                initial_backfill: InitialBackfillPolicy::Disabled,
            })
            .await;
    }

    /// Admit `owner` as the navigation owner of `key`, then send its subscribe,
    /// in AppActor's order.
    async fn navigate(&self, owner: RequestId, key: &TimelineKey) {
        self.ingress
            .admit_focused_with_owner(Some(key.clone()), Some(owner));
        self.subscribe(owner, key).await;
    }

    async fn committed_owner(&mut self) -> RequestId {
        executor::timeout(Duration::from_secs(2), self.commits.recv())
            .await
            .expect("the installed actor must publish its focused projection")
            .expect("commit lane")
            .projection_request_id
    }
}

#[tokio::test]
async fn observer_subscribe_keeps_the_owner_and_the_commit_correlates_with_it() {
    let mut owned = spawn();
    let key = focused_key();
    owned.navigate(fake_rid(1), &key).await;
    let release = owned.manager.build_started().await;
    owned.subscribe(fake_rid(2), &key).await;
    assert_eq!(owned.manager.pending_owner(&key).await, Some(fake_rid(1)));
    assert!(owned.manager.no_build_started(), "the build is reused");

    release.send(Ok(())).expect("build is pending");
    assert_eq!(owned.committed_owner().await, fake_rid(1));
}

#[tokio::test]
async fn a_renderer_started_build_is_adopted_by_the_navigation_owner() {
    let mut owned = spawn();
    let key = focused_key();
    owned.subscribe(fake_rid(2), &key).await;
    let release = owned.manager.build_started().await;
    assert_eq!(owned.manager.pending_owner(&key).await, Some(fake_rid(2)));

    owned.navigate(fake_rid(1), &key).await;
    assert_eq!(owned.manager.pending_owner(&key).await, Some(fake_rid(1)));
    assert!(owned.manager.no_build_started(), "the build is reused");

    release.send(Ok(())).expect("build is pending");
    assert_eq!(owned.committed_owner().await, fake_rid(1));
}

#[tokio::test]
async fn a_same_key_navigation_supersession_adopts_the_new_owner() {
    let mut owned = spawn();
    let key = focused_key();
    owned.navigate(fake_rid(1), &key).await;
    let release = owned.manager.build_started().await;
    owned.subscribe(fake_rid(2), &key).await;
    owned.navigate(fake_rid(3), &key).await;
    owned.subscribe(fake_rid(4), &key).await;
    assert_eq!(owned.manager.pending_owner(&key).await, Some(fake_rid(3)));

    release.send(Ok(())).expect("build is pending");
    assert_eq!(
        owned.committed_owner().await,
        fake_rid(3),
        "neither the superseded owner nor an observer correlates the commit"
    );
}

#[tokio::test]
async fn a_failed_build_terminates_the_owner_and_every_observer() {
    let mut owned = spawn();
    let key = focused_key();
    owned.navigate(fake_rid(1), &key).await;
    let release = owned.manager.build_started().await;
    owned.subscribe(fake_rid(2), &key).await;
    owned.subscribe(fake_rid(3), &key).await;
    assert_eq!(owned.manager.pending_owner(&key).await, Some(fake_rid(1)));

    release
        .send(Err(TimelineFailureKind::Sdk))
        .expect("build is pending");
    let mut failed = Vec::new();
    executor::timeout(Duration::from_secs(2), async {
        while failed.len() < 3 {
            if let CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::TimelineOperationFailed { .. },
            } = owned.manager.event_rx.recv().await.expect("event stream")
            {
                failed.push(request_id);
            }
        }
    })
    .await
    .expect("owner and observers each receive a failure terminal");
    failed.sort_by_key(|request_id| request_id.sequence);
    assert_eq!(failed, vec![fake_rid(1), fake_rid(2), fake_rid(3)]);
    assert!(
        owned.commits.try_recv().is_err(),
        "a failed build commits nothing"
    );
}
