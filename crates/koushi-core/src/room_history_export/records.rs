//! Per-room files written next to `events.jsonl`: `attachments.json` and
//! `room.json`. The page renderer reads only these and `events.jsonl`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttachmentKind {
    Image,
    Sticker,
    Video,
    Audio,
    File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttachmentStatus {
    Retrieved,
    Failed,
}

/// One attachment of the room, in event order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AttachmentRecord {
    pub(crate) event_id: String,
    pub(crate) kind: AttachmentKind,
    /// The attachment's file name as sent.
    pub(crate) name: String,
    pub(crate) size: Option<u64>,
    pub(crate) mimetype: Option<String>,
    /// `files/<name>` relative to the room folder, when retrieved.
    pub(crate) file: Option<String>,
    /// `thumbs/<name>` relative to the room folder, when generated.
    pub(crate) thumb: Option<String>,
    pub(crate) status: AttachmentStatus,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AttachmentIndex {
    pub(crate) attachments: Vec<AttachmentRecord>,
}

impl AttachmentIndex {
    pub(crate) fn by_event(&self) -> BTreeMap<&str, &AttachmentRecord> {
        self.attachments
            .iter()
            .map(|record| (record.event_id.as_str(), record))
            .collect()
    }
}

/// `room.json`: what the page needs besides the events.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct RoomMeta {
    pub(crate) room_id: String,
    pub(crate) name: String,
    pub(crate) topic: String,
    /// Sender user id → display name at export time.
    pub(crate) senders: BTreeMap<String, String>,
    pub(crate) exported_at_ms: u64,
    /// IANA zone the page's dates and times are shown in.
    pub(crate) time_zone: String,
}

impl RoomMeta {
    pub(crate) fn sender_name<'a>(&'a self, user_id: &'a str) -> &'a str {
        self.senders
            .get(user_id)
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(user_id)
    }
}
