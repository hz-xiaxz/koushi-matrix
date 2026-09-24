//! Fixtures shared by the page renderer tests.

use std::collections::BTreeMap;

use koushi_protocol::HistoryExportLabels;
use serde_json::{Value, json};

use crate::room_history_export::records::RoomMeta;

pub(super) fn labels() -> HistoryExportLabels {
    HistoryExportLabels {
        lang: "en".to_owned(),
        edited: "(edited)".to_owned(),
        in_reply_to: "In reply to {name}".to_owned(),
        reply_unavailable: "In reply to a message outside this export".to_owned(),
        thread_reply: "Thread reply".to_owned(),
        thread_root_link: "Thread start".to_owned(),
        redacted: "Message deleted".to_owned(),
        undecryptable: "Unable to decrypt message".to_owned(),
        not_retrieved: "not retrieved".to_owned(),
        reactions: "Reactions".to_owned(),
        times_in_zone: "Times are shown in {timeZone}.".to_owned(),
        exported_at: "Exported {date}".to_owned(),
        range_all: "All available history".to_owned(),
        range_period: "{start} to {end}".to_owned(),
        rooms_heading: "Rooms".to_owned(),
        status_completed: "Completed".to_owned(),
        status_skipped: "Skipped (not joined)".to_owned(),
        status_failed: "Failed".to_owned(),
        status_pending: "Not exported".to_owned(),
        events_count: "{count} events".to_owned(),
        attachments_count: "{count} attachments".to_owned(),
        failed_attachments_count: "{count} not retrieved".to_owned(),
        state_joined: "{name} joined".to_owned(),
        state_left: "{name} left".to_owned(),
        state_invited: "{name} invited {target}".to_owned(),
        state_removed: "{name} removed {target}".to_owned(),
        state_banned: "{name} banned {target}".to_owned(),
        state_renamed: "{name} changed the room name to {value}".to_owned(),
        state_topic: "{name} changed the topic to {value}".to_owned(),
        state_avatar: "{name} changed the room avatar".to_owned(),
        state_other: "{name} changed {type}".to_owned(),
    }
}

pub(super) fn meta() -> RoomMeta {
    RoomMeta {
        room_id: "!room:example.org".to_owned(),
        name: "Lab <Room>".to_owned(),
        topic: "Weekly & more".to_owned(),
        senders: BTreeMap::from([
            ("@alice:example.org".to_owned(), "Alice".to_owned()),
            ("@bob:example.org".to_owned(), "Bob".to_owned()),
        ]),
        exported_at_ms: 1_758_758_400_000,
        time_zone: "Asia/Tokyo".to_owned(),
    }
}

/// 2025-09-25T00:00:00Z plus `minutes`.
pub(super) fn ts(minutes: u64) -> u64 {
    1_758_758_400_000 + minutes * 60_000
}

pub(super) fn message(id: &str, sender: &str, minutes: u64, content: Value) -> Value {
    json!({
        "type": "m.room.message",
        "event_id": id,
        "sender": sender,
        "origin_server_ts": ts(minutes),
        "room_id": "!room:example.org",
        "content": content,
    })
}

pub(super) fn text(id: &str, sender: &str, minutes: u64, body: &str) -> Value {
    message(
        id,
        sender,
        minutes,
        json!({ "msgtype": "m.text", "body": body }),
    )
}

pub(super) fn html(id: &str, minutes: u64, body: &str, formatted: &str) -> Value {
    message(
        id,
        "@alice:example.org",
        minutes,
        json!({
            "msgtype": "m.text",
            "body": body,
            "format": "org.matrix.custom.html",
            "formatted_body": formatted,
        }),
    )
}

pub(super) fn jsonl(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        bytes.extend(serde_json::to_vec(event).unwrap());
        bytes.push(b'\n');
    }
    bytes
}
