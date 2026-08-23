//! Reducer tests for the temporary dangerous encryption-debug operation
//! state machine (issue #538).
use koushi_state::{
    AppAction, AppEffect, AppState, EncryptionDebugOperationKind, EncryptionDebugOperationOutcome,
    EncryptionDebugOperationState, RoomListSource, RoomSummary, SessionInfo, SessionState, UiEvent,
    reduce,
};

fn ready_state() -> AppState {
    AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://matrix.example.invalid".to_owned(),
            user_id: "@debug:example.invalid".to_owned(),
            device_id: "DEBUG".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        }),
        ..AppState::default()
    }
}

fn with_room(state: &mut AppState, room_id: &str) {
    state.rooms.push(RoomSummary {
        room_id: room_id.to_owned(),
        display_name: "Room".to_owned(),
        display_label: "Room".to_owned(),
        original_display_label: "Room".to_owned(),
        avatar: None,
        is_dm: false,
        dm_user_ids: Vec::new(),
        tags: koushi_state::RoomTags::default(),
        unread_count: 0,
        notification_count: 0,
        highlight_count: 0,
        marked_unread: false,
        recency_stamp: None,
        conversation_activity: None,
        latest_event: None,
        parent_space_ids: Vec::new(),
        dm_space_ids: Vec::new(),
        is_encrypted: true,
        joined_members: 1,
    });
}

fn operation<'a>(state: &'a AppState, room_id: &str) -> &'a EncryptionDebugOperationState {
    &state
        .room_interactions
        .get(room_id)
        .expect("room interaction entry")
        .encryption_debug_operation
}

#[test]
fn settled_and_failed_operations_are_admitted_in_transient_session_contexts() {
    let info = SessionInfo {
        homeserver: "https://matrix.example.invalid".to_owned(),
        user_id: "@debug:example.invalid".to_owned(),
        device_id: "DEBUG".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    };
    for session in [
        SessionState::Locked(info.clone()),
        SessionState::SwitchingAccount { info: info.clone() },
    ] {
        let mut settled = ready_state();
        with_room(&mut settled, "!r:example.invalid");
        reduce(
            &mut settled,
            AppAction::EncryptionDebugOperationStarted {
                request_id: 7,
                room_id: "!r:example.invalid".to_owned(),
                kind: EncryptionDebugOperationKind::ShareIndex0Key,
            },
        );
        settled.session = session.clone();
        let effects = reduce(
            &mut settled,
            AppAction::EncryptionDebugOperationSettled {
                request_id: 7,
                room_id: "!r:example.invalid".to_owned(),
                kind: EncryptionDebugOperationKind::ShareIndex0Key,
                outcome: EncryptionDebugOperationOutcome::Completed,
            },
        );
        assert!(matches!(
            operation(&settled, "!r:example.invalid"),
            EncryptionDebugOperationState::Settled { request_id: 7, .. }
        ));
        assert_eq!(
            effects,
            vec![AppEffect::EmitUiEvent(UiEvent::RoomInteractionsChanged)]
        );

        let mut failed = ready_state();
        with_room(&mut failed, "!r:example.invalid");
        reduce(
            &mut failed,
            AppAction::EncryptionDebugOperationStarted {
                request_id: 8,
                room_id: "!r:example.invalid".to_owned(),
                kind: EncryptionDebugOperationKind::ResendIndex0Key,
            },
        );
        failed.session = session;
        let effects = reduce(
            &mut failed,
            AppAction::EncryptionDebugOperationFailed {
                request_id: 8,
                room_id: "!r:example.invalid".to_owned(),
                kind: EncryptionDebugOperationKind::ResendIndex0Key,
                outcome: EncryptionDebugOperationOutcome::CancelledStale,
            },
        );
        assert!(matches!(
            operation(&failed, "!r:example.invalid"),
            EncryptionDebugOperationState::Failed { request_id: 8, .. }
        ));
        assert_eq!(
            effects,
            vec![AppEffect::EmitUiEvent(UiEvent::RoomInteractionsChanged)]
        );
    }
}

#[test]
fn start_is_admitted_from_idle_and_pending_starts_are_rejected() {
    let mut state = ready_state();
    with_room(&mut state, "!r:example.invalid");

    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 1,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ForceNewOutboundSession,
        },
    );
    assert!(!effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid"),
        &EncryptionDebugOperationState::Pending {
            request_id: 1,
            kind: EncryptionDebugOperationKind::ForceNewOutboundSession,
        }
    );

    // A second start while pending is rejected.
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 2,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );
    assert!(effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid").request_id(),
        Some(1)
    );
}

#[test]
fn settle_requires_matching_request_and_kind() {
    let mut state = ready_state();
    with_room(&mut state, "!r:example.invalid");
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 7,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );

    // Mismatched request id is dropped (stale completion).
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationSettled {
            request_id: 8,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
            outcome: EncryptionDebugOperationOutcome::Completed,
        },
    );
    assert!(effects.is_empty());
    assert!(matches!(
        operation(&state, "!r:example.invalid"),
        EncryptionDebugOperationState::Pending { request_id: 7, .. }
    ));

    // Mismatched kind is dropped.
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationSettled {
            request_id: 7,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ForceNewOutboundSession,
            outcome: EncryptionDebugOperationOutcome::Completed,
        },
    );
    assert!(effects.is_empty());

    // Matching settle transitions to Settled.
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationSettled {
            request_id: 7,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
            outcome: EncryptionDebugOperationOutcome::Completed,
        },
    );
    assert!(!effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid"),
        &EncryptionDebugOperationState::Settled {
            request_id: 7,
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
            outcome: EncryptionDebugOperationOutcome::Completed,
        }
    );

    // A retry from Settled is admitted.
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 9,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );
    assert!(!effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid").request_id(),
        Some(9)
    );
}

#[test]
fn resend_operation_is_fenced_by_reset_and_stale_settlement() {
    let mut state = ready_state();
    with_room(&mut state, "!r:example.invalid");
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 41,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
        },
    );
    assert_eq!(
        operation(&state, "!r:example.invalid"),
        &EncryptionDebugOperationState::Pending {
            request_id: 41,
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
        }
    );

    // Room/session teardown fences the in-flight resend before a completion
    // can settle it.
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationReset {
            room_id: "!r:example.invalid".to_owned(),
        },
    );
    assert!(operation(&state, "!r:example.invalid").is_idle());
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationSettled {
            request_id: 41,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
            outcome: EncryptionDebugOperationOutcome::Completed,
        },
    );
    assert!(effects.is_empty());
    assert!(operation(&state, "!r:example.invalid").is_idle());
}

#[test]
fn failure_maps_to_failed_and_retry_is_admitted() {
    let mut state = ready_state();
    with_room(&mut state, "!r:example.invalid");
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 3,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationFailed {
            request_id: 3,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
            outcome: EncryptionDebugOperationOutcome::RefusedIndexAdvanced,
        },
    );
    assert!(!effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid"),
        &EncryptionDebugOperationState::Failed {
            request_id: 3,
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
            outcome: EncryptionDebugOperationOutcome::RefusedIndexAdvanced,
        }
    );

    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 4,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );
    assert!(!effects.is_empty());
    assert_eq!(
        operation(&state, "!r:example.invalid").request_id(),
        Some(4)
    );
}

#[test]
fn authoritative_room_removal_drops_interaction_and_notifies_the_ui() {
    let mut state = ready_state();
    with_room(&mut state, "!removed:example.invalid");
    reduce(
        &mut state,
        AppAction::RoomListBootstrapStarted {
            generation: 2,
            source: RoomListSource::Live,
        },
    );
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 7,
            room_id: "!removed:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
        },
    );

    let effects = reduce(
        &mut state,
        AppAction::RoomListSnapshotAuthoritative {
            generation: 2,
            source: RoomListSource::Live,
            spaces: Vec::new(),
            rooms: Vec::new(),
            invites: Vec::new(),
        },
    );

    assert!(
        !state
            .room_interactions
            .contains_key("!removed:example.invalid")
    );
    assert!(effects.iter().any(|effect| {
        matches!(
            effect,
            AppEffect::EmitUiEvent(UiEvent::RoomInteractionsChanged)
        )
    }));
}

#[test]
fn provisional_room_removal_keeps_interaction_state() {
    let mut state = ready_state();
    with_room(&mut state, "!provisional:example.invalid");
    reduce(
        &mut state,
        AppAction::RoomListBootstrapStarted {
            generation: 2,
            source: RoomListSource::Live,
        },
    );
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 8,
            room_id: "!provisional:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ResendIndex0Key,
        },
    );

    reduce(
        &mut state,
        AppAction::RoomListSnapshotProvisional {
            generation: 2,
            source: RoomListSource::Live,
            spaces: Vec::new(),
            rooms: Vec::new(),
            invites: Vec::new(),
        },
    );

    assert!(matches!(
        operation(&state, "!provisional:example.invalid"),
        EncryptionDebugOperationState::Pending { request_id: 8, .. }
    ));
}

#[test]
fn lifecycle_reset_returns_to_idle() {
    let mut state = ready_state();
    with_room(&mut state, "!r:example.invalid");
    reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 5,
            room_id: "!r:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ForceNewOutboundSession,
        },
    );
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationReset {
            room_id: "!r:example.invalid".to_owned(),
        },
    );
    assert!(!effects.is_empty());
    assert!(operation(&state, "!r:example.invalid").is_idle());
}

#[test]
fn unknown_room_never_admits_a_start() {
    let mut state = ready_state();
    // No room entry at all.
    let effects = reduce(
        &mut state,
        AppAction::EncryptionDebugOperationStarted {
            request_id: 6,
            room_id: "!missing:example.invalid".to_owned(),
            kind: EncryptionDebugOperationKind::ShareIndex0Key,
        },
    );
    assert!(effects.is_empty());
    assert!(state.room_interactions.is_empty());
}

#[test]
fn outcomes_serialize_privately_without_identifiers() {
    use serde_json::json;
    let value = serde_json::to_value(EncryptionDebugOperationState::Settled {
        request_id: 1,
        kind: EncryptionDebugOperationKind::ForceNewOutboundSession,
        outcome: EncryptionDebugOperationOutcome::RefusedIndexAdvanced,
    })
    .unwrap();
    let text = value.to_string();
    assert!(!text.contains("room"));
    assert!(!text.contains("user"));
    assert!(!text.contains("session"));
    assert_eq!(
        value,
        json!({
            "state": "settled",
            "request_id": 1,
            "kind": "forceNewOutboundSession",
            "outcome": "refusedIndexAdvanced",
        })
    );

    let failed = serde_json::to_value(EncryptionDebugOperationState::Failed {
        request_id: 2,
        kind: EncryptionDebugOperationKind::ResendIndex0Key,
        outcome: EncryptionDebugOperationOutcome::CancelledStale,
    })
    .unwrap();
    assert_eq!(
        failed,
        json!({
            "state": "failed",
            "request_id": 2,
            "kind": "resendIndex0Key",
            "outcome": "cancelledStale",
        })
    );
}
