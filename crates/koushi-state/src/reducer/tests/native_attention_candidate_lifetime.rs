//! A live OS-notification candidate must outlive room-list updates that neither
//! read, clear, nor refocus it (state-machine.md, "Native Attention").
//!
//! The desktop adapter does not consume the candidate inside the reducer step that
//! raised it. The WebView sees the snapshot, then invokes
//! `show_native_attention_notification`, which re-reads the *live* snapshot. Any
//! room-list update reduced in between (another room's activity, a re-emitted
//! snapshot) currently reprojects attention with no live room while still
//! requiring one, so no room qualifies: the candidate and its payload vanish with
//! dispatch left `Idle`, and the banner is skipped as `no_candidate`. On an
//! account with busy rooms that window is a few milliseconds, so banners stop
//! appearing.

use super::*;
use crate::state::{
    NativeAttentionCandidate, NativeAttentionDispatchState, RoomAttentionKind, RoomSummary,
};

const NOTIFIED_ROOM: &str = "!notified:example.invalid";
const BUSY_ROOM: &str = "!busy:example.invalid";

fn quiet_rooms() -> Vec<RoomSummary> {
    vec![test_room(NOTIFIED_ROOM, None), test_room(BUSY_ROOM, None)]
}

fn notified_room_with_new_message() -> RoomSummary {
    let mut room = test_room(NOTIFIED_ROOM, None);
    room.unread_count = 1;
    room.notification_count = 1;
    room.latest_event = Some(latest_event("$incoming:example.invalid", 50));
    room.recency_stamp = Some(50);
    room
}

/// Ready, window unfocused (closed to tray or behind another window), with the
/// notified room's live unread increase already reduced.
fn state_with_live_candidate() -> AppState {
    let mut state = ready_state();
    state.settings.values.notifications.desktop_notifications = true;
    reduce(
        &mut state,
        AppAction::NativeWindowFocusChanged {
            focused: false,
            observation_generation: 1,
        },
    );
    reduce(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: Vec::new(),
            rooms: quiet_rooms(),
        },
    );
    reduce(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: Vec::new(),
            rooms: vec![notified_room_with_new_message(), test_room(BUSY_ROOM, None)],
        },
    );
    // Precondition, passes today: the live observation raises the candidate and
    // the Rust-owned banner payload the adapter will read.
    assert_eq!(
        state.native_attention.summary.candidate,
        Some(expected_candidate()),
        "precondition: the live unread increase raises a candidate"
    );
    assert!(
        state.native_attention.notification.is_some(),
        "precondition: the live candidate carries a notification payload"
    );
    state
}

fn expected_candidate() -> NativeAttentionCandidate {
    NativeAttentionCandidate {
        room_display_name: NOTIFIED_ROOM.to_owned(),
        kind: RoomAttentionKind::Message,
        unread_count: 1,
        highlight_count: 0,
    }
}

fn assert_candidate_still_deliverable(state: &AppState, after: &str) {
    assert_eq!(
        state.native_attention.summary.candidate,
        Some(expected_candidate()),
        "{after} must not drop the unread room's live candidate; dispatch is {:?}",
        state.native_attention.dispatch
    );
    assert!(
        state.native_attention.notification.is_some(),
        "{after} must keep the payload `show_native_attention_notification` reads, \
         otherwise the banner is skipped as no_candidate"
    );
    assert!(
        !matches!(
            state.native_attention.dispatch,
            NativeAttentionDispatchState::Suppressed { .. }
        ),
        "{after} is not an initial sync and must not suppress the live candidate: {:?}",
        state.native_attention.dispatch
    );
}

#[test]
fn live_candidate_survives_activity_in_another_room() {
    let mut state = state_with_live_candidate();

    // The user's own message lands in a different room before the WebView's
    // `show` call reaches the adapter. Nothing was read, cleared, or focused.
    let mut busy_room = test_room(BUSY_ROOM, None);
    let mut own_message = latest_event("$own:example.invalid", 51);
    own_message.sender_id = Some("@alice:example.invalid".to_owned());
    busy_room.latest_event = Some(own_message);
    busy_room.recency_stamp = Some(51);
    reduce(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: Vec::new(),
            rooms: vec![notified_room_with_new_message(), busy_room],
        },
    );

    assert_candidate_still_deliverable(&state, "activity in another room");
}

#[test]
fn live_candidate_survives_a_reemitted_room_list_snapshot() {
    let mut state = state_with_live_candidate();

    // The room-list observer re-emits the same rooms (e.g. a reorder or a
    // metadata-only diff). The unread state is unchanged.
    reduce(
        &mut state,
        AppAction::RoomListUpdated {
            spaces: Vec::new(),
            rooms: vec![notified_room_with_new_message(), test_room(BUSY_ROOM, None)],
        },
    );

    assert_candidate_still_deliverable(&state, "a re-emitted room-list snapshot");
}
