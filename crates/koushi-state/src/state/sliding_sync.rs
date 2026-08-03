use serde::{Deserialize, Serialize};

use super::{LoginAttemptId, SessionInfo};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SlidingSyncPositiveEvidence {
    pub observed_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlidingSyncCapabilityFailureKind {
    Unsupported,
    Unreachable,
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlidingSyncCapabilityResult {
    Supported {
        evidence: SlidingSyncPositiveEvidence,
    },
    Unsupported,
    Unreachable,
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SlidingSyncAdmission {
    NewLogin { attempt_id: LoginAttemptId },
    StoredSessionRestore { info: SessionInfo },
}

impl SlidingSyncAdmission {
    pub fn kind(&self) -> SlidingSyncAdmissionKind {
        match self {
            Self::NewLogin { .. } => SlidingSyncAdmissionKind::NewLogin,
            Self::StoredSessionRestore { .. } => SlidingSyncAdmissionKind::StoredSessionRestore,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlidingSyncAdmissionKind {
    NewLogin,
    StoredSessionRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlidingSyncAdmissionSource {
    Network,
    PositiveCache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SlidingSyncRevalidationState {
    NotRequired,
    Pending {
        failure: SlidingSyncCapabilityFailureKind,
    },
    Checking {
        request_id: u64,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SlidingSyncCapabilityState {
    #[default]
    Unknown,
    Checking {
        account_epoch: u64,
        request_id: u64,
        admission: SlidingSyncAdmission,
        positive_evidence: Option<SlidingSyncPositiveEvidence>,
    },
    Supported {
        account_epoch: u64,
        request_id: u64,
        admission: SlidingSyncAdmission,
        evidence: SlidingSyncPositiveEvidence,
        revalidation: SlidingSyncRevalidationState,
    },
    Blocked {
        account_epoch: u64,
        request_id: u64,
        admission: SlidingSyncAdmission,
        failure: SlidingSyncCapabilityFailureKind,
        positive_evidence: Option<SlidingSyncPositiveEvidence>,
    },
}
