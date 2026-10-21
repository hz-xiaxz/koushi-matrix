use super::*;
use koushi_diagnostics::{NavigationStage, navigation_snapshot};
use koushi_state::{RoomSummary, SessionInfo, SessionState, reduce};

fn room(id: &str, dm: bool) -> RoomSummary {
    serde_json::from_value(serde_json::json!({
        "room_id":id,"display_name":id,"display_label":id,"original_display_label":id,
        "is_dm":dm,"unread_count":0,"notification_count":0,"highlight_count":0,
        "parent_space_ids":[],"is_encrypted":false,"joined_members":0
    }))
    .unwrap()
}

fn state() -> AppState {
    AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://private.example.test".into(),
            user_id: "@private:example.test".into(),
            device_id: "PRIVATE_DEVICE".into(),
            authentication_method: Default::default(),
        }),
        rooms: vec![
            room("!private-a:example.test", false),
            room("!private-b:example.test", true),
        ],
        ..AppState::default()
    }
}

fn apply(state: &mut AppState, action: AppAction) {
    let observation = NavigationObservation::begin(state, &action, NavigationOrigin::Unknown, None);
    reduce(state, action);
    observation.finish(state);
}

#[test]
fn records_real_select_noop_reject_and_home_clear_without_private_ids() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let mut state = state();
    apply(
        &mut state,
        AppAction::SelectRoom {
            room_id: "!private-a:example.test".into(),
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .stage,
        NavigationStage::Committed
    );
    apply(
        &mut state,
        AppAction::SelectRoom {
            room_id: "!private-a:example.test".into(),
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .stage,
        NavigationStage::NoOp
    );
    apply(
        &mut state,
        AppAction::SelectRoom {
            room_id: "!missing:example.test".into(),
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .stage,
        NavigationStage::Rejected
    );
    apply(&mut state, AppAction::SelectSpace { space_id: None });
    let snapshot = navigation_snapshot();
    let last = &snapshot.records.last().unwrap().diagnostic;
    assert_eq!(last.stage, NavigationStage::Committed);
    assert!(last.previous_room_alias.is_some());
    assert!(last.committed_room_alias.is_none());
    assert_eq!(last.origin, NavigationOrigin::Unknown);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    for private in ["private", "example.test", "PRIVATE_DEVICE", "!missing"] {
        assert!(!serialized.contains(private));
    }
}

#[test]
fn records_automatic_fallback_and_dm_facts_from_real_reducer() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let mut state = state();
    apply(
        &mut state,
        AppAction::SelectRoom {
            room_id: "!private-a:example.test".into(),
        },
    );
    apply(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: vec![],
            rooms: vec![room("!private-b:example.test", true)],
        },
    );
    let record = navigation_snapshot().records.last().unwrap().diagnostic;
    assert_eq!(record.cause, NavigationCause::RoomListReplacement);
    assert_eq!(record.old_room_present, Some(false));
    assert_eq!(record.old_dm, Some(false));
    assert_eq!(record.new_dm, None);
    assert!(state.navigation.active_room_id.is_none());
    apply(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: vec![],
            rooms: vec![room("!private-b:example.test", true)],
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .new_dm,
        Some(true)
    );
    assert_eq!(record.list_authoritative, Some(true));
}

#[test]
fn alias_lookup_is_bounded_resets_ledger_and_never_reuses_ordinals() {
    let _guard = koushi_diagnostics::test_support::lock();
    let baseline_resets = navigation_snapshot().reset_count;
    let mut aliases = Aliases::default();
    let before = Capture::from_state(&state());
    let first = aliases
        .alias(AliasKey::Room(before.account.clone(), "first".into()))
        .unwrap();
    for index in 0..2048 {
        aliases.prepare(&[AliasKey::Room(
            before.account.clone(),
            format!("room-{index}"),
        )]);
        aliases.alias(AliasKey::Room(
            before.account.clone(),
            format!("room-{index}"),
        ));
    }
    assert!(aliases.values.len() <= 2048);
    assert!(navigation_snapshot().reset_count > baseline_resets);
    let last = aliases
        .alias(AliasKey::Room(before.account, "last".into()))
        .unwrap();
    assert!(last > first);
}

#[test]
fn event_navigation_completion_and_stale_generation_are_distinguished() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let mut state = state();
    apply(
        &mut state,
        AppAction::EventNavigationStarted {
            source: EventNavigationSource::Notification,
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .cause,
        NavigationCause::Notification
    );
    apply(
        &mut state,
        AppAction::EventNavigationAnchored { generation: 999 },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .stage,
        NavigationStage::NoOp
    );
    let generation = state.navigation.event_navigation.generation();
    apply(
        &mut state,
        AppAction::EventNavigationAnchored { generation },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .stage,
        NavigationStage::Completed
    );
}

#[test]
fn session_clear_preserves_numeric_request_and_resets_alias_scope() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let mut state = state();
    apply(
        &mut state,
        AppAction::SelectRoom {
            room_id: "!private-a:example.test".into(),
        },
    );
    let resets = navigation_snapshot().reset_count;
    let action = AppAction::LogoutRequested;
    let observation = NavigationObservation::begin(
        &state,
        &action,
        NavigationOrigin::RendererKeyboard,
        Some(RequestId {
            connection_id: koushi_protocol::RuntimeConnectionId(2),
            sequence: 9,
        }),
    );
    reduce(&mut state, action);
    observation.finish(&state);
    let snapshot = navigation_snapshot();
    assert!(snapshot.reset_count > resets);
    let last = &snapshot.records.last().unwrap().diagnostic;
    assert_eq!(last.cause, NavigationCause::SessionReset);
    assert_eq!(last.request_connection, Some(2));
    assert_eq!(last.request_sequence, Some(9));
    assert!(last.previous_account_alias.is_some());
    assert!(last.committed_account_alias.is_none());
    assert!(last.committed_room_alias.is_none());
}

#[test]
fn direct_dm_completion_records_its_own_before_after_scope() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let before = state();
    let mut after = before.clone();
    reduce(
        &mut after,
        AppAction::SelectRoom {
            room_id: "!private-b:example.test".into(),
        },
    );
    let (updates, snapshots) =
        tokio::sync::watch::channel(koushi_protocol::state_update::VersionedAppStateSnapshot {
            generation: 0,
            state: before,
        });
    let operation = NavigationOperation::new(
        snapshots,
        NavigationCause::StartDm,
        NavigationOrigin::Mcp,
        RequestId {
            connection_id: koushi_protocol::RuntimeConnectionId(2),
            sequence: 1,
        },
    );
    updates.send_replace(koushi_protocol::state_update::VersionedAppStateSnapshot {
        generation: 1,
        state: after,
    });
    operation.finish(true);
    let last = navigation_snapshot().records.last().unwrap().diagnostic;
    assert_eq!(last.cause, NavigationCause::StartDm);
    assert_eq!(last.origin, NavigationOrigin::Mcp);
    assert!(last.previous_room_alias.is_none());
    assert!(last.committed_room_alias.is_some());
    assert_eq!(last.new_dm, Some(true));
    assert!(!last.authoritative_transition);
}

#[test]
fn list_authority_comes_from_the_exact_reducer_variant() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let mut provisional = state();
    apply(
        &mut provisional,
        AppAction::RoomListBootstrapStarted {
            generation: 1,
            source: koushi_state::RoomListSource::Live,
        },
    );
    apply(
        &mut provisional,
        AppAction::RoomListSnapshotProvisional {
            generation: 1,
            source: koushi_state::RoomListSource::Live,
            spaces: vec![],
            rooms: vec![room("!private-a:example.test", false)],
            invites: vec![],
        },
    );
    assert_eq!(
        navigation_snapshot()
            .records
            .last()
            .unwrap()
            .diagnostic
            .list_authoritative,
        Some(false)
    );
    apply(
        &mut provisional,
        AppAction::RoomListSnapshotAuthoritative {
            generation: 1,
            source: koushi_state::RoomListSource::Live,
            spaces: vec![],
            rooms: vec![],
            invites: vec![],
        },
    );
    let last = navigation_snapshot().records.last().unwrap().diagnostic;
    assert_eq!(last.list_authoritative, Some(true));
    assert_eq!(last.old_room_present, Some(false));
    assert!(last.committed_room_alias.is_none());
}

#[test]
fn aliases_separate_identity_namespaces_and_session_epochs() {
    let _guard = koushi_diagnostics::test_support::lock();
    let mut aliases = Aliases::default();
    let mut account = Capture::from_state(&state()).account.unwrap();
    let room = aliases
        .alias(AliasKey::Room(
            Some(account.clone()),
            "same-identity".into(),
        ))
        .unwrap();
    let space = aliases
        .alias(AliasKey::Space(
            Some(account.clone()),
            "same-identity".into(),
        ))
        .unwrap();
    assert_ne!(room, space);
    account.epoch += 1;
    let next_session = aliases
        .alias(AliasKey::Room(Some(account), "same-identity".into()))
        .unwrap();
    assert_ne!(room, next_session);
}

#[test]
fn selected_account_publication_is_one_authoritative_record() {
    let _guard = koushi_diagnostics::test_support::lock();
    koushi_diagnostics::reset_navigation();
    let before = state();
    let mut after = before.clone();
    if let SessionState::Ready(info) = &mut after.session {
        info.user_id = "@second:example.test".into();
    }
    after.session_disclosure_epoch = before.session_disclosure_epoch + 1;
    record_account_selection(&before, &after, true);
    let snapshot = navigation_snapshot();
    assert_eq!(snapshot.records.len(), 1);
    let record = snapshot.records[0].diagnostic;
    assert_eq!(record.cause, NavigationCause::SelectAccount);
    assert_eq!(record.stage, NavigationStage::Committed);
    assert!(record.authoritative_transition);
    assert_ne!(
        record.previous_account_alias,
        record.committed_account_alias
    );
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains("second"));
    assert!(!serialized.contains("example.test"));
}

#[test]
fn event_admission_preserves_bounded_source_categories() {
    let _guard = koushi_diagnostics::test_support::lock();
    for (source, expected) in [
        (
            EventNavigationSource::Activity,
            NavigationCause::ActivityNavigation,
        ),
        (
            EventNavigationSource::Search,
            NavigationCause::SearchNavigation,
        ),
        (
            EventNavigationSource::Pinned,
            NavigationCause::PinnedNavigation,
        ),
    ] {
        koushi_diagnostics::reset_navigation();
        let mut state = state();
        apply(&mut state, AppAction::EventNavigationStarted { source });
        assert!(
            navigation_snapshot()
                .records
                .iter()
                .any(|record| record.diagnostic.cause == expected)
        );
    }
}
