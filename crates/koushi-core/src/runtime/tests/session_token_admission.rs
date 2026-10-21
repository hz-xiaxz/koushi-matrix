use super::*;

fn ready_info() -> SessionInfo {
    SessionInfo {
        homeserver: "https://example.invalid".into(),
        user_id: "@member:example.invalid".into(),
        device_id: "SYNTHETIC".into(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Password,
    }
}
#[tokio::test]
async fn private_token_admission_rejects_locked_or_wrong_account_without_gui_events() {
    let data = tempfile::tempdir().unwrap();
    let info = ready_info();
    let (mut actor, _, _, mut account, mut events, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(info.clone()),
            ..Default::default()
        },
    );
    for locked in [false, true] {
        let mut expected = info.clone();
        if locked {
            actor.state.session = SessionState::Locked(info.clone());
        } else {
            expected.user_id = "@other:example.invalid".into();
        }
        let before = actor.state.clone();
        let (response, result) = oneshot::channel();
        actor
            .handle_command(CoreCommandEnvelope::DirectSessionAccessToken { expected, response })
            .await;
        assert!(matches!(
            result.await.unwrap(),
            Err(crate::session_access_token::SessionAccessTokenError::AccountInactive)
        ));
        assert!(account.try_recv().is_err());
        assert!(events.try_recv().is_err());
        assert_eq!(actor.state, before);
    }
}
#[tokio::test]
async fn token_delivery_fence_rejects_session_revision_changes() {
    let data = tempfile::tempdir().unwrap();
    let info = ready_info();
    let (mut actor, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(info.clone()),
            ..Default::default()
        },
    );
    let old = actor.session_revision.unwrap();
    actor.reduce_app_action_state(AppAction::SessionLocked);
    actor.state.session = SessionState::Ready(info.clone());
    let (response, result) = oneshot::channel();
    actor
        .handle_command(CoreCommandEnvelope::ValidateSessionAccessToken {
            expected: info,
            revision: old,
            response,
        })
        .await;
    assert!(!result.await.unwrap());
}
#[tokio::test]
async fn token_admission_uses_authoritative_session_metadata() {
    let data = tempfile::tempdir().unwrap();
    let info = ready_info();
    let (mut actor, _, _, mut account, mut events, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(info.clone()),
            ..Default::default()
        },
    );
    let mut requested = info.clone();
    requested.authentication_method = Default::default();
    let (response, _result) = oneshot::channel();
    actor
        .handle_command(CoreCommandEnvelope::DirectSessionAccessToken {
            expected: requested,
            response,
        })
        .await;
    match account.recv().await.unwrap() {
        AccountMessage::DirectSessionAccessToken {
            expected, revision, ..
        } => {
            assert_eq!(expected, info);
            assert_eq!(Some(revision), actor.session_revision);
        }
        _ => panic!("expected private token read"),
    }
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn disclosure_epochs_are_unique_across_runtime_replacement_when_sync_epochs_match() {
    let (first, _first_stores) = CoreRuntime::start_isolated();
    let (second, _second_stores) = CoreRuntime::start_isolated();
    let first_connection = first.attach();
    let second_connection = second.attach();
    let first_epoch = first_connection.snapshot().session_disclosure_epoch;
    let second_epoch = second_connection.snapshot().session_disclosure_epoch;
    assert!(first_epoch > 0 && first_epoch < (1u64 << 53));
    assert!(second_epoch > 0 && second_epoch < (1u64 << 53));
    assert_ne!(first_epoch, second_epoch);
    assert_eq!(
        first_connection.snapshot().session,
        second_connection.snapshot().session
    );
    assert_eq!(
        first_connection.snapshot().sliding_sync_account_epoch,
        second_connection.snapshot().sliding_sync_account_epoch
    );
    drop(first_connection);
    drop(second_connection);
    first.shutdown().await;
    second.shutdown().await;
}

#[tokio::test]
async fn disclosure_epoch_survives_same_state_reducer_reset() {
    let data = tempfile::tempdir().unwrap();
    let epoch = crate::session_access_token::next_disclosure_epoch();
    let (mut actor, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(ready_info()),
            session_disclosure_epoch: epoch,
            ..Default::default()
        },
    );
    actor.state.session = SessionState::SignedOut;
    let revision = actor.session_revision;
    actor.reduce_app_action_state(AppAction::LogoutFinished);
    assert_eq!(actor.state.session, SessionState::SignedOut);
    assert_eq!(actor.state.session_disclosure_epoch, epoch);
    assert_eq!(actor.session_revision, revision);
}

#[tokio::test]
async fn each_actual_session_transition_invalidates_disclosure_epoch() {
    let data = tempfile::tempdir().unwrap();
    let initial = crate::session_access_token::next_disclosure_epoch();
    let (mut actor, ..) = app_actor_event_navigation_fixture(
        data.path(),
        AppState {
            session: SessionState::Ready(ready_info()),
            session_disclosure_epoch: initial,
            ..Default::default()
        },
    );
    let mut epochs = std::collections::BTreeSet::from([initial]);
    for action in [
        AppAction::SessionAuthenticationInvalidated { soft_logout: true },
        AppAction::LogoutRequested,
        AppAction::LogoutFinished,
    ] {
        let previous = actor.state.session.clone();
        actor.reduce_app_action_state(action);
        assert_ne!(actor.state.session, previous);
        let epoch = actor.state.session_disclosure_epoch;
        assert!(epoch > 0 && epoch < (1u64 << 53));
        assert!(epochs.insert(epoch));
    }
}
