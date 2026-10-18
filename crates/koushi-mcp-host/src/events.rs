//! Change feed for `wait_for_events`.
//!
//! Koushi cannot be woken by an external client, so the feed is produced by a
//! poller inside the app: it snapshots each account's room summaries, notices a
//! changed latest event or a rising unread count, and appends a compact record.
//! Records live in memory for this app run only; a cursor from a previous run is
//! rejected and the client restarts from the newest position.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::BridgeAccounts;

/// How many records to retain before dropping the oldest.
const MAX_RECORDS: usize = 500;
/// Poll cadence. Fast enough to feel live, slow enough to stay invisible.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

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

impl EventRecord {
    fn to_json(&self) -> Value {
        json!({
            "cursor": cursor_of(self.seq),
            "account_id": self.account_id,
            "room_id": self.room_id,
            "room_name": self.room_name,
            "event_id": self.event_id,
            "sender_id": self.sender_id,
            "sender_label": self.sender_label,
            "unread_count": self.unread_count,
            "highlight_count": self.highlight_count,
            "recency_stamp": self.recency_stamp,
        })
    }
}

fn cursor_of(seq: u64) -> String {
    format!("seq:{seq}")
}

pub(crate) struct EventLog {
    records: Mutex<Vec<EventRecord>>,
    next_seq: Mutex<u64>,
    /// Latest `latest_event.event_id` seen per `(account, room)`.
    seen: Mutex<HashMap<(String, String), Option<String>>>,
    notify: Notify,
}

impl EventLog {
    pub(crate) fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            next_seq: Mutex::new(1),
            seen: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    fn head(&self) -> u64 {
        self.records
            .lock()
            .map(|records| records.last().map(|record| record.seq).unwrap_or(0))
            .unwrap_or(0)
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
            let record = EventRecord {
                seq: self.take_seq(),
                account_id: account_id.to_owned(),
                room_id: room.room_id.clone(),
                room_name: room.display_label.clone(),
                event_id,
                sender_id: latest.and_then(|latest| latest.sender_id.clone()),
                sender_label: latest.and_then(|latest| latest.sender_label.clone()),
                unread_count: room.unread_count,
                highlight_count: room.highlight_count,
                recency_stamp: room.recency_stamp,
            };
            if let Ok(mut records) = self.records.lock() {
                records.push(record);
                if records.len() > MAX_RECORDS {
                    let excess = records.len() - MAX_RECORDS;
                    records.drain(..excess);
                }
                appended = true;
            }
        }
        drop(seen);
        if appended {
            self.notify.notify_waiters();
        }
    }

    fn take_seq(&self) -> u64 {
        let mut next = match self.next_seq.lock() {
            Ok(next) => next,
            Err(poisoned) => poisoned.into_inner(),
        };
        let seq = *next;
        *next += 1;
        seq
    }

    fn after(&self, account_id: &str, seq: u64) -> Vec<EventRecord> {
        self.records
            .lock()
            .map(|records| {
                records
                    .iter()
                    .filter(|record| record.seq > seq && record.account_id == account_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Parses `seq:<n>` (or a bare integer) into a cursor.
pub(crate) fn parse_cursor(cursor: &str) -> Option<u64> {
    cursor
        .strip_prefix("seq:")
        .unwrap_or(cursor)
        .parse::<u64>()
        .ok()
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
                        log.observe(&account.id, &account.connection.snapshot().rooms);
                    }
                }
            }
        }
    });
}

/// Handles `wait_for_events`: returns records newer than the cursor, waiting up
/// to `timeout_s` for one to appear.
pub(crate) async fn wait_for_events(
    log: &EventLog,
    account_id: &str,
    cursor: Option<&str>,
    timeout_s: u64,
) -> Result<Value, koushi_mcp::wire::WireError> {
    let start = cursor.and_then(parse_cursor).unwrap_or_else(|| log.head());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_s.min(50));
    loop {
        // Register interest before reading, so a record appended between the
        // read and the wait cannot be missed.
        let notified = log.notify.notified();
        let records = log.after(account_id, start);
        if !records.is_empty() {
            let next = records.last().map(|record| record.seq).unwrap_or(start);
            return Ok(json!({
                "events": records.iter().map(EventRecord::to_json).collect::<Vec<_>>(),
                "cursor": cursor_of(next),
            }));
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(json!({ "events": [], "cursor": cursor_of(start) }));
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Ok(json!({ "events": [], "cursor": cursor_of(start) }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koushi_state::RoomSummary;

    fn room(room_id: &str, event_id: &str) -> RoomSummary {
        let mut room: RoomSummary = serde_json::from_value(json!({
            "room_id": room_id,
            "display_name": room_id,
            "display_label": room_id,
            "is_dm": false,
            "unread_count": 0,
            "notification_count": 0,
            "highlight_count": 0,
            "parent_space_ids": [],
            "joined_members": 0
        }))
        .expect("room");
        room.latest_event = Some(
            serde_json::from_value(json!({
                "event_id": event_id,
                "sender_id": "@bob:example.org",
                "timestamp_ms": 1
            }))
            .expect("latest"),
        );
        room
    }

    #[test]
    fn a_first_observation_is_not_news() {
        let log = EventLog::new();
        log.observe("account:a", &[room("!r:example.org", "$e1")]);
        assert_eq!(log.head(), 0);
    }

    #[test]
    fn a_changed_latest_event_appends_one_record() {
        let log = EventLog::new();
        log.observe("account:a", &[room("!r:example.org", "$e1")]);
        log.observe("account:a", &[room("!r:example.org", "$e2")]);
        assert_eq!(log.head(), 1);
        let records = log.after("account:a", 0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event_id.as_deref(), Some("$e2"));
        assert_eq!(records[0].sender_id.as_deref(), Some("@bob:example.org"));
        assert!(log.after("account:b", 0).is_empty());
    }

    #[test]
    fn a_same_event_does_not_duplicate() {
        let log = EventLog::new();
        log.observe("account:a", &[room("!r:example.org", "$e1")]);
        log.observe("account:a", &[room("!r:example.org", "$e2")]);
        log.observe("account:a", &[room("!r:example.org", "$e2")]);
        assert_eq!(log.head(), 1);
    }

    #[test]
    fn cursors_round_trip() {
        assert_eq!(parse_cursor("seq:42"), Some(42));
        assert_eq!(parse_cursor("7"), Some(7));
        assert_eq!(parse_cursor("garbage"), None);
    }

    #[tokio::test]
    async fn waiting_returns_immediately_when_records_exist() {
        let log = EventLog::new();
        log.observe("account:a", &[room("!r:example.org", "$e1")]);
        log.observe("account:a", &[room("!r:example.org", "$e2")]);
        let result = wait_for_events(&log, "account:a", Some("seq:0"), 5)
            .await
            .expect("wait");
        assert_eq!(result["events"].as_array().map(Vec::len), Some(1));
        assert_eq!(result["cursor"], "seq:1");
    }

    #[tokio::test]
    async fn waiting_times_out_with_no_records() {
        let log = EventLog::new();
        let result = wait_for_events(&log, "account:a", None, 0)
            .await
            .expect("wait");
        assert_eq!(result["events"].as_array().map(Vec::len), Some(0));
        assert_eq!(result["cursor"], "seq:0");
    }
}
