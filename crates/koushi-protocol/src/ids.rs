//! Transport-neutral identity DTOs.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RuntimeConnectionId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RequestId {
    pub connection_id: RuntimeConnectionId,
    pub sequence: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AccountKey(pub String);

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct TimelineKey {
    pub account_key: AccountKey,
    pub kind: TimelineKind,
}

impl TimelineKey {
    pub fn room(account_key: AccountKey, room_id: impl Into<String>) -> Self {
        Self {
            account_key,
            kind: TimelineKind::Room {
                room_id: room_id.into(),
            },
        }
    }

    pub fn room_id(&self) -> &str {
        match &self.kind {
            TimelineKind::Room { room_id }
            | TimelineKind::Thread { room_id, .. }
            | TimelineKind::Focused { room_id, .. } => room_id,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum TimelineKind {
    Room {
        room_id: String,
    },
    Thread {
        room_id: String,
        root_event_id: String,
    },
    Focused {
        room_id: String,
        event_id: String,
    },
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct TimelineGeneration(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct TimelineBatchId(pub u64);
