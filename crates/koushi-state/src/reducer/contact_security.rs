//! Reducer for contact security details in User info (#1024).
//!
//! State-machine contract: docs/architecture/state-machine.md,
//! "Contact Security Details". Every transition is read-only with respect to
//! trust: nothing here pins, verifies, or changes sending policy.

use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, ContactSecurityFailureKind, ContactSecurityLoadState, ContactSecurityState,
        ContactSecuritySummary,
    },
};

use super::is_session_ready;

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(UiEvent::ContactSecurityChanged)]
}

fn is_current_load(state: &AppState, request_id: u64, user_id: &str) -> bool {
    let contact = &state.contact_security;
    contact.user_id.as_deref() == Some(user_id)
        && contact.load == (ContactSecurityLoadState::Loading { request_id })
}

pub(crate) fn handle_load_requested(
    state: &mut AppState,
    request_id: u64,
    user_id: String,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    // A new retrieval never shows the previous answer while checking, even
    // for the same contact: a stale summary is not a confirmation.
    state.contact_security = ContactSecurityState {
        user_id: Some(user_id),
        load: ContactSecurityLoadState::Loading { request_id },
        summary: None,
        verification_busy: false,
    };
    // `verification_busy` is derived by `sync_verification_busy` after every
    // action.
    changed()
}

/// Keep `verification_busy` derived from the shared verification flow. Runs
/// after every reduced action, so any flow transition (incoming, outgoing,
/// own-session, settle, session teardown) updates an open User info.
pub(crate) fn sync_verification_busy(state: &mut AppState, effects: &mut Vec<AppEffect>) {
    let busy =
        state.contact_security.user_id.is_some() && state.e2ee_trust.verification.is_in_progress();
    if state.contact_security.verification_busy == busy {
        return;
    }
    state.contact_security.verification_busy = busy;
    let event = AppEffect::EmitUiEvent(UiEvent::ContactSecurityChanged);
    if !effects.contains(&event) {
        effects.push(event);
    }
}

pub(crate) fn handle_loaded(
    state: &mut AppState,
    request_id: u64,
    user_id: &str,
    summary: ContactSecuritySummary,
) -> Vec<AppEffect> {
    if !is_current_load(state, request_id, user_id) {
        return Vec::new();
    }
    state.contact_security.load = ContactSecurityLoadState::Loaded { request_id };
    state.contact_security.summary = Some(summary);
    changed()
}

pub(crate) fn handle_load_failed(
    state: &mut AppState,
    request_id: u64,
    user_id: &str,
    failure_kind: ContactSecurityFailureKind,
) -> Vec<AppEffect> {
    if !is_current_load(state, request_id, user_id) {
        return Vec::new();
    }
    state.contact_security.load = ContactSecurityLoadState::Failed {
        request_id,
        failure_kind,
    };
    state.contact_security.summary = None;
    changed()
}

/// A local key-store change re-read without network. Accepted only on top of
/// a successful retrieval for the same contact: while checking the pending
/// retrieval supersedes it, and after a failed retrieval a store read must
/// not turn "status unavailable" into a confirmation.
pub(crate) fn handle_refreshed(
    state: &mut AppState,
    user_id: &str,
    summary: ContactSecuritySummary,
) -> Vec<AppEffect> {
    let contact = &mut state.contact_security;
    if contact.user_id.as_deref() != Some(user_id)
        || !matches!(contact.load, ContactSecurityLoadState::Loaded { .. })
        || contact.summary.as_ref() == Some(&summary)
    {
        return Vec::new();
    }
    contact.summary = Some(summary);
    changed()
}

pub(crate) fn handle_closed(state: &mut AppState) -> Vec<AppEffect> {
    if state.contact_security == ContactSecurityState::default() {
        return Vec::new();
    }
    state.contact_security = ContactSecurityState::default();
    changed()
}
