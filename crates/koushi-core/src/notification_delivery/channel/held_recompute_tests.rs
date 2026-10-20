//! #1454, #1463: a held batch and a retry are rendered from the current observation.
//!
//! An edit, a redaction, or a room that stops qualifying after the dwell but before the
//! held send (or before a retry) must never let the superseded text, counts, or label
//! leave the device, and must not move the batch's coalescing deadline.

use std::time::Duration;

use koushi_notify::{NotificationDeliveryFailure, NotificationDeliveryOutcome};
use koushi_state::{
    NotificationDeliveryContent, NotificationDeliverySettings, RoomAttentionCandidate,
};

use super::NotificationDeliveryChannel;
use super::tests::{
    DWELL, INTERVAL, ROOM, SINK, channel, dm, dm_with_text, fail, key, observe, send_after_dwell,
    settings,
};
use crate::executor::Instant;

/// Settings whose one destination carries the entire message text under a consent that
/// matches it, so a stale prefix would be visible in the body.
fn full_text_settings() -> NotificationDeliverySettings {
    let mut settings = settings();
    let full = &mut settings.sinks[0];
    full.content = NotificationDeliveryContent::Full;
    full.content_consent = Some(full.consent_for_current(1));
    settings
}

fn labelled(candidate: RoomAttentionCandidate, label: &str) -> RoomAttentionCandidate {
    RoomAttentionCandidate {
        display_label: label.to_owned(),
        ..candidate
    }
}

/// Deliver one window, then let `held` come due inside the minimum interval so the lane
/// holds it. Returns the coalescing deadline at which the held batch may be sent.
fn hold_behind_interval(
    channel: &mut NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
    held: &[RoomAttentionCandidate],
) -> Instant {
    let (sent_at, jobs) = send_after_dwell(channel, settings, Instant::now());
    assert_eq!(jobs.len(), 1);
    channel.record_outcome(
        settings,
        sent_at,
        SINK,
        &key(1),
        &NotificationDeliveryOutcome::Accepted,
        7,
    );
    // The user reads the room, and later messages qualify it again.
    observe(channel, settings, sent_at + Duration::from_secs(10), &[]);
    let rearmed = sent_at + Duration::from_secs(11);
    observe(channel, settings, rearmed, held);
    observe(channel, settings, rearmed + DWELL, held);
    assert!(
        channel
            .take_send_jobs(settings, rearmed + DWELL, koushi_state::CatalogLocale::En)
            .is_empty(),
        "held by the interval"
    );
    let deadline = sent_at + INTERVAL;
    assert_eq!(channel.next_wake_at(settings), Some(deadline));
    deadline
}

/// #1454, #1463: an edit after the dwell but before the held send replaces the held room's
/// text, counts, and label, and keeps the original coalescing deadline.
#[test]
fn a_held_room_edited_before_its_send_carries_the_current_text_counts_and_label() {
    let settings = full_text_settings();
    let mut channel = channel(&settings);
    let deadline = hold_behind_interval(
        &mut channel,
        &settings,
        &[dm_with_text(ROOM, 2, "original text")],
    );

    let edited_at = deadline - Duration::from_secs(1);
    observe(
        &mut channel,
        &settings,
        edited_at,
        &[labelled(
            dm_with_text(ROOM, 3, "revised text"),
            "Renamed Room",
        )],
    );
    // A newer observation of a held room does not move its window.
    assert!(
        channel
            .take_send_jobs(&settings, edited_at, koushi_state::CatalogLocale::En)
            .is_empty()
    );
    assert_eq!(channel.next_wake_at(&settings), Some(deadline));

    // The wake-up at the deadline sends from the newest observation.
    let jobs = channel.take_send_jobs(&settings, deadline, koushi_state::CatalogLocale::En);
    assert_eq!(jobs.len(), 1);
    let intent = &jobs[0].intent;
    assert!(intent.body.contains("> revised text"), "{}", intent.body);
    assert!(intent.body.contains("Renamed Room"), "{}", intent.body);
    assert!(intent.body.contains("3 unread"), "{}", intent.body);
    assert!(
        intent.subject.contains("Renamed Room"),
        "{}",
        intent.subject
    );
    for stale in ["original text", "Fixture Room", "2 unread"] {
        assert!(
            !intent.body.contains(stale) && !intent.subject.contains(stale),
            "the superseded {stale:?} is never sent: {intent:?}"
        );
    }
    assert_eq!(intent.idempotency_key, key(2));
}

/// #1454, #1463: a redacted latest event contributes no prefix, while another unread message
/// keeps the held room qualifying.
#[test]
fn a_held_room_whose_latest_message_is_redacted_sends_no_prefix() {
    let settings = full_text_settings();
    let mut channel = channel(&settings);
    let deadline = hold_behind_interval(
        &mut channel,
        &settings,
        &[dm_with_text(ROOM, 2, "original text")],
    );

    // The projection omits a redacted latest event's text; one unread message remains.
    observe(
        &mut channel,
        &settings,
        deadline - Duration::from_secs(1),
        &[dm(ROOM, 1)],
    );
    let jobs = channel.take_send_jobs(&settings, deadline, koushi_state::CatalogLocale::En);
    assert_eq!(jobs.len(), 1);
    let body = &jobs[0].intent.body;
    assert!(!body.contains("original text"), "{body}");
    assert!(!body.contains("\n>"), "no prefix at all: {body}");
    assert!(body.contains("1 unread"), "{body}");
}

/// #1454, #1463: a held room that stops qualifying sends nothing, and a room still held
/// beside it is sent without the dropped room's text.
#[test]
fn a_held_room_that_stops_qualifying_contributes_nothing_to_the_send() {
    let settings = full_text_settings();
    let other = "!other:example.invalid";

    let mut channel = channel(&settings);
    let deadline = hold_behind_interval(
        &mut channel,
        &settings,
        &[dm_with_text(ROOM, 2, "original text")],
    );
    observe(
        &mut channel,
        &settings,
        deadline - Duration::from_secs(1),
        &[],
    );
    assert!(
        channel
            .take_send_jobs(&settings, deadline, koushi_state::CatalogLocale::En)
            .is_empty()
    );
    assert!(!channel.has_pending_work());

    let mut channel = self::channel(&settings);
    let both = [
        dm_with_text(ROOM, 2, "original text"),
        labelled(dm_with_text(other, 1, "other text"), "Other Room"),
    ];
    let deadline = hold_behind_interval(&mut channel, &settings, &both);
    observe(
        &mut channel,
        &settings,
        deadline - Duration::from_secs(1),
        &both[1..],
    );
    let jobs = channel.take_send_jobs(&settings, deadline, koushi_state::CatalogLocale::En);
    assert_eq!(jobs.len(), 1);
    let body = &jobs[0].intent.body;
    assert!(body.contains("> other text"), "{body}");
    assert!(!body.contains("original text"), "{body}");
    assert!(!body.contains("Fixture Room"), "{body}");
}

/// #1454, #1463: a retry after the latest message is redacted is rebuilt without the
/// redacted prefix, under a new window key, and keeps the attempt bookkeeping.
#[test]
fn a_retry_after_a_redaction_never_resends_the_redacted_prefix() {
    let settings = full_text_settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let sent_at = start + DWELL;
    let original = [dm_with_text(ROOM, 2, "original text")];
    observe(&mut channel, &settings, start, &original);
    observe(&mut channel, &settings, sent_at, &original);
    let jobs = channel.take_send_jobs(&settings, sent_at, koushi_state::CatalogLocale::En);
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].intent.body.contains("> original text"));
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );

    observe(
        &mut channel,
        &settings,
        sent_at + Duration::from_secs(5),
        &[labelled(dm(ROOM, 1), "Renamed Room")],
    );
    let due_at = sent_at + INTERVAL;
    assert_eq!(channel.next_wake_at(&settings), Some(due_at));
    let retries = channel.take_due_retries(&settings, due_at, koushi_state::CatalogLocale::En);
    assert_eq!(retries.len(), 1);
    let intent = &retries[0].intent;
    assert!(!intent.body.contains("original text"), "{}", intent.body);
    assert!(!intent.body.contains("\n>"), "{}", intent.body);
    assert!(
        intent.body.contains("Renamed Room: 1 unread"),
        "{}",
        intent.body
    );
    assert_ne!(intent.idempotency_key, key(1));
    assert_eq!(intent.idempotency_key, key(2));

    // The rebuilt retry is still the window's second attempt: its outcome settles it and a
    // further retryable failure arms the third and last attempt.
    channel.record_outcome(
        &settings,
        due_at,
        SINK,
        &key(2),
        &fail(NotificationDeliveryFailure::Transport),
        8,
    );
    assert!(channel.lane_has_retry(SINK));
}

/// #1454, #1463: a retry covering a room that stopped qualifying is rebuilt from the rooms
/// that still qualify, so the dropped room's text never goes out again.
#[test]
fn a_retry_drops_a_room_that_stopped_qualifying_and_its_text() {
    let settings = full_text_settings();
    let other = "!other:example.invalid";
    let mut channel = channel(&settings);
    let start = Instant::now();
    let sent_at = start + DWELL;
    let both = [
        dm_with_text(ROOM, 2, "original text"),
        labelled(dm_with_text(other, 1, "other text"), "Other Room"),
    ];
    observe(&mut channel, &settings, start, &both);
    observe(&mut channel, &settings, sent_at, &both);
    let jobs = channel.take_send_jobs(&settings, sent_at, koushi_state::CatalogLocale::En);
    assert_eq!(jobs.len(), 1);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );

    let due_at = sent_at + INTERVAL;
    observe(&mut channel, &settings, due_at, &both[1..]);
    let retries = channel.take_due_retries(&settings, due_at, koushi_state::CatalogLocale::En);
    assert_eq!(retries.len(), 1);
    let body = &retries[0].intent.body;
    assert!(body.contains("> other text"), "{body}");
    assert!(!body.contains("original text"), "{body}");
    assert_eq!(retries[0].intent.idempotency_key, key(2));
}
