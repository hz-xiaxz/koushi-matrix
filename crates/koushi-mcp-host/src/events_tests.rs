//! Sampled room-change feed and its cursor contract (#1551).

use super::*;
use koushi_mcp::wire::{ErrorCode, WireError};
use koushi_state::RoomSummary;

const A: &str = "account:@member1:example.invalid";
const B: &str = "account:@member2:example.invalid";

fn room_with_unread(room_id: &str, event_id: &str, unread: u64) -> RoomSummary {
    let mut room: RoomSummary = serde_json::from_value(json!({
        "room_id": room_id,
        "display_name": room_id,
        "display_label": room_id,
        "is_dm": false,
        "unread_count": unread,
        "notification_count": 0,
        "highlight_count": 0,
        "parent_space_ids": [],
        "joined_members": 0
    }))
    .expect("room");
    room.latest_event = Some(
        serde_json::from_value(json!({
            "event_id": event_id,
            "sender_id": "@member3:example.invalid",
            "timestamp_ms": 1
        }))
        .expect("latest"),
    );
    room
}

fn room(room_id: &str, event_id: &str) -> RoomSummary {
    room_with_unread(room_id, event_id, 0)
}

/// Appends exactly `count` records for `account`, in a room this log has not
/// seen yet, so the seeding observation emits nothing.
fn append(log: &EventLog, account: &str, count: usize) {
    let room_id = format!("!r{}:example.invalid", log.head());
    log.observe(account, &[room(&room_id, "$seed")]);
    for index in 0..count {
        log.observe(account, &[room(&room_id, &format!("$e{index}"))]);
    }
}

async fn poll(log: &EventLog, account: &str, cursor: Option<&str>) -> Result<Value, WireError> {
    wait_for_events(log, account, cursor, 0).await
}

fn assert_resync(result: Result<Value, WireError>, reason: &str) {
    let error = result.expect_err("an unusable cursor must not look like a quiet poll");
    assert_eq!(error.code, ErrorCode::ResyncRequired);
    assert_eq!(error.reason.as_deref(), Some(reason));
    assert!(
        error.message.starts_with(reason),
        "{reason}: {}",
        error.message
    );
    assert!(
        error.message.contains("without a cursor"),
        "{}",
        error.message
    );
}

#[test]
fn a_first_observation_is_not_news() {
    let log = EventLog::new();
    log.observe(A, &[room("!r:example.invalid", "$e1")]);
    assert_eq!(log.head(), 0);
}

#[test]
fn a_changed_latest_event_appends_one_record() {
    let log = EventLog::new();
    log.observe(A, &[room("!r:example.invalid", "$e1")]);
    log.observe(A, &[room("!r:example.invalid", "$e2")]);
    assert_eq!(log.head(), 1);
    let records = log.after(A, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].event_id.as_deref(), Some("$e2"));
    assert!(log.after(B, 0).is_empty());
}

#[test]
fn a_burst_between_samples_coalesces_into_one_record() {
    let log = EventLog::new();
    log.observe(A, &[room("!r:example.invalid", "$e1")]);
    // $e2 and $e3 were posted between samples; only $e3 is seen.
    log.observe(A, &[room("!r:example.invalid", "$e3")]);
    log.observe(A, &[room("!r:example.invalid", "$e3")]);
    let records = log.after(A, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].event_id.as_deref(), Some("$e3"));
}

#[tokio::test]
async fn reading_a_room_does_not_erase_its_record() {
    let log = EventLog::new();
    log.observe(A, &[room("!r:example.invalid", "$e1")]);
    log.observe(A, &[room_with_unread("!r:example.invalid", "$e2", 1)]);
    // The user reads the room: the unread count drops, the event is the same.
    log.observe(A, &[room_with_unread("!r:example.invalid", "$e2", 0)]);
    let start = log.cursor_for(A, 0);
    let result = poll(&log, A, Some(&start)).await.expect("poll");
    assert_eq!(result["events"].as_array().map(Vec::len), Some(1));
    assert_eq!(result["events"][0]["unread_count"], 1);
}

#[tokio::test]
async fn an_omitted_cursor_starts_at_the_head_and_a_quiet_poll_is_resumable() {
    let log = EventLog::new();
    append(&log, A, 2);
    let first = poll(&log, A, None).await.expect("poll");
    assert_eq!(first["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(first["coverage"], "sampled_room_changes");
    let cursor = first["cursor"].as_str().expect("cursor").to_owned();
    let again = poll(&log, A, Some(&cursor)).await.expect("same cursor");
    assert_eq!(again["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(again["cursor"], cursor.as_str());

    append(&log, A, 1);
    let next = poll(&log, A, Some(&cursor)).await.expect("next");
    assert_eq!(next["events"].as_array().map(Vec::len), Some(1));
    assert_ne!(next["cursor"], cursor.as_str());
}

#[tokio::test]
async fn a_cursor_from_a_previous_run_requires_a_resync() {
    // The issue's repro: seq 131 retained from one run, four records in a
    // fresh log, polled with timeout 0.
    let previous = EventLog::with_generation("0123456789abcdef");
    let old_cursor = previous.cursor_for(A, 131);
    let log = EventLog::with_generation("fedcba9876543210");
    append(&log, A, 4);
    assert_resync(poll(&log, A, Some(&old_cursor)).await, "previous_run");

    // An old sequence below the new head is still refused.
    let low_cursor = previous.cursor_for(A, 2);
    assert_resync(poll(&log, A, Some(&low_cursor)).await, "previous_run");
}

#[tokio::test]
async fn the_tool_result_types_a_resync_apart_from_a_quiet_poll() {
    let previous = EventLog::with_generation("0123456789abcdef");
    let log = EventLog::with_generation("fedcba9876543210");
    append(&log, A, 4);

    let failed =
        crate::handlers::tool_result(poll(&log, A, Some(&previous.cursor_for(A, 131))).await);
    assert_eq!(failed["isError"], true);
    assert_eq!(
        failed["structuredContent"]["error"]["code"],
        "resync_required"
    );
    assert_eq!(
        failed["structuredContent"]["error"]["reason"],
        "previous_run"
    );
    assert!(
        failed["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("resync_required: previous_run"))
    );

    let quiet = crate::handlers::tool_result(poll(&log, A, None).await);
    assert_eq!(quiet["isError"], false);
    assert_eq!(quiet["structuredContent"]["events"], json!([]));
    assert_eq!(
        quiet["structuredContent"]["coverage"],
        "sampled_room_changes"
    );
}

#[tokio::test]
async fn malformed_and_legacy_cursors_require_a_resync() {
    let log = EventLog::new();
    append(&log, A, 4);
    for cursor in ["seq:131", "131", "garbage", "", "ev1.x.y.z"] {
        assert_resync(poll(&log, A, Some(cursor)).await, "malformed_cursor");
    }
}

#[tokio::test]
async fn another_accounts_cursor_requires_a_resync() {
    let log = EventLog::new();
    append(&log, A, 1);
    append(&log, B, 1);
    let cursor = log.cursor_for(B, 0);
    assert_resync(poll(&log, A, Some(&cursor)).await, "wrong_account");
}

#[tokio::test]
async fn a_future_cursor_requires_a_resync() {
    let log = EventLog::new();
    append(&log, A, 2);
    let cursor = log.cursor_for(A, log.head() + 1);
    assert_resync(poll(&log, A, Some(&cursor)).await, "future_cursor");
}

#[tokio::test]
async fn a_cursor_older_than_the_retained_window_requires_a_resync() {
    let log = EventLog::new();
    append(&log, A, MAX_RECORDS + 3);
    // Seqs 1..=3 were evicted. A cursor at 2 missed seq 3.
    let expired = log.cursor_for(A, 2);
    assert_resync(poll(&log, A, Some(&expired)).await, "expired_cursor");
    // A cursor at 3 saw everything that was evicted: the rest is complete.
    let boundary = log.cursor_for(A, 3);
    let result = poll(&log, A, Some(&boundary)).await.expect("boundary");
    assert_eq!(result["events"].as_array().map(Vec::len), Some(MAX_RECORDS));
}

#[tokio::test]
async fn another_accounts_eviction_does_not_expire_a_cursor() {
    let log = EventLog::new();
    append(&log, A, 1);
    let cursor = log.cursor_for(A, log.head());
    append(&log, B, MAX_RECORDS + 1);
    append(&log, A, 1);
    let result = poll(&log, A, Some(&cursor)).await.expect("still valid");
    assert_eq!(result["events"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn waiting_returns_immediately_when_records_exist() {
    let log = EventLog::new();
    log.observe(A, &[room("!r:example.invalid", "$e1")]);
    log.observe(A, &[room("!r:example.invalid", "$e2")]);
    let start = log.cursor_for(A, 0);
    let result = wait_for_events(&log, A, Some(&start), 5)
        .await
        .expect("wait");
    assert_eq!(result["events"].as_array().map(Vec::len), Some(1));
    assert_eq!(result["cursor"], log.cursor_for(A, 1).as_str());
    assert_eq!(result["events"][0]["cursor"], log.cursor_for(A, 1).as_str());
}
