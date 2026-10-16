use serde::{Deserialize, Serialize};

/// Coarse status only: keys, peer identities and wire nonces never cross into UI state.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryKeyRecoveryStatus {
    #[default]
    Idle,
    Sending,
    Requested {
        recipients: u16,
        responses: u16,
        received_keys: u32,
    },
    Failed {
        reason: HistoryKeyRecoveryFailure,
    },
    Partial {
        received_keys: u32,
        reason: HistoryKeyRecoveryFailure,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryKeyRecoveryFailure {
    Network,
    VerificationRequired,
    MembershipChanged,
    HistoryNotShared,
    TooLarge,
    NoRecipients,
    RateLimited,
    Storage,
    InvalidResponse,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryKeyRecoveryState {
    pub generation: u64,
    pub status: HistoryKeyRecoveryStatus,
}

/// A full joined-member population is required before selecting the broad default.
pub fn history_key_request_targets_all(
    joined_count: Option<usize>,
    force_all: bool,
    automatic: bool,
) -> bool {
    !automatic && (force_all || joined_count.is_some_and(|count| count <= 30))
}
