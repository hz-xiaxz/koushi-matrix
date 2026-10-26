//! Non-touching, export-time observations; no identities or image handles escape.

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::{AvatarThumbnailFailureKind, AvatarThumbnailState, OwnProfile};

use super::CoreConnection;

impl CoreConnection {
    /// Observe the published own profile without cloning AppState or subscribing
    /// to events. Cache presence is non-atomic with the published snapshot.
    pub fn record_own_avatar_diagnostics(&self, account_index: u64) {
        let snapshot = self.snapshot_rx.borrow();
        record(own_avatar_event(&snapshot.state.profile.own, account_index));
    }
}

fn own_avatar_event(profile: &OwnProfile, account_index: u64) -> DiagnosticEvent {
    let (state, failure, retained) = match profile.avatar.as_ref().map(|avatar| &avatar.thumbnail) {
        None => ("absent", "none", "not_applicable"),
        Some(AvatarThumbnailState::NotRequested) => ("not_requested", "none", "not_applicable"),
        Some(AvatarThumbnailState::Loading { .. }) => ("loading", "none", "not_applicable"),
        Some(AvatarThumbnailState::Ready { source_ref, .. }) => (
            "ready",
            "none",
            if crate::renderable_thumbnail::contains_renderable_thumbnail(source_ref) {
                "present"
            } else {
                "missing"
            },
        ),
        Some(AvatarThumbnailState::Failed { kind, .. }) => (
            "failed",
            match kind {
                AvatarThumbnailFailureKind::Network => "network",
                AvatarThumbnailFailureKind::Forbidden => "forbidden",
                AvatarThumbnailFailureKind::Unsupported => "unsupported",
                AvatarThumbnailFailureKind::Sdk => "sdk",
                AvatarThumbnailFailureKind::Capacity => "capacity",
            },
            "not_applicable",
        ),
    };
    DiagnosticEvent::new(DiagnosticLevel::Info, "core.avatar", "own_profile")
        .field(DiagnosticField::count("account_index", account_index))
        .field(DiagnosticField::token(
            "sample_scope",
            "non_atomic_observation",
        ))
        .field(DiagnosticField::token("thumbnail_state", state))
        .field(DiagnosticField::token("failure_kind", failure))
        .field(DiagnosticField::token("retained_bytes", retained))
}

#[cfg(test)]
#[path = "avatar_diagnostics_tests.rs"]
mod tests;
