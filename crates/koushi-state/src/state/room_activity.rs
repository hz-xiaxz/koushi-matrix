//! #1364: per-room conversation activity remembered across sessions.
//!
//! Canon: `docs/architecture/state-machine.md`, "Sidebar Sections And Low
//! Priority".
//!
//! The SDK restores a room's Event Cache window from its own store, but the
//! room-list subscription asks for a single timeline event, so a restart can
//! leave a room whose newest event is a sticker, poll, call, reaction, or state
//! change with nothing to compare. The reducer keeps the newest conversation
//! timestamp it has ever observed per room and merges it back into the
//! installed `RoomSummary`, so Recent-first ordering survives a cold start.
//!
//! The record is a local cache of what this client observed. It never creates
//! activity from membership, state, receipt, typing, presence, redaction, edit,
//! or reaction events, it never outranks a live observation, and it is never
//! sent to a homeserver.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::room::{ConversationActivity, ConversationActivitySource, RoomSummary};

/// One remembered observation for a room.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomActivityRecord {
    pub timestamp_ms: u64,
    pub source: ConversationActivitySource,
}

impl RoomActivityRecord {
    pub fn activity(self) -> ConversationActivity {
        ConversationActivity {
            timestamp_ms: self.timestamp_ms,
            source: self.source,
        }
    }
}

impl From<ConversationActivity> for RoomActivityRecord {
    fn from(activity: ConversationActivity) -> Self {
        Self {
            timestamp_ms: activity.timestamp_ms,
            source: activity.source,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomActivityState {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rooms: BTreeMap<String, RoomActivityRecord>,
}

impl RoomActivityState {
    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    /// Remember one observed activity. Returns `true` only when the stored
    /// record advanced, so a repeated projection writes nothing.
    pub fn observe(&mut self, room_id: &str, activity: ConversationActivity) -> bool {
        match self.rooms.get(room_id) {
            Some(existing) if existing.timestamp_ms >= activity.timestamp_ms => false,
            _ => {
                self.rooms
                    .insert(room_id.to_owned(), RoomActivityRecord::from(activity));
                true
            }
        }
    }

    pub fn activity_for(&self, room_id: &str) -> Option<ConversationActivity> {
        self.rooms
            .get(room_id)
            .copied()
            .map(RoomActivityRecord::activity)
    }

    /// Merge the remembered activity into one room when it is newer than what
    /// the live projection carries. Returns `true` when the room changed.
    pub fn merge_into(&self, room: &mut RoomSummary) -> bool {
        let Some(remembered) = self.activity_for(&room.room_id) else {
            return false;
        };
        match room.conversation_activity {
            Some(live) if live.timestamp_ms >= remembered.timestamp_ms => false,
            _ => {
                room.conversation_activity = Some(remembered);
                true
            }
        }
    }

    /// Merge into every installed room. Returns `true` when any room changed.
    pub fn merge_into_rooms(&self, rooms: &mut [RoomSummary]) -> bool {
        let mut changed = false;
        for room in rooms.iter_mut() {
            changed |= self.merge_into(room);
        }
        changed
    }

    /// Drop records the client should no longer remember. Returns `true` when
    /// anything was removed.
    pub fn retain_rooms(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.rooms.len();
        self.rooms.retain(|room_id, _| keep(room_id));
        self.rooms.len() != before
    }
}
