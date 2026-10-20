//! Unit tests for the delivery decision engine.
//!
//! Every test drives the pure functions with plain `Instant + Duration`
//! arithmetic: no runtime, no paused clock, no sleeping.

use std::time::Duration;

use koushi_state::{
    BoundedNotificationDeliveryText, DirectPartner, NotificationDeliveryContent,
    NotificationDeliveryPartnerLabel, NotificationSinkConfig, NotificationSinkSettings,
    RoomAttentionKind,
};

use crate::executor::Instant;

use super::digest::{
    MAX_NOTIFICATION_DIGEST_ROOMS, MAX_SERVERCHAN_TITLE_GRAPHEMES,
    NOTIFICATION_TEXT_TRUNCATED_MARKER, NotificationDeliveryPreviewSample, coalesce_window,
    preview_digest, render_intent,
};
use super::dwell::{DwellTracker, QualifyingRoom};

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
        space_labels: Vec::new(),
        kind,
        direct_partner: None,
        unread,
        mentions,
        message_text: None,
    }
}

fn unread(room_id: &str, label: &str, unread: u32) -> QualifyingRoom {
    room(room_id, label, RoomAttentionKind::Message, unread, 0)
}

fn with_text(mut room: QualifyingRoom, text: &str) -> QualifyingRoom {
    room.message_text = Some(BoundedNotificationDeliveryText {
        text: text.to_owned(),
        truncated_at_source: false,
    });
    room
}

fn ids(rooms: &[QualifyingRoom]) -> Vec<&str> {
    rooms.iter().map(|room| room.room_id.as_str()).collect()
}

/// A complete Resend destination, names only.
fn sink(id: &str, destination: &str) -> NotificationSinkSettings {
    NotificationSinkSettings {
        id: id.to_owned(),
        destination: destination.to_owned(),
        from_address: "koushi@example.invalid".to_owned(),
        sender_name: "Koushi".to_owned(),
        ..NotificationSinkSettings::default()
    }
}

/// The same destination with `content` under a consent for it as configured now.
fn consented(
    mut sink: NotificationSinkSettings,
    content: NotificationDeliveryContent,
) -> NotificationSinkSettings {
    sink.content = content;
    sink.content_consent = Some(sink.consent_for_current(1));
    sink
}

/// A ServerChan destination whose SendKey was saved under one revision.
fn serverchan(content: NotificationDeliveryContent) -> NotificationSinkSettings {
    let mut sink = NotificationSinkSettings {
        id: "wechat".to_owned(),
        config: NotificationSinkConfig::ServerChan {
            endpoint: "https://sctapi.ftqq.com".to_owned(),
        },
        ..NotificationSinkSettings::default()
    };
    sink.secret_binding = Some(sink.credential_binding("a1"));
    consented(sink, content)
}

/// The body one consented destination renders for a single room carrying `text`.
fn text_body(text: &str, content: NotificationDeliveryContent) -> String {
    let sink = consented(sink("home", "me@example.invalid"), content);
    render_intent(
        &sink,
        &[with_text(unread(ROOM_ID, "Room A", 1), text)],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent")
    .body
}

/// The quoted text lines of a single-room body, without the `> ` marker.
fn quoted(body: &str) -> Vec<&str> {
    body.lines()
        .filter_map(|line| line.strip_prefix("> "))
        .collect()
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
fn a_names_only_body_names_rooms_and_spaces_without_ids_or_text() {
    let mut mention = room(
        "!mention:example.invalid",
        "Design",
        RoomAttentionKind::Mention,
        4,
        2,
    );
    mention.space_labels = vec!["Fixture Space".to_owned()];
    let rooms = [
        with_text(unread("!group:example.invalid", "Lobby", 1), "secret text"),
        mention,
        room("!dm:example.invalid", "Alice", RoomAttentionKind::Dm, 3, 0),
    ];
    let intent = render_intent(
        &sink("home", "me@example.invalid"),
        &rooms,
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");

    assert_eq!(
        intent.subject,
        "Mention in Design · Fixture Space and 2 more rooms"
    );
    assert_eq!(
        intent.body,
        "Mention in Design · Fixture Space: 2 mentions, 4 unread\n\n\
         Direct message in Alice: 3 unread\n\n\
         Message in Lobby: 1 unread"
    );
    assert_eq!(intent.destination, "me@example.invalid");
    assert_eq!(intent.from_address, "koushi@example.invalid");
    // Names only never carries text, even when the room holds some.
    assert!(!intent.body.contains("secret text"));
    for id in ["!mention", "!dm", "!group", "example.invalid"] {
        assert!(!intent.body.contains(id), "{id} must not be rendered");
        assert!(!intent.subject.contains(id), "{id} must not be rendered");
    }
}

#[test]
fn a_single_room_subject_has_no_more_rooms_tail() {
    let intent = render_intent(
        &sink("home", "me@example.invalid"),
        &[unread(ROOM_ID, "Room A", 1)],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    assert_eq!(intent.subject, "Message in Room A");
}

#[test]
fn an_unconsented_text_mode_renders_names_only() {
    let mut unconsented = sink("home", "me@example.invalid");
    unconsented.content = NotificationDeliveryContent::Full;
    let intent = render_intent(
        &unconsented,
        &[with_text(unread(ROOM_ID, "Room A", 1), "private text")],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    assert!(!intent.body.contains("private text"));
}

#[test]
fn the_entire_text_mode_quotes_every_line_without_a_marker() {
    let body = text_body("first line\nsecond line", NotificationDeliveryContent::Full);
    assert_eq!(quoted(&body), vec!["first line", "second line"]);
    assert!(!body.contains(NOTIFICATION_TEXT_TRUNCATED_MARKER));
}

#[test]
fn a_prefix_is_cut_at_a_grapheme_boundary_and_marked_truncated() {
    // Multi-byte text: the cut never splits a character.
    let body = text_body(
        "あいうえおかきくけこ",
        NotificationDeliveryContent::Prefix { graphemes: 4 },
    );
    assert_eq!(
        quoted(&body),
        vec!["あいうえ", NOTIFICATION_TEXT_TRUNCATED_MARKER]
    );

    // A family emoji is one grapheme of several code points joined by ZWJ.
    let family = "👨\u{200d}👩\u{200d}👧";
    let body = text_body(
        &format!("{family}{family}{family}"),
        NotificationDeliveryContent::Prefix { graphemes: 2 },
    );
    assert_eq!(
        quoted(&body),
        vec![
            format!("{family}{family}").as_str(),
            NOTIFICATION_TEXT_TRUNCATED_MARKER
        ]
    );

    // Text that fits the prefix is not marked.
    let body = text_body(
        "short",
        NotificationDeliveryContent::Prefix { graphemes: 80 },
    );
    assert_eq!(quoted(&body), vec!["short"]);
}

#[test]
fn text_cut_at_the_source_or_by_the_provider_budget_is_never_called_entire() {
    let mut cut_at_source = with_text(unread(ROOM_ID, "Room A", 1), "the start of it");
    cut_at_source
        .message_text
        .as_mut()
        .expect("text")
        .truncated_at_source = true;
    let sink = consented(
        sink("home", "me@example.invalid"),
        NotificationDeliveryContent::Full,
    );
    let body = render_intent(
        &sink,
        &[cut_at_source],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent")
    .body;
    assert_eq!(
        quoted(&body),
        vec!["the start of it", NOTIFICATION_TEXT_TRUNCATED_MARKER]
    );

    // ServerChan's budget is smaller than the mail budget; entire text over it is cut.
    let wechat = serverchan(NotificationDeliveryContent::Full);
    let budget = wechat.text_budget_graphemes();
    let long = "x".repeat(budget + 50);
    let body = render_intent(
        &wechat,
        &[with_text(unread(ROOM_ID, "Room A", 1), &long)],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent")
    .body;
    let lines = quoted(&body);
    assert_eq!(
        lines.last().copied(),
        Some(NOTIFICATION_TEXT_TRUNCATED_MARKER)
    );
    assert!(lines[0].trim_end().chars().count() <= budget);
}

#[test]
fn a_message_newline_cannot_forge_another_rooms_line() {
    let body = text_body(
        "hello\nMention in Bank: 9 mentions\u{2028}Message in Other: 1 unread",
        NotificationDeliveryContent::Full,
    );
    let headlines: Vec<&str> = body
        .lines()
        .filter(|line| !line.starts_with("> "))
        .collect();
    assert_eq!(headlines, vec!["Message in Room A: 1 unread"]);
}

#[test]
fn a_room_label_with_a_line_break_stays_one_line() {
    let intent = render_intent(
        &sink("home", "me@example.invalid"),
        &[unread(ROOM_ID, "Room\nMention in Bank", 1)],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    assert!(!intent.subject.contains('\n'));
    assert_eq!(intent.body.lines().count(), 1);
}

#[test]
fn a_serverchan_title_is_bounded_and_its_body_cannot_inject_markdown() {
    let wechat = serverchan(NotificationDeliveryContent::Full);
    let long_label = "a very long room name that goes on and on and on";
    let rooms = [with_text(
        unread(ROOM_ID, long_label, 1),
        "[click](https://phish.example.invalid) ![x](https://track.example.invalid/p.png) <b>",
    )];
    let intent =
        render_intent(&wechat, &rooms, "w-1", koushi_state::CatalogLocale::En).expect("an intent");

    assert!(intent.subject.chars().count() <= MAX_SERVERCHAN_TITLE_GRAPHEMES);
    assert!(intent.subject.ends_with('…'));
    assert!(
        intent.destination.is_empty(),
        "the SendKey names the recipient"
    );
    assert!(!intent.body.contains("](https"), "{}", intent.body);
    assert!(!intent.body.contains("![x]"));
    assert!(!intent.body.contains("<b>"));
    assert!(intent.body.contains(r"\[click\]"));
}

#[test]
fn the_order_and_the_idempotency_key_are_deterministic() {
    let rooms = [
        unread("!b:example.invalid", "Beta", 1),
        room("!m:example.invalid", "Mu", RoomAttentionKind::Mention, 1, 1),
        unread("!a:example.invalid", "Alpha", 1),
        room("!d:example.invalid", "Delta", RoomAttentionKind::Dm, 2, 0),
    ];
    let mut reversed = rooms.clone();
    reversed.reverse();
    let home = sink("home", "me@example.invalid");

    let first = render_intent(&home, &rooms, "nonce-7", koushi_state::CatalogLocale::En)
        .expect("an intent");
    let second = render_intent(&home, &reversed, "nonce-7", koushi_state::CatalogLocale::En)
        .expect("an intent");
    assert_eq!(first, second);
    assert_eq!(first.idempotency_key, "koushi-notification/home/nonce-7");
    assert!(first.body.starts_with("Mention in Mu"));
    assert!(first.body.find("Alpha") < first.body.find("Beta"));

    // A later window never reuses the key.
    let later = render_intent(&home, &rooms, "nonce-8", koushi_state::CatalogLocale::En)
        .expect("an intent");
    assert_ne!(first.idempotency_key, later.idempotency_key);
    assert_eq!(first.body, later.body);
}

#[test]
fn a_room_observed_twice_is_rendered_once() {
    let rooms = [unread(ROOM_ID, "Room A", 1), unread(ROOM_ID, "Room A", 3)];
    let intent = render_intent(
        &sink("home", "me@example.invalid"),
        &rooms,
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    assert_eq!(intent.body, "Message in Room A: 3 unread");
    assert_eq!(intent.subject, "Message in Room A");
}

#[test]
fn no_intent_is_built_for_no_rooms_or_an_incomplete_destination() {
    let rooms = [unread(ROOM_ID, "Room A", 1)];
    assert!(
        render_intent(
            &sink("home", "me@example.invalid"),
            &[],
            "w-1",
            koushi_state::CatalogLocale::En
        )
        .is_none()
    );
    assert!(
        render_intent(
            &sink("home", " "),
            &rooms,
            "w-1",
            koushi_state::CatalogLocale::En
        )
        .is_none()
    );
    assert!(
        render_intent(
            &sink(" ", "me@example.invalid"),
            &rooms,
            "w-1",
            koushi_state::CatalogLocale::En
        )
        .is_none()
    );
    let mut no_sender = sink("home", "me@example.invalid");
    no_sender.from_address.clear();
    assert!(render_intent(&no_sender, &rooms, "w-1", koushi_state::CatalogLocale::En).is_none());
    let mut no_endpoint = sink("home", "me@example.invalid");
    no_endpoint.config = NotificationSinkConfig::Resend {
        endpoint: String::new(),
    };
    assert!(render_intent(&no_endpoint, &rooms, "w-1", koushi_state::CatalogLocale::En).is_none());
    // ServerChan needs no mailbox: the SendKey names the recipient.
    assert!(
        render_intent(
            &serverchan(NotificationDeliveryContent::NamesOnly),
            &rooms,
            "w-1",
            koushi_state::CatalogLocale::En
        )
        .is_some()
    );
}

#[test]
fn many_rooms_render_a_bounded_body_with_a_counted_tail() {
    let rooms: Vec<QualifyingRoom> = (0..MAX_NOTIFICATION_DIGEST_ROOMS + 3)
        .map(|index| {
            unread(
                &format!("!room{index}:example.invalid"),
                &format!("Room {index}"),
                1,
            )
        })
        .collect();
    let intent = render_intent(
        &sink("home", "me@example.invalid"),
        &rooms,
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    let named = intent
        .body
        .lines()
        .filter(|line| line.starts_with("Message in"))
        .count();
    assert_eq!(named, MAX_NOTIFICATION_DIGEST_ROOMS);
    assert!(intent.body.ends_with("and 3 more"));
    assert!(intent.subject.ends_with(&format!(
        "and {} more rooms",
        MAX_NOTIFICATION_DIGEST_ROOMS + 2
    )));
}

#[test]
fn the_interval_gate_holds_until_the_minimum_interval_has_elapsed() {
    let interval = Duration::from_secs(900);
    let start = Instant::now();
    assert!(coalesce_window(interval, None, start));
    assert!(!coalesce_window(interval, Some(start), start));
    assert!(!coalesce_window(
        interval,
        Some(start),
        start + interval - Duration::from_secs(1)
    ));
    assert!(coalesce_window(interval, Some(start), start + interval));
    assert!(coalesce_window(Duration::ZERO, Some(start), start));
    // A last attempt in the future (a clock that moved backwards) fails closed.
    assert!(!coalesce_window(
        interval,
        Some(start + Duration::from_secs(5)),
        start
    ));
}

#[test]
fn delivery_debug_output_redacts_identifiers_and_message_text() {
    let mut room = with_text(unread(ROOM_ID, "Room A", 1), "Alice: the code is 1234");
    room.space_labels = vec!["Secret Space".to_owned()];
    let room_debug = format!("{room:?}");
    assert!(room_debug.contains("RoomId(..)"));
    assert!(room_debug.contains("MessageText(..)"));
    for leaked in [ROOM_ID, "Room A", "Secret Space", "Alice", "1234"] {
        assert!(!room_debug.contains(leaked), "{leaked} must not be logged");
    }

    let home = consented(
        sink("home", "me@example.invalid"),
        NotificationDeliveryContent::Full,
    );
    let intent = render_intent(&home, &[room], "window-1", koushi_state::CatalogLocale::En)
        .expect("an intent");
    assert!(
        intent.body.contains("1234"),
        "the consented body carries the text"
    );
    let intent_debug = format!("{intent:?}");
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

const PARTNER_ID: &str = "@partner:example.invalid";

/// A one-to-one direct message with `PARTNER_ID`, whose room label is a display name.
fn direct(kind: RoomAttentionKind, alias: Option<&str>) -> QualifyingRoom {
    let mut room = room("!direct:example.invalid", "partner", kind, 1, 0);
    room.space_labels = vec!["Fixture Space".to_owned()];
    room.direct_partner = Some(DirectPartner {
        user_id: PARTNER_ID.to_owned(),
        local_alias: alias.map(str::to_owned),
    });
    room
}

fn subject_for(sink: &NotificationSinkSettings, room: QualifyingRoom) -> String {
    render_intent(sink, &[room], "w-1", koushi_state::CatalogLocale::En)
        .expect("an intent")
        .subject
}

#[test]
fn a_direct_message_is_named_from_its_partners_alias_without_a_space() {
    let home = sink("home", "me@example.invalid");
    assert_eq!(
        subject_for(&home, direct(RoomAttentionKind::Dm, Some("Hiroshi (lab)"))),
        "Direct message from Hiroshi (lab)"
    );
    let intent = render_intent(
        &home,
        &[direct(RoomAttentionKind::Dm, Some("Hiroshi (lab)"))],
        "w-1",
        koushi_state::CatalogLocale::En,
    )
    .expect("an intent");
    assert_eq!(intent.body, "Direct message from Hiroshi (lab): 1 unread");
}

#[test]
fn a_direct_message_without_an_alias_or_with_the_option_off_names_the_full_user_id() {
    let home = sink("home", "me@example.invalid");
    assert_eq!(
        subject_for(&home, direct(RoomAttentionKind::Dm, None)),
        format!("Direct message from {PARTNER_ID}")
    );
    // A blank alias is no alias.
    assert_eq!(
        subject_for(&home, direct(RoomAttentionKind::Dm, Some("  "))),
        format!("Direct message from {PARTNER_ID}")
    );
    let ids_only = NotificationSinkSettings {
        direct_partner_label: NotificationDeliveryPartnerLabel::MatrixId,
        ..home
    };
    assert_eq!(
        subject_for(&ids_only, direct(RoomAttentionKind::Dm, Some("Hiroshi"))),
        format!("Direct message from {PARTNER_ID}")
    );
}

#[test]
fn a_mention_in_a_direct_message_is_from_the_partner_and_a_group_room_stays_in() {
    let home = sink("home", "me@example.invalid");
    assert_eq!(
        subject_for(&home, direct(RoomAttentionKind::Mention, Some("Hiroshi"))),
        "Mention from Hiroshi"
    );
    let mut group = room(
        "!group:example.invalid",
        "Lab",
        RoomAttentionKind::Mention,
        1,
        1,
    );
    group.space_labels = vec!["Fixture Space".to_owned()];
    assert_eq!(subject_for(&home, group), "Mention in Lab · Fixture Space");
}

fn preview_sample() -> NotificationDeliveryPreviewSample {
    NotificationDeliveryPreviewSample {
        partner_user_id: PARTNER_ID.to_owned(),
        partner_alias: "Hiroshi".to_owned(),
        message_text: "See you at 3".to_owned(),
    }
}

#[test]
fn the_preview_is_what_a_consented_send_renders_for_the_sample() {
    // #1476: the preview and the send agree in every catalog locale.
    for (locale, content) in [
        koushi_state::CatalogLocale::En,
        koushi_state::CatalogLocale::Ja,
        koushi_state::CatalogLocale::ZhHans,
        koushi_state::CatalogLocale::ZhHant,
        koushi_state::CatalogLocale::Ko,
    ]
    .into_iter()
    .flat_map(|locale| {
        [
            NotificationDeliveryContent::NamesOnly,
            NotificationDeliveryContent::Prefix { graphemes: 4 },
            NotificationDeliveryContent::Full,
        ]
        .map(|content| (locale, content))
    }) {
        for draft in [sink("home", "me@example.invalid"), serverchan(content)] {
            let mut draft = consented(draft, content);
            let preview = preview_digest(&draft, &preview_sample(), locale);
            let mut room = direct(RoomAttentionKind::Dm, Some("Hiroshi"));
            room.space_labels.clear();
            let room = with_text(room, "See you at 3");
            let sent = render_intent(&draft, &[room], "w-1", locale).expect("an intent");
            assert_eq!(preview.subject, sent.subject);
            assert_eq!(preview.body, sent.body);
            assert_eq!(preview.sender_name, sent.sender_name);

            // The draft's own choice is previewed before its consent is accepted.
            draft.content_consent = None;
            assert_eq!(preview_digest(&draft, &preview_sample(), locale), preview);
        }
    }
}

#[test]
fn the_preview_shows_the_default_sender_name_and_none_for_serverchan() {
    let mut draft = sink("home", "me@example.invalid");
    draft.sender_name.clear();
    let preview = preview_digest(&draft, &preview_sample(), koushi_state::CatalogLocale::En);
    assert_eq!(preview.sender_name, "Koushi");
    assert_eq!(preview.subject, "Direct message from Hiroshi");
    assert_eq!(preview.body, "Direct message from Hiroshi: 1 unread");

    let wechat = serverchan(NotificationDeliveryContent::Full);
    let preview = preview_digest(&wechat, &preview_sample(), koushi_state::CatalogLocale::En);
    assert_eq!(preview.sender_name, "");
    assert!(preview.body.contains("See you at 3"));
}
