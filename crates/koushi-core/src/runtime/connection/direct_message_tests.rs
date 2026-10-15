use super::*;
use crate::runtime::request_outcome::{
    OutcomeCorrelation, RequestOutcome, RequestOutcomeError, RequestOutcomeExpectation,
};
use futures_util::FutureExt;
use koushi_protocol::event::RoomEvent;
use koushi_protocol::ids::AccountKey;
use koushi_state::{AppState, RoomSummary, RoomTags, SessionInfo, SessionState};

fn existing_dm_snapshot(generation: u64) -> VersionedAppStateSnapshot {
    let mut state = AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@me:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
            authentication_method: Default::default(),
        }),
        ..Default::default()
    };
    state.rooms.push(RoomSummary {
        room_id: "!dm:example.invalid".to_owned(),
        display_name: "Synthetic DM".to_owned(),
        display_name_placeholder: None,
        display_label: "Synthetic DM".to_owned(),
        original_display_label: "Synthetic DM".to_owned(),
        display_label_placeholder: None,
        avatar: None,
        is_dm: true,
        dm_user_ids: vec!["@target:example.invalid".to_owned()],
        tags: RoomTags::default(),
        unread_count: 0,
        notification_count: 0,
        highlight_count: 0,
        thread_unread_count: 0,
        thread_highlight_count: 0,
        marked_unread: false,
        recency_stamp: None,
        conversation_activity: None,
        latest_event: None,
        parent_space_ids: Vec::new(),
        dm_space_ids: Vec::new(),
        is_encrypted: false,
        joined_members: 2,
    });
    VersionedAppStateSnapshot { generation, state }
}

#[tokio::test]
async fn existing_dm_start_settles_without_an_unrelated_state_delta() {
    let (mut connection, _commands, events, snapshots) = super::tests::scripted_connection(4);
    let baseline = existing_dm_snapshot(7);
    snapshots
        .send(baseline.clone())
        .expect("existing DM snapshot");
    let request_id = connection.next_request_id();
    events
        .send(CoreEvent::Room(RoomEvent::DirectMessageStarted {
            request_id,
            room_id: "!dm:example.invalid".to_owned(),
        }))
        .expect("correlated DM completion");

    let outcome = connection
        .wait_for_request_outcome(
            OutcomeCorrelation::Request(request_id),
            RequestOutcomeExpectation::DirectMessageStarted {
                request_id,
                account_key: AccountKey("@me:example.invalid".to_owned()),
            },
            baseline.generation,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await;
    assert!(
        matches!(
            outcome,
            Ok(RequestOutcome::DirectMessageStarted {
                request_id: settled,
                generation: 7,
                ..
            }) if settled == request_id
        ),
        "existing DM must settle from its event and current projection: {outcome:?}"
    );
}

#[tokio::test]
async fn new_dm_start_waits_for_its_room_projection() {
    let (mut connection, _commands, events, snapshots) = super::tests::scripted_connection(4);
    let mut initial = existing_dm_snapshot(7);
    initial.state.rooms.clear();
    snapshots.send(initial).expect("initial snapshot");
    let request_id = connection.next_request_id();
    events
        .send(CoreEvent::Room(RoomEvent::DirectMessageStarted {
            request_id,
            room_id: "!dm:example.invalid".to_owned(),
        }))
        .expect("correlated DM completion");
    let mut waiter = Box::pin(connection.wait_for_request_outcome(
        OutcomeCorrelation::Request(request_id),
        RequestOutcomeExpectation::DirectMessageStarted {
            request_id,
            account_key: AccountKey("@me:example.invalid".to_owned()),
        },
        7,
        tokio::time::Instant::now() + Duration::from_secs(1),
    ));
    assert!(waiter.as_mut().now_or_never().is_none());

    snapshots
        .send(existing_dm_snapshot(8))
        .expect("new room projection");
    assert!(matches!(
        waiter.await,
        Ok(RequestOutcome::DirectMessageStarted { generation: 8, .. })
    ));
}

#[tokio::test]
async fn baseline_dm_projection_requires_the_matching_event_and_account() {
    for case in [
        "no_event",
        "other_request",
        "other_room",
        "other_account",
        "old_snapshot",
    ] {
        let (mut connection, _commands, events, snapshots) = super::tests::scripted_connection(4);
        let generation = if case == "old_snapshot" { 6 } else { 7 };
        snapshots
            .send(existing_dm_snapshot(generation))
            .expect("existing DM snapshot");
        let request_id = connection.next_request_id();
        let event_request_id = if case == "other_request" {
            connection.next_request_id()
        } else {
            request_id
        };
        if case != "no_event" {
            events
                .send(CoreEvent::Room(RoomEvent::DirectMessageStarted {
                    request_id: event_request_id,
                    room_id: if case == "other_room" {
                        "!other:example.invalid".to_owned()
                    } else {
                        "!dm:example.invalid".to_owned()
                    },
                }))
                .expect("DM completion");
        }
        let account_key = if case == "other_account" {
            "@other:example.invalid"
        } else {
            "@me:example.invalid"
        };
        let outcome = connection
            .wait_for_request_outcome(
                OutcomeCorrelation::Request(request_id),
                RequestOutcomeExpectation::DirectMessageStarted {
                    request_id,
                    account_key: AccountKey(account_key.to_owned()),
                },
                7,
                tokio::time::Instant::now() + Duration::from_millis(20),
            )
            .await;
        assert!(
            matches!(outcome, Err(RequestOutcomeError::TimedOut)),
            "{case} must not settle: {outcome:?}"
        );
    }
}
