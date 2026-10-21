use super::*;
use matrix_sdk::{
    ruma::{event_id, events::StateEventType, room_id},
    test_utils::mocks::MatrixMockServer,
};
use matrix_sdk_test::{JoinedRoomBuilder, event_factory::EventFactory};
use std::future::Future;

async fn session(server: &MatrixMockServer) -> MatrixClientSession {
    let client = server.client_builder().build().await;
    MatrixClientSession {
        info: SessionInfo {
            homeserver: server.server().uri(),
            user_id: client.user_id().unwrap().to_string(),
            device_id: client.device_id().unwrap().to_string(),
            authentication_method: Default::default(),
        },
        client,
        diagnostic_counters: koushi_diagnostics::DiagnosticCounterContext::registered(),
    }
}

#[tokio::test]
async fn topic_http_ack_waits_for_exact_local_commit_and_immediate_reloads_never_regress() {
    let server = MatrixMockServer::new().await;
    let session = session(&server).await;
    let room_id = room_id!("!topic-commit:example.org");
    let other_id = room_id!("!other-topic:example.org");
    server.sync_joined_room(&session.client(), room_id).await;
    server.sync_joined_room(&session.client(), other_id).await;
    let factory = EventFactory::new().sender(session.client().user_id().unwrap());
    for (index, target) in ["description one", "", "description two", ""]
        .into_iter()
        .enumerate()
    {
        let sent = format!("$topic-{index}:example.org")
            .parse::<matrix_sdk::ruma::OwnedEventId>()
            .unwrap();
        let _http = server
            .mock_room_send_state()
            .for_type(StateEventType::RoomTopic)
            .ok(&sent)
            .mock_once()
            .mount_as_scoped()
            .await;
        let change =
            MatrixRoomSettingChange::Topic((!target.is_empty()).then(|| target.to_owned()));
        let mut write = Box::pin(update_room_setting(&session, room_id.as_str(), change));
        assert!(
            tokio::time::timeout(Duration::from_millis(150), write.as_mut())
                .await
                .is_err(),
            "HTTP acknowledgement must not report an uncommitted topic as successful"
        );
        // Neither the right event in another room nor the desired value under
        // another event ID proves this write reached this SDK room's state.
        server
            .mock_sync()
            .ok_and_run(&session.client(), |builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(other_id)
                        .add_state_event(factory.room_topic(target).event_id(&sent)),
                );
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id).add_state_event(
                        factory
                            .room_topic(target)
                            .event_id(event_id!("$unrelated:example.org")),
                    ),
                );
            })
            .await;
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(write.as_mut().poll(cx).is_pending()))
                .await
        );
        server
            .mock_sync()
            .ok_and_run(&session.client(), |builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id)
                        .add_state_event(factory.room_topic(target).event_id(&sent)),
                );
            })
            .await;
        let confirmed = write.await.unwrap();
        let expected = (!target.is_empty()).then(|| target.to_owned());
        assert_eq!(confirmed.topic, expected);
        assert_eq!(
            get_room_settings_snapshot(&session, room_id.as_str())
                .await
                .unwrap()
                .topic,
            expected
        );
    }
}

#[tokio::test]
async fn topic_commit_returns_current_remote_supersession_instead_of_requested_value() {
    let server = MatrixMockServer::new().await;
    let session = session(&server).await;
    let room_id = room_id!("!topic-superseded:example.org");
    server.sync_joined_room(&session.client(), room_id).await;
    let sent = event_id!("$local-write:example.org");
    let _http = server
        .mock_room_send_state()
        .for_type(StateEventType::RoomTopic)
        .ok(sent)
        .mock_once()
        .mount_as_scoped()
        .await;
    let mut write = Box::pin(update_room_setting(
        &session,
        room_id.as_str(),
        MatrixRoomSettingChange::Topic(Some("local".into())),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(150), write.as_mut())
            .await
            .is_err()
    );
    let factory = EventFactory::new().sender(session.client().user_id().unwrap());
    // Product sliding sync provides committed State::After batches. Preserve
    // the earlier canonical proof even if the owner consumes after supersession.
    use matrix_sdk::ruma::api::client::sync::sync_events::v5 as sliding;
    for (topic, id) in [
        ("local", sent),
        ("newer remote", event_id!("$later-remote:example.org")),
    ] {
        let mut response = sliding::Response::new(id.to_string());
        let mut update = sliding::response::Room::new();
        update
            .required_state
            .push(factory.room_topic(topic).event_id(id).into_raw_sync_state());
        response.rooms.insert(room_id.to_owned(), update);
        session
            .client()
            .process_sliding_sync_test_helper(&response, &Default::default())
            .await
            .unwrap();
    }
    assert_eq!(write.await.unwrap().topic.as_deref(), Some("newer remote"));
}

#[tokio::test]
async fn timeline_only_topic_does_not_settle_before_authoritative_sliding_sync_state() {
    use matrix_sdk::ruma::api::client::sync::sync_events::v5 as sliding;
    let server = MatrixMockServer::new().await;
    let session = session(&server).await;
    let room_id = room_id!("!topic-sliding:example.org");
    server.sync_joined_room(&session.client(), room_id).await;
    let sent = event_id!("$timeline-topic:example.org");
    let _http = server
        .mock_room_send_state()
        .for_type(StateEventType::RoomTopic)
        .ok(sent)
        .mock_once()
        .mount_as_scoped()
        .await;
    let mut write = Box::pin(update_room_setting(
        &session,
        room_id.as_str(),
        MatrixRoomSettingChange::Topic(Some("requested".into())),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(150), write.as_mut())
            .await
            .is_err()
    );
    let factory = EventFactory::new().sender(session.client().user_id().unwrap());
    let mut stale = sliding::Response::new("timeline-first".into());
    let mut room = sliding::response::Room::new();
    room.required_state.push(
        factory
            .room_topic("previous")
            .event_id(event_id!("$previous-topic:example.org"))
            .into_raw_sync_state(),
    );
    room.timeline.push(
        factory
            .room_topic("requested")
            .event_id(sent)
            .into_raw_sync(),
    );
    stale.rooms.insert(room_id.to_owned(), room);
    session
        .client()
        .process_sliding_sync_test_helper(&stale, &Default::default())
        .await
        .unwrap();
    assert_eq!(
        session
            .client()
            .get_room(room_id)
            .unwrap()
            .topic()
            .as_deref(),
        Some("previous")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), write.as_mut())
            .await
            .is_err(),
        "a timeline event is not proof that authoritative topic state committed"
    );
    let mut committed = sliding::Response::new("required-state-later".into());
    let mut room = sliding::response::Room::new();
    room.required_state.push(
        factory
            .room_topic("requested")
            .event_id(sent)
            .into_raw_sync_state(),
    );
    committed.rooms.insert(room_id.to_owned(), room);
    session
        .client()
        .process_sliding_sync_test_helper(&committed, &Default::default())
        .await
        .unwrap();
    assert_eq!(write.await.unwrap().topic.as_deref(), Some("requested"));
    assert_eq!(
        get_room_settings_snapshot(&session, room_id.as_str())
            .await
            .unwrap()
            .topic
            .as_deref(),
        Some("requested")
    );
}

#[tokio::test]
async fn acknowledged_event_already_canonical_needs_no_repeated_typed_callback() {
    let server = MatrixMockServer::new().await;
    let session = session(&server).await;
    let room_id = room_id!("!already-canonical:example.org");
    server.sync_joined_room(&session.client(), room_id).await;
    let sent = event_id!("$already-canonical:example.org");
    let factory = EventFactory::new().sender(session.client().user_id().unwrap());
    server
        .mock_sync()
        .ok_and_run(&session.client(), |builder| {
            builder.add_joined_room(
                JoinedRoomBuilder::new(room_id)
                    .add_state_event(factory.room_topic("requested").event_id(sent)),
            );
        })
        .await;
    let _http = server
        .mock_room_send_state()
        .for_type(StateEventType::RoomTopic)
        .ok(sent)
        .mock_once()
        .mount_as_scoped()
        .await;
    let result = tokio::time::timeout(
        Duration::from_millis(150),
        update_room_setting(
            &session,
            room_id.as_str(),
            MatrixRoomSettingChange::Topic(Some("requested".into())),
        ),
    )
    .await;
    assert!(
        result.is_ok(),
        "canonical SDK state must not require another typed event callback"
    );
    assert_eq!(result.unwrap().unwrap().topic.as_deref(), Some("requested"));
}
