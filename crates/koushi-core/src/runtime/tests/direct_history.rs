use super::*;

#[tokio::test]
async fn direct_history_preserves_open_gui_navigation_and_read_state_on_success_and_failure() {
    let data = tempfile::tempdir().unwrap();
    let state = AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".into(),
            user_id: "@member:example.invalid".into(),
            device_id: "SYNTHETIC".into(),
            authentication_method: Default::default(),
        }),
        ..Default::default()
    };
    let (mut actor, _, _, mut account, mut events, ..) =
        app_actor_event_navigation_fixture(data.path(), state);
    actor.state.rooms = vec![unread_diagnostic_room("!gui:example.invalid")];
    actor.state.navigation.active_room_id = Some("!gui:example.invalid".into());
    actor.state.navigation.active_space_id = Some("!space:example.invalid".into());
    let before = actor.state.clone();
    for failure in [false, true] {
        let (response, result) = oneshot::channel();
        actor
            .handle_direct_room_messages("!other:example.invalid".into(), 20, None, response)
            .await;
        let AccountMessage::DirectRoomMessages {
            room_id,
            limit,
            from,
            response,
        } = account.recv().await.unwrap()
        else {
            panic!("private history request")
        };
        assert_eq!(room_id, "!other:example.invalid");
        assert_eq!(limit, 20);
        assert!(from.is_none());
        if failure {
            let _ = response.send(Err(
                crate::direct_operations::RoomMessagesFailure::AccountInactive,
            ));
            assert!(result.await.unwrap().is_err());
        } else {
            let _ = response.send(Ok(koushi_protocol::event::RoomMessagesPage {
                messages: vec![],
                next: None,
            }));
            assert!(result.await.unwrap().is_ok());
        }
        assert_eq!(actor.state, before);
        assert!(events.try_recv().is_err());
        assert!(account.try_recv().is_err());
    }
    actor.state.session = SessionState::Locked(match before.session {
        SessionState::Ready(info) => info,
        _ => unreachable!(),
    });
    let locked = actor.state.clone();
    let (response, result) = oneshot::channel();
    actor
        .handle_direct_room_messages("!other:example.invalid".into(), 20, None, response)
        .await;
    assert!(result.await.unwrap().is_err());
    assert_eq!(actor.state, locked);
    assert!(account.try_recv().is_err());
    assert!(events.try_recv().is_err());
}
