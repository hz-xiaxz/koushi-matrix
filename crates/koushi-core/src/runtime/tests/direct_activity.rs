use super::*;

#[test]
fn recent_delivery_revalidates_only_observed_room_notification_modes() {
    use koushi_state::{RoomNotificationMode, RoomNotificationSettings};
    let mut state = AppState::default();
    let read = crate::direct_activity::RecentActivitySnapshot {
        session: SessionInfo {
            homeserver: "https://example.invalid".into(),
            user_id: "@member:example.invalid".into(),
            device_id: "SYNTHETIC".into(),
            authentication_method: Default::default(),
        },
        session_revision: 1,
        session_epoch: 1,
        notification_modes: std::collections::BTreeMap::from([(
            "!observed:example.invalid".into(),
            RoomNotificationMode::All,
        )]),
        rows: vec![],
    };
    assert!(
        read.matches_notification_modes(&state),
        "missing policy means All"
    );
    state.room_notification_settings.insert(
        "!unrelated:example.invalid".into(),
        RoomNotificationSettings {
            mode: RoomNotificationMode::Mentions,
            ..Default::default()
        },
    );
    assert!(read.matches_notification_modes(&state));
    state.room_notification_settings.insert(
        "!observed:example.invalid".into(),
        RoomNotificationSettings {
            mode: RoomNotificationMode::All,
            ..Default::default()
        },
    );
    assert!(read.matches_notification_modes(&state));
    state
        .room_notification_settings
        .get_mut("!observed:example.invalid")
        .unwrap()
        .mode = RoomNotificationMode::Mentions;
    assert!(
        !read.matches_notification_modes(&state),
        "All projection must not be admitted under Mentions policy"
    );
}

#[tokio::test]
async fn recent_delivery_rejects_same_identity_after_session_lifetime_changes() {
    let data = tempfile::tempdir().unwrap();
    let info = SessionInfo {
        homeserver: "https://example.invalid".into(),
        user_id: "@member:example.invalid".into(),
        device_id: "SYNTHETIC".into(),
        authentication_method: Default::default(),
    };
    let (mut actor, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(info.clone()),
            ..Default::default()
        },
    );
    let reply_waiting_for_delivery = actor.read_recent_activity().unwrap();
    assert!(reply_waiting_for_delivery.matches_session(&actor.state));
    actor.reduce_app_action_state(AppAction::SessionLocked);
    assert!(!reply_waiting_for_delivery.matches_session(&actor.state));
    actor.reduce_app_action_state(AppAction::RestoreSessionRequested);
    actor.reduce_app_action_state(AppAction::RestoreSessionSucceeded(info));
    actor.reduce_app_action_state(AppAction::CurrentDeviceTrustChanged(
        koushi_state::CurrentDeviceTrustState::Verified,
    ));
    assert_eq!(
        actor.state.session,
        SessionState::Ready(reply_waiting_for_delivery.session.clone())
    );
    assert_ne!(
        actor.state.session_disclosure_epoch,
        reply_waiting_for_delivery.session_epoch
    );
    assert!(!reply_waiting_for_delivery.matches_session(&actor.state));
    assert!(
        actor
            .read_recent_activity()
            .unwrap()
            .matches_session(&actor.state)
    );
}

#[tokio::test]
async fn recent_read_works_with_activity_closed_and_preserves_gui_state() {
    let data = tempfile::tempdir().unwrap();
    let session = SessionInfo {
        homeserver: "https://example.invalid".into(),
        user_id: "@member:example.invalid".into(),
        device_id: "SYNTHETIC".into(),
        authentication_method: Default::default(),
    };
    let mut state = AppState {
        session: SessionState::Ready(session),
        ..Default::default()
    };
    state.rooms = vec![unread_diagnostic_room("!room:example.invalid")];
    let (mut actor, _, _, _, mut events, ..) =
        app_actor_event_navigation_fixture(data.path(), state);
    actor
        .activity_projection
        .ingest(vec![koushi_state::ActivityRow::event(
            "!room:example.invalid".into(),
            "$event:example.invalid".into(),
            Some("@sender:example.invalid".into()),
            "Synthetic room".into(),
            None,
            Some("Synthetic preview".into()),
            100,
            false,
            false,
        )]);
    let before = actor.state.clone();
    let read = actor.read_recent_activity().expect("ready read");
    assert_eq!(read.session_epoch, before.session_disclosure_epoch);
    assert_eq!(
        read.notification_modes.get("!room:example.invalid"),
        Some(&koushi_state::RoomNotificationMode::All)
    );
    assert!(
        read.rows
            .iter()
            .any(|row| row.event_id.as_deref() == Some("$event:example.invalid"))
    );
    assert_eq!(actor.state, before);
    assert!(
        events.try_recv().is_err(),
        "private read publishes no GUI event"
    );
}

#[tokio::test]
async fn recent_read_fails_for_inactive_session() {
    let data = tempfile::tempdir().unwrap();
    let (mut actor, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(SessionInfo {
                homeserver: "https://example.invalid".into(),
                user_id: "@member:example.invalid".into(),
                device_id: "SYNTHETIC".into(),
                authentication_method: Default::default(),
            }),
            ..Default::default()
        },
    );
    actor.state.session = SessionState::SignedOut;
    assert!(actor.read_recent_activity().is_err());
}
