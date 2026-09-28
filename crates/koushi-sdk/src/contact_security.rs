//! Contact security details for User info (#1024).
//!
//! Read-only projection of the SDK's answers about another user's keys:
//!
//! - Devices: `Device::is_cross_signed_by_owner()` — the owner's
//!   self-signing key signed the device key. This deliberately does not use
//!   `is_verified()` / `is_verified_with_cross_signing()`, which include the
//!   viewing user's trust and local trust.
//! - Identity: `UserIdentity::is_verified()` (the viewing user's user-signing
//!   key signed the contact's master key) and
//!   `has_verification_violation()` (verified before, not any more).
//!
//! Nothing here pins identities, verifies, withdraws verification, or sets
//! local trust. Devices whose own self-signature is invalid are rejected by
//! the SDK during `/keys/query` and never reach this projection; deleted
//! devices are removed from the store. Dehydrated devices are excluded from
//! the aggregate, as Element excludes them from its user device list.

use std::pin::Pin;

use futures_util::{Stream, StreamExt, stream};
use koushi_state::{
    ContactDeviceCounts, ContactDeviceSignature, ContactDevicesStatus, ContactIdentityVerification,
    ContactSecurityFailureKind, ContactSecuritySummary,
};
use matrix_sdk::ruma::UserId;

use crate::MatrixClientSession;

/// Facts the SDK reports for one of the contact's devices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactDeviceFacts {
    /// `Device::is_cross_signed_by_owner()`.
    pub cross_signed_by_owner: bool,
    /// The device keys carry a signature by the owner's account under a key
    /// id other than the device's own key (i.e. a cross-signing signature).
    pub has_owner_cross_signature: bool,
    pub dehydrated: bool,
}

/// Facts the SDK reports for the contact's cross-signing identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactIdentityFacts {
    /// `UserIdentity::is_verified()`.
    pub verified: bool,
    /// `UserIdentity::has_verification_violation()`.
    pub verification_violation: bool,
}

/// Pure fold of SDK facts into the reducer summary. `devices` must already be
/// in a stable order.
pub fn classify_contact_security(
    identity: Option<ContactIdentityFacts>,
    devices: impl IntoIterator<Item = ContactDeviceFacts>,
) -> ContactSecuritySummary {
    let mut counts = ContactDeviceCounts::default();
    let mut device_signatures = Vec::new();
    for device in devices {
        if device.dehydrated {
            counts.excluded_dehydrated += 1;
            continue;
        }
        let signature = if identity.is_none() {
            ContactDeviceSignature::OwnerIdentityMissing
        } else if device.cross_signed_by_owner {
            ContactDeviceSignature::OwnerSigned
        } else if device.has_owner_cross_signature {
            ContactDeviceSignature::OwnerSignatureInvalid
        } else {
            ContactDeviceSignature::NotOwnerSigned
        };
        counts.total += 1;
        match signature {
            ContactDeviceSignature::OwnerSigned => counts.owner_signed += 1,
            ContactDeviceSignature::OwnerSignatureInvalid => {
                counts.not_owner_signed += 1;
                counts.owner_signature_invalid += 1;
            }
            ContactDeviceSignature::NotOwnerSigned
            | ContactDeviceSignature::OwnerIdentityMissing => counts.not_owner_signed += 1,
        }
        device_signatures.push(signature);
    }

    let devices = if counts.total == 0 {
        ContactDevicesStatus::NoDevices
    } else if identity.is_none() {
        ContactDevicesStatus::OwnerIdentityMissing
    } else if counts.not_owner_signed == 0 {
        ContactDevicesStatus::AllOwnerSigned
    } else {
        ContactDevicesStatus::SomeNotOwnerSigned
    };
    let identity = match identity {
        None => ContactIdentityVerification::Unknown,
        Some(facts) if facts.verification_violation => {
            ContactIdentityVerification::ChangedAfterVerification
        }
        Some(facts) if facts.verified => ContactIdentityVerification::VerifiedByYou,
        Some(_) => ContactIdentityVerification::NotVerifiedByYou,
    };
    ContactSecuritySummary {
        devices,
        device_counts: counts,
        device_signatures,
        identity,
    }
}

fn parse_user_id(user_id: &str) -> Result<&UserId, ContactSecurityFailureKind> {
    <&UserId>::try_from(user_id).map_err(|_| ContactSecurityFailureKind::Sdk)
}

fn classify_retrieval_error(error: &matrix_sdk::Error) -> ContactSecurityFailureKind {
    match error {
        matrix_sdk::Error::Http(_)
        | matrix_sdk::Error::Io(_)
        | matrix_sdk::Error::ConcurrentRequestFailed
        | matrix_sdk::Error::Timeout => ContactSecurityFailureKind::Network,
        _ => ContactSecurityFailureKind::Sdk,
    }
}

/// Fresh retrieval: `/keys/query` for the contact, then the store read.
pub async fn load_contact_security(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<ContactSecuritySummary, ContactSecurityFailureKind> {
    let user_id = parse_user_id(user_id)?;
    session
        .client
        .encryption()
        .request_user_identity(user_id)
        .await
        .map_err(|error| classify_retrieval_error(&error))?;
    read_contact_security_from_store(&session.client, user_id).await
}

/// Store-only re-read after a key-store change notification.
pub async fn read_contact_security(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<ContactSecuritySummary, ContactSecurityFailureKind> {
    read_contact_security_from_store(&session.client, parse_user_id(user_id)?).await
}

async fn read_contact_security_from_store(
    client: &matrix_sdk::Client,
    user_id: &UserId,
) -> Result<ContactSecuritySummary, ContactSecurityFailureKind> {
    let encryption = client.encryption();
    let identity = encryption
        .get_user_identity(user_id)
        .await
        .map_err(|_| ContactSecurityFailureKind::Sdk)?
        .map(|identity| ContactIdentityFacts {
            verified: identity.is_verified(),
            verification_violation: identity.has_verification_violation(),
        });
    let devices = encryption
        .get_user_devices(user_id)
        .await
        .map_err(|_| ContactSecurityFailureKind::Sdk)?;
    let mut devices: Vec<_> = devices.devices().collect();
    devices.sort_by(|left, right| left.device_id().cmp(right.device_id()));
    let facts = devices.iter().map(|device| ContactDeviceFacts {
        cross_signed_by_owner: device.is_cross_signed_by_owner(),
        has_owner_cross_signature: device.signatures().get(user_id).is_some_and(|signatures| {
            signatures
                .keys()
                .any(|key_id| key_id.key_name() != device.device_id())
        }),
        dehydrated: device.is_dehydrated(),
    });
    Ok(classify_contact_security(identity, facts))
}

/// A unit item whenever the SDK's device or identity store changes. Items
/// are not filtered by user: a device removal is delivered without device
/// maps, and a change to the viewing user's own identity changes whether a
/// contact is verified, so the caller re-reads and de-duplicates.
pub type ContactSecurityChanges = Pin<Box<dyn Stream<Item = ()> + Send>>;

pub async fn observe_contact_security_changes(
    session: &MatrixClientSession,
) -> Result<ContactSecurityChanges, ContactSecurityFailureKind> {
    let encryption = session.client.encryption();
    let devices = encryption
        .devices_stream()
        .await
        .map_err(|_| ContactSecurityFailureKind::Sdk)?
        .map(|_| ());
    let identities = encryption
        .user_identities_stream()
        .await
        .map_err(|_| ContactSecurityFailureKind::Sdk)?
        .map(|_| ());
    Ok(Box::pin(stream::select(devices, identities)))
}

#[cfg(test)]
mod tests;
