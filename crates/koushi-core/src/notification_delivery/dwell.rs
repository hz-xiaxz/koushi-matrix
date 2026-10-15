//! The dwell timer that decides when a qualifying room becomes deliverable.
//!
//! The dwell exists so that a conversation the user is already reading is never
//! mailed: a room becomes deliverable only when its qualifying attention state
//! persists for the configured duration
//! (`docs/architecture/state-machine.md`, "Local Notification Delivery").
//!
//! The state machine is pure and owns no timer of its own. The caller passes the
//! current [`Instant`] and the whole current qualifying set, and asks again
//! later; this module never sleeps, never reads a clock, and never decides when
//! to be called. Room identity is the Matrix room id, which stays in this module
//! and is never rendered by [`super::digest`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use koushi_state::RoomAttentionKind;

use crate::executor::Instant;

/// One room the attention projection currently considers deliverable, as the
/// delivery channel sees it.
///
/// The caller builds this from the same per-room attention projection that drives
/// the badge and the unread marks, so a digest built from these rooms can never
/// describe a state the badge does not show. Only [`Self::display_label`] is
/// rendered by [`super::digest`]: the room id is the dwell identity and the
/// ordering tiebreak, and it never reaches a message.
///
/// [`Self::plaintext_prefix`] is message text the caller already extracted and
/// bounded. This module stores it and hands it back; it never reads it, and only
/// [`super::digest`] renders it, and only for a sink whose stored consent covers
/// it.
#[derive(Clone, Eq, PartialEq)]
pub struct QualifyingRoom {
    /// The room's Matrix id. It is the dwell identity and the final ordering
    /// tiebreak, and it is never rendered into a message.
    pub room_id: String,
    /// The safe display label the attention projection produced for the room,
    /// which is the same label the badge and the room list show. An empty label
    /// falls back to `Room` when a digest names the room.
    pub display_label: String,
    /// Which qualifying attention state the room is in. It decides the ranking
    /// of the room inside a digest and the wording of its line.
    pub kind: RoomAttentionKind,
    /// The room's unread message count, from the same projection that feeds the
    /// badge. Thread replies are already counted here by
    /// `room_activity_unread_count`.
    pub unread: u32,
    /// The room's mention count, which includes a keyword hit: keywords are
    /// `kind: content` push rules the SDK evaluates into the highlight counter.
    pub mentions: u32,
    /// The already-extracted, caller-bounded plaintext preview of the triggering
    /// message, if the caller could produce one. It is rendered only under a
    /// matching prefix consent, and only up to the configured grapheme cap.
    pub plaintext_prefix: Option<String>,
}

/// `Debug` redacts the room id, the display label, and the prefix text: a room
/// id is a Matrix identifier and the other two may carry room and message
/// content, so a struct dump must not become a log line.
impl fmt::Debug for QualifyingRoom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QualifyingRoom")
            .field("room_id", &"RoomId(..)")
            .field("display_label", &"RoomName(..)")
            .field("kind", &self.kind)
            .field("unread", &self.unread)
            .field("mentions", &self.mentions)
            .field(
                "plaintext_prefix",
                &self
                    .plaintext_prefix
                    .as_ref()
                    .map(|_| "PlaintextPrefix(..)"),
            )
            .finish()
    }
}

/// Holds each qualifying room for the configured dwell and releases it once.
///
/// The tracker owns no clock, no task, and no persistence: the caller passes the
/// current [`Instant`] and the whole qualifying set, and reads the rooms that
/// came due. The timer is anchored on the observation that first reports a room
/// as qualifying, so a room that keeps receiving messages keeps its original
/// anchor and still comes due. A room that leaves the qualifying set - read,
/// muted, focused, or otherwise no longer in the projection - is cancelled and
/// arms again from scratch when it qualifies again. A room that comes due is
/// released once and is not reported again until it leaves the qualifying set and
/// re-enters it, so one unread state is never delivered twice.
pub struct DwellTracker {
    dwell: Duration,
    /// Armed timers, keyed by room id. Iteration order is the room id order, so
    /// the due list is independent of the order of the qualifying set.
    pending: BTreeMap<String, Instant>,
    /// Rooms already released during their current qualifying episode. They stay
    /// here while they keep qualifying, so a released room cannot be delivered
    /// twice from one unread state, and they are dropped when the room leaves the
    /// qualifying set.
    delivered: BTreeSet<String>,
}

impl DwellTracker {
    /// Create a tracker whose rooms come due `dwell` after they first qualify.
    ///
    /// A zero dwell makes every room due on the first observation that reports
    /// it, which is what the headless lane configures. The duration is used as
    /// given: the caller bounds it through the settings canonicalization, and
    /// this type clamps nothing.
    pub fn new(dwell: Duration) -> Self {
        Self {
            dwell,
            pending: BTreeMap::new(),
            delivered: BTreeSet::new(),
        }
    }

    /// Observe the whole current qualifying set at `now` and return the rooms
    /// whose dwell has elapsed.
    ///
    /// `qualifying` is the complete set the attention projection reports as
    /// deliverable right now, in any order. A room absent from it is treated as
    /// no longer qualifying: its timer is cancelled and any release it already
    /// had is forgotten, so the next time it qualifies it arms from scratch. A
    /// room id repeated inside one observation counts once, and the first entry
    /// wins.
    ///
    /// The returned rooms are the current observation of each due room, not the
    /// observation that armed it, so the counts are the newest the caller has.
    /// They are in ascending room id order, whatever the order of `qualifying`
    /// was. Each returned room is released exactly once: it is not returned again
    /// while it keeps qualifying.
    pub fn observe(&mut self, now: Instant, qualifying: &[QualifyingRoom]) -> Vec<QualifyingRoom> {
        let mut observed: BTreeMap<&str, &QualifyingRoom> = BTreeMap::new();
        for room in qualifying {
            observed.entry(room.room_id.as_str()).or_insert(room);
        }

        self.pending
            .retain(|room_id, _| observed.contains_key(room_id.as_str()));
        self.delivered
            .retain(|room_id| observed.contains_key(room_id.as_str()));

        let mut due = Vec::new();
        for (room_id, room) in observed {
            if self.delivered.contains(room_id) {
                continue;
            }
            let anchor = match self.pending.get(room_id).copied() {
                Some(anchor) => anchor,
                None => {
                    self.pending.insert(room_id.to_owned(), now);
                    now
                }
            };
            // A caller that passes a stale `now` produces no elapsed duration and
            // therefore no delivery; the timer keeps its anchor and stays armed.
            if now
                .checked_duration_since(anchor)
                .is_some_and(|elapsed| elapsed >= self.dwell)
            {
                self.pending.remove(room_id);
                self.delivered.insert(room_id.to_owned());
                due.push(room.clone());
            }
        }

        due
    }

    /// Whether the room currently has an armed timer that has not come due.
    ///
    /// A room is pending from the observation that first reports it as qualifying
    /// until it comes due, leaves the qualifying set, or is discarded. A room that
    /// was already released for its current qualifying episode is not pending: its
    /// timer was consumed, and it arms again only after it leaves and re-enters the
    /// qualifying set.
    pub fn is_pending(&self, room_id: &str) -> bool {
        self.pending.contains_key(room_id)
    }

    /// The earliest instant at which a currently armed room comes due, or `None` when
    /// no room is armed.
    ///
    /// This is what the caller arms its one timer on: it reports the earliest anchor
    /// plus the dwell rather than deciding anything, so a caller that wakes late
    /// delivers the same rooms a punctual one would, and a caller that wakes early
    /// delivers none.
    pub fn next_due_at(&self) -> Option<Instant> {
        self.pending
            .values()
            .filter_map(|anchor| anchor.checked_add(self.dwell))
            .min()
    }
}

/// `Debug` reports the dwell and the number of armed and released rooms. Room ids
/// are Matrix identifiers, so a tracker dump names none of them.
impl fmt::Debug for DwellTracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DwellTracker")
            .field("dwell", &self.dwell)
            .field("pending_count", &self.pending.len())
            .field("delivered_count", &self.delivered.len())
            .finish()
    }
}
