use koushi_state::{
    AppAction, AppState, OperationFailureKind, SessionInfo, SessionState, SpaceMemberEntry,
    SpaceMemberInviteOutcome, SpaceMemberMembership, SpaceMembersOperationState,
    SpaceMembersProjection, reduce,
};

const SPACE_ID: &str = "!space:example.invalid";
const CHILD_ROOM_ID: &str = "!child:example.invalid";
const USER_ID: &str = "@child:example.invalid";

fn ready_state() -> AppState {
    AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@self:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        }),
        ..AppState::default()
    }
}

fn child_only_entry() -> SpaceMemberEntry {
    SpaceMemberEntry {
        user_id: USER_ID.to_owned(),
        display_name: Some("Child Room Name".to_owned()),
        display_label: "Child Room Name".to_owned(),
        original_display_label: "Child Room Name".to_owned(),
        avatar_url: None,
        power_level: Some(0),
        role: koushi_state::RoomMemberRole::User,
        membership: SpaceMemberMembership::ChildRoomOnly,
        child_room_ids: vec![CHILD_ROOM_ID.to_owned()],
        invite_pending: false,
    }
}

fn projection(generation: u64, child_only: Vec<SpaceMemberEntry>) -> SpaceMembersProjection {
    SpaceMembersProjection {
        space_id: SPACE_ID.to_owned(),
        generation,
        space_joined: Vec::new(),
        space_invited: Vec::new(),
        child_room_only: child_only,
        child_room_count: 1,
        complete_child_room_count: 1,
        incomplete_child_room_count: 0,
    }
}

#[test]
fn loaded_projection_is_generation_fenced_and_keeps_three_sections() {
    let mut state = ready_state();

    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: projection(1, vec![]),
        },
    );
    assert_eq!(state.space_members.generation, 2);
    assert!(state.space_members.child_room_only.is_empty());

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    assert_eq!(state.space_members.space_joined.len(), 0);
    assert_eq!(state.space_members.space_invited.len(), 0);
    assert_eq!(state.space_members.child_room_only.len(), 1);
    assert_eq!(state.space_members.child_room_only[0].user_id, USER_ID);
}

#[test]
fn invite_moves_child_only_person_to_pending_optimistically_and_deduplicates() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: projection(2, vec![child_only_entry()]),
        },
    );

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 7,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    let first_operation = state.space_members.operation.clone();
    assert!(matches!(
        first_operation,
        SpaceMembersOperationState::Inviting { .. }
    ));
    assert!(state.space_members.child_room_only.is_empty());
    assert_eq!(state.space_members.space_invited.len(), 1);
    assert!(state.space_members.space_invited[0].invite_pending);

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 8,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    assert_eq!(state.space_members.operation, first_operation);
    assert_eq!(state.space_members.space_invited.len(), 1);
}

#[test]
fn invite_settlement_reconciles_success_already_joined_and_failure() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: projection(2, vec![child_only_entry()]),
        },
    );

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 7,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteSettled {
            request_id: 7,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
            outcome: SpaceMemberInviteOutcome::Invited,
        },
    );
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Idle
    ));
    assert_eq!(state.space_members.space_invited.len(), 1);

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: SpaceMembersProjection {
                space_joined: vec![SpaceMemberEntry {
                    membership: SpaceMemberMembership::SpaceJoined,
                    invite_pending: false,
                    ..child_only_entry()
                }],
                ..projection(2, vec![])
            },
        },
    );
    assert_eq!(state.space_members.space_joined.len(), 1);
    assert!(state.space_members.space_invited.is_empty());

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 9,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteSettled {
            request_id: 9,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
            outcome: SpaceMemberInviteOutcome::Failed(OperationFailureKind::Forbidden),
        },
    );
    assert!(state.space_members.space_invited.is_empty());
    assert_eq!(state.space_members.child_room_only.len(), 1);
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Failed { .. }
    ));
}

#[test]
fn stale_invite_settlement_cannot_mutate_new_space_generation() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            space_id: SPACE_ID.to_owned(),
            generation: 4,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteSettled {
            request_id: 1,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 3,
            outcome: SpaceMemberInviteOutcome::Failed(OperationFailureKind::Sdk),
        },
    );
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Loading { .. }
    ));
}

#[test]
fn loading_projection_observes_non_empty_child_profiles_for_receipt_fallback() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::UserProfilesUpdated {
            profiles: vec![koushi_state::UserProfile {
                user_id: USER_ID.to_owned(),
                display_name: Some("Child Room Name".to_owned()),
                display_label: String::new(),
                original_display_label: String::new(),
                mention_search_terms: Vec::new(),
                avatar: None,
            }],
        },
    );
    reduce(
        &mut state,
        AppAction::LiveRoomReceiptsUpdated {
            room_id: CHILD_ROOM_ID.to_owned(),
            receipts_by_event: vec![koushi_state::LiveEventReceipts {
                event_id: "$event:example.invalid".to_owned(),
                receipts: vec![koushi_state::LiveReadReceipt {
                    user_id: USER_ID.to_owned(),
                    display_name: None,
                    original_display_label: String::new(),
                    avatar: None,
                    timestamp_ms: Some(1),
                }],
            }],
        },
    );
    assert_eq!(
        state.live_signals.rooms[CHILD_ROOM_ID].receipts_by_event["$event:example.invalid"].readers
            [0]
        .display_name
        .as_deref(),
        Some("Child Room Name")
    );
}
