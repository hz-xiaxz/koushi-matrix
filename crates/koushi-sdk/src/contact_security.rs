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
//! A retrieval counts as fresh only when the contact's homeserver answered
//! `/keys/query`; otherwise the cached store answer is not projected.
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
    ContactSecurityFailureKind, ContactSecuritySummary, ContactVerificationDirectChat,
    ContactVerificationOffer,
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

/// Facts deciding whether **Verify user** can be offered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactVerificationFacts {
    /// This session holds your private user-signing key, which signs the
    /// contact's identity when verification completes.
    pub can_sign_identities: bool,
    /// The direct chat the SDK would send the request in.
    pub direct_chat: ContactVerificationDirectChat,
}

/// Pure fold of SDK facts into the reducer summary. `devices` must already be
/// in a stable order.
pub fn classify_contact_security(
    identity: Option<ContactIdentityFacts>,
    devices: impl IntoIterator<Item = ContactDeviceFacts>,
    verification: ContactVerificationFacts,
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
    let verification = match identity {
        ContactIdentityVerification::VerifiedByYou | ContactIdentityVerification::Unknown => {
            ContactVerificationOffer::NotOffered
        }
        ContactIdentityVerification::NotVerifiedByYou
        | ContactIdentityVerification::ChangedAfterVerification
            if !verification.can_sign_identities =>
        {
            ContactVerificationOffer::RequiresYourCrossSigning
        }
        ContactIdentityVerification::NotVerifiedByYou
        | ContactIdentityVerification::ChangedAfterVerification => {
            ContactVerificationOffer::Offered {
                direct_chat: verification.direct_chat,
            }
        }
    };
    ContactSecuritySummary {
        devices,
        device_counts: counts,
        device_signatures,
        identity,
        verification,
    }
}

/// The direct chat `UserIdentity::request_verification*` uses: the SDK's
/// first DM with the user, or a new encrypted DM when there is none.
fn verification_direct_chat(
    client: &matrix_sdk::Client,
    user_id: &UserId,
) -> ContactVerificationDirectChat {
    match client.get_dm_room(user_id) {
        None => ContactVerificationDirectChat::New,
        Some(room) if room.encryption_state().is_encrypted() => {
            ContactVerificationDirectChat::ExistingEncrypted
        }
        Some(_) => ContactVerificationDirectChat::ExistingUnencrypted,
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

/// Whether the homeserver answered `/keys/query` for the contact's server.
///
/// `/keys/query` succeeds with HTTP 200 even when the contact's homeserver
/// could not be reached: that server is listed under `failures` and the SDK
/// keeps serving its cached keys, which may be arbitrarily stale (for
/// example a verification from before an identity reset). The SDK's
/// `request_user_identity` does not surface `failures`, so the same typed
/// query is issued here and a listed server means the retrieval was not
/// fresh.
async fn contact_server_answered(
    client: &matrix_sdk::Client,
    user_id: &UserId,
) -> Result<bool, matrix_sdk::HttpError> {
    use matrix_sdk::ruma::api::client::keys::get_keys;

    let mut request = get_keys::v3::Request::new();
    request.device_keys.insert(user_id.to_owned(), Vec::new());
    let response = client.send(request).await?;
    Ok(!response
        .failures
        .contains_key(user_id.server_name().as_str()))
}

/// Fresh retrieval: `/keys/query` for the contact, then the store read. A
/// query the contact's homeserver did not answer is a failed retrieval,
/// never the cached answer.
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
    if !contact_server_answered(&session.client, user_id)
        .await
        .map_err(|_| ContactSecurityFailureKind::Network)?
    {
        return Err(ContactSecurityFailureKind::Network);
    }
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
    let can_sign_identities = encryption
        .cross_signing_status()
        .await
        .is_some_and(|status| status.has_user_signing);
    let verification = ContactVerificationFacts {
        can_sign_identities,
        direct_chat: verification_direct_chat(client, user_id),
    };
    Ok(classify_contact_security(identity, facts, verification))
}

/// **Verify user** (#1024): send an interactive SAS verification request
/// to the contact. As in Element and the SDK, the request is sent in the
/// direct chat with them; a new encrypted DM is created when none exists.
/// Refuses a contact without an identity, and one already verified by you.
pub async fn request_user_verification(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<crate::MatrixVerificationRequestHandle, crate::E2eeTrustError> {
    let user_id = <&UserId>::try_from(user_id)
        .map_err(|_| crate::E2eeTrustError::Sdk("invalid verification user id".to_owned()))?;
    let identity = session
        .client
        .encryption()
        .request_user_identity(user_id)
        .await
        .map_err(|error| {
            crate::E2eeTrustError::Classified(crate::e2ee::trust_failure_kind(&error))
        })?
        .ok_or_else(|| crate::E2eeTrustError::Sdk("contact has no identity".to_owned()))?;
    // Never request against a cached identity the contact's server did not
    // confirm just now.
    if !contact_server_answered(&session.client, user_id)
        .await
        .map_err(|_| crate::E2eeTrustError::Classified(crate::E2eeTrustFailureKind::Network))?
    {
        return Err(crate::E2eeTrustError::Classified(
            crate::E2eeTrustFailureKind::Network,
        ));
    }
    if identity.is_verified() {
        return Err(crate::E2eeTrustError::Sdk(
            "contact is already verified".to_owned(),
        ));
    }
    let request = identity
        .request_verification_with_methods(vec![
            matrix_sdk::ruma::events::key::verification::VerificationMethod::SasV1,
        ])
        .await
        .map_err(|error| match error {
            matrix_sdk::encryption::identities::RequestVerificationError::Sdk(error) => {
                crate::E2eeTrustError::Classified(crate::e2ee::trust_failure_kind(&error))
            }
            matrix_sdk::encryption::identities::RequestVerificationError::RoomCreation(_) => {
                crate::E2eeTrustError::Classified(crate::E2eeTrustFailureKind::Network)
            }
        })?;
    Ok(crate::MatrixVerificationRequestHandle::from_sdk(request))
}

/// A unit item whenever the SDK's device or identity store changes, or the
/// `m.direct` account data (which direct chat Verify user uses) does. Items
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
    // `m.direct` changes decide which direct chat Verify user would use (for
    // example one a failed first attempt created). The observer must outlive
    // its subscriber, so the stream owns it.
    let direct_chats = session
        .client
        .observe_events::<matrix_sdk::ruma::events::direct::DirectEvent, ()>();
    let direct_chat_changes = direct_chats.subscribe().map(move |_| {
        let _observer = &direct_chats;
    });
    Ok(Box::pin(stream::select(
        stream::select(devices, identities),
        direct_chat_changes,
    )))
}

#[cfg(test)]
mod tests;
