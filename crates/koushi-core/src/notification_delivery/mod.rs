//! The device-local notification delivery decision engine (#1427).
//!
//! This module answers two questions and nothing else: when this device may
//! deliver a notification, and what the message says. It is a module of its own
//! because that channel is the only outbound path whose content and timing are
//! decided entirely on this device, and because both answers have to be
//! provable without a runtime, a network, or a real clock. The slice that
//! follows wires this decision into the core actor loop; keeping the decision
//! pure first is what makes its unit tests deterministic.
//!
//! - [`dwell`] holds each room that entered a qualifying attention state for
//!   the configured dwell and releases it once, anchored on that transition.
//! - [`digest`] turns the rooms that came due into one plain-text message per
//!   enabled sink, naming each room by its safe display label, kind, and counts,
//!   with no room, event, sender, or transaction identifier, and with an
//!   idempotency key that keeps a retry of one window from sending twice.
//!
//! Both halves read the same [`RoomAttentionKind`] and count authority the
//! badge and the unread marks are built from
//! (`docs/architecture/state-machine.md`, "Local Notification Delivery"), so a
//! delivered message cannot describe a state the badge does not show. Neither
//! half reads a clock, sleeps, touches the network, or knows a sink secret: the
//! caller passes the current [`crate::executor::Instant`] and the current
//! qualifying set, and reads the credential when it sends.
//!
//! Which rooms qualify is the caller's decision, not this module's. The enabled
//! trigger kinds, the watched rooms and spaces, the window focus, and the dwell
//! all shape the qualifying set while it is built; `digest` takes that set as
//! given and must not re-filter it. Only the channel's own `enabled` switch turns
//! a due set into no message at all.
//!
//! Nothing calls this module yet. The runtime slice that observes the attention
//! projection, owns the timer, coalesces sends, and reads a sink secret is the
//! next change; this one is the decision engine and its tests.
//!
//! [`RoomAttentionKind`]: koushi_state::RoomAttentionKind

pub mod digest;
pub mod dwell;

#[cfg(test)]
mod tests;
