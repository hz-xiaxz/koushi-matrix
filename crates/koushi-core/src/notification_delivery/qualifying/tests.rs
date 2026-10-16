//! #1427, #1460: which rooms qualify for one destination, and why.

use koushi_state::{
    BoundedNotificationDeliveryText, NotificationDeliveryContent, NotificationDeliveryTarget,
    NotificationDeliveryTargets, NotificationSinkSettings, RoomAttentionCandidate,
    RoomAttentionKind,
};

use super::qualifying_rooms;

fn candidate(
    room_id: &str,
    kind: RoomAttentionKind,
    mentions: u32,
    unread: u32,
) -> RoomAttentionCandidate {
    RoomAttentionCandidate {
        room_id: room_id.to_owned(),
        display_label: "Fixture Room".to_owned(),
        kind,
        unread,
        mentions,
        parent_space_ids: Vec::new(),
        parent_space_labels: Vec::new(),
        direct_partner: None,
        newest_message_text: None,
    }
}

fn text(value: &str) -> Option<BoundedNotificationDeliveryText> {
    Some(BoundedNotificationDeliveryText {
        text: value.to_owned(),
        truncated_at_source: false,
    })
}

/// A complete Resend destination with the default targets and timing.
fn sink() -> NotificationSinkSettings {
    NotificationSinkSettings {
        id: "home".to_owned(),
        destination: "me@example.invalid".to_owned(),
        from_address: "notice@example.invalid".to_owned(),
        ..NotificationSinkSettings::default()
    }
}

fn room_ids(
    sink: &NotificationSinkSettings,
    candidates: &[RoomAttentionCandidate],
    focused: bool,
) -> Vec<String> {
    qualifying_rooms(sink, candidates, focused)
        .into_iter()
        .map(|room| room.room_id)
        .collect()
}

fn watching(target: NotificationDeliveryTarget) -> NotificationSinkSettings {
    let mut sink = sink();
    sink.targets = NotificationDeliveryTargets {
        watched: vec![target],
        ..NotificationDeliveryTargets::none()
    };
    sink
}

#[test]
fn a_destination_with_no_target_qualifies_nothing() {
    let mut silent = sink();
    silent.targets = NotificationDeliveryTargets::none();
    let candidates = [
        candidate("!dm:example.invalid", RoomAttentionKind::Dm, 0, 2),
        candidate("!mention:example.invalid", RoomAttentionKind::Mention, 1, 3),
    ];
    assert!(room_ids(&silent, &candidates, false).is_empty());
}

#[test]
fn the_trigger_kinds_decide_their_own_rooms() {
    let candidates = [
        candidate("!dm:example.invalid", RoomAttentionKind::Dm, 0, 2),
        candidate("!mention:example.invalid", RoomAttentionKind::Mention, 1, 3),
        candidate("!group:example.invalid", RoomAttentionKind::Message, 0, 1),
    ];

    // The defaults: direct messages, mentions and keywords on; a plain group message never.
    assert_eq!(
        room_ids(&sink(), &candidates, false),
        vec!["!dm:example.invalid", "!mention:example.invalid"]
    );

    let mut direct_messages_off = sink();
    direct_messages_off.targets.direct_messages = false;
    assert_eq!(
        room_ids(&direct_messages_off, &candidates, false),
        vec!["!mention:example.invalid"]
    );

    let mut mentions_off = sink();
    mentions_off.targets.mentions = false;
    mentions_off.targets.keywords = false;
    assert_eq!(
        room_ids(&mentions_off, &candidates, false),
        vec!["!dm:example.invalid"]
    );
    // A keyword hit reaches the projection as a highlight, so `keywords` alone carries a
    // mention-kind room the same way `mentions` does.
    let mut keywords_only = mentions_off.clone();
    keywords_only.targets.keywords = true;
    assert_eq!(
        room_ids(&keywords_only, &candidates, false),
        vec!["!dm:example.invalid", "!mention:example.invalid"]
    );

    // Nothing but the watch list can carry a plain group message.
    let watched_group = watching(NotificationDeliveryTarget::Room {
        room_id: "!group:example.invalid".to_owned(),
    });
    assert_eq!(
        room_ids(&watched_group, &candidates, false),
        vec!["!group:example.invalid"]
    );
}

#[test]
fn a_watched_room_arrives_with_its_category_off() {
    let sink = watching(NotificationDeliveryTarget::Room {
        room_id: "!dm:example.invalid".to_owned(),
    });
    let candidates = [candidate(
        "!dm:example.invalid",
        RoomAttentionKind::Dm,
        0,
        2,
    )];
    assert_eq!(
        room_ids(&sink, &candidates, false),
        vec!["!dm:example.invalid"]
    );
}

#[test]
fn a_room_added_to_a_watched_space_later_arrives_from_the_room_list() {
    let sink = watching(NotificationDeliveryTarget::Space {
        space_id: "!space:example.invalid".to_owned(),
    });
    // The watch list names only the space; the room's membership comes from the current
    // observation, so a room that joined the space after the list was written is covered.
    let mut inside = candidate("!group:example.invalid", RoomAttentionKind::Message, 0, 1);
    inside.parent_space_ids = vec!["!space:example.invalid".to_owned()];
    let outside = candidate("!other:example.invalid", RoomAttentionKind::Message, 0, 1);

    assert_eq!(
        room_ids(&sink, &[inside, outside], false),
        vec!["!group:example.invalid"]
    );
}

#[test]
fn a_watch_never_overrides_read_mute_or_focus() {
    // Read and muted rooms are not candidates at all, so a watch has nothing to resurrect.
    let sink = watching(NotificationDeliveryTarget::Room {
        room_id: "!dm:example.invalid".to_owned(),
    });
    assert!(room_ids(&sink, &[], false).is_empty());
    // Focus suppression applies to watched rooms too.
    let candidates = [candidate(
        "!dm:example.invalid",
        RoomAttentionKind::Dm,
        0,
        2,
    )];
    assert!(room_ids(&sink, &candidates, true).is_empty());
}

#[test]
fn a_focused_device_qualifies_nothing_under_only_when_inactive() {
    let candidates = [candidate(
        "!dm:example.invalid",
        RoomAttentionKind::Dm,
        0,
        2,
    )];
    assert!(
        sink().timing.only_when_unfocused,
        "the default is only when inactive"
    );
    assert!(room_ids(&sink(), &candidates, true).is_empty());
    // An unknown focus is read as focused, so this fails closed; turning the option off is the
    // only way a focused device delivers.
    let mut always = sink();
    always.timing.only_when_unfocused = false;
    assert_eq!(
        room_ids(&always, &candidates, true),
        vec!["!dm:example.invalid"]
    );
}

#[test]
fn a_qualifying_room_carries_the_label_space_kind_and_counts_the_badge_shows() {
    let mut mention = candidate("!mention:example.invalid", RoomAttentionKind::Mention, 2, 5);
    mention.parent_space_labels = vec!["Fixture Space".to_owned()];
    let qualifying = qualifying_rooms(&sink(), &[mention], false);
    assert_eq!(qualifying.len(), 1);
    assert_eq!(qualifying[0].display_label, "Fixture Room");
    assert_eq!(qualifying[0].space_labels, vec!["Fixture Space"]);
    assert_eq!(qualifying[0].kind, RoomAttentionKind::Mention);
    assert_eq!(qualifying[0].unread, 5);
    assert_eq!(qualifying[0].mentions, 2);
    assert!(qualifying[0].message_text.is_none());
}

/// #1460: only a destination whose effective content mode carries text holds the text, and
/// a consent that no longer names the destination leaves it names only.
#[test]
fn only_a_consented_text_mode_carries_the_newest_message() {
    let mut with_text = candidate("!with-text:example.invalid", RoomAttentionKind::Dm, 0, 2);
    with_text.newest_message_text = text("the newest message");
    let candidates = [
        with_text,
        candidate("!no-text:example.invalid", RoomAttentionKind::Dm, 0, 1),
    ];

    // Names only, the default: the text never enters the lane.
    let rooms = qualifying_rooms(&sink(), &candidates, false);
    assert!(rooms.iter().all(|room| room.message_text.is_none()));

    // A text mode without consent is still names only.
    let mut unconsented = sink();
    unconsented.content = NotificationDeliveryContent::Full;
    let rooms = qualifying_rooms(&unconsented, &candidates, false);
    assert!(rooms.iter().all(|room| room.message_text.is_none()));

    // A consent for the destination as configured now carries it.
    let mut consented = unconsented.clone();
    consented.content_consent = Some(consented.consent_for_current(1));
    let rooms = qualifying_rooms(&consented, &candidates, false);
    assert_eq!(rooms.len(), 2);
    assert_eq!(
        rooms[0]
            .message_text
            .as_ref()
            .map(|text| text.text.as_str()),
        Some("the newest message")
    );
    assert!(
        rooms[1].message_text.is_none(),
        "a room with nothing readable still qualifies, names only"
    );

    // Changing the recipient invalidates that consent.
    let mut moved = consented.clone();
    moved.destination = "other@example.invalid".to_owned();
    let rooms = qualifying_rooms(&moved, &candidates, false);
    assert!(rooms.iter().all(|room| room.message_text.is_none()));
}
