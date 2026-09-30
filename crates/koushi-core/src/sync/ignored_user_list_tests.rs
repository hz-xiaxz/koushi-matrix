use std::collections::BTreeSet;
use std::time::Duration;

use koushi_state::AppAction;
use matrix_sdk::ruma::{OwnedUserId, user_id};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use matrix_sdk_test::event_factory::EventFactory;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::{IGNORED_USER_LIST_DRAIN_TIMEOUT, IgnoredUserListForwarder};
use crate::timeline::TimelineMessage;

const DEADLINE: Duration = Duration::from_secs(10);

fn filler_set() -> BTreeSet<String> {
    BTreeSet::from(["@filler:example.org".to_owned()])
}

fn user_set(users: &[OwnedUserId]) -> BTreeSet<String> {
    users.iter().map(ToString::to_string).collect()
}

async fn recv_action_set(
    rx: &mut mpsc::Receiver<Vec<AppAction>>,
    deadline: Instant,
) -> BTreeSet<String> {
    let actions = tokio::time::timeout_at(deadline, rx.recv())
        .await
        .expect("ignored-user action delivered before deadline")
        .expect("action channel open");
    match actions.as_slice() {
        [AppAction::IgnoredUsersLoaded { user_ids }] => user_ids.clone(),
        _ => panic!("unexpected action batch"),
    }
}

async fn recv_timeline_set(
    rx: &mut mpsc::Receiver<TimelineMessage>,
    deadline: Instant,
) -> BTreeSet<String> {
    let message = tokio::time::timeout_at(deadline, rx.recv())
        .await
        .expect("ignored-user timeline message delivered before deadline")
        .expect("timeline channel open");
    match message {
        TimelineMessage::IgnoredUsersUpdated { user_ids } => user_ids,
        _ => panic!("unexpected timeline message"),
    }
}

/// Every delivered set is a known change, strictly newer than the one before.
fn assert_only_newer_sets(order: &[BTreeSet<String>], delivered: &[BTreeSet<String>]) {
    let positions: Vec<_> = delivered
        .iter()
        .map(|set| {
            order
                .iter()
                .position(|known| known == set)
                .expect("known set")
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "stale set delivered after a newer one: {positions:?}"
    );
}

/// Registers the production forwarder with both single-slot mailboxes already
/// full, so the next forward observes `Full`.
fn register_with_full_mailboxes(
    client: &matrix_sdk::Client,
) -> (
    IgnoredUserListForwarder,
    mpsc::Receiver<Vec<AppAction>>,
    mpsc::Receiver<TimelineMessage>,
) {
    let (action_tx, action_rx) = mpsc::channel(1);
    let (timeline_tx, timeline_rx) = mpsc::channel(1);
    action_tx
        .try_send(vec![AppAction::IgnoredUsersLoaded {
            user_ids: filler_set(),
        }])
        .expect("fill action mailbox");
    timeline_tx
        .try_send(TimelineMessage::IgnoredUsersUpdated {
            user_ids: filler_set(),
        })
        .expect("fill timeline mailbox");
    let forwarder = IgnoredUserListForwarder::register(client, action_tx, timeline_tx);
    (forwarder, action_rx, timeline_rx)
}

#[tokio::test]
async fn ignored_user_list_change_survives_full_mailboxes() {
    let deadline = Instant::now() + DEADLINE;
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let (forwarder, mut action_rx, mut timeline_rx) = register_with_full_mailboxes(&client);

    let ignored = vec![user_id!("@bob:example.org").to_owned()];
    tokio::time::timeout_at(
        deadline,
        server.mock_sync().ok_and_run(&client, |builder| {
            builder.add_global_account_data(EventFactory::new().ignored_user_list(ignored.clone()));
        }),
    )
    .await
    .expect("sync handler does not block on full mailboxes");

    assert_eq!(
        recv_action_set(&mut action_rx, deadline).await,
        filler_set()
    );
    assert_eq!(
        recv_timeline_set(&mut timeline_rx, deadline).await,
        filler_set()
    );
    assert_eq!(
        recv_action_set(&mut action_rx, deadline).await,
        user_set(&ignored)
    );
    assert_eq!(
        recv_timeline_set(&mut timeline_rx, deadline).await,
        user_set(&ignored)
    );

    tokio::time::timeout_at(deadline, forwarder.shutdown(&client))
        .await
        .expect("forwarder settles");
}

#[tokio::test]
async fn rapid_ignored_user_list_changes_converge_to_latest_set() {
    let deadline = Instant::now() + DEADLINE;
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let (forwarder, mut action_rx, mut timeline_rx) = register_with_full_mailboxes(&client);

    let changes = [
        vec![user_id!("@alice:example.org").to_owned()],
        vec![
            user_id!("@alice:example.org").to_owned(),
            user_id!("@bob:example.org").to_owned(),
        ],
        vec![user_id!("@carol:example.org").to_owned()],
    ];
    for users in &changes {
        tokio::time::timeout_at(
            deadline,
            server.mock_sync().ok_and_run(&client, |builder| {
                builder
                    .add_global_account_data(EventFactory::new().ignored_user_list(users.clone()));
            }),
        )
        .await
        .expect("sync handler does not block on full mailboxes");
    }
    let order: Vec<_> = changes.iter().map(|users| user_set(users)).collect();
    let latest = order.last().expect("changes").clone();

    assert_eq!(
        recv_action_set(&mut action_rx, deadline).await,
        filler_set()
    );
    assert_eq!(
        recv_timeline_set(&mut timeline_rx, deadline).await,
        filler_set()
    );
    let mut actions = Vec::new();
    while actions.last() != Some(&latest) {
        actions.push(recv_action_set(&mut action_rx, deadline).await);
    }
    let mut timeline = Vec::new();
    while timeline.last() != Some(&latest) {
        timeline.push(recv_timeline_set(&mut timeline_rx, deadline).await);
    }
    assert_only_newer_sets(&order, &actions);
    assert_only_newer_sets(&order, &timeline);

    // Settle the forwarder so nothing can arrive after the final check.
    tokio::time::timeout_at(deadline, forwarder.shutdown(&client))
        .await
        .expect("forwarder settles");
    assert!(action_rx.try_recv().is_err(), "no set after the latest one");
    assert!(
        timeline_rx.try_recv().is_err(),
        "no set after the latest one"
    );
}

#[tokio::test]
async fn shutdown_with_a_stuck_mailbox_settles_within_the_drain_bound() {
    let deadline = Instant::now() + DEADLINE;
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    let (forwarder, mut action_rx, _timeline_rx) = register_with_full_mailboxes(&client);

    let ignored = vec![user_id!("@bob:example.org").to_owned()];
    tokio::time::timeout_at(
        deadline,
        server.mock_sync().ok_and_run(&client, |builder| {
            builder.add_global_account_data(EventFactory::new().ignored_user_list(ignored.clone()));
        }),
    )
    .await
    .expect("sync handler does not block on full mailboxes");

    // Nothing drains the action mailbox, so the pending set can never be sent.
    let started = Instant::now();
    tokio::time::timeout_at(deadline, forwarder.shutdown(&client))
        .await
        .expect("forwarder settles");
    assert!(
        started.elapsed() < IGNORED_USER_LIST_DRAIN_TIMEOUT * 3,
        "shutdown bounded by the drain timeout"
    );

    // The aborted task released its sender: only the filler remains.
    assert_eq!(
        recv_action_set(&mut action_rx, deadline).await,
        filler_set()
    );
    assert!(
        tokio::time::timeout_at(deadline, action_rx.recv())
            .await
            .expect("action channel settles")
            .is_none(),
        "forwarder task settled without a detached sender"
    );
}
