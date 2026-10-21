//! Sampled room-change feed for `wait_for_events` (#1551).
//!
//! Koushi cannot be woken by an external client, so the feed is produced by a
//! poller inside the app: about every two seconds it samples each account's
//! room summaries and appends one record for every room whose latest event
//! changed since the previous sample. It is not a message log: several posts
//! between samples coalesce into one record, the first sample of a room only
//! seeds a baseline, invites are not sampled, and reading a room does not
//! remove its record.
//!
//! Records live in memory for this bridge run only, at most [`MAX_RECORDS`]
//! across all accounts. Cursors are opaque and bound to the log's random
//! generation and to the account. A cursor that is malformed, from a previous
//! run, for another account, ahead of the log, or older than the retained
//! records is rejected with `resync_required` instead of silently restarting at
//! the head, so an empty result always means "no change was sampled".

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use koushi_mcp::wire::{ErrorCode, WireError};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::BridgeAccounts;

/// How many records to retain, across all accounts, before dropping the oldest.
pub(crate) const MAX_RECORDS: usize = 500;
/// Poll cadence. Fast enough to feel live, slow enough to stay invisible.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// The `coverage` value of every result: sampled room changes, not a log.
pub(crate) const COVERAGE: &str = "sampled_room_changes";
const CURSOR_VERSION: &str = "ev1";

#[derive(Clone, Debug)]
pub(crate) struct EventRecord {
    pub(crate) seq: u64,
    pub(crate) account_id: String,
    pub(crate) room_id: String,
    pub(crate) room_name: String,
    pub(crate) event_id: Option<String>,
    pub(crate) sender_id: Option<String>,
    pub(crate) sender_label: Option<String>,
    pub(crate) unread_count: u64,
    pub(crate) highlight_count: u64,
    pub(crate) recency_stamp: Option<u64>,
}

/// Why a cursor cannot continue the feed. The token leads the error message
/// so a client can branch on it without parsing prose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorRejection {
    Malformed,
    PreviousRun,
    WrongAccount,
    Future,
    Expired,
}

impl CursorRejection {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Malformed => "malformed_cursor",
            Self::PreviousRun => "previous_run",
            Self::WrongAccount => "wrong_account",
            Self::Future => "future_cursor",
            Self::Expired => "expired_cursor",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::Malformed => "the cursor was not issued by wait_for_events",
            Self::PreviousRun => {
                "the cursor is from an earlier Koushi run or bridge start; records do not survive a restart"
            }
            Self::WrongAccount => "the cursor was issued for another account",
            Self::Future => "the cursor is ahead of the newest record",
            Self::Expired => "records after the cursor were dropped from the 500-record window",
        }
    }

    fn into_error(self) -> WireError {
        WireError::with_reason(
            ErrorCode::ResyncRequired,
            self.reason(),
            format!(
                "{}: {}. Changes may have been missed. Call wait_for_events without a cursor to start at the newest position, and use read_room_messages to catch up on the rooms you need, de-duplicating by room and event id.",
                self.reason(),
                self.explanation()
            ),
        )
    }
}

struct LogState {
    records: Vec<EventRecord>,
    next_seq: u64,
    /// Highest evicted seq per account. A cursor below it missed a record.
    evicted_through: HashMap<String, u64>,
}

pub(crate) struct EventLog {
    generation: String,
    state: Mutex<LogState>,
    /// Latest `latest_event.event_id` seen per `(account, room)`.
    seen: Mutex<HashMap<(String, String), Option<String>>>,
    notify: Notify,
}

impl EventLog {
    /// A log with a fresh random generation: every cursor of an earlier log
    /// (earlier app run or bridge start) is refused.
    pub(crate) fn new() -> Self {
        Self::with_generation(&new_generation())
    }

    pub(crate) fn with_generation(generation: &str) -> Self {
        Self {
            generation: generation.to_owned(),
            state: Mutex::new(LogState {
                records: Vec::new(),
                next_seq: 1,
                evicted_through: HashMap::new(),
            }),
            seen: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LogState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The newest sequence number issued, whether or not still retained.
    pub(crate) fn head(&self) -> u64 {
        self.lock().next_seq - 1
    }

    /// The opaque cursor for `seq`, bound to this log and `account_id`.
    pub(crate) fn cursor_for(&self, account_id: &str, seq: u64) -> String {
        format!(
            "{CURSOR_VERSION}.{}.{}.{seq}",
            self.generation,
            self.account_tag(account_id)
        )
    }

    fn account_tag(&self, account_id: &str) -> String {
        // Binds a cursor to its account. Not a secret: possessing a cursor
        // never authorizes anything, the grant check does.
        let mut hasher = DefaultHasher::new();
        self.generation.hash(&mut hasher);
        account_id.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// The seq a cursor continues from, or why it cannot.
    pub(crate) fn resolve_cursor(
        &self,
        account_id: &str,
        cursor: &str,
    ) -> Result<u64, CursorRejection> {
        let mut parts = cursor.split('.');
        let (Some(version), Some(generation), Some(tag), Some(seq), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(CursorRejection::Malformed);
        };
        let seq = seq.parse::<u64>().map_err(|_| CursorRejection::Malformed)?;
        if version != CURSOR_VERSION
            || generation.len() != 16
            || tag.len() != 16
            || !generation.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CursorRejection::Malformed);
        }
        if generation != self.generation {
            return Err(CursorRejection::PreviousRun);
        }
        if tag != self.account_tag(account_id) {
            return Err(CursorRejection::WrongAccount);
        }
        if seq > self.head() {
            return Err(CursorRejection::Future);
        }
        if self.expired(account_id, seq) {
            return Err(CursorRejection::Expired);
        }
        Ok(seq)
    }

    /// Whether a record of `account_id` newer than `seq` was evicted.
    fn expired(&self, account_id: &str, seq: u64) -> bool {
        self.lock()
            .evicted_through
            .get(account_id)
            .is_some_and(|evicted| seq < *evicted)
    }

    fn record_json(&self, record: &EventRecord) -> Value {
        json!({
            "cursor": self.cursor_for(&record.account_id, record.seq),
            "account_id": record.account_id,
            "room_id": record.room_id,
            "room_name": record.room_name,
            "event_id": record.event_id,
            "sender_id": record.sender_id,
            "sender_label": record.sender_label,
            "unread_count": record.unread_count,
            "highlight_count": record.highlight_count,
            "recency_stamp": record.recency_stamp,
        })
    }

    /// Sample only joined room summaries. Invites are a separate workflow.
    pub(crate) fn observe_snapshot(&self, account_id: &str, snapshot: &koushi_state::AppState) {
        self.observe(account_id, &snapshot.rooms);
    }

    /// Records room activity that changed since the previous observation.
    pub(crate) fn observe(&self, account_id: &str, rooms: &[koushi_state::RoomSummary]) {
        let mut seen = match self.seen.lock() {
            Ok(seen) => seen,
            Err(_) => return,
        };
        let mut appended = false;
        for room in rooms {
            let event_id = room
                .latest_event
                .as_ref()
                .map(|latest| latest.event_id.clone());
            let key = (account_id.to_owned(), room.room_id.clone());
            // First observation seeds the baseline without emitting: an app
            // start is not news.
            let changed = seen.get(&key).is_some_and(|previous| previous != &event_id);
            let is_new_room = !seen.contains_key(&key);
            seen.insert(key, event_id.clone());
            if is_new_room || !changed {
                continue;
            }
            let latest = room.latest_event.as_ref();
            let mut state = self.lock();
            let seq = state.next_seq;
            state.next_seq += 1;
            state.records.push(EventRecord {
                seq,
                account_id: account_id.to_owned(),
                room_id: room.room_id.clone(),
                room_name: room.display_label.clone(),
                event_id,
                sender_id: latest.and_then(|latest| latest.sender_id.clone()),
                sender_label: latest.and_then(|latest| latest.sender_label.clone()),
                unread_count: room.unread_count,
                highlight_count: room.highlight_count,
                recency_stamp: room.recency_stamp,
            });
            if state.records.len() > MAX_RECORDS {
                let excess = state.records.len() - MAX_RECORDS;
                let evicted: Vec<EventRecord> = state.records.drain(..excess).collect();
                for record in evicted {
                    let through = state.evicted_through.entry(record.account_id).or_insert(0);
                    *through = (*through).max(record.seq);
                }
            }
            appended = true;
        }
        drop(seen);
        if appended {
            self.notify.notify_waiters();
        }
    }

    pub(crate) fn after(&self, account_id: &str, seq: u64) -> Vec<EventRecord> {
        self.lock()
            .records
            .iter()
            .filter(|record| record.seq > seq && record.account_id == account_id)
            .cloned()
            .collect()
    }
}

/// 16 hex digits from the OS random source; the clock and process id only if
/// that source fails, which still differs between runs.
fn new_generation() -> String {
    let mut bytes = [0u8; 8];
    if crate::endpoint::fill_random(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0);
        bytes = (nanos ^ u64::from(std::process::id()).rotate_left(32)).to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Polls every account's room summaries and feeds the log.
pub(crate) fn spawn_poller(
    accounts: std::sync::Arc<dyn BridgeAccounts>,
    log: std::sync::Arc<EventLog>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return;
                    }
                }
                _ = ticker.tick() => {
                    for account in accounts.accounts() {
                        log.observe_snapshot(&account.id, &account.connection.snapshot());
                    }
                }
            }
        }
    });
}

/// Handles `wait_for_events`: returns records newer than the cursor, waiting up
/// to `timeout_s` for one to appear. An unusable cursor fails at once with
/// `resync_required`; it never restarts silently at the head.
pub(crate) async fn wait_for_events(
    log: &EventLog,
    account_id: &str,
    cursor: Option<&str>,
    timeout_s: u64,
) -> Result<Value, WireError> {
    let start = match cursor {
        Some(cursor) => log
            .resolve_cursor(account_id, cursor)
            .map_err(CursorRejection::into_error)?,
        None => log.head(),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_s.min(50));
    let empty = || {
        json!({
            "events": [],
            "cursor": log.cursor_for(account_id, start),
            "coverage": COVERAGE,
        })
    };
    loop {
        // Register interest before reading, so a record appended between the
        // read and the wait cannot be missed.
        let notified = log.notify.notified();
        // A wait can outlive the window: re-check that nothing after the
        // start was evicted meanwhile.
        if log.expired(account_id, start) {
            return Err(CursorRejection::Expired.into_error());
        }
        let records = log.after(account_id, start);
        if let Some(last) = records.last() {
            return Ok(json!({
                "events": records.iter().map(|record| log.record_json(record)).collect::<Vec<_>>(),
                "cursor": log.cursor_for(account_id, last.seq),
                "coverage": COVERAGE,
            }));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(empty());
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Ok(empty());
        }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
