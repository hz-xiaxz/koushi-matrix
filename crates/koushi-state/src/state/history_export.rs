//! Rust-owned history export archive workflow.
//!
//! One export writes a folder for a room, or for every joined non-DM room of a
//! Space. This state records the request correlation, the scope, the range,
//! and per-room progress. Paths, file names, and event content stay in the
//! Core export task.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which part of the history to export.
///
/// A period is resolved by the platform adapter from civil dates into UTC
/// instants: `start_ms` is 00:00 of the start day and `end_exclusive_ms` is
/// 00:00 of the day after the end day, both in `time_zone`. Rust owns the
/// validation and the per-event inclusion test.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryExportRange {
    /// Every event this account can read from the server.
    AllAvailable,
    /// Events with `start_ms <= origin_server_ts < end_exclusive_ms`.
    Period {
        start_ms: u64,
        end_exclusive_ms: u64,
        /// IANA time-zone name used to resolve the civil dates.
        time_zone: String,
    },
}

impl HistoryExportRange {
    /// Whether the range can be exported: a period must be non-empty and name
    /// the time zone its civil dates were resolved in.
    pub fn is_valid(&self) -> bool {
        match self {
            Self::AllAvailable => true,
            Self::Period {
                start_ms,
                end_exclusive_ms,
                time_zone,
            } => start_ms < end_exclusive_ms && !time_zone.trim().is_empty(),
        }
    }

    /// Whether an event timestamp falls inside the range.
    pub fn contains(&self, origin_server_ts: u64) -> bool {
        match self {
            Self::AllAvailable => true,
            Self::Period {
                start_ms,
                end_exclusive_ms,
                ..
            } => *start_ms <= origin_server_ts && origin_server_ts < *end_exclusive_ms,
        }
    }
}

impl fmt::Debug for HistoryExportRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllAvailable => formatter.write_str("HistoryExportRange::AllAvailable"),
            Self::Period {
                start_ms,
                end_exclusive_ms,
                ..
            } => formatter
                .debug_struct("HistoryExportRange::Period")
                .field("start_ms", start_ms)
                .field("end_exclusive_ms", end_exclusive_ms)
                .field("time_zone", &"TimeZone(..)")
                .finish(),
        }
    }
}

/// What one export covers.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryExportScope {
    Room { room_id: String },
    /// Every joined non-DM room reachable through joined subspaces.
    Space { space_id: String },
}

impl fmt::Debug for HistoryExportScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Room { .. } => "HistoryExportScope::Room(RoomId(..))",
            Self::Space { .. } => "HistoryExportScope::Space(RoomId(..))",
        })
    }
}

/// Private-data-free counts for one room of an export.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryExportRoomCounts {
    /// Distinct events read from the server.
    pub fetched_events: u64,
    /// Events written to `messages.json`.
    pub exported_events: u64,
    /// Written events that could not be decrypted.
    pub undecryptable_events: u64,
    pub attachments_total: u64,
    pub attachments_done: u64,
    pub attachments_failed: u64,
}

/// Where one room is in the export. Phases only move forward.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryExportRoomPhase {
    Pending,
    Fetching,
    Attachments,
    Rendering,
    Completed,
    Skipped,
    Failed,
}

impl HistoryExportRoomPhase {
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Completed | Self::Skipped | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryExportRoomSkipReason {
    /// The account has not joined the room, so its history cannot be read.
    NotJoined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryExportRoomFailureKind {
    Network,
    Sdk,
    Write,
}

/// One room of an export. `display_name` is carried because rooms the account
/// has not joined are absent from `AppState.rooms`; it is never logged.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryExportRoom {
    pub room_id: String,
    pub display_name: String,
    pub phase: HistoryExportRoomPhase,
    pub counts: HistoryExportRoomCounts,
    pub skip_reason: Option<HistoryExportRoomSkipReason>,
    pub failure_kind: Option<HistoryExportRoomFailureKind>,
}

impl fmt::Debug for HistoryExportRoom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryExportRoom")
            .field("room_id", &"RoomId(..)")
            .field("display_name", &"DisplayName(..)")
            .field("phase", &self.phase)
            .field("counts", &self.counts)
            .field("skip_reason", &self.skip_reason)
            .field("failure_kind", &self.failure_kind)
            .finish()
    }
}

/// Why a whole export ended without completing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryExportFailureKind {
    InvalidRange,
    RoomNotFound,
    SpaceNotFound,
    /// The platform did not register a directory for this request.
    DestinationUnavailable,
    /// The chosen folder holds an export of another scope or range.
    ManifestMismatch,
    /// The export folder could not be written.
    Write,
    /// The disk is full.
    NoSpace,
    Network,
    Sdk,
}

/// History export state machine. See "History Export" in
/// `docs/architecture/state-machine.md`.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryExportState {
    #[default]
    Idle,
    /// Core is resolving the folder, the manifest, and the rooms.
    Preparing {
        request_id: u64,
        scope: HistoryExportScope,
        range: HistoryExportRange,
        stop_requested: bool,
    },
    Running {
        request_id: u64,
        scope: HistoryExportScope,
        range: HistoryExportRange,
        rooms: Vec<HistoryExportRoom>,
        stop_requested: bool,
    },
    Completed {
        request_id: u64,
        scope: HistoryExportScope,
        range: HistoryExportRange,
        rooms: Vec<HistoryExportRoom>,
    },
    Stopped {
        request_id: u64,
        scope: HistoryExportScope,
        range: HistoryExportRange,
        rooms: Vec<HistoryExportRoom>,
    },
    Failed {
        request_id: u64,
        scope: HistoryExportScope,
        range: HistoryExportRange,
        rooms: Vec<HistoryExportRoom>,
        failure_kind: HistoryExportFailureKind,
    },
}

impl HistoryExportState {
    /// The request currently in flight, if any.
    pub fn active_request_id(&self) -> Option<u64> {
        match self {
            Self::Preparing { request_id, .. } | Self::Running { request_id, .. } => {
                Some(*request_id)
            }
            _ => None,
        }
    }

    /// The request this state belongs to, in flight or settled.
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Preparing { request_id, .. }
            | Self::Running { request_id, .. }
            | Self::Completed { request_id, .. }
            | Self::Stopped { request_id, .. }
            | Self::Failed { request_id, .. } => Some(*request_id),
        }
    }
}

impl fmt::Debug for HistoryExportState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, request_id, scope, range, rooms) = match self {
            Self::Idle => return formatter.write_str("HistoryExportState::Idle"),
            Self::Preparing {
                request_id,
                scope,
                range,
                ..
            } => ("Preparing", request_id, scope, range, None),
            Self::Running {
                request_id,
                scope,
                range,
                rooms,
                ..
            } => ("Running", request_id, scope, range, Some(rooms)),
            Self::Completed {
                request_id,
                scope,
                range,
                rooms,
            } => ("Completed", request_id, scope, range, Some(rooms)),
            Self::Stopped {
                request_id,
                scope,
                range,
                rooms,
            } => ("Stopped", request_id, scope, range, Some(rooms)),
            Self::Failed {
                request_id,
                scope,
                range,
                rooms,
                ..
            } => ("Failed", request_id, scope, range, Some(rooms)),
        };
        let mut debug = formatter.debug_struct(&format!("HistoryExportState::{name}"));
        debug
            .field("request_id", request_id)
            .field("scope", scope)
            .field("range", range);
        if let Some(rooms) = rooms {
            debug.field("rooms", rooms);
        }
        match self {
            Self::Preparing { stop_requested, .. } | Self::Running { stop_requested, .. } => {
                debug.field("stop_requested", stop_requested);
            }
            Self::Failed { failure_kind, .. } => {
                debug.field("failure_kind", failure_kind);
            }
            _ => {}
        }
        debug.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(phase: HistoryExportRoomPhase) -> HistoryExportRoom {
        HistoryExportRoom {
            room_id: "!private-room:example.org".to_owned(),
            display_name: "Private Room Name".to_owned(),
            phase,
            counts: HistoryExportRoomCounts::default(),
            skip_reason: None,
            failure_kind: None,
        }
    }

    #[test]
    fn scope_serializes_with_kind_tag() {
        let space = HistoryExportScope::Space {
            space_id: "!s:example.org".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&space).unwrap(),
            serde_json::json!({ "kind": "space", "space_id": "!s:example.org" })
        );
        let room_scope = HistoryExportScope::Room {
            room_id: "!r:example.org".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&room_scope).unwrap(),
            serde_json::json!({ "kind": "room", "room_id": "!r:example.org" })
        );
    }

    #[test]
    fn running_state_carries_rooms_and_phases() {
        let state = HistoryExportState::Running {
            request_id: 7,
            scope: HistoryExportScope::Space {
                space_id: "!s:example.org".to_owned(),
            },
            range: HistoryExportRange::AllAvailable,
            rooms: vec![room(HistoryExportRoomPhase::Attachments)],
            stop_requested: false,
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["kind"], "running");
        assert_eq!(value["rooms"][0]["phase"], "attachments");
        assert_eq!(value["rooms"][0]["counts"]["attachments_failed"], 0);
        assert_eq!(state.active_request_id(), Some(7));
        let back: HistoryExportState = serde_json::from_value(value).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn terminal_states_have_no_active_request() {
        let state = HistoryExportState::Failed {
            request_id: 3,
            scope: HistoryExportScope::Room {
                room_id: "!r:example.org".to_owned(),
            },
            range: HistoryExportRange::AllAvailable,
            rooms: Vec::new(),
            failure_kind: HistoryExportFailureKind::ManifestMismatch,
        };
        assert_eq!(state.active_request_id(), None);
        assert_eq!(state.request_id(), Some(3));
        assert_eq!(HistoryExportState::Idle.request_id(), None);
    }

    #[test]
    fn debug_redacts_room_ids_and_names() {
        let state = HistoryExportState::Stopped {
            request_id: 1,
            scope: HistoryExportScope::Space {
                space_id: "!private-space:example.org".to_owned(),
            },
            range: HistoryExportRange::AllAvailable,
            rooms: vec![room(HistoryExportRoomPhase::Completed)],
        };
        let debug = format!("{state:?}");
        assert!(!debug.contains("private-space"));
        assert!(!debug.contains("private-room"));
        assert!(!debug.contains("Private Room Name"));
        assert!(debug.contains("Completed"));
    }
}
