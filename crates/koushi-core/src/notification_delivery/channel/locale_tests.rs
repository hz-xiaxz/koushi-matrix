//! #1476: delivery digests are worded in the locale selected when they are rendered, and
//! localization never changes which message text a destination receives.

use koushi_notify::NotificationDeliveryFailure;
use koushi_state::CatalogLocale;

use super::tests::{
    DWELL, INTERVAL, ROOM, SINK, channel, dm, dm_with_text, fail, key, observe, send_after_dwell,
    settings,
};
use crate::executor::Instant;

#[test]
fn a_held_room_is_worded_in_the_locale_selected_at_send_time() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    // The room qualifies while the app is in English ...
    observe(&mut channel, &settings, start, &[dm(ROOM, 2)]);
    observe(&mut channel, &settings, start + DWELL, &[dm(ROOM, 2)]);
    // ... and the user switches to Japanese before the send.
    let jobs = channel.take_send_jobs(&settings, start + DWELL, CatalogLocale::Ja);
    assert_eq!(jobs.len(), 1);
    let intent = &jobs[0].intent;
    assert!(intent.body.contains("未読 2 件"), "{}", intent.body);
    assert!(!intent.body.contains("unread"), "{}", intent.body);
    assert!(
        !intent.subject.contains("Direct message"),
        "{}",
        intent.subject
    );
}

#[test]
fn a_retry_after_a_locale_change_is_reworded_under_a_new_key() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
    let first = jobs[0].intent.clone();
    assert!(first.body.contains("unread"), "{}", first.body);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );

    let due_at = sent_at + INTERVAL;
    observe(&mut channel, &settings, due_at, &[dm(ROOM, 2)]);
    let retries = channel.take_due_retries(&settings, due_at, CatalogLocale::Ko);
    assert_eq!(retries.len(), 1);
    let retry = &retries[0].intent;
    assert!(retry.body.contains("읽지 않음 2개"), "{}", retry.body);
    // The payload changed, so the provider must not answer it from the old key's cache.
    assert_ne!(retry.idempotency_key, first.idempotency_key);
}

#[test]
fn a_names_only_destination_carries_no_message_text_in_any_locale() {
    let settings = settings();
    for locale in [
        CatalogLocale::En,
        CatalogLocale::Ja,
        CatalogLocale::ZhHans,
        CatalogLocale::ZhHant,
        CatalogLocale::Ko,
        CatalogLocale::Pseudo,
    ] {
        let mut channel = channel(&settings);
        let start = Instant::now();
        let room = [dm_with_text(ROOM, 1, "synthetic private text")];
        observe(&mut channel, &settings, start, &room);
        observe(&mut channel, &settings, start + DWELL, &room);
        let jobs = channel.take_send_jobs(&settings, start + DWELL, locale);
        assert_eq!(jobs.len(), 1, "{locale:?}");
        let intent = &jobs[0].intent;
        assert!(
            !intent.body.contains("synthetic private text")
                && !intent.subject.contains("synthetic private text"),
            "{locale:?}: {intent:?}"
        );
        assert!(!intent.body.contains('>'), "{locale:?}: {}", intent.body);
    }
}
