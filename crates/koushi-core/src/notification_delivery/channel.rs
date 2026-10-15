//! The runtime state of the device-local delivery channel.
//!
//! Everything the channel has to remember between two observations lives here: the dwell
//! trackers of the rooms that are still qualifying, the rooms that came due while the
//! coalescing window was closed, which sinks hold a credential, how each sink's last send
//! ended, and the retries of the window that is still open. Nothing here reads the network,
//! the credential store, or the wall clock - the caller passes the current [`Instant`] and
//! the current qualifying set, spawns the sends the channel hands back, and reports their
//! outcomes. That keeps the whole state machine provable without a runtime, and keeps a sink
//! secret out of this type entirely: only "configured: yes or no" is stored.
//!
//! Bounds: the held set is capped at [`MAX_HELD_NOTIFICATION_ROOMS`], at most
//! [`MAX_NOTIFICATION_DELIVERY_ATTEMPTS`] attempts are made per sink per window, and a retry
//! waits at most [`MAX_NOTIFICATION_DELIVERY_RETRY`]. Nothing is persisted, so exiting drops
//! the queue, exactly as the canon requires.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use koushi_notify::{NotificationDeliveryFailure, NotificationDeliveryOutcome};
use koushi_state::{
    NotificationDeliveryOutcomeState, NotificationDeliveryPresetState,
    NotificationDeliverySettings, NotificationDeliverySinkState, NotificationDeliveryState,
    NotificationSinkSettings, SmtpTls,
};

use super::digest::{NotificationDeliveryIntent, coalesce_window, digest};
use super::dwell::{DwellTracker, QualifyingRoom};
use crate::executor::Instant;

/// How many rooms one held batch may carry into the next window.
///
/// The digest names [`super::digest::MAX_NOTIFICATION_DIGEST_ROOMS`] of them and counts the
/// rest, so this cap only bounds the memory a busy account can pin while the window is shut.
pub(crate) const MAX_HELD_NOTIFICATION_ROOMS: usize = 128;

/// How many attempts one sink gets for one window: the first send and two retries.
///
/// Only a rate limit and a transport failure are ever retried, so this bounds a provider that
/// keeps answering 429, or a network that keeps timing out, inside one window.
pub(crate) const MAX_NOTIFICATION_DELIVERY_ATTEMPTS: u32 = 3;

/// The shortest wait before a retry, whatever the provider and the settings say.
///
/// A provider that rate-limits without a `Retry-After` header is still asking this device to
/// slow down, and a transport failure deserves a moment before the same connect is tried
/// again. This is a floor, never a ceiling: `min_interval_secs` and the provider's own delay
/// both raise it.
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

/// A retry of the window that is still open, armed for a later instant.
struct NotificationDeliveryRetry {
    intent: NotificationDeliveryIntent,
    not_before: Instant,
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
    dwell: Duration,
    tracker: DwellTracker,
    /// Rooms that came due while the channel could not send - the coalescing window was
    /// closed, or no sink could send - keyed by room id so a newer observation of the same
    /// room replaces the older one.
    held: BTreeMap<String, QualifyingRoom>,
    /// What each sink's credential was, as of the last refresh, and whether the store held it.
    /// An absent entry reads as not configured.
    configured: BTreeMap<String, SinkCredential>,
    /// The sinks whose stored credential this device deleted because it was entered for a server
    /// or account the sink no longer names.
    retired: BTreeSet<String>,
    /// The last observed outcome per sink, in the shape the frontend renders.
    outcomes: BTreeMap<String, NotificationDeliveryOutcomeState>,
    /// The intent of each send that is in flight right now, so a retryable failure can be
    /// retried with the same body and the same idempotency key.
    inflight: BTreeMap<String, NotificationDeliveryIntent>,
    /// Armed retries of the current window, keyed by sink.
    retries: BTreeMap<String, NotificationDeliveryRetry>,
    /// Attempts made for each sink in the current window, so a retry loop is bounded.
    attempts: BTreeMap<String, u32>,
    /// When the channel last attempted a send, which is what the coalescing window measures
    /// from.
    last_attempt_at: Option<Instant>,
    /// Monotonic counter: the window half of the idempotency key. It only ever grows, so a
    /// later window can never reuse an earlier window's key.
    window_sequence: u64,
}

impl NotificationDeliveryChannel {
    pub(crate) fn new(settings: &NotificationDeliverySettings) -> Self {
        Self {
            dwell: Duration::from_secs(settings.dwell_secs),
            tracker: DwellTracker::new(Duration::from_secs(settings.dwell_secs)),
            held: BTreeMap::new(),
            configured: BTreeMap::new(),
            retired: BTreeSet::new(),
            outcomes: BTreeMap::new(),
            inflight: BTreeMap::new(),
            retries: BTreeMap::new(),
            attempts: BTreeMap::new(),
            last_attempt_at: None,
            window_sequence: 0,
        }
    }

    /// Whether the channel is holding rooms for the next window, waiting on a retry, or holding
    /// a dwell timer that has not come due. A channel with nothing to do is left alone.
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.held.is_empty() || !self.retries.is_empty() || self.tracker.next_due_at().is_some()
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

    /// Observe that nothing qualifies, which forgets the dwell, the held rooms, and the retries.
    ///
    /// This is what switching the channel off does: the canon re-arms a room's dwell on its next
    /// transition, so a channel that was off anchors on the transition that follows it back on.
    pub(crate) fn observe_empty(&mut self, settings: &NotificationDeliverySettings, now: Instant) {
        self.observe(settings, now, &[]);
    }

    /// Record what this sink's credential is, and whether the store holds it.
    ///
    /// Both halves come from the same observation of the same sink, so a verdict can never be
    /// attributed to a binding it was not reached for.
    pub(crate) fn record_credential(&mut self, sink: &NotificationSinkSettings, configured: bool) {
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
    fn is_configured(&self, sink_id: &str) -> bool {
        self.configured
            .get(sink_id)
            .is_some_and(|credential| credential.configured)
    }

    /// Observe the whole current qualifying set, and return the rooms that came due.
    ///
    /// A room that is no longer in the set - read, muted, low priority, no longer highlighted,
    /// or suppressed because the device is focused - drops out of both the dwell tracker and
    /// the held batch, so it re-arms from scratch if it qualifies again. A channel whose
    /// qualifying set is empty has nothing to tell anyone, so an armed retry of an older window
    /// is dropped too rather than mailing a state the badge no longer shows.
    pub(crate) fn observe(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        qualifying: &[QualifyingRoom],
    ) -> Vec<QualifyingRoom> {
        self.sync_dwell(settings);
        let due = self.tracker.observe(now, qualifying);
        let live: BTreeSet<&str> = qualifying
            .iter()
            .map(|room| room.room_id.as_str())
            .collect();
        self.held
            .retain(|room_id, _| live.contains(room_id.as_str()));
        if qualifying.is_empty() {
            self.retries.clear();
        }
        for room in &due {
            if self.held.len() >= MAX_HELD_NOTIFICATION_ROOMS {
                break;
            }
            self.held
                .entry(room.room_id.clone())
                .or_insert_with(|| room.clone());
        }
        due
    }

    /// The earliest instant this channel needs to be called again, or `None` when it is idle.
    ///
    /// This is what arms the core's one delivery timer: the first room's dwell deadline, or an
    /// armed retry, whichever comes first. It mirrors the tracker's own deadline rather than
    /// deciding anything, so a missed wake-up produces no delivery, never a wrong one.
    pub(crate) fn next_wake_at(&self) -> Option<Instant> {
        match (self.tracker.next_due_at(), self.earliest_retry()) {
            (Some(due), Some(retry)) => Some(due.min(retry)),
            (due, retry) => due.or(retry),
        }
    }

    /// Take the rooms that came due into one digest per sink that can actually send.
    ///
    /// The rooms are released only when at least one sink is enabled, complete, and holding a
    /// credential: with no sink to send through, the batch stays held for the next window
    /// instead of being silently dropped, and the coalescing window does not advance, so a user
    /// who configures a credential is not made to wait an interval for a delivery that never
    /// happened.
    pub(crate) fn take_send_jobs(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
    ) -> Vec<NotificationDeliveryJob> {
        if self.held.is_empty() || !coalesce_window(settings, self.last_attempt_at, now) {
            return Vec::new();
        }
        let window_id = (self.window_sequence + 1).to_string();
        let rooms: Vec<QualifyingRoom> = self.held.values().cloned().collect();
        let mut jobs = Vec::new();
        for intent in digest(settings, &rooms, &window_id) {
            if !self.is_configured(&intent.sink_id) {
                continue;
            }
            jobs.push(NotificationDeliveryJob {
                sink_id: intent.sink_id.clone(),
                intent,
            });
        }
        if jobs.is_empty() {
            return Vec::new();
        }

        self.window_sequence += 1;
        self.held.clear();
        self.last_attempt_at = Some(now);
        for job in &jobs {
            // A still-armed retry of an older window is superseded: this window's digest is
            // newer, and the canon keeps no durable outbox to deliver both from.
            self.retries.remove(&job.sink_id);
            self.attempts.insert(job.sink_id.clone(), 1);
            self.inflight
                .insert(job.sink_id.clone(), job.intent.clone());
        }
        jobs
    }

    /// The retries whose wait has elapsed, to be sent again with the same intent and key.
    ///
    /// A taken retry is in flight again, so its own outcome can be attributed to the same sink
    /// and can arm the next attempt until the cap is reached.
    pub(crate) fn take_due_retries(&mut self, now: Instant) -> Vec<NotificationDeliveryJob> {
        let due: Vec<String> = self
            .retries
            .iter()
            .filter(|(_, retry)| retry.not_before <= now)
            .map(|(sink_id, _)| sink_id.clone())
            .collect();
        let mut jobs = Vec::new();
        for sink_id in due {
            let Some(retry) = self.retries.remove(&sink_id) else {
                continue;
            };
            self.inflight.insert(sink_id.clone(), retry.intent.clone());
            jobs.push(NotificationDeliveryJob {
                sink_id,
                intent: retry.intent,
            });
        }
        jobs
    }

    /// Record one sink's send outcome, and arm a retry when the failure is one the canon allows
    /// to be retried and this window has attempts left.
    ///
    /// A terminal verdict - an invalid credential, a sender the provider refuses, a rejected
    /// request - is never retried here: the product shows it and the user fixes it. The outcome
    /// is recorded either way, so a failed sink can never read as ON.
    pub(crate) fn record_outcome(
        &mut self,
        settings: &NotificationDeliverySettings,
        now: Instant,
        sink_id: &str,
        outcome: &NotificationDeliveryOutcome,
        at_ms: u64,
    ) {
        self.outcomes
            .insert(sink_id.to_owned(), outcome_state(outcome, at_ms));
        let Some(intent) = self.inflight.remove(sink_id) else {
            return;
        };
        let attempts = self.attempts.get(sink_id).copied().unwrap_or(1);
        let Some(delay) = outcome
            .failure()
            .and_then(|failure| bounded_retry_delay(settings, failure))
            .filter(|_| attempts < MAX_NOTIFICATION_DELIVERY_ATTEMPTS)
        else {
            self.attempts.remove(sink_id);
            self.retries.remove(sink_id);
            return;
        };
        self.attempts.insert(sink_id.to_owned(), attempts + 1);
        self.retries.insert(
            sink_id.to_owned(),
            NotificationDeliveryRetry {
                intent,
                not_before: now + delay,
            },
        );
    }

    /// Record the outcome of a test send.
    ///
    /// A test send is deliberately not a delivery: it consumes no window, arms no retry, and
    /// touches neither the held batch nor the dwell, so pressing "send a test" can never
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
    /// sink never inherits an older verdict.
    pub(crate) fn retain_sinks(&mut self, settings: &NotificationDeliverySettings) {
        let known: BTreeSet<&str> = settings.sinks.iter().map(|sink| sink.id.as_str()).collect();
        self.outcomes
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.configured
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.inflight
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.retries
            .retain(|sink_id, _| known.contains(sink_id.as_str()));
        self.attempts
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
                    secret_configured: self.is_configured(&sink.id),
                    secret_retired: self.retired.contains(&sink.id),
                    last_outcome: self.outcomes.get(&sink.id).copied(),
                })
                .collect(),
            presets: preset_states(),
        }
    }

    fn earliest_retry(&self) -> Option<Instant> {
        self.retries.values().map(|retry| retry.not_before).min()
    }

    /// Re-anchor the dwell when the settings change it.
    fn sync_dwell(&mut self, settings: &NotificationDeliverySettings) {
        let dwell = Duration::from_secs(settings.dwell_secs);
        if dwell == self.dwell {
            return;
        }
        self.dwell = dwell;
        self.tracker = DwellTracker::new(dwell);
    }
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
        None => NotificationDeliveryOutcomeState::Delivered { at_ms },
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
/// this device waits at least the channel's own minimum interval, at least the provider's own
/// delay when it gave one, and at least [`NOTIFICATION_DELIVERY_RETRY_FLOOR`], never more than
/// [`MAX_NOTIFICATION_DELIVERY_RETRY`].
fn bounded_retry_delay(
    settings: &NotificationDeliverySettings,
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
            .max(Duration::from_secs(settings.min_interval_secs))
            .max(NOTIFICATION_DELIVERY_RETRY_FLOOR)
            .min(MAX_NOTIFICATION_DELIVERY_RETRY),
    )
}

#[cfg(test)]
#[path = "channel/tests.rs"]
mod tests;
