//! #1427: which rooms qualify for a device-local delivery, and why.

use koushi_state::{
    NotificationDeliverySettings, NotificationDeliveryTarget, RoomAttentionCandidate,
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
    }
}

fn settings() -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        enabled: true,
        ..NotificationDeliverySettings::default()
    }
}

fn room_ids(
    settings: &NotificationDeliverySettings,
    candidates: &[RoomAttentionCandidate],
    focused: bool,
) -> Vec<String> {
    qualifying_rooms(settings, candidates, focused)
        .into_iter()
        .map(|room| room.room_id)
        .collect()
}

#[test]
fn nothing_qualifies_while_the_channel_is_off() {
    let mut disabled = settings();
    disabled.enabled = false;
    let candidates = [
        candidate("!dm:example.invalid", RoomAttentionKind::Dm, 0, 2),
        candidate("!mention:example.invalid", RoomAttentionKind::Mention, 1, 3),
    ];
    assert!(room_ids(&disabled, &candidates, false).is_empty());
}

#[test]
fn the_trigger_kinds_decide_their_own_rooms() {
    let candidates = [
        candidate("!dm:example.invalid", RoomAttentionKind::Dm, 0, 2),
        candidate("!mention:example.invalid", RoomAttentionKind::Mention, 1, 3),
        candidate("!group:example.invalid", RoomAttentionKind::Message, 0, 1),
    ];

    // The defaults: direct messages and mentions on, a plain group message never.
    assert_eq!(
        room_ids(&settings(), &candidates, false),
        vec!["!dm:example.invalid", "!mention:example.invalid"]
    );

    let mut direct_messages_off = settings();
    direct_messages_off.direct_messages = false;
    assert_eq!(
        room_ids(&direct_messages_off, &candidates, false),
        vec!["!mention:example.invalid"]
    );

    // A keyword hit reaches the projection as a highlight, so `keywords` alone carries a
    // mention-kind room the same way `mentions` does.
    let mut mentions_off = settings();
    mentions_off.mentions = false;
    mentions_off.keywords = false;
    // Turning mentions off leaves the direct message alone, and it takes the keyword switch down
    // with it.
    assert_eq!(
        room_ids(&mentions_off, &candidates, false),
        vec!["!dm:example.invalid"]
    );
    let mut keywords_only = mentions_off.clone();
    keywords_only.keywords = true;
    assert_eq!(
        room_ids(&keywords_only, &candidates, false),
        vec!["!dm:example.invalid", "!mention:example.invalid"]
    );

    // Nothing but the watch list can carry a plain group message.
    let mut watched_group = settings();
    watched_group.direct_messages = false;
    watched_group.mentions = false;
    watched_group.keywords = false;
    watched_group.watched = vec![NotificationDeliveryTarget::Room {
        room_id: "!group:example.invalid".to_owned(),
    }];
    assert_eq!(
        room_ids(&watched_group, &candidates, false),
        vec!["!group:example.invalid"]
    );
}

#[test]
fn a_watched_room_arrives_with_its_category_off() {
    let mut settings = settings();
    settings.direct_messages = false;
    settings.mentions = false;
    settings.keywords = false;
    settings.watched = vec![NotificationDeliveryTarget::Room {
        room_id: "!dm:example.invalid".to_owned(),
    }];
    let candidates = [candidate(
        "!dm:example.invalid",
        RoomAttentionKind::Dm,
        0,
        2,
    )];
    assert_eq!(
        room_ids(&settings, &candidates, false),
        vec!["!dm:example.invalid"]
    );
}

#[test]
fn a_room_inside_a_watched_space_arrives_from_the_room_list() {
    let mut settings = settings();
    settings.direct_messages = false;
    settings.mentions = false;
    settings.keywords = false;
    settings.watched = vec![NotificationDeliveryTarget::Space {
        space_id: "!space:example.invalid".to_owned(),
    }];
    let mut inside = candidate("!group:example.invalid", RoomAttentionKind::Message, 0, 1);
    inside.parent_space_ids = vec!["!space:example.invalid".to_owned()];
    let outside = candidate("!other:example.invalid", RoomAttentionKind::Message, 0, 1);

    assert_eq!(
        room_ids(&settings, &[inside, outside], false),
        vec!["!group:example.invalid"]
    );
}

#[test]
fn a_focused_device_qualifies_nothing_under_the_option() {
    let candidates = [candidate(
        "!dm:example.invalid",
        RoomAttentionKind::Dm,
        0,
        2,
    )];
    assert!(room_ids(&settings(), &candidates, true).is_empty());
    // An unknown focus is read as focused, so this fails closed; turning the option off is the
    // only way a focused device delivers.
    let mut always = settings();
    always.only_when_unfocused = false;
    assert_eq!(
        room_ids(&always, &candidates, true),
        vec!["!dm:example.invalid"]
    );
}

#[test]
fn a_qualifying_room_carries_the_label_kind_and_counts_the_badge_shows() {
    let candidates = [RoomAttentionCandidate {
        room_id: "!mention:example.invalid".to_owned(),
        display_label: "Fixture Room".to_owned(),
        kind: RoomAttentionKind::Mention,
        unread: 5,
        mentions: 2,
        parent_space_ids: Vec::new(),
    }];
    let qualifying = qualifying_rooms(&settings(), &candidates, false);
    assert_eq!(qualifying.len(), 1);
    assert_eq!(qualifying[0].display_label, "Fixture Room");
    assert_eq!(qualifying[0].kind, RoomAttentionKind::Mention);
    assert_eq!(qualifying[0].unread, 5);
    assert_eq!(qualifying[0].mentions, 2);
    assert!(qualifying[0].plaintext_prefix.is_none());
}
