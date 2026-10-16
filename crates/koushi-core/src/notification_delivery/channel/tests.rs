//! #1427, #1460: the delivery channel's state machine, proved without a runtime.
//!
//! Every test drives [`NotificationDeliveryChannel`] with hand-made attention candidates, a
//! hand-made instant, and a hand-made outcome, and reads the jobs and the projection the
//! frontend would render. Nothing here reads a credential, opens a socket, or waits on a clock.

use std::time::Duration;

use koushi_notify::{NotificationDeliveryFailure, NotificationDeliveryOutcome};
use koushi_state::{
    BoundedNotificationDeliveryText, NotificationDeliveryContent, NotificationDeliveryOutcomeState,
    NotificationDeliverySettings, NotificationDeliverySinkState, NotificationSinkConfig,
    NotificationSinkSettings, RoomAttentionCandidate, RoomAttentionKind,
};

use super::{
    MAX_NOTIFICATION_DELIVERY_ATTEMPTS, NotificationDeliveryChannel, NotificationDeliveryJob,
    preset_states,
};
use crate::executor::Instant;

const SINK: &str = "qq-mail";
const ROOM: &str = "!room:example.invalid";
const NONCE: &str = "n0";
const DWELL: Duration = Duration::from_secs(60);
const INTERVAL: Duration = Duration::from_secs(900);

fn sink(id: &str) -> NotificationSinkSettings {
    let mut sink = NotificationSinkSettings {
        id: id.to_owned(),
        destination: "fixture@example.invalid".to_owned(),
        from_address: "koushi@example.invalid".to_owned(),
        sender_name: "Koushi".to_owned(),
        config: NotificationSinkConfig::Resend {
            endpoint: "https://relay.example.invalid/emails".to_owned(),
        },
        ..NotificationSinkSettings::default()
    };
    sink.timing.dwell_secs = DWELL.as_secs();
    sink.timing.min_interval_secs = INTERVAL.as_secs();
    sink.secret_binding = Some(sink.credential_binding("a1"));
    sink
}

fn settings() -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        sinks: vec![sink(SINK)],
    }
}

/// An unread direct message, which the default targets carry.
fn dm(room_id: &str, unread: u32) -> RoomAttentionCandidate {
    RoomAttentionCandidate {
        room_id: room_id.to_owned(),
        display_label: "Fixture Room".to_owned(),
        kind: RoomAttentionKind::Dm,
        unread,
        mentions: 0,
        parent_space_ids: Vec::new(),
        parent_space_labels: Vec::new(),
        newest_message_text: None,
    }
}

fn dm_with_text(room_id: &str, unread: u32, text: &str) -> RoomAttentionCandidate {
    RoomAttentionCandidate {
        newest_message_text: Some(BoundedNotificationDeliveryText {
            text: text.to_owned(),
            truncated_at_source: false,
        }),
        ..dm(room_id, unread)
    }
}

/// A channel already observed as holding a credential for every sink, bound to the
/// configuration each sink has now, which is what makes that credential usable at all.
fn channel(settings: &NotificationDeliverySettings) -> NotificationDeliveryChannel {
    let mut channel = NotificationDeliveryChannel::with_nonce(NONCE.to_owned());
    for sink in &settings.sinks {
        channel.record_credential(sink, true);
    }
    channel
}

fn observe(
    channel: &mut NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
    now: Instant,
    candidates: &[RoomAttentionCandidate],
) {
    channel.observe(settings, now, candidates, false);
}

/// Take one room through its whole dwell, and report when the send happened.
fn send_after_dwell(
    channel: &mut NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
    start: Instant,
) -> (Instant, Vec<NotificationDeliveryJob>) {
    let sent_at = start + DWELL;
    observe(channel, settings, start, &[dm(ROOM, 2)]);
    observe(channel, settings, sent_at, &[dm(ROOM, 2)]);
    let jobs = channel.take_send_jobs(settings, sent_at);
    (sent_at, jobs)
}

fn key(window: u64) -> String {
    format!("koushi-notification/{SINK}/{NONCE}-{window}")
}

fn fail(failure: NotificationDeliveryFailure) -> NotificationDeliveryOutcome {
    NotificationDeliveryOutcome::Failed(failure)
}

fn configured_of(
    channel: &NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
) -> bool {
    channel.projection(settings).sinks[0].secret_configured
}

fn outcome_of(
    channel: &NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
) -> Option<NotificationDeliveryOutcomeState> {
    channel.projection(settings).sinks[0].last_outcome
}

#[test]
fn a_room_sends_only_after_its_dwell() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();

    observe(&mut channel, &settings, start, &[dm(ROOM, 2)]);
    assert!(channel.take_send_jobs(&settings, start).is_empty());
    assert_eq!(channel.next_wake_at(&settings), Some(start + DWELL));
    assert!(channel.has_pending_work());

    let (_, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].sink_id, SINK);
    assert_eq!(jobs[0].intent.destination, "fixture@example.invalid");
    assert_eq!(jobs[0].intent.idempotency_key, key(1));
    assert!(channel.lane_is_inflight(SINK));
}

#[test]
fn a_room_read_here_or_on_another_device_re_arms_instead_of_sending() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    observe(&mut channel, &settings, start, &[dm(ROOM, 2)]);
    // Read on another device: the next sync clears the unread count, so the candidate is gone.
    observe(
        &mut channel,
        &settings,
        start + Duration::from_secs(30),
        &[],
    );
    assert!(!channel.has_pending_work());
    assert!(channel.next_wake_at(&settings).is_none());
    assert!(
        channel
            .take_send_jobs(&settings, start + Duration::from_secs(120))
            .is_empty()
    );
}

#[test]
fn a_held_room_read_elsewhere_before_the_window_opens_is_never_sent() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let (sent_at, _) = send_after_dwell(&mut channel, &settings, start);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &NotificationDeliveryOutcome::Accepted,
        7,
    );
    let other = "!other:example.invalid";
    let due = sent_at + DWELL;
    observe(
        &mut channel,
        &settings,
        sent_at,
        &[dm(ROOM, 2), dm(other, 1)],
    );
    observe(&mut channel, &settings, due, &[dm(ROOM, 2), dm(other, 1)]);
    assert!(
        channel.take_send_jobs(&settings, due).is_empty(),
        "held by the interval"
    );

    // The held room is read on another device before the interval elapses.
    observe(
        &mut channel,
        &settings,
        due + Duration::from_secs(1),
        &[dm(ROOM, 2)],
    );
    assert!(!channel.has_pending_work());
    assert!(
        channel
            .take_send_jobs(&settings, sent_at + INTERVAL)
            .is_empty()
    );
}

/// #1455: a batch held behind the minimum interval wakes the channel at the interval's
/// deadline, without waiting for an unrelated event.
#[test]
fn a_held_batch_wakes_the_channel_at_the_coalescing_deadline() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &NotificationDeliveryOutcome::Accepted,
        7,
    );

    // The user reads the room, and a later message qualifies it again.
    let second = sent_at + Duration::from_secs(10);
    observe(&mut channel, &settings, second, &[]);
    let rearmed = second + Duration::from_secs(1);
    let second_due = rearmed + DWELL;
    observe(&mut channel, &settings, rearmed, &[dm(ROOM, 1)]);
    observe(&mut channel, &settings, second_due, &[dm(ROOM, 1)]);
    // It came due inside the minimum interval, so it is held and the window does not move.
    assert!(channel.take_send_jobs(&settings, second_due).is_empty());
    assert!(channel.has_pending_work());

    // The only thing left to wait for is the interval, and the timer is armed for it.
    let deadline = sent_at + INTERVAL;
    assert_eq!(channel.next_wake_at(&settings), Some(deadline));

    // The wake-up observes nothing new and still releases the batch, under a new window.
    observe(&mut channel, &settings, deadline, &[dm(ROOM, 1)]);
    let jobs = channel.take_send_jobs(&settings, deadline);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].intent.idempotency_key, key(2));
}

#[test]
fn a_destination_without_a_credential_holds_nothing_and_anchors_afresh_once_saved() {
    let settings = settings();
    let mut channel = NotificationDeliveryChannel::with_nonce(NONCE.to_owned());
    let start = Instant::now();
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert!(jobs.is_empty());
    assert!(!configured_of(&channel, &settings));
    assert!(!channel.has_pending_work());

    // Saving a credential does not release a room that came due while nothing could send;
    // the room anchors on this observation and waits its own dwell.
    channel.record_credential(&settings.sinks[0], true);
    observe(&mut channel, &settings, sent_at, &[dm(ROOM, 2)]);
    assert!(channel.take_send_jobs(&settings, sent_at).is_empty());
    observe(&mut channel, &settings, sent_at + DWELL, &[dm(ROOM, 2)]);
    let jobs = channel.take_send_jobs(&settings, sent_at + DWELL);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].intent.idempotency_key, key(1));
}

#[test]
fn every_failure_maps_to_its_own_outcome_state() {
    let settings = settings();
    let cases = [
        (
            NotificationDeliveryOutcome::Accepted,
            NotificationDeliveryOutcomeState::Accepted { at_ms: 7 },
        ),
        (
            fail(NotificationDeliveryFailure::InvalidSecret),
            NotificationDeliveryOutcomeState::InvalidSecret { at_ms: 7 },
        ),
        (
            fail(NotificationDeliveryFailure::NotPermitted),
            NotificationDeliveryOutcomeState::NotPermitted { at_ms: 7 },
        ),
        (
            fail(NotificationDeliveryFailure::Rejected),
            NotificationDeliveryOutcomeState::Rejected { at_ms: 7 },
        ),
        (
            fail(NotificationDeliveryFailure::RateLimited {
                retry_after_secs: Some(120),
            }),
            NotificationDeliveryOutcomeState::RateLimited {
                at_ms: 7,
                retry_after_secs: Some(120),
            },
        ),
        (
            fail(NotificationDeliveryFailure::Transport),
            NotificationDeliveryOutcomeState::Transport { at_ms: 7 },
        ),
    ];
    for (outcome, expected) in cases {
        let mut sent = channel(&settings);
        let (sent_at, _) = send_after_dwell(&mut sent, &settings, Instant::now());
        sent.record_outcome(&settings, sent_at, SINK, &key(1), &outcome, 7);
        assert_eq!(outcome_of(&sent, &settings), Some(expected), "{outcome:?}");
        // A test send's verdict is projected the same way.
        let mut tested = channel(&settings);
        tested.record_test_outcome(SINK, &outcome, 7);
        assert_eq!(outcome_of(&tested, &settings), Some(expected));
    }
}

#[test]
fn terminal_failures_are_never_retried() {
    for outcome in [
        fail(NotificationDeliveryFailure::InvalidSecret),
        fail(NotificationDeliveryFailure::NotPermitted),
        fail(NotificationDeliveryFailure::Rejected),
    ] {
        let settings = settings();
        let mut channel = channel(&settings);
        let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
        assert_eq!(jobs.len(), 1);
        channel.record_outcome(&settings, sent_at, SINK, &key(1), &outcome, 7);

        assert!(!channel.lane_has_retry(SINK));
        assert!(!channel.lane_is_inflight(SINK));
        assert!(channel.next_wake_at(&settings).is_none());
        assert!(
            channel
                .take_due_retries(&settings, sent_at + Duration::from_secs(86_400))
                .is_empty()
        );
        // The verdict is still the sink's last, so a sink that failed a terminal way cannot
        // read as healthy.
        assert!(matches!(
            outcome_of(&channel, &settings),
            Some(
                NotificationDeliveryOutcomeState::InvalidSecret { .. }
                    | NotificationDeliveryOutcomeState::NotPermitted { .. }
                    | NotificationDeliveryOutcomeState::Rejected { .. }
            )
        ));
    }
}

#[test]
fn an_unchanged_retry_repeats_the_message_under_the_same_key() {
    for outcome in [
        // A provider that gives no delay still waits the destination's own minimum interval,
        // which is larger than the floor.
        fail(NotificationDeliveryFailure::RateLimited {
            retry_after_secs: None,
        }),
        fail(NotificationDeliveryFailure::Transport),
    ] {
        let settings = settings();
        let mut channel = channel(&settings);
        let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
        let intent = jobs[0].intent.clone();
        channel.record_outcome(&settings, sent_at, SINK, &key(1), &outcome, 7);

        let due_at = sent_at + INTERVAL;
        assert_eq!(channel.next_wake_at(&settings), Some(due_at));
        assert!(
            channel
                .take_due_retries(&settings, due_at - Duration::from_secs(1))
                .is_empty()
        );
        observe(&mut channel, &settings, due_at, &[dm(ROOM, 2)]);
        let retries = channel.take_due_retries(&settings, due_at);
        assert_eq!(retries.len(), 1);
        // Byte-identical, so a provider that already accepted it can deduplicate.
        assert_eq!(retries[0].intent, intent);
    }
}

/// #1454: a retry is rebuilt from the current observation, never replayed. Text that was
/// edited, or counts that moved, go out under a new window key, so a provider cannot answer
/// the obsolete payload's key from its cache.
#[test]
fn a_retry_carries_the_current_state_under_a_new_key_when_it_changed() {
    let mut settings = settings();
    let full = &mut settings.sinks[0];
    full.content = NotificationDeliveryContent::Full;
    full.content_consent = Some(full.consent_for_current(1));
    let mut channel = channel(&settings);
    let start = Instant::now();
    let sent_at = start + DWELL;
    observe(
        &mut channel,
        &settings,
        start,
        &[dm_with_text(ROOM, 1, "meet at 10")],
    );
    observe(
        &mut channel,
        &settings,
        sent_at,
        &[dm_with_text(ROOM, 1, "meet at 10")],
    );
    let jobs = channel.take_send_jobs(&settings, sent_at);
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].intent.body.contains("> meet at 10"));
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );

    // The message is edited and another one arrives before the retry is due.
    let due_at = sent_at + INTERVAL;
    observe(
        &mut channel,
        &settings,
        due_at,
        &[dm_with_text(ROOM, 2, "meet at 11")],
    );
    let retries = channel.take_due_retries(&settings, due_at);
    assert_eq!(retries.len(), 1);
    let body = &retries[0].intent.body;
    assert!(body.contains("> meet at 11"), "{body}");
    assert!(body.contains("2 unread"), "{body}");
    assert!(
        !body.contains("meet at 10"),
        "the obsolete text is never resent"
    );
    assert_eq!(retries[0].intent.idempotency_key, key(2));
}

#[test]
fn a_retry_whose_rooms_were_all_read_elsewhere_is_cancelled() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (sent_at, _) = send_after_dwell(&mut channel, &settings, Instant::now());
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );
    assert!(channel.lane_has_retry(SINK));

    observe(
        &mut channel,
        &settings,
        sent_at + Duration::from_secs(5),
        &[],
    );
    assert!(!channel.lane_has_retry(SINK));
    assert!(channel.next_wake_at(&settings).is_none());
    assert!(
        channel
            .take_due_retries(&settings, sent_at + INTERVAL)
            .is_empty()
    );
}

#[test]
fn a_provider_that_asks_for_a_day_waits_at_most_an_hour() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (sent_at, _) = send_after_dwell(&mut channel, &settings, Instant::now());
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::RateLimited {
            retry_after_secs: Some(86_400),
        }),
        7,
    );
    assert_eq!(
        channel.next_wake_at(&settings),
        Some(sent_at + Duration::from_secs(3_600))
    );
}

#[test]
fn a_retry_loop_is_bounded_by_the_attempt_cap() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (mut now, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
    assert_eq!(jobs.len(), 1);
    let failure = fail(NotificationDeliveryFailure::Transport);

    channel.record_outcome(&settings, now, SINK, &key(1), &failure, 7);
    for attempt in 2..=MAX_NOTIFICATION_DELIVERY_ATTEMPTS {
        now += INTERVAL;
        observe(&mut channel, &settings, now, &[dm(ROOM, 2)]);
        let jobs = channel.take_due_retries(&settings, now);
        assert_eq!(jobs.len(), 1, "attempt {attempt} was not armed");
        assert_eq!(jobs[0].intent.idempotency_key, key(1));
        channel.record_outcome(&settings, now, SINK, &key(1), &failure, 7);
    }
    assert!(!channel.lane_has_retry(SINK));
    assert!(channel.next_wake_at(&settings).is_none());
}

#[test]
fn a_test_send_consumes_no_window_and_arms_no_retry() {
    let settings = settings();
    let mut channel = channel(&settings);
    assert_eq!(channel.next_test_window_id(SINK), format!("test-{NONCE}-1"));
    assert_eq!(channel.next_test_window_id(SINK), format!("test-{NONCE}-2"));
    // Only the newest press is still pending: the first one's verdict is stale.
    let first = format!("koushi-notification/{SINK}/test-{NONCE}-1");
    let second = format!("koushi-notification/{SINK}/test-{NONCE}-2");
    channel.record_outcome(
        &settings,
        Instant::now(),
        SINK,
        &first,
        &NotificationDeliveryOutcome::Accepted,
        5,
    );
    assert_eq!(outcome_of(&channel, &settings), None);
    channel.record_outcome(
        &settings,
        Instant::now(),
        SINK,
        &second,
        &fail(NotificationDeliveryFailure::Transport),
        7,
    );

    assert!(!channel.lane_has_retry(SINK));
    assert!(!channel.lane_is_inflight(SINK));
    assert_eq!(
        outcome_of(&channel, &settings),
        Some(NotificationDeliveryOutcomeState::Transport { at_ms: 7 })
    );
    // A real delivery still arrives on its own dwell and its own first window.
    let (_, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].intent.idempotency_key, key(1));
}

#[test]
fn two_channels_never_share_an_idempotency_key() {
    let settings = settings();
    let mut keys = Vec::new();
    for _ in 0..2 {
        let mut channel = NotificationDeliveryChannel::new();
        channel.record_credential(&settings.sinks[0], true);
        let (_, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
        keys.push(jobs[0].intent.idempotency_key.clone());
        keys.push(channel.next_test_window_id(SINK));
    }
    assert_ne!(keys[0], keys[2], "a restart counts windows from one again");
    assert_ne!(keys[1], keys[3]);
}

#[test]
fn a_replaced_credential_drops_the_lane_and_ignores_the_late_outcome() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
    assert_eq!(jobs.len(), 1);

    // The user saves a new credential while the send is in flight.
    let mut rebound = settings.clone();
    rebound.sinks[0].secret_binding = Some(rebound.sinks[0].credential_binding("b2"));
    channel.record_credential(&rebound.sinks[0], true);
    assert!(!channel.lane_is_inflight(SINK));

    // The old send's failure arrives late: no retry, and no verdict about the new credential.
    channel.record_outcome(
        &rebound,
        sent_at,
        SINK,
        &key(1),
        &fail(NotificationDeliveryFailure::InvalidSecret),
        7,
    );
    assert!(!channel.lane_has_retry(SINK));
    assert_eq!(outcome_of(&channel, &rebound), None);
    assert!(configured_of(&channel, &rebound));
}

#[test]
fn an_outcome_for_another_attempt_never_settles_the_in_flight_one() {
    let settings = settings();
    let mut channel = channel(&settings);
    let (sent_at, _) = send_after_dwell(&mut channel, &settings, Instant::now());
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        "koushi-notification/qq-mail/test-n0-1",
        &NotificationDeliveryOutcome::Accepted,
        7,
    );
    assert!(channel.lane_is_inflight(SINK));
    channel.abandon_inflight(SINK, &key(1));
    assert!(!channel.lane_is_inflight(SINK));
    assert!(!channel.lane_has_retry(SINK));
}

#[test]
fn each_destination_keeps_its_own_lane_and_interval() {
    let mut settings = settings();
    let mut fast = sink("wechat");
    fast.timing.min_interval_secs = 0;
    settings.sinks.push(fast);
    let mut channel = channel(&settings);
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, Instant::now());
    assert_eq!(jobs.len(), 2);
    for job in &jobs {
        channel.record_outcome(
            &settings,
            sent_at,
            &job.sink_id,
            &job.intent.idempotency_key,
            &NotificationDeliveryOutcome::Accepted,
            7,
        );
    }

    // A second room comes due: only the destination with no interval sends it now.
    let other = "!other:example.invalid";
    observe(
        &mut channel,
        &settings,
        sent_at,
        &[dm(ROOM, 2), dm(other, 1)],
    );
    observe(
        &mut channel,
        &settings,
        sent_at + DWELL,
        &[dm(ROOM, 2), dm(other, 1)],
    );
    let jobs = channel.take_send_jobs(&settings, sent_at + DWELL);
    let sinks: Vec<&str> = jobs.iter().map(|job| job.sink_id.as_str()).collect();
    assert_eq!(sinks, vec!["wechat"]);
}

#[test]
fn the_projection_names_the_settings_sinks_and_the_sender_catalog() {
    let settings = settings();
    let channel = channel(&settings);
    let projection = channel.projection(&settings);
    assert_eq!(
        projection.sinks,
        vec![NotificationDeliverySinkState {
            sink_id: SINK.to_owned(),
            secret_configured: true,
            last_outcome: None,
            secret_retired: false,
        }]
    );
    let presets = preset_states();
    assert_eq!(projection.presets, presets);
    assert!(!presets.is_empty());
    assert_eq!(presets[0].id, koushi_notify::SMTP_PRESETS[0].id);
    assert_eq!(presets[0].label, koushi_notify::SMTP_PRESETS[0].label);
    assert_eq!(
        presets[0].secret_hint,
        koushi_notify::SMTP_PRESETS[0].secret_hint
    );
}

#[test]
fn a_sink_the_settings_dropped_forgets_its_verdict_and_lane() {
    let settings = settings();
    let mut channel = channel(&settings);
    channel.record_test_outcome(SINK, &NotificationDeliveryOutcome::Accepted, 7);
    observe(&mut channel, &settings, Instant::now(), &[dm(ROOM, 2)]);
    channel.retain_sinks(&NotificationDeliverySettings::default());
    assert_eq!(outcome_of(&channel, &settings), None);
    assert!(!configured_of(&channel, &settings));
    assert!(!channel.has_pending_work());
}

#[test]
fn the_configured_probe_reports_staleness_when_the_list_or_a_binding_changes() {
    let settings = settings();
    let mut channel = NotificationDeliveryChannel::with_nonce(NONCE.to_owned());
    assert!(!channel.configured_sinks_match(&settings));
    channel.record_credential(&settings.sinks[0], false);
    assert!(channel.configured_sinks_match(&settings));

    let mut added = settings.clone();
    added.sinks.push(sink("gmail"));
    assert!(!channel.configured_sinks_match(&added));

    // A credential entered for one server is not a verdict about another.
    let mut rebound = settings.clone();
    rebound.sinks[0].config = NotificationSinkConfig::Resend {
        endpoint: "https://elsewhere.example.invalid/emails".to_owned(),
    };
    rebound.sinks[0].secret_binding = Some(rebound.sinks[0].credential_binding("a1"));
    assert!(!channel.configured_sinks_match(&rebound));
}
