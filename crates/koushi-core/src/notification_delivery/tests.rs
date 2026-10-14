//! Unit tests for the delivery decision engine.
//!
//! Every test drives the pure functions with plain `Instant + Duration`
//! arithmetic: no runtime, no paused clock, no sleeping.

use std::time::Duration;

use koushi_state::{
    NotificationDeliveryPrefixConsent, NotificationDeliverySettings, NotificationSinkConfig,
    NotificationSinkSettings, RoomAttentionCandidate, RoomAttentionKind,
};

use crate::executor::Instant;

use koushi_state::MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS;

use super::digest::{MAX_NOTIFICATION_DIGEST_ROOMS, coalesce_window, digest};
use super::dwell::{DwellTracker, QualifyingRoom};
use super::qualifying::qualifying_rooms;

const DWELL: Duration = Duration::from_secs(60);
const ROOM_ID: &str = "!room:example.invalid";

fn room(
    room_id: &str,
    label: &str,
    kind: RoomAttentionKind,
    unread: u32,
    mentions: u32,
) -> QualifyingRoom {
    QualifyingRoom {
        room_id: room_id.to_owned(),
        display_label: label.to_owned(),
        kind,
        unread,
        mentions,
        plaintext_prefix: None,
    }
}

fn unread(room_id: &str, label: &str, unread: u32) -> QualifyingRoom {
    room(room_id, label, RoomAttentionKind::Message, unread, 0)
}

fn ids(rooms: &[QualifyingRoom]) -> Vec<&str> {
    rooms.iter().map(|room| room.room_id.as_str()).collect()
}

fn sink(id: &str, destination: &str) -> NotificationSinkSettings {
    NotificationSinkSettings {
        id: id.to_owned(),
        enabled: true,
        destination: destination.to_owned(),
        from_address: "koushi@example.invalid".to_owned(),
        sender_name: "Koushi".to_owned(),
        config: NotificationSinkConfig::default(),
        secret_binding: None,
    }
}

/// Settings for an enabled channel whose one sink is enabled and complete. Which
/// rooms qualify - trigger kinds, watched rooms and spaces, focus - is the
/// caller's decision and is not re-checked by the digest.
fn settings_with_sink() -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        enabled: true,
        sinks: vec![sink("home", "me@example.invalid")],
        ..NotificationDeliverySettings::default()
    }
}

/// Settings whose single sink has consented to the plaintext prefix.
fn settings_with_consented_prefix(prefix_chars: u32) -> NotificationDeliverySettings {
    let mut settings = settings_with_sink();
    let endpoint = settings.sinks[0].endpoint();
    settings.prefix_chars = prefix_chars;
    settings.prefix_consent = Some(NotificationDeliveryPrefixConsent {
        sink_id: "home".to_owned(),
        destination: "me@example.invalid".to_owned(),
        endpoint,
        accepted_at_ms: 1,
    });
    settings
}

/// The body one consented sink renders for a single room carrying `prefix`.
fn prefixed_body(prefix: &str, prefix_chars: u32) -> String {
    let settings = settings_with_consented_prefix(prefix_chars);
    let mut room = unread(ROOM_ID, "Room A", 1);
    room.plaintext_prefix = Some(prefix.to_owned());
    let mut intents = digest(&settings, &[room], "window-1");
    assert_eq!(intents.len(), 1);
    intents.remove(0).body
}

#[test]
fn an_unread_room_that_stays_unread_is_delivered_exactly_once() {
    let mut tracker = DwellTracker::new(DWELL);
    let start = Instant::now();
    let room = unread(ROOM_ID, "Room A", 2);
    let qualifying = [room];

    assert!(tracker.observe(start, &qualifying).is_empty());
    assert!(tracker.is_pending(ROOM_ID));
    assert!(
        tracker
            .observe(start + Duration::from_secs(59), &qualifying)
            .is_empty()
    );

    assert_eq!(ids(&tracker.observe(start + DWELL, &qualifying)), [ROOM_ID]);
    assert!(!tracker.is_pending(ROOM_ID));

    // The room is still unread and still qualifying, but its unread state was
    // already delivered: it must not be reported again however long it stays.
    assert!(tracker.observe(start + DWELL * 10, &qualifying).is_empty());
    assert!(
        tracker
            .observe(start + DWELL * 1000, &qualifying)
            .is_empty()
    );
}

#[test]
fn a_room_read_before_the_dwell_expires_is_never_delivered() {
    let mut tracker = DwellTracker::new(DWELL);
    let start = Instant::now();
    let room = unread(ROOM_ID, "Room A", 2);

    assert!(tracker.observe(start, &[room]).is_empty());
    assert!(tracker.is_pending(ROOM_ID));

    // Read: the room leaves the projection, which cancels the timer.
    assert!(
        tracker
            .observe(start + Duration::from_secs(30), &[])
            .is_empty()
    );
    assert!(!tracker.is_pending(ROOM_ID));
    assert!(tracker.observe(start + DWELL * 24, &[]).is_empty());
}

#[test]
fn a_room_read_and_then_unread_again_is_delivered_once_with_the_later_anchor() {
    let mut tracker = DwellTracker::new(DWELL);
    let start = Instant::now();
    let room = unread(ROOM_ID, "Room A", 2);

    tracker.observe(start, std::slice::from_ref(&room));
    tracker.observe(start + Duration::from_secs(30), &[]);
    let re_qualified = start + Duration::from_secs(40);
    assert!(
        tracker
            .observe(re_qualified, std::slice::from_ref(&room))
            .is_empty()
    );
    assert!(tracker.is_pending(ROOM_ID));

    // The first anchor is gone: the room is armed on the new transition only.
    assert!(
        tracker
            .observe(start + DWELL, std::slice::from_ref(&room))
            .is_empty()
    );
    assert_eq!(
        ids(&tracker.observe(re_qualified + DWELL, std::slice::from_ref(&room),)),
        [ROOM_ID]
    );
    assert!(
        tracker
            .observe(re_qualified + DWELL * 5, &[room])
            .is_empty()
    );
}

#[test]
fn a_busy_room_keeps_its_original_anchor_and_reports_its_newest_counts() {
    let mut tracker = DwellTracker::new(DWELL);
    let start = Instant::now();

    tracker.observe(start, &[unread(ROOM_ID, "Room A", 1)]);
    let busy = unread(ROOM_ID, "Room A", 9);
    assert!(
        tracker
            .observe(start + Duration::from_secs(30), std::slice::from_ref(&busy))
            .is_empty()
    );

    let delivered = tracker.observe(start + DWELL, &[busy]);
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].unread, 9,
        "the released room carries the newest observation, not the one that armed it"
    );
}

#[test]
fn a_zero_dwell_is_due_on_the_first_observation() {
    let mut tracker = DwellTracker::new(Duration::ZERO);
    let start = Instant::now();
    let room = unread(ROOM_ID, "Room A", 1);

    assert_eq!(
        ids(&tracker.observe(start, std::slice::from_ref(&room))),
        [ROOM_ID]
    );
    // Still the same unread state, so still no second delivery.
    assert!(
        tracker
            .observe(start + Duration::from_millis(1), &[room])
            .is_empty()
    );
}

#[test]
fn observe_is_independent_of_the_order_of_the_qualifying_set() {
    let start = Instant::now();
    let mut rooms = vec![
        unread("!b:example.invalid", "Room B", 1),
        unread("!a:example.invalid", "Room A", 1),
    ];

    let mut forward = DwellTracker::new(DWELL);
    forward.observe(start, &rooms);
    let forward_due = forward.observe(start + DWELL, &rooms);

    rooms.reverse();
    let mut backward = DwellTracker::new(DWELL);
    backward.observe(start, &rooms);
    let backward_due = backward.observe(start + DWELL, &rooms);

    assert_eq!(forward_due, backward_due);
    assert_eq!(
        ids(&forward_due),
        ["!a:example.invalid", "!b:example.invalid"]
    );
}

#[test]
fn a_room_that_is_read_while_another_room_waits_is_cancelled_on_its_own() {
    let mut tracker = DwellTracker::new(DWELL);
    let start = Instant::now();
    let staying = unread("!staying:example.invalid", "Room S", 1);
    let going = unread("!going:example.invalid", "Room G", 1);

    tracker.observe(start, &[staying.clone(), going]);
    assert!(tracker.is_pending("!going:example.invalid"));

    let due = tracker.observe(start + DWELL, &[staying]);
    assert_eq!(ids(&due), ["!staying:example.invalid"]);
    assert!(!tracker.is_pending("!going:example.invalid"));
}

#[test]
fn the_counts_only_body_names_rooms_without_ids_or_message_text() {
    let settings = settings_with_sink();
    let due = [
        room(
            "!workspace:example.invalid",
            "Synthetic Workspace",
            RoomAttentionKind::Mention,
            3,
            1,
        ),
        room(
            "!member:example.invalid",
            "Member 1",
            RoomAttentionKind::Dm,
            2,
            0,
        ),
        unread("!quiet:example.invalid", "Fixture Room", 4),
    ];

    let intents = digest(&settings, &due, "window-1");
    assert_eq!(intents.len(), 1);
    let intent = &intents[0];
    assert_eq!(intent.subject, "3 rooms need attention");
    assert_eq!(
        intent.body,
        "Mention in Synthetic Workspace: 1 mention, 3 unread\n\n\
         Direct message in Member 1: 2 unread\n\n\
         Message in Fixture Room: 4 unread"
    );
    assert_eq!(intent.sink_id, "home");
    assert_eq!(intent.destination, "me@example.invalid");
    assert_eq!(intent.from_address, "koushi@example.invalid");
    assert_eq!(intent.sender_name, "Koushi");

    // No room id, no sender id, no event id, and no markup.
    for identifier in [
        "!workspace",
        "!member",
        "!quiet",
        "example.invalid",
        "@",
        "$",
        "<",
        ">",
    ] {
        assert!(
            !intent.body.contains(identifier) && !intent.subject.contains(identifier),
            "{identifier} must not reach the message"
        );
    }
}

/// A room projection candidate: what the attention projection hands the delivery channel.
fn candidate(
    room_id: &str,
    label: &str,
    newest_message_text: Option<&str>,
) -> RoomAttentionCandidate {
    RoomAttentionCandidate {
        room_id: room_id.to_owned(),
        display_label: label.to_owned(),
        kind: RoomAttentionKind::Dm,
        unread: 1,
        mentions: 0,
        parent_space_ids: Vec::new(),
        newest_message_text: newest_message_text.map(str::to_owned),
    }
}

/// #1427: the whole chain, from what the room projection already holds to what a sink
/// renders. A room whose newest message is on this device carries its text through
/// qualification, and `digest` renders it for the one sink whose stored consent names
/// that sink, destination and endpoint - and for nobody else. A room whose newest message
/// is not available still qualifies and is still delivered, without a prefix.
#[test]
fn the_rooms_newest_text_reaches_only_the_sink_whose_consent_names_it() {
    let mut settings = settings_with_consented_prefix(80);
    settings.sinks.push(sink("work", "work@example.invalid"));
    let candidates = [
        candidate(ROOM_ID, "Room A", Some("Alice: review the budget")),
        candidate("!quiet:example.invalid", "Room B", None),
    ];

    let qualifying = qualifying_rooms(&settings, &candidates, false);
    assert_eq!(ids(&qualifying), [ROOM_ID, "!quiet:example.invalid"]);
    assert_eq!(
        qualifying[1].plaintext_prefix, None,
        "a room with no readable newest message carries no text"
    );

    let intents = digest(&settings, &qualifying, "window-1");
    let home = intents
        .iter()
        .find(|intent| intent.sink_id == "home")
        .expect("the consented sink is enabled");
    let work = intents
        .iter()
        .find(|intent| intent.sink_id == "work")
        .expect("the other sink is enabled");

    // The prefix is the message text alone: no sender, room id, or label rides with it.
    assert!(
        home.body.starts_with("Alice: review the budget\n"),
        "the consented sink renders the room's newest text first: {}",
        home.body
    );
    assert!(
        home.body.contains("Room B"),
        "both rooms are still delivered"
    );
    assert!(!work.body.contains("review the budget"));

    // The option off removes the text and changes nothing else about the digest.
    settings.prefix_chars = 0;
    let intents = digest(&settings, &qualifying, "window-1");
    assert_eq!(intents.len(), 2);
    assert!(!intents[0].body.contains("review the budget"));
    assert!(intents[0].body.contains("Room A"));
}

#[test]
fn a_single_room_digest_uses_the_singular_subject() {
    let settings = settings_with_sink();
    let intents = digest(&settings, &[unread(ROOM_ID, "Room A", 1)], "window-1");
    assert_eq!(intents[0].subject, "1 room needs attention");
}

#[test]
fn the_prefix_appears_only_for_the_sink_and_destination_the_consent_names() {
    let mut settings = settings_with_consented_prefix(80);
    settings.sinks.push(sink("work", "work@example.invalid"));
    let mut room = unread(ROOM_ID, "Room A", 1);
    room.plaintext_prefix = Some("Alice: review the budget".to_owned());
    let due = [room];

    let intents = digest(&settings, &due, "window-1");
    let home = intents
        .iter()
        .find(|intent| intent.sink_id == "home")
        .expect("the consented sink is enabled");
    let work = intents
        .iter()
        .find(|intent| intent.sink_id == "work")
        .expect("the other sink is enabled");
    assert!(home.body.starts_with("Alice: review the budget\n"));
    assert!(!work.body.contains("review the budget"));

    // The same sink, another destination: another recipient, no consent.
    settings.sinks[0].destination = "other@example.invalid".to_owned();
    let intents = digest(&settings, &due, "window-1");
    assert!(!intents[0].body.contains("review the budget"));

    // The same sink and destination behind another endpoint: another recipient too.
    settings.sinks[0].destination = "me@example.invalid".to_owned();
    settings.sinks[0].config = NotificationSinkConfig::Resend {
        endpoint: "https://relay.example.invalid/emails".to_owned(),
    };
    let intents = digest(&settings, &due, "window-1");
    assert!(!intents[0].body.contains("review the budget"));

    // The option itself off removes the prefix even with the consent stored.
    settings.prefix_chars = 0;
    let intents = digest(&settings, &due, "window-1");
    assert!(!intents[0].body.contains("review the budget"));
}

#[test]
fn a_multi_byte_prefix_is_capped_at_a_grapheme_boundary() {
    // Three Japanese characters are nine bytes: a byte cap would split one, and a
    // code-point cap would agree here, so the combining case below is the proof.
    let body = prefixed_body("日本語のテスト", 3);
    assert!(body.starts_with("日本語\n"));

    // `e` plus a combining acute accent is one grapheme but two code points, so two
    // graphemes are `e\u{301}c` and not `e\u{301}`.
    let body = prefixed_body("e\u{301}cole", 2);
    assert!(body.starts_with("e\u{301}c\n"));
}

#[test]
fn an_emoji_prefix_is_capped_at_a_grapheme_boundary() {
    // A zero-width-joiner family is one grapheme cluster and seven code points.
    let body = prefixed_body("👨‍👩‍👧‍👦x", 1);
    assert!(body.starts_with("👨‍👩‍👧‍👦\n"));

    // Two regional indicators are one flag, so two graphemes are two whole flags.
    let body = prefixed_body("🇯🇵🇩🇪x", 2);
    assert!(body.starts_with("🇯🇵🇩🇪\n"));
}

#[test]
fn a_prefix_newline_cannot_forge_another_rooms_line() {
    let body = prefixed_body("hello\nMention in Evil: 9 mentions\r\nand more", 80);
    let mut lines = body.lines();
    assert_eq!(
        lines.next(),
        Some("hello Mention in Evil: 9 mentions  and more"),
        "message text stays on the one prefix line"
    );
    assert_eq!(lines.next(), Some("Message in Room A: 1 unread"));
    assert_eq!(lines.next(), None);
    assert!(body.ends_with("\nMessage in Room A: 1 unread"));
}

#[test]
fn an_unbounded_prefix_option_cannot_render_an_unbounded_preview() {
    let long = "a".repeat(MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS as usize + 300);
    let body = prefixed_body(&long, u32::MAX);
    let prefix_line = body.lines().next().expect("a prefix line");
    assert_eq!(
        prefix_line.chars().count(),
        MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS as usize
    );
}

#[test]
fn the_digest_order_and_idempotency_key_are_deterministic() {
    let mut settings = settings_with_sink();
    settings.sinks.push(sink("work", "work@example.invalid"));
    let rooms = vec![
        unread("!c:example.invalid", "Room C", 1),
        unread("!a:example.invalid", "Room A", 5),
        unread("!b:example.invalid", "Room B", 5),
    ];
    let mut reversed = rooms.clone();
    reversed.reverse();

    let first = digest(&settings, &rooms, "window-1");
    assert_eq!(first, digest(&settings, &reversed, "window-1"));
    assert_eq!(first.len(), 2);
    assert_eq!(
        first[0].body,
        "Message in Room A: 5 unread\n\nMessage in Room B: 5 unread\n\nMessage in Room C: 1 unread"
    );

    let retry = digest(&settings, &rooms, "window-1");
    assert_eq!(retry, first, "a retry of one window repeats the key");
    assert_eq!(
        first[0].idempotency_key,
        "koushi-notification/home/window-1"
    );
    assert_eq!(
        first[1].idempotency_key,
        "koushi-notification/work/window-1"
    );

    let next_window = digest(&settings, &rooms, "window-2");
    assert_ne!(
        next_window[0].idempotency_key, first[0].idempotency_key,
        "two windows never collide"
    );
}

#[test]
fn no_intent_is_built_for_a_disabled_channel_an_empty_due_set_or_an_incomplete_sink() {
    let mut settings = settings_with_sink();
    assert!(digest(&settings, &[], "window-1").is_empty());

    // The channel's own off switch: no due set can produce a message from an app
    // the user has not turned on.
    settings.enabled = false;
    assert!(digest(&settings, &[unread(ROOM_ID, "Room A", 1)], "window-1").is_empty());
    settings.enabled = true;

    settings.sinks[0].enabled = false;
    assert!(digest(&settings, &[unread(ROOM_ID, "Room A", 1)], "window-1").is_empty());

    settings.sinks[0].enabled = true;
    settings.sinks[0].destination.clear();
    assert!(digest(&settings, &[unread(ROOM_ID, "Room A", 1)], "window-1").is_empty());

    settings.sinks[0].destination = "me@example.invalid".to_owned();
    settings.sinks[0].from_address.clear();
    assert!(digest(&settings, &[unread(ROOM_ID, "Room A", 1)], "window-1").is_empty());
}

#[test]
fn a_digest_of_many_rooms_names_a_bounded_number_and_counts_the_rest() {
    let settings = settings_with_sink();
    let rooms: Vec<QualifyingRoom> = (0..MAX_NOTIFICATION_DIGEST_ROOMS + 3)
        .map(|index| {
            unread(
                &format!("!r{index}:example.invalid"),
                &format!("Room {index}"),
                1,
            )
        })
        .collect();

    let intents = digest(&settings, &rooms, "window-1");
    assert_eq!(intents[0].subject, "8 rooms need attention");
    assert_eq!(
        intents[0]
            .body
            .lines()
            .filter(|line| line.ends_with("unread"))
            .count(),
        MAX_NOTIFICATION_DIGEST_ROOMS
    );
    assert!(intents[0].body.ends_with("\n\nand 3 more"));
}

#[test]
fn the_interval_gate_holds_until_the_minimum_interval_has_elapsed() {
    let mut settings = NotificationDeliverySettings::default();
    let start = Instant::now();

    assert!(coalesce_window(&settings, None, start));
    settings.min_interval_secs = 900;
    assert!(!coalesce_window(&settings, Some(start), start));
    assert!(!coalesce_window(
        &settings,
        Some(start),
        start + Duration::from_secs(899)
    ));
    assert!(coalesce_window(
        &settings,
        Some(start),
        start + Duration::from_secs(900)
    ));

    // An interval of zero never holds a delivery, and a clock that moved backwards
    // fails closed instead of sending.
    settings.min_interval_secs = 0;
    assert!(coalesce_window(&settings, Some(start), start));
    settings.min_interval_secs = 900;
    assert!(!coalesce_window(
        &settings,
        Some(start + Duration::from_secs(60)),
        start
    ));
}

#[test]
fn delivery_debug_output_redacts_identifiers_and_message_text() {
    let mut room = unread(ROOM_ID, "Room A", 1);
    room.plaintext_prefix = Some("Alice: the code is 1234".to_owned());
    let room_debug = format!("{room:?}");
    assert!(room_debug.contains("RoomId(..)"));
    assert!(room_debug.contains("PlaintextPrefix(..)"));
    for leaked in [ROOM_ID, "Room A", "Alice", "1234"] {
        assert!(!room_debug.contains(leaked), "{leaked} must not be logged");
    }

    let settings = settings_with_consented_prefix(80);
    let intents = digest(&settings, &[room], "window-1");
    let intent_debug = format!("{:?}", intents[0]);
    assert!(intent_debug.contains("Body(..)"));
    assert!(intent_debug.contains("Recipient(..)"));
    assert!(intent_debug.contains("koushi-notification/home/window-1"));
    for leaked in ["me@example.invalid", "Alice", "1234", "Room A", ROOM_ID] {
        assert!(
            !intent_debug.contains(leaked),
            "{leaked} must not be logged"
        );
    }
}
