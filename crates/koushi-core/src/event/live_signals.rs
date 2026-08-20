use std::fmt;

use koushi_state::PresenceKind;
use serde::{Deserialize, Serialize};

use crate::ids::{RequestId, TimelineKey};

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LiveSignalsEvent {
    PresenceUpdated {
        user_id: String,
        presence: PresenceKind,
    },
    ReadReceiptSent {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    FullyReadSet {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    TypingSet {
        request_id: RequestId,
        key: TimelineKey,
        is_typing: bool,
    },
    PresenceSet {
        request_id: RequestId,
        presence: PresenceKind,
    },
}
impl fmt::Debug for LiveSignalsEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PresenceUpdated { presence, .. } => formatter
                .debug_struct("PresenceUpdated")
                .field("user_id", &"UserId(..)")
                .field("presence", presence)
                .finish(),
            Self::ReadReceiptSent { request_id, .. } => formatter
                .debug_struct("ReadReceiptSent")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::FullyReadSet { request_id, .. } => formatter
                .debug_struct("FullyReadSet")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::TypingSet {
                request_id,
                is_typing,
                ..
            } => formatter
                .debug_struct("TypingSet")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("is_typing", is_typing)
                .finish(),
            Self::PresenceSet {
                request_id,
                presence,
            } => formatter
                .debug_struct("PresenceSet")
                .field("request_id", request_id)
                .field("presence", presence)
                .finish(),
        }
    }
}
