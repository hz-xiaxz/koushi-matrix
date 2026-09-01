//! Transport-neutral identity DTOs.

use std::fmt;

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

#[derive(Clone, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct SessionKeyId {
    pub homeserver: String,
    pub user_id: String,
    pub device_id: String,
}

impl fmt::Debug for SessionKeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionKeyId")
            .field("homeserver", &"Homeserver(..)")
            .field("user_id", &"UserId(..)")
            .field("device_id", &"DeviceId(..)")
            .finish()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_debug_redacts_all_identity_fields() {
        let key = SessionKeyId {
            homeserver: "https://private-homeserver.invalid".to_owned(),
            user_id: "@private-user:example.invalid".to_owned(),
            device_id: "PRIVATE-DEVICE".to_owned(),
        };
        let debug = format!("{key:?}");
        assert!(!debug.contains(&key.homeserver));
        assert!(!debug.contains(&key.user_id));
        assert!(!debug.contains(&key.device_id));
    }
}
