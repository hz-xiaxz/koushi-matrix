//! Rust-owned room-history export workflow (#59).
//!
//! The export writes an Element-compatible chat-export JSON file. This state
//! records only the request correlation, the room, the requested range, and
//! private-data-free counts; event content, paths, and file bytes stay in the
//! Core export task and the platform sink.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which part of the room history to export.
///
/// A period is resolved by the platform adapter from civil dates into UTC
/// instants: `start_ms` is 00:00 of the start day and `end_exclusive_ms` is
/// 00:00 of the day after the end day, both in `time_zone`. Rust owns the
/// validation and the per-event inclusion test.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RoomHistoryExportRange {
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

impl RoomHistoryExportRange {
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

impl fmt::Debug for RoomHistoryExportRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllAvailable => formatter.write_str("RoomHistoryExportRange::AllAvailable"),
            Self::Period {
                start_ms,
                end_exclusive_ms,
                ..
            } => formatter
                .debug_struct("RoomHistoryExportRange::Period")
                .field("start_ms", start_ms)
                .field("end_exclusive_ms", end_exclusive_ms)
                .field("time_zone", &"TimeZone(..)")
                .finish(),
        }
    }
}

/// Private-data-free counts for one export.
///
/// `fetched_events` counts distinct events read from the server,
/// `exported_events` counts events written to the file, and
/// `undecryptable_events` counts written events that could not be decrypted
/// and were therefore exported as Element's `m.bad.encrypted` placeholder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomHistoryExportProgress {
    pub fetched_events: u64,
    pub exported_events: u64,
    pub undecryptable_events: u64,
}

/// Why an export did not produce a complete file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoomHistoryExportFailureKind {
    /// The requested range is empty or lacks its time zone.
    InvalidRange,
    /// The room is not known to the current session.
    RoomNotFound,
    /// The platform did not register a destination for this request.
    DestinationUnavailable,
    /// Creating, writing, or finalizing the destination file failed.
    Write,
    /// Fetching history from the homeserver failed.
    Network,
    /// Any other SDK failure.
    Sdk,
}

/// Room-history export state machine.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RoomHistoryExportState {
    #[default]
    Idle,
    Exporting {
        request_id: u64,
        room_id: String,
        range: RoomHistoryExportRange,
        progress: RoomHistoryExportProgress,
        cancel_requested: bool,
    },
    Completed {
        request_id: u64,
        room_id: String,
        range: RoomHistoryExportRange,
        progress: RoomHistoryExportProgress,
    },
    Cancelled {
        request_id: u64,
        room_id: String,
        progress: RoomHistoryExportProgress,
    },
    Failed {
        request_id: u64,
        room_id: String,
        progress: RoomHistoryExportProgress,
        failure_kind: RoomHistoryExportFailureKind,
    },
}

impl RoomHistoryExportState {
    /// The request currently in flight, if any.
    pub fn active_request_id(&self) -> Option<u64> {
        match self {
            Self::Exporting { request_id, .. } => Some(*request_id),
            _ => None,
        }
    }
}

impl fmt::Debug for RoomHistoryExportState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => formatter.write_str("RoomHistoryExportState::Idle"),
            Self::Exporting {
                request_id,
                range,
                progress,
                cancel_requested,
                ..
            } => formatter
                .debug_struct("RoomHistoryExportState::Exporting")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("range", range)
                .field("progress", progress)
                .field("cancel_requested", cancel_requested)
                .finish(),
            Self::Completed {
                request_id,
                range,
                progress,
                ..
            } => formatter
                .debug_struct("RoomHistoryExportState::Completed")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("range", range)
                .field("progress", progress)
                .finish(),
            Self::Cancelled {
                request_id,
                progress,
                ..
            } => formatter
                .debug_struct("RoomHistoryExportState::Cancelled")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("progress", progress)
                .finish(),
            Self::Failed {
                request_id,
                progress,
                failure_kind,
                ..
            } => formatter
                .debug_struct("RoomHistoryExportState::Failed")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("progress", progress)
                .field("failure_kind", failure_kind)
                .finish(),
        }
    }
}
