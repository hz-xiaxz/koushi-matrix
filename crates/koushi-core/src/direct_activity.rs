//! Private, side-effect-free reads of Core's existing Recent projection.
use koushi_state::{ActivityRow, SessionInfo};

#[derive(Clone)]
pub struct RecentActivitySnapshot {
    pub session: SessionInfo,
    pub session_revision: u64,
    pub session_epoch: u64,
    pub notification_modes: std::collections::BTreeMap<String, koushi_state::RoomNotificationMode>,
    pub rows: Vec<ActivityRow>,
}

impl RecentActivitySnapshot {
    /// Only rooms observed in this projection participate in its policy fence.
    pub fn matches_notification_modes(&self, state: &koushi_state::AppState) -> bool {
        self.notification_modes.iter().all(|(room, mode)| {
            state
                .room_notification_settings
                .get(room)
                .map(|settings| settings.mode)
                .unwrap_or_default()
                == *mode
        })
    }

    /// Fence private delivery against session changes after the actor replied.
    pub fn matches_session(&self, state: &koushi_state::AppState) -> bool {
        state.session == koushi_state::SessionState::Ready(self.session.clone())
            && state.session_disclosure_epoch == self.session_epoch
    }
}

impl std::fmt::Debug for RecentActivitySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentActivitySnapshot")
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecentActivityFailure {
    #[error("account is inactive")]
    AccountInactive,
}
