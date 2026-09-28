//! Contact security details shown in User info (#1024).
//!
//! Two independent facts are kept separate on purpose:
//!
//! - **The contact's devices**: whether each retrieved encryption device key
//!   carries a valid signature from its owner's cross-signing identity. The
//!   owner confirms (signs) their devices in their own app.
//! - **Your verification of the contact**: whether the viewing user verified
//!   the contact's identity (master key) with their own user-signing key.
//!
//! Neither fact says anything about whether a conversation is encrypted, and
//! neither is combined into a single Verified/Unverified badge. All crypto
//! checks are the SDK's; this slice only carries its answers.
//!
//! The snapshot carries no device ids, device names or key material. Devices
//! are an ordered list of signature states that the GUI labels by ordinal.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Reducer-owned contact security details for the one contact whose User info
/// is open. `user_id` is the fence: any result for another user is stale.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContactSecurityState {
    pub user_id: Option<String>,
    pub load: ContactSecurityLoadState,
    /// The latest successfully retrieved summary for `user_id`. `None` while
    /// checking or after a failed retrieval: a failure is never shown as
    /// confirmation, and a stale summary is never kept for another retrieval.
    pub summary: Option<ContactSecuritySummary>,
    /// A verification flow (with anyone, or of this session) is in progress
    /// while User info is open, so **Verify user** cannot start another one
    /// until it settles. Derived by the reducer from
    /// `E2eeTrustState.verification`; always `false` while no contact is open.
    #[serde(default)]
    pub verification_busy: bool,
}

impl fmt::Debug for ContactSecurityState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContactSecurityState")
            .field("has_user_id", &self.user_id.is_some())
            .field("load", &self.load)
            .field("summary", &self.summary)
            .field("verification_busy", &self.verification_busy)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContactSecurityLoadState {
    #[default]
    Idle,
    /// A fresh `/keys/query` for the contact is in flight ("checking").
    Loading {
        request_id: u64,
    },
    Loaded {
        request_id: u64,
    },
    /// Retrieval failed ("status unavailable").
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        failure_kind: ContactSecurityFailureKind,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactSecurityFailureKind {
    SessionRequired,
    Network,
    Sdk,
}

/// Private-data-free projection of the SDK's device and identity answers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContactSecuritySummary {
    pub devices: ContactDevicesStatus,
    pub device_counts: ContactDeviceCounts,
    /// One entry per counted device, in a stable order, for the expanded view.
    pub device_signatures: Vec<ContactDeviceSignature>,
    pub identity: ContactIdentityVerification,
    /// Whether **Verify user** is offered, decided in Rust from the identity
    /// state and this session's own cross-signing keys.
    pub verification: ContactVerificationOffer,
}

/// Availability of the optional **Verify user** action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContactVerificationOffer {
    /// Already verified by you, or the contact has no identity to verify.
    NotOffered,
    /// The contact can be verified, but this session cannot sign their
    /// identity because your own cross-signing keys are not available here.
    RequiresYourCrossSigning,
    /// Offered (also to re-verify after an identity change). The request is
    /// sent in a direct chat with the contact, as Element/the SDK do.
    Offered {
        direct_chat: ContactVerificationDirectChat,
    },
}

/// The direct chat the verification request will use.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactVerificationDirectChat {
    /// Your existing encrypted direct chat with the contact.
    ExistingEncrypted,
    /// Your existing direct chat, which is not encrypted. Emoji comparison
    /// does not rely on room encryption.
    ExistingUnencrypted,
    /// No direct chat yet: a new encrypted one is created for the request.
    New,
}

/// Aggregate owner-confirmation status of the latest retrieved device list.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactDevicesStatus {
    /// Every counted device has a valid signature from its owner's identity.
    AllOwnerSigned,
    /// At least one counted device has no valid owner signature.
    SomeNotOwnerSigned,
    /// The retrieved list has no counted devices. Not a confirmation.
    NoDevices,
    /// The owner has no cross-signing identity, so no device can carry an
    /// owner signature. Distinct from unsigned devices under an identity.
    OwnerIdentityMissing,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContactDeviceCounts {
    /// Devices counted in the aggregate.
    pub total: u32,
    pub owner_signed: u32,
    /// Counted devices without a valid owner signature (includes
    /// `owner_signature_invalid`).
    pub not_owner_signed: u32,
    /// Subset of `not_owner_signed` whose keys carry a signature by the
    /// owner's account that the SDK does not accept as a signature of the
    /// owner's current identity (an invalid or superseded signature).
    pub owner_signature_invalid: u32,
    /// Dehydrated (offline-recovery) devices, excluded from the aggregate as
    /// Element does; the SDK withholds room keys from unverified dehydrated
    /// devices on its own.
    pub excluded_dehydrated: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactDeviceSignature {
    OwnerSigned,
    NotOwnerSigned,
    OwnerSignatureInvalid,
    OwnerIdentityMissing,
}

/// The viewing user's verification of the contact's identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactIdentityVerification {
    VerifiedByYou,
    /// Includes an identity that changed but was never verified by you:
    /// that stays neutral.
    NotVerifiedByYou,
    /// You verified an earlier identity and the current one is not verified
    /// (SDK verification violation). The only attention state.
    ChangedAfterVerification,
    /// No cross-signing identity is known for the contact.
    Unknown,
}
