//! Sender device advisories shown in the timeline (#1494).
//!
//! The timeline-wide counterpart of the **Their devices** row in User info: for
//! each open room, the senders who have at least one device they never
//! confirmed (`ContactDevicesStatus::SomeNotOwnerSigned`). The Room timeline
//! actor classifies them from the crypto store; this slice only carries the
//! answer. It carries user ids only, never device ids, names, or counts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SenderDeviceAdvisoriesState {
    /// Room id to the senders that earn the mark. A room without an entry, or
    /// a sender missing from its set, shows no mark; no entry is ever empty.
    pub rooms: BTreeMap<String, BTreeSet<String>>,
}

impl fmt::Debug for SenderDeviceAdvisoriesState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SenderDeviceAdvisoriesState")
            .field("rooms", &self.rooms.len())
            .field(
                "users",
                &self.rooms.values().map(BTreeSet::len).sum::<usize>(),
            )
            .finish()
    }
}
