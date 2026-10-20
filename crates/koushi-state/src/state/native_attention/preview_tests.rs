//! #994: the OS banner shows message text only when the projection can verify the
//! event it stands for; every other case keeps the count body.

use super::*;
use crate::state::{RoomSummary, RoomTags};

const ROOM: &str = "!room:example.invalid";

fn room(text: Option<&str>) -> RoomSummary {
    RoomSummary {
        display_name_placeholder: None,
        display_label_placeholder: None,
        room_id: ROOM.to_owned(),
        display_name: "Room".to_owned(),
        display_label: "Room".to_owned(),
        original_display_label: "Room".to_owned(),
        avatar: None,
        is_dm: true,
        dm_user_ids: Vec::new(),
        tags: RoomTags::default(),
        unread_count: 1,
        notification_count: 1,
        highlight_count: 0,
        thread_unread_count: 0,
        thread_highlight_count: 0,
        marked_unread: false,
        recency_stamp: Some(42),
        conversation_activity: None,
        latest_event: Some(RoomLatestEventSummary {
            event_id: "$observed:example.invalid".to_owned(),
            relation_type: None,
            relation_event_id: None,
            thread_root_event_id: None,
            sender_id: Some("@alice:example.invalid".to_owned()),
            sender_label: Some("Alice".to_owned()),
            sender_avatar: None,
            preview: Some("preview".to_owned()),
            message_text: text.map(str::to_owned),
            timestamp_ms: 42,
            is_redacted: false,
        }),
        parent_space_ids: Vec::new(),
        dm_space_ids: Vec::new(),
        is_encrypted: true,
        joined_members: 2,
    }
}

fn banner_body(
    room: &RoomSummary,
    message_previews: bool,
    observation: NativeAttentionObservationKind,
    live_room_id: Option<&str>,
) -> Option<String> {
    let rooms = [room.clone()];
    native_attention_projection_from_rooms_with_live_room(
        NativeAttentionProjectionInput {
            rooms: &rooms,
            active_room_id: None,
            muted_room_ids: &[],
            room_notification_modes: &HashMap::new(),
            ignored_user_ids: &BTreeSet::new(),
            window_focused: false,
            observation,
            previous_candidate: None,
            message_previews,
            capabilities: NativeAttentionCapabilities::default(),
            locale: crate::CatalogLocale::En,
        },
        live_room_id,
        live_room_id,
        true,
    )
    .state
    .notification
    .map(|payload| payload.body)
}

fn live_body(room: &RoomSummary, message_previews: bool) -> Option<String> {
    banner_body(
        room,
        message_previews,
        NativeAttentionObservationKind::Live,
        Some(ROOM),
    )
}

#[test]
fn previews_are_off_by_default_and_the_body_is_the_count() {
    assert!(!crate::state::NotificationSettings::default().message_previews);
    assert_eq!(
        live_body(&room(Some("decrypted text")), false).as_deref(),
        Some("1 unread")
    );
}

#[test]
fn a_verified_decrypted_message_becomes_the_body() {
    assert_eq!(
        live_body(&room(Some("decrypted text")), true).as_deref(),
        Some("decrypted text")
    );
}

#[test]
fn undecryptable_non_text_redacted_and_edited_events_keep_the_count() {
    // An undecryptable or non-text event has no message text.
    assert_eq!(live_body(&room(None), true).as_deref(), Some("1 unread"));
    assert_eq!(
        live_body(&room(Some(" \n\t ")), true).as_deref(),
        Some("1 unread")
    );

    // An edit's latest text is a replacement, not the event that raised the count.
    for relation in ["m.replace", "m.annotation"] {
        let mut edited = room(Some("edited text"));
        edited.latest_event.as_mut().expect("event").relation_type = Some(relation.to_owned());
        assert_eq!(
            live_body(&edited, true).as_deref(),
            Some("1 unread"),
            "{relation}"
        );
    }

    // A redacted event raises no banner at all, so it cannot carry stale text.
    let mut redacted = room(Some("removed text"));
    redacted.latest_event.as_mut().expect("event").is_redacted = true;
    assert_eq!(live_body(&redacted, true), None);
}

#[test]
fn a_thread_reply_is_the_latest_event_and_may_be_previewed() {
    let mut threaded = room(Some("thread reply"));
    let event = threaded.latest_event.as_mut().expect("event");
    event.relation_type = Some("m.thread".to_owned());
    event.thread_root_event_id = Some("$root:example.invalid".to_owned());
    assert_eq!(live_body(&threaded, true).as_deref(), Some("thread reply"));
}

#[test]
fn a_room_that_did_not_rise_in_this_observation_never_shows_text() {
    // Another room rose: this one is not the event the banner stands for.
    assert_eq!(
        banner_body(
            &room(Some("old text")),
            true,
            NativeAttentionObservationKind::Live,
            Some("!other:example.invalid"),
        ),
        None
    );
    // Without the live-room check (the test-only projection) the text stays out.
    let rooms = [room(Some("unverified text"))];
    let projection = native_attention_projection_from_rooms(NativeAttentionProjectionInput {
        rooms: &rooms,
        active_room_id: None,
        muted_room_ids: &[],
        room_notification_modes: &HashMap::new(),
        ignored_user_ids: &BTreeSet::new(),
        window_focused: false,
        observation: NativeAttentionObservationKind::Live,
        previous_candidate: None,
        message_previews: true,
        capabilities: NativeAttentionCapabilities::default(),
        locale: crate::CatalogLocale::En,
    });
    assert_eq!(
        projection
            .state
            .notification
            .map(|payload| payload.body)
            .as_deref(),
        Some("1 unread")
    );
}

#[test]
fn the_preview_is_one_bounded_line_and_never_printed() {
    let long = format!("first\nsecond\u{2028}{}", "あ".repeat(400));
    let body = live_body(&room(Some(&long)), true).expect("banner");
    assert!(body.starts_with("first second "));
    assert!(!body.contains('\n'));
    assert!(body.ends_with('…'));
    // The ellipsis is inside the bound, so a platform cut at the same length keeps it.
    assert_eq!(
        body.chars().count(),
        MAX_NATIVE_NOTIFICATION_PREVIEW_GRAPHEMES
    );
    let exact = "a".repeat(MAX_NATIVE_NOTIFICATION_PREVIEW_GRAPHEMES);
    assert_eq!(
        live_body(&room(Some(&exact)), true).as_deref(),
        Some(exact.as_str())
    );

    let rooms = [room(Some("private banner text"))];
    let projection = native_attention_projection_from_rooms_with_live_room(
        NativeAttentionProjectionInput {
            rooms: &rooms,
            active_room_id: None,
            muted_room_ids: &[],
            room_notification_modes: &HashMap::new(),
            ignored_user_ids: &BTreeSet::new(),
            window_focused: false,
            observation: NativeAttentionObservationKind::Live,
            previous_candidate: None,
            message_previews: true,
            capabilities: NativeAttentionCapabilities::default(),
            locale: crate::CatalogLocale::En,
        },
        Some(ROOM),
        Some(ROOM),
        true,
    );
    assert!(!format!("{projection:?}").contains("private banner text"));
}
