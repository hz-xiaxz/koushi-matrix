use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomInteractionState {
    pub pinned_events: Vec<PinnedEvent>,
    pub pin_operation: PinOperationState,
    /// Rust-owned state machine for the temporary dangerous encryption
    /// debugging controls (issue #538). React renders this snapshot and
    /// dispatches typed commands only; it never derives busy or interprets
    /// outcomes locally.
    #[serde(default)]
    pub encryption_debug_operation: EncryptionDebugOperationState,
}

/// The kind of manual encryption-debug operation (issue #538).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncryptionDebugOperationKind {
    #[default]
    ForceNewOutboundSession,
    ShareIndex0Key,
}

/// Closed outcome of a manual encryption-debug operation (issue #538).
/// Tokens mirror the diagnostic allowlist; no identifiers or key material.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncryptionDebugOperationOutcome {
    #[default]
    Completed,
    RefusedNotEncrypted,
    RefusedIndexAdvanced,
    CancelledStale,
    PolicyBlocked,
    Deadline,
    Failed,
}

/// Guarded per-room state machine for the manual encryption-debug controls
/// (issue #538).
///
/// Start admission is `Idle | Settled | Failed`; a start while `Pending` is
/// rejected. Settle/failure require matching request_id + room + kind;
/// mismatched completions are dropped (stale). Lifecycle changes (logout,
/// session replacement, room leave/removal) reset to `Idle` through the
/// reducer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum EncryptionDebugOperationState {
    #[default]
    Idle,
    Pending {
        request_id: u64,
        kind: EncryptionDebugOperationKind,
    },
    Settled {
        request_id: u64,
        kind: EncryptionDebugOperationKind,
        outcome: EncryptionDebugOperationOutcome,
    },
    Failed {
        request_id: u64,
        kind: EncryptionDebugOperationKind,
        outcome: EncryptionDebugOperationOutcome,
    },
}

impl EncryptionDebugOperationState {
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Pending { request_id, .. } => Some(*request_id),
            Self::Settled { request_id, .. } => Some(*request_id),
            Self::Failed { .. } => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// A new start is admitted from `Idle`, `Settled`, or `Failed`; a
    /// concurrent `Pending` start is rejected.
    pub fn accepts_new_request(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Settled { .. } | Self::Failed { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PinnedEvent {
    pub event_id: String,
    pub sender: Option<String>,
    #[serde(default)]
    pub sender_label: Option<String>,
    pub body_preview: Option<String>,
    pub redacted: bool,
    #[serde(default)]
    pub timestamp_ms: Option<u64>,
    #[serde(default)]
    pub state: PinnedEventState,
    #[serde(default)]
    pub thread_root_event_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinnedEventState {
    #[default]
    Ready,
    UnableToDecrypt,
    Unavailable,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyQuoteCodeBlock {
    pub language: Option<String>,
    pub body: String,
}

impl fmt::Debug for ReplyQuoteCodeBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplyQuoteCodeBlock")
            .field(
                "language",
                &self.language.as_ref().map(|_| "CodeBlockLanguage(..)"),
            )
            .field("body", &"CodeBlockBody(..)")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyQuoteFormattedBody {
    pub html: String,
    pub plain_text: String,
    pub code_blocks: Vec<ReplyQuoteCodeBlock>,
}

impl fmt::Debug for ReplyQuoteFormattedBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplyQuoteFormattedBody")
            .field("html", &"FormattedHtml(..)")
            .field("plain_text", &"FormattedPlainText(..)")
            .field("code_blocks", &self.code_blocks.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyQuote {
    pub event_id: String,
    pub sender: Option<String>,
    #[serde(default)]
    pub sender_label: Option<String>,
    pub body_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatted: Option<ReplyQuoteFormattedBody>,
    pub state: ReplyQuoteState,
}

impl fmt::Debug for ReplyQuote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplyQuote")
            .field("event_id", &"EventId(..)")
            .field("sender", &self.sender.as_ref().map(|_| "UserId(..)"))
            .field(
                "sender_label",
                &self.sender_label.as_ref().map(|_| "SenderLabel(..)"),
            )
            .field(
                "body_preview",
                &self.body_preview.as_ref().map(|_| "BodyPreview(..)"),
            )
            .field(
                "formatted",
                &self
                    .formatted
                    .as_ref()
                    .map(|_| "ReplyQuoteFormattedBody(..)"),
            )
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplyQuoteState {
    Ready,
    Redacted,
    Missing,
    Unsupported,
}

impl ReplyQuoteState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Redacted => "redacted",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinOp {
    Pin,
    Unpin,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PinOperationState {
    #[default]
    Idle,
    Pending {
        request_id: u64,
        room_id: String,
        event_id: String,
        op: PinOp,
    },
    Failed {
        room_id: String,
        event_id: String,
        op: PinOp,
        recoverable: bool,
    },
}

impl PinOperationState {
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Pending { request_id, .. } => Some(*request_id),
            Self::Failed { .. } => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub fn accepts_new_request(&self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::Failed {
                    recoverable: true,
                    ..
                }
        )
    }
}
