//! Redacted public failures (overview.md Security Model: coarse public
//! failures with non-secret kinds; never raw SDK errors).

use serde::{Deserialize, Serialize};

use koushi_state::AuthFailureKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CoreFailure {
    SessionRequired,
    /// The credential store is healthy but holds no stored session for the
    /// requested account (restore / switch target). UI: go to login quietly.
    SessionNotFound,
    LoginFailed {
        kind: LoginFailureKind,
    },
    RecoveryFailed {
        kind: RecoveryFailureKind,
    },
    SyncFailed {
        kind: SyncFailureKind,
    },
    RoomOperationFailed {
        kind: RoomFailureKind,
    },
    TimelineOperationFailed {
        kind: TimelineFailureKind,
    },
    ProfileOperationFailed {
        kind: ProfileFailureKind,
    },
    AccountOperationFailed {
        kind: AuthFailureKind,
    },
    SecureBackupSetupConfirmationRequired,
    SecureBackupSetupFailedNoOp,
    /// A verification request was refused because another verification flow
    /// is still in progress (#1024).
    VerificationInProgress,
    SearchFailed {
        kind: SearchFailureKind,
    },
    ReportOperationFailed {
        kind: ReportFailureKind,
    },
    LocalEncryptionUnavailable,
    PreferenceRejected,
    /// #1427: a device-local delivery command was refused before it touched the
    /// credential store or the network. The kind says which precondition failed; no
    /// submitted value is echoed back.
    NotificationDeliveryRejected {
        kind: NotificationDeliveryFailureKind,
    },
    StoreUnavailable,
    ShutdownFailed,
}

/// Why a device-local delivery command was refused (#1427).
///
/// These are the preconditions the user can fix - the sink's identifier, the secret's
/// shape, the sink's existence, and whether a secret is stored - and nothing else. A
/// send's own outcome is a separate, typed fact carried by the delivery projection, not
/// a command failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NotificationDeliveryFailureKind {
    /// The id is not a sink id the credential store could ever key on.
    InvalidSinkId,
    /// The submitted secret is empty, oversized, or carries control characters.
    InvalidSecret,
    /// The settings hold no sink with this id, so a secret would have no owner.
    UnknownSink,
    /// A test send was asked for a sink whose secret this device does not hold.
    SecretMissing,
    /// The secret is not a ServerChan Turbo SendKey: it is a ServerChan³ key (`sctp…`), or
    /// it does not have the `SCT…` shape. Nothing was stored (#1460).
    UnsupportedSendKey,
    /// The destination lacks a field it needs to be attempted at all (an address, a host),
    /// so saving it would only drop it again. Nothing was written (#1468).
    IncompleteSink,
    /// Adding this destination would exceed the destination limit, so nothing was
    /// written (#1468).
    SinkLimitReached,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LoginFailureKind {
    InvalidCredentials,
    Network,
    RateLimited,
    Server,
    Store,
    /// The client gave up waiting for a response (#1268).
    Timeout,
    /// The server does not support the selected sign-in method (#1268).
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RecoveryFailureKind {
    InvalidRecoveryKey,
    Network,
    Server,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SyncFailureKind {
    Http,
    Auth,
    Store,
    Internal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RoomFailureKind {
    AliasInUse,
    Forbidden,
    InvalidInvite,
    NotFound,
    Network,
    Sdk,
    /// The current join-rule content has allow conditions this client does not
    /// model, so rewriting it would drop them (#1177).
    UnsupportedPolicyCondition,
    /// The current join-rule policy could not be read from the store before
    /// the write, so the edit was not attempted (#1177).
    PolicyNotVerified,
    /// A create request combined `visibility=public` with an explicit
    /// restricted access policy (#1177).
    PublicRoomWithRestrictedAccess,
    /// A create request combined an explicit access policy with
    /// `invitedOnly=true` (#1177).
    ExplicitAccessPolicyWithInvitedOnly,
    /// A create request selected a restricted access policy without a single
    /// membership allow target (#1177).
    EmptyAccessPolicyTargets,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadStateFailureKind {
    Timeout,
    Transport,
    RateLimited,
    Authentication,
    Server,
    Capacity,
    Sdk,
}

impl ReadStateFailureKind {
    pub fn token(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::RateLimited => "rate_limited",
            Self::Authentication => "authentication",
            Self::Server => "server",
            Self::Capacity => "capacity",
            Self::Sdk => "sdk",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TimelineFailureKind {
    InvalidDirection,
    InvalidReactionTarget,
    InvalidReactionState,
    InvalidSendTarget,
    InvalidSendState,
    SecureBackupRequired,
    ComposerRevisionExhausted,
    UnsupportedSlashCommand,
    NotSubscribed,
    Forbidden,
    /// The server answered `M_NOT_FOUND`, e.g. it withholds a thread root the
    /// account may not see (#1473).
    NotFound,
    Network,
    Timeout,
    Sdk,
    QueueOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProfileFailureKind {
    Forbidden,
    Network,
    InvalidMimeType,
    Sdk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchFailureKind {
    IndexUnavailable,
    Query,
    Internal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReportFailureKind {
    Forbidden,
    Network,
    InvalidUserId,
    InvalidRoomId,
    InvalidEventId,
    Sdk,
}
