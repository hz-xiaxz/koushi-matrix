//! Reducer contract for contact security details in User info (#1024).
//!
//! The device/identity classification itself is the SDK adapter's
//! (`koushi-sdk` contact_security tests drive it with real signed keys); these
//! tests pin the reducer fences: checking, unavailable, stale responses after
//! switching contacts or accounts, and store-driven refreshes.

use koushi_state::{
    AppAction, AppState, ContactDeviceCounts, ContactDeviceSignature, ContactDevicesStatus,
    ContactIdentityVerification, ContactSecurityFailureKind, ContactSecurityLoadState,
    ContactSecuritySummary, SessionInfo, SessionState, reduce,
};

const ALICE: &str = "@alice:example.test";
const BOB: &str = "@bob:example.test";

fn session_info(user_id: &str) -> SessionInfo {
    SessionInfo {
        homeserver: "https://example.test".to_owned(),
        user_id: user_id.to_owned(),
        device_id: "DEVICE".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    }
}

fn ready_state() -> AppState {
    AppState {
        session: SessionState::Ready(session_info("@me:example.test")),
        ..AppState::default()
    }
}

fn summary(
    devices: ContactDevicesStatus,
    signatures: Vec<ContactDeviceSignature>,
    identity: ContactIdentityVerification,
) -> ContactSecuritySummary {
    let owner_signed = signatures
        .iter()
        .filter(|signature| **signature == ContactDeviceSignature::OwnerSigned)
        .count() as u32;
    let invalid = signatures
        .iter()
        .filter(|signature| **signature == ContactDeviceSignature::OwnerSignatureInvalid)
        .count() as u32;
    ContactSecuritySummary {
        devices,
        device_counts: ContactDeviceCounts {
            total: signatures.len() as u32,
            owner_signed,
            not_owner_signed: signatures.len() as u32 - owner_signed,
            owner_signature_invalid: invalid,
            excluded_dehydrated: 0,
        },
        device_signatures: signatures,
        identity,
    }
}

fn all_signed_not_verified() -> ContactSecuritySummary {
    summary(
        ContactDevicesStatus::AllOwnerSigned,
        vec![ContactDeviceSignature::OwnerSigned; 2],
        ContactIdentityVerification::NotVerifiedByYou,
    )
}

fn all_signed_verified() -> ContactSecuritySummary {
    summary(
        ContactDevicesStatus::AllOwnerSigned,
        vec![ContactDeviceSignature::OwnerSigned; 2],
        ContactIdentityVerification::VerifiedByYou,
    )
}

fn unsigned_device_verified() -> ContactSecuritySummary {
    summary(
        ContactDevicesStatus::SomeNotOwnerSigned,
        vec![
            ContactDeviceSignature::OwnerSigned,
            ContactDeviceSignature::NotOwnerSigned,
        ],
        ContactIdentityVerification::VerifiedByYou,
    )
}

fn request(state: &mut AppState, request_id: u64, user_id: &str) -> usize {
    reduce(
        state,
        AppAction::ContactSecurityLoadRequested {
            request_id,
            user_id: user_id.to_owned(),
        },
    )
    .len()
}

fn load(
    state: &mut AppState,
    request_id: u64,
    user_id: &str,
    summary: ContactSecuritySummary,
) -> usize {
    reduce(
        state,
        AppAction::ContactSecurityLoaded {
            request_id,
            user_id: user_id.to_owned(),
            summary,
        },
    )
    .len()
}

fn refresh(state: &mut AppState, user_id: &str, summary: ContactSecuritySummary) -> usize {
    reduce(
        state,
        AppAction::ContactSecurityRefreshed {
            user_id: user_id.to_owned(),
            summary,
        },
    )
    .len()
}

#[test]
fn opening_user_info_enters_checking_without_a_summary() {
    let mut state = ready_state();
    assert_eq!(request(&mut state, 1, ALICE), 1);
    assert_eq!(state.contact_security.user_id.as_deref(), Some(ALICE));
    assert_eq!(
        state.contact_security.load,
        ContactSecurityLoadState::Loading { request_id: 1 }
    );
    assert!(state.contact_security.summary.is_none());
}

#[test]
fn load_requires_a_ready_session() {
    let mut state = AppState::default();
    assert_eq!(request(&mut state, 1, ALICE), 0);
    assert_eq!(state.contact_security, Default::default());
}

#[test]
fn loaded_summaries_keep_devices_and_your_verification_independent() {
    for expected in [
        all_signed_not_verified(),
        all_signed_verified(),
        unsigned_device_verified(),
    ] {
        let mut state = ready_state();
        request(&mut state, 7, ALICE);
        assert_eq!(load(&mut state, 7, ALICE, expected.clone()), 1);
        assert_eq!(
            state.contact_security.load,
            ContactSecurityLoadState::Loaded { request_id: 7 }
        );
        assert_eq!(state.contact_security.summary.as_ref(), Some(&expected));
    }
}

#[test]
fn retrieval_failure_is_unavailable_and_never_a_confirmation() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    load(&mut state, 1, ALICE, all_signed_verified());
    // A retry clears the previous summary: a stale answer is not kept while
    // checking again.
    request(&mut state, 2, ALICE);
    assert!(state.contact_security.summary.is_none());
    let effects = reduce(
        &mut state,
        AppAction::ContactSecurityLoadFailed {
            request_id: 2,
            user_id: ALICE.to_owned(),
            failure_kind: ContactSecurityFailureKind::Network,
        },
    );
    assert_eq!(effects.len(), 1);
    assert_eq!(
        state.contact_security.load,
        ContactSecurityLoadState::Failed {
            request_id: 2,
            failure_kind: ContactSecurityFailureKind::Network,
        }
    );
    assert!(state.contact_security.summary.is_none());
    // A store refresh after a failed retrieval does not turn it into success.
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 0);
    assert!(state.contact_security.summary.is_none());
}

#[test]
fn stale_response_after_switching_contacts_is_dropped() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    request(&mut state, 2, BOB);
    // Alice's late answer, by request id and by user id.
    assert_eq!(load(&mut state, 1, ALICE, all_signed_verified()), 0);
    assert_eq!(load(&mut state, 2, ALICE, all_signed_verified()), 0);
    assert_eq!(
        reduce(
            &mut state,
            AppAction::ContactSecurityLoadFailed {
                request_id: 1,
                user_id: ALICE.to_owned(),
                failure_kind: ContactSecurityFailureKind::Sdk,
            },
        )
        .len(),
        0
    );
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 0);
    assert_eq!(state.contact_security.user_id.as_deref(), Some(BOB));
    assert_eq!(
        state.contact_security.load,
        ContactSecurityLoadState::Loading { request_id: 2 }
    );
    assert!(state.contact_security.summary.is_none());

    assert_eq!(load(&mut state, 2, BOB, unsigned_device_verified()), 1);
    assert_eq!(
        state.contact_security.summary,
        Some(unsigned_device_verified())
    );
}

#[test]
fn stale_response_after_switching_accounts_is_dropped() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    reduce(
        &mut state,
        AppAction::SwitchAccountRequested {
            info: session_info("@other:example.test"),
        },
    );
    assert_eq!(state.contact_security, Default::default());
    assert_eq!(load(&mut state, 1, ALICE, all_signed_verified()), 0);
    assert_eq!(state.contact_security, Default::default());
}

#[test]
fn store_refresh_tracks_device_additions_and_removals() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    load(&mut state, 1, ALICE, all_signed_verified());

    // The contact adds an unsigned device: your verification is preserved.
    assert_eq!(refresh(&mut state, ALICE, unsigned_device_verified()), 1);
    let refreshed = state.contact_security.summary.clone().expect("summary");
    assert_eq!(refreshed.devices, ContactDevicesStatus::SomeNotOwnerSigned);
    assert_eq!(
        refreshed.identity,
        ContactIdentityVerification::VerifiedByYou
    );
    assert_eq!(
        state.contact_security.load,
        ContactSecurityLoadState::Loaded { request_id: 1 }
    );

    // The unsigned device is removed again.
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 1);
    assert_eq!(state.contact_security.summary, Some(all_signed_verified()));

    // An unchanged re-read emits nothing.
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 0);
}

#[test]
fn store_refresh_projects_identity_change_attention_only_after_verification() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    load(&mut state, 1, ALICE, all_signed_verified());
    let changed = summary(
        ContactDevicesStatus::SomeNotOwnerSigned,
        vec![ContactDeviceSignature::OwnerSignatureInvalid],
        ContactIdentityVerification::ChangedAfterVerification,
    );
    assert_eq!(refresh(&mut state, ALICE, changed.clone()), 1);
    assert_eq!(state.contact_security.summary, Some(changed));
}

#[test]
fn store_refresh_is_ignored_while_checking() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 0);
    assert!(state.contact_security.summary.is_none());
}

#[test]
fn closing_user_info_drops_later_answers() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    assert_eq!(
        reduce(&mut state, AppAction::ContactSecurityClosed).len(),
        1
    );
    assert_eq!(state.contact_security, Default::default());
    assert_eq!(load(&mut state, 1, ALICE, all_signed_verified()), 0);
    assert_eq!(refresh(&mut state, ALICE, all_signed_verified()), 0);
    assert_eq!(state.contact_security, Default::default());
    // Closing twice is a no-op.
    assert_eq!(
        reduce(&mut state, AppAction::ContactSecurityClosed).len(),
        0
    );
}

#[test]
fn contact_security_debug_omits_the_contact_user_id() {
    let mut state = ready_state();
    request(&mut state, 1, ALICE);
    let debug = format!("{:?}", state.contact_security);
    assert!(!debug.contains("alice"), "{debug}");
}
