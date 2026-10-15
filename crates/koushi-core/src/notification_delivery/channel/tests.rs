//! #1427: the delivery channel's state machine, proved without a runtime.
//!
//! Every test drives [`NotificationDeliveryChannel`] with a hand-made qualifying set, a
//! hand-made instant, and a hand-made outcome, and reads the projection the frontend would
//! render. Nothing here reads a credential, opens a socket, or waits on a clock.

use std::time::Duration;

use koushi_notify::{NotificationDeliveryFailure, NotificationDeliveryOutcome};
use koushi_state::{
    NotificationDeliveryOutcomeState, NotificationDeliverySettings, NotificationDeliverySinkState,
    NotificationSinkConfig, NotificationSinkSettings, RoomAttentionKind,
};

use super::{
    MAX_NOTIFICATION_DELIVERY_ATTEMPTS, NotificationDeliveryChannel, NotificationDeliveryJob,
    preset_states,
};
use crate::executor::Instant;
use crate::notification_delivery::dwell::QualifyingRoom;

const SINK: &str = "qq-mail";
const ROOM: &str = "!room:example.invalid";

fn settings() -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        enabled: true,
        only_when_unfocused: false,
        dwell_secs: 60,
        min_interval_secs: 900,
        sinks: vec![NotificationSinkSettings {
            id: SINK.to_owned(),
            enabled: true,
            destination: "fixture@example.invalid".to_owned(),
            from_address: "koushi@example.invalid".to_owned(),
            sender_name: "Koushi".to_owned(),
            config: NotificationSinkConfig::Resend {
                endpoint: "https://relay.example.invalid/emails".to_owned(),
            },
            secret_binding: None,
        }],
        ..NotificationDeliverySettings::default()
    }
}

fn room(room_id: &str) -> QualifyingRoom {
    QualifyingRoom {
        room_id: room_id.to_owned(),
        display_label: "Fixture Room".to_owned(),
        kind: RoomAttentionKind::Dm,
        unread: 2,
        mentions: 0,
        plaintext_prefix: None,
    }
}

/// A channel already observed as holding a credential for its one sink, bound to the
/// configuration the sink has now, which is what makes that credential usable at all.
fn channel(settings: &NotificationDeliverySettings) -> NotificationDeliveryChannel {
    let mut channel = NotificationDeliveryChannel::new(settings);
    for sink in &settings.sinks {
        let mut sink = sink.clone();
        sink.secret_binding = Some(sink.credential_identity());
        channel.record_credential(&sink, true);
    }
    channel
}

/// Take one room through its whole dwell, and report when the send happened.
fn send_after_dwell(
    channel: &mut NotificationDeliveryChannel,
    settings: &NotificationDeliverySettings,
    start: Instant,
) -> (Instant, Vec<NotificationDeliveryJob>) {
    let sent_at = start + Duration::from_secs(settings.dwell_secs);
    channel.observe(settings, start, &[room(ROOM)]);
    channel.observe(settings, sent_at, &[room(ROOM)]);
    let jobs = channel.take_send_jobs(settings, sent_at);
    (sent_at, jobs)
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

    channel.observe(&settings, start, &[room(ROOM)]);
    assert!(channel.take_send_jobs(&settings, start).is_empty());
    assert_eq!(
        channel.next_wake_at(),
        Some(start + Duration::from_secs(settings.dwell_secs))
    );
    assert!(channel.has_pending_work());

    let (_, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].sink_id, SINK);
    assert_eq!(jobs[0].intent.destination, "fixture@example.invalid");
    assert_eq!(
        jobs[0].intent.idempotency_key,
        "koushi-notification/qq-mail/1"
    );
}

#[test]
fn a_room_that_stops_qualifying_re_arms_instead_of_sending() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    channel.observe(&settings, start, &[room(ROOM)]);
    // Read, muted, or suppressed by focus: the next observation no longer lists it.
    channel.observe(&settings, start + Duration::from_secs(30), &[]);
    assert!(!channel.has_pending_work());
    assert!(channel.next_wake_at().is_none());
    assert!(
        channel
            .take_send_jobs(&settings, start + Duration::from_secs(120))
            .is_empty()
    );
}

#[test]
fn a_due_room_waits_for_the_coalescing_window() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &NotificationDeliveryOutcome::Delivered,
        7,
    );

    // The user reads the room, and a later message qualifies it again: a room that came due is
    // released once per qualifying episode, so it has to leave the set before it can arm again.
    let second = sent_at + Duration::from_secs(10);
    channel.observe(&settings, second, &[]);
    let rearmed = second + Duration::from_secs(1);
    let second_due = rearmed + Duration::from_secs(settings.dwell_secs);
    channel.observe(&settings, rearmed, &[room(ROOM)]);
    channel.observe(&settings, second_due, &[room(ROOM)]);
    // The room came due well inside the minimum interval, so the batch is held and the window
    // does not move.
    assert!(channel.take_send_jobs(&settings, second_due).is_empty());
    assert!(channel.has_pending_work());

    // Once the interval has elapsed the held batch goes out, under a new window.
    let third = sent_at + Duration::from_secs(settings.min_interval_secs);
    channel.observe(&settings, third, &[room(ROOM)]);
    let jobs = channel.take_send_jobs(&settings, third);
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].intent.idempotency_key,
        "koushi-notification/qq-mail/2"
    );
}

#[test]
fn a_channel_with_no_credential_holds_the_batch_and_does_not_advance_the_window() {
    let settings = settings();
    let mut channel = NotificationDeliveryChannel::new(&settings);
    let start = Instant::now();
    let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert!(jobs.is_empty());
    assert!(!configured_of(&channel, &settings));
    // Nothing was attempted, so nothing may have advanced the window: a user who configures the
    // credential is not made to wait an interval for a delivery that never left.
    assert!(channel.last_attempt_at.is_none());

    let mut bound = settings.sinks[0].clone();
    bound.secret_binding = Some(bound.credential_identity());
    channel.record_credential(&bound, true);
    assert_eq!(channel.take_send_jobs(&settings, sent_at).len(), 1);
}

#[test]
fn every_failure_maps_to_its_own_outcome_state() {
    let settings = settings();
    let cases = [
        (
            NotificationDeliveryOutcome::Delivered,
            NotificationDeliveryOutcomeState::Delivered { at_ms: 7 },
        ),
        (
            NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::InvalidSecret),
            NotificationDeliveryOutcomeState::InvalidSecret { at_ms: 7 },
        ),
        (
            NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::NotPermitted),
            NotificationDeliveryOutcomeState::NotPermitted { at_ms: 7 },
        ),
        (
            NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Rejected),
            NotificationDeliveryOutcomeState::Rejected { at_ms: 7 },
        ),
        (
            NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::RateLimited {
                retry_after_secs: Some(120),
            }),
            NotificationDeliveryOutcomeState::RateLimited {
                at_ms: 7,
                retry_after_secs: Some(120),
            },
        ),
        (
            NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
            NotificationDeliveryOutcomeState::Transport { at_ms: 7 },
        ),
    ];
    for (outcome, expected) in cases {
        let mut channel = channel(&settings);
        channel.record_outcome(&settings, Instant::now(), SINK, &outcome, 7);
        assert_eq!(
            outcome_of(&channel, &settings),
            Some(expected),
            "{outcome:?}"
        );
    }
}

#[test]
fn terminal_failures_are_never_retried() {
    for outcome in [
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::InvalidSecret),
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::NotPermitted),
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Rejected),
    ] {
        let settings = settings();
        let mut channel = channel(&settings);
        let start = Instant::now();
        let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
        assert_eq!(jobs.len(), 1);
        channel.record_outcome(&settings, sent_at, SINK, &outcome, 7);

        assert!(channel.retries.is_empty());
        assert!(channel.next_wake_at().is_none());
        assert!(
            channel
                .take_due_retries(sent_at + Duration::from_secs(86_400))
                .is_empty()
        );
        // The verdict is still the sink's last, so a sink that failed a terminal way cannot read
        // as ON.
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
fn a_rate_limit_and_a_transport_failure_are_retried_in_the_window() {
    for outcome in [
        // A provider that gives no delay still gets the channel's own minimum interval, which is
        // larger than the floor.
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::RateLimited {
            retry_after_secs: None,
        }),
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
    ] {
        let settings = settings();
        let mut channel = channel(&settings);
        let start = Instant::now();
        let (sent_at, jobs) = send_after_dwell(&mut channel, &settings, start);
        let intent = jobs[0].intent.clone();
        channel.record_outcome(&settings, sent_at, SINK, &outcome, 7);

        let due_at = sent_at + Duration::from_secs(settings.min_interval_secs);
        assert_eq!(channel.next_wake_at(), Some(due_at));
        assert!(
            channel
                .take_due_retries(due_at - Duration::from_secs(1))
                .is_empty()
        );
        let retries = channel.take_due_retries(due_at);
        assert_eq!(retries.len(), 1);
        // The retry is the same message under the same key, so a provider that already accepted
        // it does not send a second copy.
        assert_eq!(retries[0].intent, intent);
    }
}

#[test]
fn a_provider_that_asks_for_a_day_waits_at_most_an_hour() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let (sent_at, _) = send_after_dwell(&mut channel, &settings, start);
    channel.record_outcome(
        &settings,
        sent_at,
        SINK,
        &NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::RateLimited {
            retry_after_secs: Some(86_400),
        }),
        7,
    );
    assert_eq!(
        channel.next_wake_at(),
        Some(sent_at + Duration::from_secs(3_600))
    );
}

#[test]
fn a_retry_loop_is_bounded_by_the_attempt_cap() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    let (mut now, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    let failure = NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport);
    let step = Duration::from_secs(settings.min_interval_secs);

    channel.record_outcome(&settings, now, SINK, &failure, 7);
    for attempt in 2..=MAX_NOTIFICATION_DELIVERY_ATTEMPTS {
        let jobs = channel.take_due_retries(now + step);
        assert_eq!(jobs.len(), 1, "attempt {attempt} was not armed");
        now += step;
        channel.record_outcome(&settings, now, SINK, &failure, 7);
    }
    assert!(channel.retries.is_empty());
    assert!(channel.next_wake_at().is_none());
}

#[test]
fn a_test_send_consumes_no_window_and_arms_no_retry() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    channel.record_test_outcome(
        SINK,
        &NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
        7,
    );

    assert!(channel.last_attempt_at.is_none());
    assert!(channel.retries.is_empty());
    assert!(
        channel.inflight.is_empty(),
        "a test send is never in flight as a delivery"
    );
    assert_eq!(
        outcome_of(&channel, &settings),
        Some(NotificationDeliveryOutcomeState::Transport { at_ms: 7 })
    );
    // A real delivery still arrives on its own dwell and its own first window.
    let (_, jobs) = send_after_dwell(&mut channel, &settings, start);
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].intent.idempotency_key,
        "koushi-notification/qq-mail/1"
    );
}

#[test]
fn switching_the_channel_off_forgets_everything_held() {
    let settings = settings();
    let mut channel = channel(&settings);
    let start = Instant::now();
    channel.observe(&settings, start, &[room(ROOM)]);
    assert!(channel.has_pending_work());

    channel.observe_empty(&settings, start + Duration::from_secs(1));
    assert!(!channel.has_pending_work());
    assert!(channel.held.is_empty());
    assert!(channel.next_wake_at().is_none());
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
    // The vendor's own name and the frontend's catalog key for the credential, passed through.
    assert_eq!(presets[0].id, koushi_notify::SMTP_PRESETS[0].id);
    assert_eq!(presets[0].label, koushi_notify::SMTP_PRESETS[0].label);
    assert_eq!(
        presets[0].secret_hint,
        koushi_notify::SMTP_PRESETS[0].secret_hint
    );
}

#[test]
fn a_sink_the_settings_dropped_forgets_its_verdict() {
    let settings = settings();
    let mut channel = channel(&settings);
    channel.record_outcome(
        &settings,
        Instant::now(),
        SINK,
        &NotificationDeliveryOutcome::Delivered,
        7,
    );
    channel.retain_sinks(&NotificationDeliverySettings::default());
    assert_eq!(outcome_of(&channel, &settings), None);
    assert!(!configured_of(&channel, &settings));
}

#[test]
fn the_configured_probe_reports_staleness_only_when_the_sink_list_changes() {
    let settings = settings();
    let mut channel = NotificationDeliveryChannel::new(&settings);
    assert!(!channel.configured_sinks_match(&settings));
    channel.record_credential(&settings.sinks[0], false);
    assert!(channel.configured_sinks_match(&settings));

    let mut added = settings.clone();
    added.sinks.push(NotificationSinkSettings {
        id: "gmail".to_owned(),
        ..settings.sinks[0].clone()
    });
    assert!(!channel.configured_sinks_match(&added));

    // A credential entered for one server is not a verdict about another: changing the binding
    // makes the recorded one stale even though the sink list is unchanged.
    let mut rebound = settings.clone();
    rebound.sinks[0].secret_binding =
        Some("resend:https://elsewhere.example.invalid/emails".to_owned());
    assert!(!channel.configured_sinks_match(&rebound));
}
