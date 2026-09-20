//! Issue #961: a Space shows every child room it advertises, with the
//! account's membership beside it.
//!
//! The joined room list stays authoritative for everything it contains: these
//! tests pin that the not-joined lane never duplicates, replaces or reorders a
//! joined room, and that the account's own invite list decides what counts as
//! an invitation.

use koushi_state::{
    AppState, InvitePreview, RoomSummary, RoomTags, SpaceChildMembership, SpaceChildSummary,
    SpaceChildrenState, SpaceSummary, compose_sidebar_for_state,
};

const SPACE_ID: &str = "!space:example.invalid";

fn joined_room(room_id: &str, label: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: label.to_owned(),
        display_label: label.to_owned(),
        original_display_label: label.to_owned(),
        avatar: None,
        is_dm: false,
        dm_user_ids: Vec::new(),
        tags: RoomTags::default(),
        unread_count: 3,
        notification_count: 3,
        highlight_count: 0,
        marked_unread: false,
        recency_stamp: Some(10),
        conversation_activity: None,
        latest_event: None,
        parent_space_ids: vec![SPACE_ID.to_owned()],
        dm_space_ids: Vec::new(),
        is_encrypted: false,
        joined_members: 2,
    }
}

fn child(room_id: &str, label: &str, membership: SpaceChildMembership) -> SpaceChildSummary {
    SpaceChildSummary {
        room_id: room_id.to_owned(),
        display_name: label.to_owned(),
        avatar: None,
        membership,
        can_join: matches!(
            membership,
            SpaceChildMembership::NotJoined | SpaceChildMembership::Invited
        ),
        is_space: false,
        joined_members: 4,
    }
}

fn state_with_children(children: Vec<SpaceChildSummary>) -> AppState {
    let mut state = AppState::default();
    state.navigation.active_space_id = Some(SPACE_ID.to_owned());
    state.spaces = vec![SpaceSummary {
        space_id: SPACE_ID.to_owned(),
        raw_name: Some("Space".to_owned()),
        display_name: "Space".to_owned(),
        avatar: None,
        child_room_ids: vec!["!joined:example.invalid".to_owned()],
    }];
    state.rooms = vec![joined_room("!joined:example.invalid", "Joined Room")];
    state.space_children = SpaceChildrenState {
        selected_space_id: Some(SPACE_ID.to_owned()),
        generation: 1,
        children,
        load: Default::default(),
    };
    state
}

fn names(items: &[koushi_state::RoomListItem]) -> Vec<&str> {
    items
        .iter()
        .map(|item| item.display_name.as_str())
        .collect()
}

#[test]
fn not_joined_children_get_their_own_lane_without_touching_the_joined_rooms() {
    let state = state_with_children(vec![
        child(
            "!joined:example.invalid",
            "Joined Room",
            SpaceChildMembership::Joined,
        ),
        child(
            "!open:example.invalid",
            "Open Room",
            SpaceChildMembership::NotJoined,
        ),
        child(
            "!private:example.invalid",
            "!private:example.invalid",
            SpaceChildMembership::Unknown,
        ),
    ]);

    let sidebar = compose_sidebar_for_state(&state);

    assert_eq!(names(&sidebar.sections.rooms), ["Joined Room"]);
    assert_eq!(
        names(&sidebar.sections.not_joined),
        ["!private:example.invalid", "Open Room"]
    );
    assert_eq!(
        sidebar.sections.rooms[0].membership,
        SpaceChildMembership::Joined
    );
    assert_eq!(sidebar.sections.rooms[0].unread_count, 3);
    // A room the account is not in has no read state to report.
    assert!(
        sidebar
            .sections
            .not_joined
            .iter()
            .all(|item| item.unread_count == 0 && item.highlight_count == 0)
    );
}

#[test]
fn a_joined_room_is_never_repeated_in_the_not_joined_lane() {
    // A hierarchy response that crossed a join still calls the room not joined.
    // The account's own room list is the authority.
    let state = state_with_children(vec![child(
        "!joined:example.invalid",
        "Joined Room",
        SpaceChildMembership::NotJoined,
    )]);

    let sidebar = compose_sidebar_for_state(&state);

    assert_eq!(names(&sidebar.sections.rooms), ["Joined Room"]);
    assert!(sidebar.sections.not_joined.is_empty());
    assert!(sidebar.not_joined_space_rooms.is_empty());
}

#[test]
fn a_pending_invitation_is_reported_as_invited_inside_its_space() {
    let mut state = state_with_children(vec![child(
        "!invited:example.invalid",
        "Invited Room",
        SpaceChildMembership::NotJoined,
    )]);
    state.invites = vec![InvitePreview {
        room_id: "!invited:example.invalid".to_owned(),
        display_name: "Invited Room".to_owned(),
        avatar: None,
        topic: None,
        inviter_display_name: None,
        inviter_user_id: None,
        is_dm: false,
        is_space: false,
    }];

    let sidebar = compose_sidebar_for_state(&state);

    assert_eq!(names(&sidebar.sections.not_joined), ["Invited Room"]);
    assert_eq!(
        sidebar.sections.not_joined[0].membership,
        SpaceChildMembership::Invited
    );
}

#[test]
fn another_spaces_children_never_leak_into_the_selected_space() {
    let mut state = state_with_children(vec![child(
        "!elsewhere:example.invalid",
        "Elsewhere",
        SpaceChildMembership::NotJoined,
    )]);
    state.space_children.selected_space_id = Some("!other-space:example.invalid".to_owned());

    let sidebar = compose_sidebar_for_state(&state);

    assert!(sidebar.sections.not_joined.is_empty());
}
