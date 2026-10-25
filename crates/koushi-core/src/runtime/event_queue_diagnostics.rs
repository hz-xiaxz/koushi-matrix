//! Export-time observations of the existing broadcast. No subscriptions,
//! receives, payload clones, serialization, or diagnostic events on that bus.

use super::{CoreConnection, CoreEvent, CoreRuntime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventQueueDiagnostics {
    pub capacity_events: usize,
    pub retained_events: usize,
    pub receiver_count: usize,
    /// Event values only; excludes slot synchronization and every nested heap
    /// allocation. This is not the retained payload's heap size.
    pub inline_slot_bytes_lower_bound: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventReceiverDiagnostics {
    /// Sender position minus reader position. May exceed the ring capacity;
    /// those excess positions have already been overwritten, not retained.
    pub unread_position_distance_events: usize,
}

impl CoreRuntime {
    pub fn event_queue_diagnostics(&self) -> EventQueueDiagnostics {
        EventQueueDiagnostics {
            capacity_events: self.event_capacity,
            retained_events: self.event_tx.len(),
            receiver_count: self.event_tx.receiver_count(),
            inline_slot_bytes_lower_bound: (self.event_capacity as u64)
                .saturating_mul(std::mem::size_of::<CoreEvent>() as u64),
        }
    }
}

impl CoreConnection {
    pub fn event_receiver_diagnostics(&self) -> EventReceiverDiagnostics {
        EventReceiverDiagnostics {
            unread_position_distance_events: self.event_rx.len(),
        }
    }
}
