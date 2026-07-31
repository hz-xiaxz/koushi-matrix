use koushi_state::{
    AppAction, AppState, OperationFailureKind, SessionInfo, SessionState, SpaceMemberEntry,
    SpaceMemberInviteOutcome, SpaceMemberMembership, SpaceMembersOperationState,
    SpaceMembersProjection, UserProfile, admit_space_member_invite, reduce,
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
            request_id: 1,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 1,
            projection: projection(1, vec![]),
        },
    );
    assert_eq!(state.space_members.generation, 2);
    assert!(state.space_members.child_room_only.is_empty());

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 1,
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
            request_id: 2,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 2,
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
            request_id: 3,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 3,
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
        AppAction::SpaceMembersLoadRequested {
            request_id: 4,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 4,
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
        AppAction::SpaceMembersLoadRequested {
            request_id: 5,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 5,
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
            request_id: 4,
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

#[test]
fn same_generation_out_of_order_loads_cannot_overwrite_newer_request() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 10,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 11,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 11,
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 10,
            projection: projection(2, vec![]),
        },
    );

    assert_eq!(state.space_members.child_room_only.len(), 1);
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Idle
    ));
}

#[test]
fn load_result_cannot_clobber_an_active_invite() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 20,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 20,
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 21,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );

    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 20,
            projection: projection(2, vec![]),
        },
    );

    assert_eq!(state.space_members.space_invited.len(), 1);
    assert!(state.space_members.space_invited[0].invite_pending);
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Inviting { request_id: 21, .. }
    ));
}

#[test]
fn invite_reconciliation_applies_authoritative_projection_and_profile_observation() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 22,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 22,
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 23,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );

    let authoritative_entry = SpaceMemberEntry {
        membership: SpaceMemberMembership::SpaceJoined,
        display_name: Some("Authoritative space label".to_owned()),
        display_label: "Authoritative space label".to_owned(),
        original_display_label: "Authoritative space label".to_owned(),
        invite_pending: false,
        ..child_only_entry()
    };
    reduce(
        &mut state,
        AppAction::SpaceMembersProjectionReconciled {
            request_id: 23,
            projection: SpaceMembersProjection {
                space_joined: vec![authoritative_entry],
                ..projection(2, Vec::new())
            },
            profiles: vec![UserProfile {
                user_id: USER_ID.to_owned(),
                display_name: Some("Observed profile label".to_owned()),
                display_label: String::new(),
                original_display_label: String::new(),
                mention_search_terms: Vec::new(),
                avatar: None,
            }],
        },
    );

    assert_eq!(
        state.profile.users[USER_ID].display_name.as_deref(),
        Some("Observed profile label")
    );
    assert_eq!(state.space_members.space_joined.len(), 1);
    assert_eq!(
        state.space_members.space_joined[0].display_label,
        "Authoritative space label"
    );
    assert!(state.space_members.space_invited.is_empty());
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Inviting { request_id: 23, .. }
    ));

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteSettled {
            request_id: 23,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
            outcome: SpaceMemberInviteOutcome::AlreadyJoined,
        },
    );
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Idle
    ));
    assert_eq!(state.space_members.space_joined.len(), 1);
    assert!(state.space_members.space_invited.is_empty());
}

#[test]
fn incomplete_child_projection_preserves_last_known_and_pending_entries() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 30,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 30,
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 31,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 31,
            projection: SpaceMembersProjection {
                incomplete_child_room_count: 1,
                complete_child_room_count: 0,
                ..projection(2, Vec::new())
            },
        },
    );
    assert_eq!(state.space_members.child_room_only.len(), 1);

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 32,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 31,
            projection: SpaceMembersProjection {
                incomplete_child_room_count: 1,
                complete_child_room_count: 0,
                ..projection(2, Vec::new())
            },
        },
    );
    assert_eq!(state.space_members.space_invited.len(), 1);
    assert!(state.space_members.space_invited[0].invite_pending);
}

#[test]
fn space_level_load_failure_preserves_last_valid_projection() {
    let mut state = ready_state();
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 60,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 60,
            projection: SpaceMembersProjection {
                space_joined: vec![SpaceMemberEntry {
                    membership: SpaceMemberMembership::SpaceJoined,
                    ..child_only_entry()
                }],
                space_invited: vec![SpaceMemberEntry {
                    membership: SpaceMemberMembership::SpaceInvited,
                    ..child_only_entry()
                }],
                child_room_only: Vec::new(),
                ..projection(2, Vec::new())
            },
        },
    );
    let previous = state.space_members.clone();

    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 61,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadFailed {
            request_id: 61,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
            kind: OperationFailureKind::Sdk,
        },
    );

    assert_eq!(state.space_members.space_joined, previous.space_joined);
    assert_eq!(state.space_members.space_invited, previous.space_invited);
    assert_eq!(
        state.space_members.child_room_only,
        previous.child_room_only
    );
    assert_eq!(
        state.space_members.child_room_count,
        previous.child_room_count
    );
    assert!(matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Failed { request_id: 61, .. }
    ));
}

#[test]
fn invite_admission_rejects_wrong_space_and_duplicate_without_side_effect_ticket() {
    let mut state = ready_state();
    state.space_members.selected_space_id = Some(SPACE_ID.to_owned());
    state.space_members.generation = 2;
    state.space_members.child_room_only = vec![child_only_entry()];

    assert!(admit_space_member_invite(&state.space_members, SPACE_ID, USER_ID, 2).is_ok());
    assert!(
        admit_space_member_invite(&state.space_members, "!other:example.invalid", USER_ID, 2)
            .is_err()
    );

    reduce(
        &mut state,
        AppAction::SpaceMemberInviteRequested {
            request_id: 40,
            space_id: SPACE_ID.to_owned(),
            user_id: USER_ID.to_owned(),
            generation: 2,
        },
    );
    assert!(admit_space_member_invite(&state.space_members, SPACE_ID, USER_ID, 2).is_err());
}

#[test]
fn cached_avatar_refreshes_space_member_row_when_room_avatar_is_missing() {
    let mut state = ready_state();
    state.profile.users.insert(
        USER_ID.to_owned(),
        koushi_state::UserProfile {
            user_id: USER_ID.to_owned(),
            display_name: None,
            display_label: String::new(),
            original_display_label: String::new(),
            mention_search_terms: Vec::new(),
            avatar: Some(koushi_state::AvatarImage {
                mxc_uri: "mxc://example.invalid/cached-avatar".to_owned(),
                thumbnail: koushi_state::AvatarThumbnailState::NotRequested,
            }),
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoadRequested {
            request_id: 50,
            space_id: SPACE_ID.to_owned(),
            generation: 2,
        },
    );
    reduce(
        &mut state,
        AppAction::SpaceMembersLoaded {
            request_id: 50,
            projection: projection(2, vec![child_only_entry()]),
        },
    );
    assert_eq!(
        state.space_members.child_room_only[0].avatar_url.as_deref(),
        Some("mxc://example.invalid/cached-avatar")
    );
}

#[test]
fn app_state_without_space_members_deserializes_with_default_projection() {
    let mut value = serde_json::to_value(AppState::default()).expect("serialize app state");
    value
        .as_object_mut()
        .expect("app state object")
        .remove("space_members");

    let restored: AppState = serde_json::from_value(value).expect("legacy app state");
    assert_eq!(restored.space_members, Default::default());
}
