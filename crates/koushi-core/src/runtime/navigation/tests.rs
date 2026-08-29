use super::*;
use crate::ids::{AccountKey, RuntimeConnectionId};
use koushi_state::SessionInfo;

fn focused_projection_fixture(sequence: u64) -> PendingFocusedNavigation {
    PendingFocusedNavigation {
        projection_request_id: RequestId {
            connection_id: RuntimeConnectionId(3),
            sequence,
        },
        key: TimelineKey {
            account_key: AccountKey("@qa:example.invalid".to_owned()),
            kind: TimelineKind::Focused {
                room_id: "!room:example.invalid".to_owned(),
                event_id: "$target".to_owned(),
            },
        },
        room_id: "!room:example.invalid".to_owned(),
        event_id: "$target".to_owned(),
        allow_live_fallback: true,
    }
}
#[test]
fn focused_projection_ack_requires_same_owner_and_key_and_is_idempotent() {
    let expected = focused_projection_fixture(9);
    let mut pending = Some(expected.clone());
    let stale_id = RequestId {
        connection_id: RuntimeConnectionId(3),
        sequence: 8,
    };
    assert!(take_acknowledged_focused_navigation(&mut pending, stale_id, &expected.key).is_none());
    assert_eq!(pending, Some(expected.clone()));

    let wrong_key = TimelineKey::room(
        AccountKey("@qa:example.invalid".to_owned()),
        "!room:example.invalid",
    );
    assert!(
        take_acknowledged_focused_navigation(
            &mut pending,
            expected.projection_request_id,
            &wrong_key,
        )
        .is_none()
    );
    assert_eq!(pending, Some(expected.clone()));

    assert_eq!(
        take_acknowledged_focused_navigation(
            &mut pending,
            expected.projection_request_id,
            &expected.key,
        ),
        Some(expected.clone())
    );
    assert!(pending.is_none());
    assert!(
        take_acknowledged_focused_navigation(
            &mut pending,
            expected.projection_request_id,
            &expected.key,
        )
        .is_none()
    );
}
#[test]
fn focused_anchor_action_is_impossible_before_actor_acceptance() {
    let expected = focused_projection_fixture(12);
    let mut pending = Some(expected.clone());
    assert!(
        anchored_action_after_projection_ack(
            &mut pending,
            expected.projection_request_id,
            &expected.key,
            false,
            true,
            true,
        )
        .is_none()
    );
    assert_eq!(pending, Some(expected.clone()));

    let action = anchored_action_after_projection_ack(
        &mut pending,
        expected.projection_request_id,
        &expected.key,
        true,
        true,
        true,
    )
    .expect("accepted exact projection advances the anchor");
    assert!(matches!(
        action,
        AppAction::EnterAnchoredTimeline { room_id, event_id }
            if room_id == expected.room_id && event_id == expected.event_id
    ));
    assert!(pending.is_none());

    let mut target_missing = Some(expected.clone());
    assert_eq!(
        anchored_action_after_projection_ack(
            &mut target_missing,
            expected.projection_request_id,
            &expected.key,
            true,
            false,
            true,
        ),
        Some(AppAction::CloseFocusedContext)
    );
    assert!(
        target_missing.is_none(),
        "an accepted target-missing projection must terminate the focused attempt"
    );

    let mut actor_missing = Some(expected.clone());
    assert_eq!(
        anchored_action_after_projection_ack(
            &mut actor_missing,
            expected.projection_request_id,
            &expected.key,
            true,
            true,
            false,
        ),
        Some(AppAction::CloseFocusedContext)
    );
    assert!(
        actor_missing.is_none(),
        "the frontend and actor must both prove that the target is present"
    );

    let thread_key = TimelineKey {
        account_key: expected.key.account_key.clone(),
        kind: TimelineKind::Thread {
            room_id: expected.room_id,
            root_event_id: "$thread-root".to_owned(),
        },
    };
    assert!(
        anchored_action_after_projection_ack(
            &mut pending,
            expected.projection_request_id,
            &thread_key,
            true,
            true,
            true,
        )
        .is_none()
    );
}
#[test]
fn focused_navigation_lifecycle_uses_the_reduced_state() {
    let expected = focused_projection_fixture(13);
    let mut state = AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@synthetic:example.invalid".to_owned(),
            device_id: "SYNTHETIC".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        }),
        focused_context: FocusedContextState::Open {
            room_id: expected.room_id.clone(),
            event_id: expected.event_id.clone(),
            is_subscribed: true,
        },
        ..AppState::default()
    };
    state.navigation.active_room_id = Some(expected.room_id.clone());
    state.navigation.main_timeline_anchor = Some(koushi_state::MainTimelineAnchor {
        event_id: expected.event_id.clone(),
    });
    assert_eq!(
        focused_navigation_outcome_after_reduce(&state, &expected, true),
        IntentOutcome::Committed
    );

    state.navigation.main_timeline_anchor = None;
    state.focused_context = FocusedContextState::Closed;
    assert_eq!(
        focused_navigation_outcome_after_reduce(&state, &expected, false),
        IntentOutcome::BenignNoOp(IntentNoOpReason::TimelineTargetMissing)
    );

    let mut pinned_navigation = expected.clone();
    pinned_navigation.allow_live_fallback = false;
    assert_eq!(
        focused_navigation_outcome_after_reduce(&state, &pinned_navigation, false),
        IntentOutcome::FailedNoOp(IntentNoOpReason::TimelineTargetMissing)
    );

    state.navigation.active_room_id = Some("!other:example.invalid".to_owned());
    assert_eq!(
        focused_navigation_outcome_after_reduce(&state, &expected, true),
        IntentOutcome::FailedNoOp(IntentNoOpReason::RoomNotInState)
    );
}
#[test]
fn replacement_focused_helper_preserves_same_key_and_unsubscribes_different_key() {
    let account_key = AccountKey("@alice:example.invalid".to_owned());
    let current = TimelineKey {
        account_key: account_key.clone(),
        kind: TimelineKind::Focused {
            room_id: "!room:example.invalid".to_owned(),
            event_id: "$event-a:example.invalid".to_owned(),
        },
    };
    let same = current.clone();
    let different = TimelineKey {
        account_key,
        kind: TimelineKind::Focused {
            room_id: "!room:example.invalid".to_owned(),
            event_id: "$event-b:example.invalid".to_owned(),
        },
    };

    assert_eq!(
        unsubscribe_replaced_focused_context_timeline_key(Some(current.clone()), same),
        None
    );
    assert_eq!(
        unsubscribe_replaced_focused_context_timeline_key(Some(current.clone()), different),
        Some(current)
    );
    assert_eq!(
        unsubscribe_replaced_focused_context_timeline_key(
            None,
            focused_key("$event-c:example.invalid")
        ),
        None
    );
}
#[test]
fn select_space_cleanup_targets_previous_room_only_when_active_room_changes() {
    let action = AppAction::SelectSpace {
        space_id: Some("!space:example.invalid".to_owned()),
    };

    assert_eq!(
        navigation_replacement_room_for_cleanup(
            &action,
            Some("!old:example.invalid"),
            Some("!next:example.invalid"),
        ),
        Some(NavigationReplacementRoomForCleanup::Room(
            "!next:example.invalid".to_owned()
        ))
    );
    assert_eq!(
        navigation_replacement_room_for_cleanup(&action, Some("!old:example.invalid"), None,),
        Some(NavigationReplacementRoomForCleanup::Cleared)
    );
    assert_eq!(
        navigation_replacement_room_for_cleanup(
            &action,
            Some("!same:example.invalid"),
            Some("!same:example.invalid"),
        ),
        None
    );
    assert_eq!(
        navigation_replacement_room_for_cleanup(&action, None, None),
        None
    );
}
#[test]
fn select_room_cleanup_still_uses_explicit_target_room() {
    let action = AppAction::SelectRoom {
        room_id: "!target:example.invalid".to_owned(),
    };

    assert_eq!(
        navigation_replacement_room_for_cleanup(
            &action,
            Some("!old:example.invalid"),
            Some("!target:example.invalid"),
        ),
        Some(NavigationReplacementRoomForCleanup::Room(
            "!target:example.invalid".to_owned()
        ))
    );
}
fn focused_key(event_id: &str) -> TimelineKey {
    TimelineKey {
        account_key: AccountKey("@alice:example.invalid".to_owned()),
        kind: TimelineKind::Focused {
            room_id: "!room:example.invalid".to_owned(),
            event_id: event_id.to_owned(),
        },
    }
}
