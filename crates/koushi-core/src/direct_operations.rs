//! In-process operations an out-of-band caller (the MCP bridge) performs on a
//! live account runtime (#1524).
//!
//! These types are the public shape of those operations. They live outside the
//! runtime module so account, room and timeline sources can name them without
//! reaching into runtime internals.

/// One attachment written into an account's managed media cache (#1524).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchedAttachment {
    /// Absolute path inside Koushi's per-account media cache.
    pub path: std::path::PathBuf,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
}

/// Failure classes for a direct (non-GUI) attachment fetch (#1524).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentFetchFailure {
    /// The account has no ready session.
    AccountInactive,
    /// The room is unknown to this account or not joined.
    RoomNotFound,
    /// The event is unknown to the room.
    EventNotFound,
    /// The event carries no downloadable media.
    Unsupported,
    /// The download itself failed (network, key, or write).
    DownloadFailed,
}
