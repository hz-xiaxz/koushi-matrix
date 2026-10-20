//! The runtime state of the device-local delivery channel: one lane per destination.
//!
//! Each destination is an independent lane (#1460): its dwell tracker, the room ids it
//! holds for the next window, its last attempt, its window sequence, its attempts, its
//! in-flight send, and its retry. A lane holds room **ids**, never a rendered payload: the
//! message is rendered at send time from the lane's current observation, and a retry is
//! rebuilt the same way, so neither can carry text or counts the badge no longer shows
//! (#1454). [`NotificationDeliveryChannel::next_wake_at`] includes each lane's coalescing
//! deadline while it holds rooms, so a batch held behind the minimum interval is released
//! at that deadline instead of waiting for an unrelated event (#1455).
//!
//! Nothing here reads the network, the credential store, or the wall clock - the caller
//! passes the current [`Instant`] and the current attention candidates, spawns the sends
//! the channel hands back, and reports their outcomes. That keeps the whole state machine
//! provable without a runtime, and keeps a sink secret out of this type entirely: only
//! "configured: yes or no" is stored.
//!
//! Bounds: each lane holds at most [`MAX_HELD_NOTIFICATION_ROOMS`] rooms, makes at most
//! [`MAX_NOTIFICATION_DELIVERY_ATTEMPTS`] attempts per window, and waits at most
//! [`MAX_NOTIFICATION_DELIVERY_RETRY`] for a retry. Nothing is persisted, so exiting drops
//! the queue, exactly as the canon requires.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use koushi_notify::{NotificationDeliveryFailure, NotificationDeliveryOutcome};
use koushi_state::{
    NotificationDeliveryOutcomeState, NotificationDeliveryPresetState,
    NotificationDeliverySettings, NotificationDeliverySinkState, NotificationDeliveryState,
    NotificationSinkSettings, RoomAttentionCandidate, SmtpTls,
};

use super::digest::{NotificationDeliveryIntent, coalesce_window, idempotency_key, render_intent};
use super::dwell::{DwellTracker, QualifyingRoom};
use super::qualifying::qualifying_rooms;
use crate::executor::Instant;

/// How many rooms one lane may hold for its next window.
///
/// The digest names [`super::digest::MAX_NOTIFICATION_DIGEST_ROOMS`] of them and counts the
/// rest, so this cap only bounds the memory a busy account can pin while the window is shut.
pub(crate) const MAX_HELD_NOTIFICATION_ROOMS: usize = 128;

/// How many attempts one lane gets for one window: the first send and two retries.
///
/// Only a rate limit and a transport failure are ever retried, so this bounds a provider that
/// keeps answering 429, or a network that keeps timing out, inside one window.
pub(crate) const MAX_NOTIFICATION_DELIVERY_ATTEMPTS: u32 = 3;

/// The shortest wait before a retry, whatever the provider and the settings say.
///
/// A provider that rate-limits without a `Retry-After` header is still asking this device to
/// slow down, and a transport failure deserves a moment before the same connect is tried
/// again. This is a floor, never a ceiling: the destination's minimum interval and the
/// provider's own delay both raise it.
const NOTIFICATION_DELIVERY_RETRY_FLOOR: Duration = Duration::from_secs(30);

/// The longest wait before a retry, so a provider that asks for a day cannot pin the
/// channel's only timer for a day.
const MAX_NOTIFICATION_DELIVERY_RETRY: Duration = Duration::from_secs(60 * 60);

/// One send the caller has to perform: an intent, and the sink it belongs to.
///
/// The credential is not here and is not read here; the caller reads it when it sends, so a
/// secret never enters this state machine or its `Debug`.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct NotificationDeliveryJob {
    pub sink_id: String,
    pub intent: NotificationDeliveryIntent,
}

impl fmt::Debug for NotificationDeliveryJob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationDeliveryJob")
            .field("sink_id", &self.sink_id)
            .field("intent", &self.intent)
            .finish()
    }
}

/// One attempt a lane made: which rooms it covered and what it sent.
///
/// The rendered intent is kept only so a retry can tell whether its rebuilt payload is
/// byte-identical, which is what decides whether it may reuse the idempotency key. It is
/// never sent again as is.
struct LaneAttempt {
    room_ids: BTreeSet<String>,
    intent: NotificationDeliveryIntent,
}

/// A retry of the lane's last attempt, armed for a later instant.
struct LaneRetry {
    attempt: LaneAttempt,
    not_before: Instant,
}

/// One destination's lane.
struct NotificationDeliveryLane {
    dwell: Duration,
    tracker: DwellTracker,
    /// The lane's current qualifying set, from the last observation. Rendering reads this,
    /// so a send always describes the state the badge shows at that moment.
    current: Vec<QualifyingRoom>,
    /// Room ids that came due and wait for the coalescing window.
    held: BTreeSet<String>,
    /// When this lane last attempted a send, which is what its minimum interval measures from.
    last_attempt_at: Option<Instant>,
    /// Monotonic counter: with the channel's nonce, the window half of the idempotency key. It
    /// only ever grows, so a later window can never reuse an earlier window's key.
    window_sequence: u64,
    /// Attempts made in the current window.
    attempts: u32,
    /// The send that is in flight now, if any.
    inflight: Option<LaneAttempt>,
    /// The armed retry of the current window, if any.
    retry: Option<LaneRetry>,
}

impl NotificationDeliveryLane {
    fn new(dwell: Duration) -> Self {
        Self {
            dwell,
            tracker: DwellTracker::new(dwell),
            current: Vec::new(),
            held: BTreeSet::new(),
            last_attempt_at: None,
            window_sequence: 0,
            attempts: 0,
            inflight: None,
            retry: None,
        }
    }

    fn has_pending_work(&self) -> bool {
        !self.held.is_empty() || self.retry.is_some() || self.tracker.next_due_at().is_some()
    }

    /// Observe this lane's whole current qualifying set.
    ///
    /// A room that is no longer in the set - read here or on another device, muted, no longer
    /// highlighted, or suppressed by focus - drops out of the dwell, the held ids, and the
    /// retry coverage. A retry that covers no live room is cancelled.
    fn observe(
        &mut self,
        sink: &NotificationSinkSettings,
        now: Instant,
        qualifying: Vec<QualifyingRoom>,
    ) {
        let dwell = Duration::from_secs(sink.timing.dwell_secs);
        if dwell != self.dwell {
            // A new dwell re-anchors every room on the next transition rather than reusing an
            // anchor that was measured against another duration.
            self.dwell = dwell;
            self.tracker = DwellTracker::new(dwell);
        }
        let due = self.tracker.observe(now, &qualifying);
        let live: BTreeSet<&str> = qualifying
            .iter()
            .map(|room| room.room_id.as_str())
            .collect();
        self.held.retain(|room_id| live.contains(room_id.as_str()));
        if let Some(retry) = &mut self.retry {
            retry
                .attempt
                .room_ids
                .retain(|room_id| live.contains(room_id.as_str()));
            if retry.attempt.room_ids.is_empty() {
                self.retry = None;
                self.attempts = 0;
            }
        }
        for room in due {
            if self.held.len() >= MAX_HELD_NOTIFICATION_ROOMS {
                break;
            }
            self.held.insert(room.room_id);
        }
        self.current = qualifying;
    }

    /// The current observation of these rooms, in the lane's own order.
    fn current_rooms(&self, room_ids: &BTreeSet<String>) -> Vec<QualifyingRoom> {
        self.current
            .iter()
            .filter(|room| room_ids.contains(&room.room_id))
            .cloned()
            .collect()
    }

    fn coalescing_deadline(&self, sink: &NotificationSinkSettings) -> Option<Instant> {
        self.last_attempt_at?
            .checked_add(Duration::from_secs(sink.timing.min_interval_secs))
    }

    fn next_window_id(&mut self, nonce: &str) -> String {
        self.window_sequence += 1;
        format!("{nonce}-{}", self.window_sequence)
    }
}

/// What the channel last observed about one sink's credential.
///
/// The binding is recorded with the verdict rather than beside it, because both are read at the
/// same instant and a verdict recorded for one binding says nothing about another: a preset or
/// host change drops the binding in the settings, and the credential that is still in the store
/// belongs to the server the sink used to point at.
#[derive(Clone, Eq, PartialEq)]
struct SinkCredential {
    binding: Option<String>,
    configured: bool,
}

pub(crate) struct NotificationDeliveryChannel {
    /// A random token minted once per channel, which is once per account runtime. It is part of
    /// every idempotency key, so two accounts that name a sink alike, and two runs of the app
    /// that both start counting windows at one, never hand a provider the same key for
    /// different messages.
    nonce: String,
    /// The window half of a test send's key, counted apart from every lane.
    test_sequence: u64,
    /// The key of the newest test send per sink. Only its outcome is recorded, so the verdict of
    /// a test made with a replaced credential, or of an older press, never paints the current
    /// one.
    pending_tests: BTreeMap<String, String>,
    lanes: BTreeMap<String, NotificationDeliveryLane>,
    /// What each sink's credential was, as of the last refresh, and whether the store held it.
    /// An absent entry reads as not configured.
    configured: BTreeMap<String, SinkCredential>,
    /// The sinks whose stored credential this device deleted because it was entered for a server
    /// or account the sink no longer names.
    retired: BTreeSet<String>,
    /// The last observed outcome per sink, in the shape the frontend renders.
    outcomes: BTreeMap<String, NotificationDeliveryOutcomeState>,
}

impl NotificationDeliveryChannel {
    pub(crate) fn new() -> Self {
        Self::with_nonce(random_hex_token())
    }

    /// A channel with a chosen nonce, so a test can predict its keys.
    pub(crate) fn with_nonce(nonce: String) -> Self {
        Self {
            nonce,
            test_sequence: 0,
            pending_tests: BTreeMap::new(),
            lanes: BTreeMap::new(),
            configured: BTreeMap::new(),
            retired: BTreeSet::new(),
            outcomes: BTreeMap::new(),
        }
    }

    /// Whether any lane holds rooms, waits on a retry, or holds a dwell timer that has not come
    /// due. A channel with nothing to do is left alone.
    pub(crate) fn has_pending_work(&self) -> bool {
        self.lanes
            .values()
            .any(NotificationDeliveryLane::has_pending_work)
    }

    /// Whether every sink in the settings has been observed for the binding it carries now.
    ///
    /// The caller reads the credential store when this is false, which is once per process start,
    /// once per change to the sink list, and once per change to a sink's credential binding, and
    /// never per observation. A binding change is what a preset, host, port, username or endpoint
    /// edit looks like from here, so it is what makes the stored credential stale.
    pub(crate) fn configured_sinks_match(&self, settings: &NotificationDeliverySettings) -> bool {
        self.configured.len() == settings.sinks.len()
            && settings.sinks.iter().all(|sink| {
                self.configured
                    .get(&sink.id)
                    .is_some_and(|credential| credential.binding == sink.secret_binding)
            })
    }

    /// Record what this sink's credential is, and whether the store holds it.
    ///
    /// Both halves come from the same observation of the same sink, so a verdict can never be
    /// attributed to a binding it was not reached for.
    ///
    /// A binding that differs from the one recorded before means the credential was replaced or
    /// the destination it belongs to was edited, so the sink's lane is dropped: a held room, an
    /// armed retry, or an in-flight attempt was decided for the previous credential, and a late
    /// outcome for it must not settle anything the new credential sends.
    pub(crate) fn record_credential(&mut self, sink: &NotificationSinkSettings, configured: bool) {
        if self
            .configured
            .get(&sink.id)
            .is_some_and(|credential| credential.binding != sink.secret_binding)
        {
            self.lanes.remove(&sink.id);
            self.pending_tests.remove(&sink.id);
        }
        self.configured.insert(
            sink.id.clone(),
            SinkCredential {
                binding: sink.secret_binding.clone(),
                configured,
            },
        );
    }

    /// Whether this sink may be sent through: the store confirmed a credential, and confirmed it
    /// for the binding the sink carries now.
    pub(crate) fn is_configured(&self, sink: &NotificationSinkSettings) -> bool {
        self.configured.get(&sink.id).is_some_and(|credential| {
            credential.configured && credential.binding == sink.secret_binding
        })
    }

    /// Observe the current attention candidates for every destination.
    ///
    /// Each lane builds its own qualifying set from its destination's targets, focus rule, and
    /// content mode. A destination that is not configured observes nothing, so it holds no room
    /// and a credential saved later anchors on a fresh transition rather than releasing a room
    /// that came due while the destination could not send.
    pub(crate) fn observe(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        candidates: &[RoomAttentionCandidate],
        focused: bool,
    ) {
        for sink in &settings.sinks {
            let qualifying = if self.is_configured(sink) {
                qualifying_rooms(sink, candidates, focused)
            } else {
                Vec::new()
            };
            if qualifying.is_empty() && !self.lanes.contains_key(&sink.id) {
                continue;
            }
            let dwell = Duration::from_secs(sink.timing.dwell_secs);
            self.lanes
                .entry(sink.id.clone())
                .or_insert_with(|| NotificationDeliveryLane::new(dwell))
                .observe(sink, now, qualifying);
        }
    }

    /// The earliest instant this channel needs to be called again, or `None` when it is idle.
    ///
    /// This arms the core's one delivery timer: per lane, the first room's dwell deadline, an
    /// armed retry, and - while the lane holds rooms, has no send in flight, and its destination
    /// is configured - the coalescing deadline (#1455). It mirrors the lanes' own deadlines rather
    /// than deciding anything, so a missed wake-up produces no delivery, never a wrong one.
    pub(crate) fn next_wake_at(&self, settings: &NotificationDeliverySettings) -> Option<Instant> {
        let mut earliest: Option<Instant> = None;
        let mut consider = |instant: Option<Instant>| {
            if let Some(instant) = instant {
                earliest = Some(earliest.map_or(instant, |current| current.min(instant)));
            }
        };
        for sink in &settings.sinks {
            let Some(lane) = self.lanes.get(&sink.id) else {
                continue;
            };
            consider(lane.tracker.next_due_at());
            consider(lane.retry.as_ref().map(|retry| retry.not_before));
            if !lane.held.is_empty() && lane.inflight.is_none() && self.is_configured(sink) {
                consider(lane.coalescing_deadline(sink));
            }
        }
        earliest
    }

    /// Start a new window for every lane whose held rooms may be sent now.
    ///
    /// A lane sends when it holds rooms, its coalescing window is open, no send of its own is
    /// in flight, and its destination is configured. The payload is rendered now, from the
    /// current observation of the held rooms and of any rooms an armed retry still covers, so
    /// the new window supersedes that retry instead of racing it.
    pub(crate) fn take_send_jobs(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        locale: koushi_state::CatalogLocale,
    ) -> Vec<NotificationDeliveryJob> {
        let mut jobs = Vec::new();
        for sink in &settings.sinks {
            if !self.is_configured(sink) {
                continue;
            }
            let Some(lane) = self.lanes.get_mut(&sink.id) else {
                continue;
            };
            if lane.held.is_empty()
                || lane.inflight.is_some()
                || !coalesce_window(
                    Duration::from_secs(sink.timing.min_interval_secs),
                    lane.last_attempt_at,
                    now,
                )
            {
                continue;
            }
            let mut room_ids = std::mem::take(&mut lane.held);
            if let Some(retry) = lane.retry.take() {
                room_ids.extend(retry.attempt.room_ids);
            }
            let rooms = lane.current_rooms(&room_ids);
            let window_id = lane.next_window_id(&self.nonce);
            let Some(intent) = render_intent(sink, &rooms, &window_id, locale) else {
                lane.attempts = 0;
                continue;
            };
            lane.last_attempt_at = Some(now);
            lane.attempts = 1;
            lane.inflight = Some(LaneAttempt {
                room_ids: rooms.into_iter().map(|room| room.room_id).collect(),
                intent: intent.clone(),
            });
            jobs.push(NotificationDeliveryJob {
                sink_id: sink.id.clone(),
                intent,
            });
        }
        jobs
    }

    /// The retries whose wait has elapsed, rebuilt from the current observation (#1454).
    ///
    /// A retry covers the rooms its attempt covered that still qualify; one that covers none is
    /// cancelled. If the rebuilt payload is byte-identical to the attempt's, the retry reuses its
    /// idempotency key, so a provider that already accepted it can deduplicate; otherwise it takes
    /// a new window key, because a possible duplicate notice is preferred to resending obsolete
    /// text under a key the provider may answer from its cache.
    pub(crate) fn take_due_retries(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        locale: koushi_state::CatalogLocale,
    ) -> Vec<NotificationDeliveryJob> {
        let mut jobs = Vec::new();
        for sink in &settings.sinks {
            let configured = self.is_configured(sink);
            let Some(lane) = self.lanes.get_mut(&sink.id) else {
                continue;
            };
            let due = lane
                .retry
                .as_ref()
                .is_some_and(|retry| retry.not_before <= now);
            if !due || lane.inflight.is_some() {
                continue;
            }
            let Some(retry) = lane.retry.take() else {
                continue;
            };
            if !configured {
                lane.attempts = 0;
                continue;
            }
            let rooms = lane.current_rooms(&retry.attempt.room_ids);
            let previous = retry.attempt.intent;
            let Some(mut intent) = render_intent(sink, &rooms, "retry", locale) else {
                lane.attempts = 0;
                continue;
            };
            if intent.subject == previous.subject
                && intent.body == previous.body
                && intent.destination == previous.destination
            {
                intent.idempotency_key = previous.idempotency_key;
            } else {
                let window_id = lane.next_window_id(&self.nonce);
                intent = render_intent(sink, &rooms, &window_id, locale).unwrap_or(intent);
            }
            lane.last_attempt_at = Some(now);
            lane.inflight = Some(LaneAttempt {
                room_ids: rooms.into_iter().map(|room| room.room_id).collect(),
                intent: intent.clone(),
            });
            jobs.push(NotificationDeliveryJob {
                sink_id: sink.id.clone(),
                intent,
            });
        }
        jobs
    }

    /// Record one send outcome, and arm a retry when the failure is one the canon allows to be
    /// retried and the window has attempts left.
    ///
    /// Only an outcome whose idempotency key names the lane's in-flight attempt is recorded, so a
    /// test send that finishes while a real one is in flight cannot settle the real one, and a
    /// late outcome for a credential that has since been replaced - its lane was dropped - can
    /// neither arm a retry nor paint the new credential with the old one's verdict. A matching
    /// outcome is always projected, so a failed destination never reads as healthy.
    pub(crate) fn record_outcome(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        sink_id: &str,
        idempotency_key: &str,
        outcome: &NotificationDeliveryOutcome,
        at_ms: u64,
    ) {
        let Some(sink) = settings.sink(sink_id) else {
            return;
        };
        if self.take_pending_test(sink_id, idempotency_key) {
            self.record_test_outcome(sink_id, outcome, at_ms);
            return;
        }
        let Some(lane) = self.lanes.get_mut(sink_id) else {
            return;
        };
        let Some(attempt) = lane
            .inflight
            .take_if(|attempt| attempt.intent.idempotency_key == idempotency_key)
        else {
            return;
        };
        self.outcomes
            .insert(sink_id.to_owned(), outcome_state(outcome, at_ms));
        let Some(delay) = outcome
            .failure()
            .and_then(|failure| bounded_retry_delay(sink, failure))
            .filter(|_| lane.attempts < MAX_NOTIFICATION_DELIVERY_ATTEMPTS)
        else {
            lane.attempts = 0;
            return;
        };
        lane.attempts += 1;
        lane.retry = Some(LaneRetry {
            attempt,
            not_before: now + delay,
        });
    }

    /// Settle an in-flight attempt that never reached a sink - the credential vanished or the
    /// store could not be read - without arming a retry: nothing left the device.
    pub(crate) fn abandon_inflight(&mut self, sink_id: &str, idempotency_key: &str) {
        if self.take_pending_test(sink_id, idempotency_key) {
            return;
        }
        if let Some(lane) = self.lanes.get_mut(sink_id)
            && lane
                .inflight
                .take_if(|attempt| attempt.intent.idempotency_key == idempotency_key)
                .is_some()
        {
            lane.attempts = 0;
        }
    }

    /// The window id for one test send through `sink_id`: unique per press and per run, so a
    /// provider never sees two test messages under one idempotency key. It becomes the sink's
    /// pending test, the only one whose outcome is still recorded.
    pub(crate) fn next_test_window_id(&mut self, sink_id: &str) -> String {
        self.test_sequence += 1;
        let window_id = format!("test-{}-{}", self.nonce, self.test_sequence);
        self.pending_tests
            .insert(sink_id.to_owned(), idempotency_key(sink_id, &window_id));
        window_id
    }

    /// Settle the sink's pending test send if `idempotency_key` is its key.
    fn take_pending_test(&mut self, sink_id: &str, idempotency_key: &str) -> bool {
        if self
            .pending_tests
            .get(sink_id)
            .is_some_and(|key| key == idempotency_key)
        {
            self.pending_tests.remove(sink_id);
            return true;
        }
        false
    }

    /// Record the outcome of a test send, or of a failure that is the sink's and not a lane's.
    ///
    /// A test send is deliberately not a delivery: it consumes no window, arms no retry, and
    /// touches neither the held rooms nor the dwell, so pressing "send a test" can never
    /// suppress or delay a real notification. It does record the same typed outcome, because
    /// the point of the test is to prove which verdict this sink gives.
    pub(crate) fn record_test_outcome(
        &mut self,
        sink_id: &str,
        outcome: &NotificationDeliveryOutcome,
        at_ms: u64,
    ) {
        self.outcomes
            .insert(sink_id.to_owned(), outcome_state(outcome, at_ms));
    }

    /// Drop what was observed for sinks the settings no longer hold, so a deleted and re-added
    /// sink never inherits an older verdict or lane.
    pub(crate) fn retain_sinks(&mut self, settings: &NotificationDeliverySettings) {
        let known: BTreeSet<&str> = settings.sinks.iter().map(|sink| sink.id.as_str()).collect();
        self.outcomes
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.configured
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.lanes
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.pending_tests
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.retired
            .retain(|sink_id| known.contains(sink_id.as_str()));
    }

    /// Remember that this device deleted a stored credential for this sink, because the credential
    /// was entered for a server or account the sink no longer names. Only a deletion that actually
    /// happened is worth reporting: a sink that never had a credential is not retired, it was
    /// never configured.
    pub(crate) fn mark_credential_retired(&mut self, sink_id: &str) {
        self.retired.insert(sink_id.to_owned());
    }

    /// Forget that this sink's credential was retired, which is what saving a new one does: the
    /// line the frontend draws for a retired credential is about a password that is no longer
    /// there, so a new password takes it away.
    pub(crate) fn clear_credential_retired(&mut self, sink_id: &str) {
        self.retired.remove(sink_id);
    }

    /// The whole delivery projection, as the frontend sees it.
    pub(crate) fn projection(
        &self,
        settings: &NotificationDeliverySettings,
    ) -> NotificationDeliveryState {
        NotificationDeliveryState {
            sinks: settings
                .sinks
                .iter()
                .map(|sink| NotificationDeliverySinkState {
                    sink_id: sink.id.clone(),
                    secret_configured: self.is_configured(sink),
                    secret_retired: self.retired.contains(&sink.id),
                    last_outcome: self.outcomes.get(&sink.id).copied(),
                })
                .collect(),
            presets: preset_states(),
        }
    }

    /// Whether this lane has a send in flight. Tests read it to prove a lane is settled.
    #[cfg(test)]
    pub(crate) fn lane_is_inflight(&self, sink_id: &str) -> bool {
        self.lanes
            .get(sink_id)
            .is_some_and(|lane| lane.inflight.is_some())
    }

    /// Whether this lane has an armed retry.
    #[cfg(test)]
    pub(crate) fn lane_has_retry(&self, sink_id: &str) -> bool {
        self.lanes
            .get(sink_id)
            .is_some_and(|lane| lane.retry.is_some())
    }
}

/// A random lowercase hex token of 32 digits.
///
/// It reuses the SDK's transaction-id generator, which is a v4 UUID's hex, rather than adding a
/// random-number dependency to the core.
pub(crate) fn random_hex_token() -> String {
    matrix_sdk::ruma::TransactionId::new()
        .as_str()
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|character| character.to_ascii_lowercase())
        .take(32)
        .collect()
}

/// The projection's own copy of the sender crate's mailbox presets.
///
/// `koushi-state` must not depend on a sender crate, so the state layer owns only the shape and
/// the core fills it from the crate that owns the data.
pub(crate) fn preset_states() -> Vec<NotificationDeliveryPresetState> {
    koushi_notify::SMTP_PRESETS
        .iter()
        .map(|preset| NotificationDeliveryPresetState {
            id: preset.id.to_owned(),
            label: preset.label.to_owned(),
            host: preset.host.to_owned(),
            port: preset.port,
            tls: match preset.tls {
                koushi_notify::SmtpTls::Implicit => SmtpTls::Implicit,
                koushi_notify::SmtpTls::StartTls => SmtpTls::StartTls,
            },
            username_is_full_address: preset.username_is_full_address,
            secret_hint: preset.secret_hint.to_owned(),
        })
        .collect()
}

/// One provider or transport verdict as the product renders it, with no value of its own.
fn outcome_state(
    outcome: &NotificationDeliveryOutcome,
    at_ms: u64,
) -> NotificationDeliveryOutcomeState {
    match outcome.failure() {
        None => NotificationDeliveryOutcomeState::Accepted { at_ms },
        Some(NotificationDeliveryFailure::InvalidSecret) => {
            NotificationDeliveryOutcomeState::InvalidSecret { at_ms }
        }
        Some(NotificationDeliveryFailure::NotPermitted) => {
            NotificationDeliveryOutcomeState::NotPermitted { at_ms }
        }
        Some(NotificationDeliveryFailure::Rejected) => {
            NotificationDeliveryOutcomeState::Rejected { at_ms }
        }
        Some(NotificationDeliveryFailure::RateLimited { retry_after_secs }) => {
            NotificationDeliveryOutcomeState::RateLimited {
                at_ms,
                retry_after_secs,
            }
        }
        Some(NotificationDeliveryFailure::Transport) => {
            NotificationDeliveryOutcomeState::Transport { at_ms }
        }
    }
}

/// How long to wait before retrying this failure, or `None` when the canon says never.
///
/// The canon allows exactly two automatic reactions - a rate limit and a transport failure - and
/// this device waits at least the destination's own minimum interval, at least the provider's own
/// delay when it gave one, and at least [`NOTIFICATION_DELIVERY_RETRY_FLOOR`], never more than
/// [`MAX_NOTIFICATION_DELIVERY_RETRY`].
fn bounded_retry_delay(
    sink: &NotificationSinkSettings,
    failure: NotificationDeliveryFailure,
) -> Option<Duration> {
    let provider = match failure {
        NotificationDeliveryFailure::RateLimited { retry_after_secs } => {
            Duration::from_secs(retry_after_secs.unwrap_or_default())
        }
        NotificationDeliveryFailure::Transport => NOTIFICATION_DELIVERY_RETRY_FLOOR,
        NotificationDeliveryFailure::InvalidSecret
        | NotificationDeliveryFailure::NotPermitted
        | NotificationDeliveryFailure::Rejected => return None,
    };
    Some(
        provider
            .max(Duration::from_secs(sink.timing.min_interval_secs))
            .max(NOTIFICATION_DELIVERY_RETRY_FLOOR)
            .min(MAX_NOTIFICATION_DELIVERY_RETRY),
    )
}

#[cfg(test)]
#[path = "channel/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "channel/held_recompute_tests.rs"]
mod held_recompute_tests;

#[cfg(test)]
#[path = "channel/locale_tests.rs"]
mod locale_tests;
