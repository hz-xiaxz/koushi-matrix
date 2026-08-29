//! Runtime navigation persistence and projection helpers.

use super::{AppActor, composer_draft_session_key};
use crate::event::{IntentNoOpReason, IntentOutcome};
use crate::executor;
use crate::ids::{RequestId, TimelineKey, TimelineKind};
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::{
    AppAction, AppEffect, AppState, FocusedContextState, NavigationState, SessionState, reduce,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NavigationPersistenceStatus {
    Unloaded,
    Loaded(koushi_key::SessionKeyId),
    LoadFailed(koushi_key::SessionKeyId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingFocusedNavigation {
    pub(super) projection_request_id: RequestId,
    pub(super) key: TimelineKey,
    pub(super) room_id: String,
    pub(super) event_id: String,
    pub(super) allow_live_fallback: bool,
}

fn take_acknowledged_focused_navigation(
    pending: &mut Option<PendingFocusedNavigation>,
    projection_request_id: RequestId,
    key: &TimelineKey,
) -> Option<PendingFocusedNavigation> {
    let matches = pending.as_ref().is_some_and(|candidate| {
        candidate.projection_request_id == projection_request_id && candidate.key == *key
    });
    matches.then(|| {
        pending
            .take()
            .expect("matching pending navigation must exist")
    })
}

pub(super) fn anchored_action_after_projection_ack(
    pending: &mut Option<PendingFocusedNavigation>,
    projection_request_id: RequestId,
    key: &TimelineKey,
    actor_accepted: bool,
    frontend_target_present: bool,
    actor_target_present: bool,
) -> Option<AppAction> {
    if !actor_accepted {
        return None;
    }
    let accepted = take_acknowledged_focused_navigation(pending, projection_request_id, key)?;
    if frontend_target_present && actor_target_present {
        Some(AppAction::EnterAnchoredTimeline {
            room_id: accepted.room_id,
            event_id: accepted.event_id,
        })
    } else {
        Some(AppAction::CloseFocusedContext)
    }
}

pub(super) fn focused_navigation_outcome_after_reduce(
    state: &AppState,
    navigation: &PendingFocusedNavigation,
    target_found: bool,
) -> IntentOutcome {
    let room_is_active =
        state.navigation.active_room_id.as_deref() == Some(navigation.room_id.as_str());
    let focused_is_closed = state.focused_context == FocusedContextState::Closed;
    let exact_anchor = state
        .navigation
        .main_timeline_anchor
        .as_ref()
        .is_some_and(|anchor| anchor.event_id == navigation.event_id);
    let settled = if target_found {
        room_is_active && exact_anchor
    } else {
        room_is_active && focused_is_closed && state.navigation.main_timeline_anchor.is_none()
    };

    if settled {
        if target_found {
            IntentOutcome::Committed
        } else if navigation.allow_live_fallback {
            IntentOutcome::BenignNoOp(IntentNoOpReason::TimelineTargetMissing)
        } else {
            IntentOutcome::FailedNoOp(IntentNoOpReason::TimelineTargetMissing)
        }
    } else if !matches!(state.session, SessionState::Ready(_)) {
        IntentOutcome::FailedNoOp(IntentNoOpReason::SessionNotReady)
    } else {
        IntentOutcome::FailedNoOp(IntentNoOpReason::RoomNotInState)
    }
}

impl AppActor {
    pub(super) async fn load_navigation_for_current_session(&mut self) {
        let Some(key_id) = navigation_session_key(&self.state) else {
            self.navigation_loaded_for = None;
            self.navigation_persistence_status = NavigationPersistenceStatus::Unloaded;
            return;
        };
        if self.navigation_loaded_for.as_ref() == Some(&key_id) {
            return;
        }

        let store = self.composer_draft_store_actor.clone();
        let load_key_id = key_id.clone();
        let load_result =
            executor::spawn_blocking(move || store.load_navigation(&load_key_id)).await;
        let navigation = match load_result {
            Ok(Ok(navigation)) => {
                self.navigation_persistence_status =
                    NavigationPersistenceStatus::Loaded(key_id.clone());
                record(
                    DiagnosticEvent::new(DiagnosticLevel::Info, "core.space_order", "loaded")
                        .field(DiagnosticField::count(
                            "ledger_entries",
                            navigation.space_order.len() as u64,
                        ))
                        .field(DiagnosticField::token("result", "success")),
                );
                navigation
            }
            Ok(Err(_)) | Err(_) => {
                self.navigation_persistence_status =
                    NavigationPersistenceStatus::LoadFailed(key_id.clone());
                record(
                    DiagnosticEvent::new(DiagnosticLevel::Error, "core.space_order", "load_failed")
                        .field(DiagnosticField::token("result", "failure")),
                );
                NavigationState::default()
            }
        };
        let effects = reduce(&mut self.state, AppAction::NavigationLoaded { navigation });
        self.navigation_loaded_for = Some(key_id);
        self.handle_ui_event_effects(&effects).await;
    }

    pub(super) async fn persist_navigation(
        &mut self,
        key_id: koushi_key::SessionKeyId,
        navigation: NavigationState,
    ) {
        let ledger_entries = navigation.space_order.len() as u64;
        let status_key_id = key_id.clone();
        let store = self.composer_draft_store_actor.clone();
        let result =
            executor::spawn_blocking(move || store.save_navigation(&key_id, &navigation)).await;
        match result {
            Ok(Ok(())) => {
                self.navigation_persistence_status =
                    NavigationPersistenceStatus::Loaded(status_key_id);
                record(
                    DiagnosticEvent::new(DiagnosticLevel::Info, "core.space_order", "persisted")
                        .field(DiagnosticField::count("ledger_entries", ledger_entries))
                        .field(DiagnosticField::token("result", "success")),
                );
            }
            Ok(Err(_)) | Err(_) => {
                self.navigation_persistence_status =
                    NavigationPersistenceStatus::LoadFailed(status_key_id);
                record(
                    DiagnosticEvent::new(
                        DiagnosticLevel::Error,
                        "core.space_order",
                        "persist_failed",
                    )
                    .field(DiagnosticField::count("ledger_entries", ledger_entries))
                    .field(DiagnosticField::token("result", "failure")),
                );
            }
        }
    }

    pub(super) fn current_focused_context_timeline_key(&self) -> Option<TimelineKey> {
        let account_key = self.current_account_key()?;
        match &self.state.focused_context {
            koushi_state::FocusedContextState::Opening { room_id, event_id }
            | koushi_state::FocusedContextState::Open {
                room_id, event_id, ..
            } => Some(TimelineKey {
                account_key,
                kind: TimelineKind::Focused {
                    room_id: room_id.clone(),
                    event_id: event_id.clone(),
                },
            }),
            koushi_state::FocusedContextState::Closed => None,
        }
    }

    pub(super) fn unsubscribe_replaced_focused_context_timeline(
        &self,
        room_id: &str,
        event_id: &str,
    ) -> Option<TimelineKey> {
        let replacement_key = TimelineKey {
            account_key: self.current_account_key()?,
            kind: TimelineKind::Focused {
                room_id: room_id.to_owned(),
                event_id: event_id.to_owned(),
            },
        };
        unsubscribe_replaced_focused_context_timeline_key(
            self.current_focused_context_timeline_key(),
            replacement_key,
        )
    }
}

fn unsubscribe_replaced_focused_context_timeline_key(
    current_key: Option<TimelineKey>,
    replacement_key: TimelineKey,
) -> Option<TimelineKey> {
    unsubscribe_replaced_timeline_key(current_key, replacement_key)
}

pub(super) fn unsubscribe_replaced_timeline_key(
    current_key: Option<TimelineKey>,
    replacement_key: TimelineKey,
) -> Option<TimelineKey> {
    current_key.filter(|current_key| current_key != &replacement_key)
}

pub(super) fn cancel_replaced_room_timeline_pagination_key(
    current_key: Option<TimelineKey>,
    replacement_room_id: Option<&str>,
) -> Option<TimelineKey> {
    current_key.filter(|current_key| match &current_key.kind {
        TimelineKind::Room { room_id } => {
            replacement_room_id.map_or(true, |replacement| room_id != replacement)
        }
        TimelineKind::Thread { .. } | TimelineKind::Focused { .. } => false,
    })
}

pub(super) fn cancel_replaced_room_timeline_link_previews_key(
    current_key: Option<TimelineKey>,
    replacement_room_id: Option<&str>,
) -> Option<TimelineKey> {
    current_key.filter(|current_key| match &current_key.kind {
        TimelineKind::Room { room_id } => {
            replacement_room_id.map_or(true, |replacement| room_id != replacement)
        }
        TimelineKind::Thread { .. } | TimelineKind::Focused { .. } => false,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NavigationReplacementRoomForCleanup {
    Room(String),
    Cleared,
}

impl NavigationReplacementRoomForCleanup {
    pub(super) fn room_id(&self) -> Option<&str> {
        match self {
            Self::Room(room_id) => Some(room_id),
            Self::Cleared => None,
        }
    }
}

pub(super) fn navigation_replacement_room_for_cleanup(
    action: &AppAction,
    active_room_before_reduce: Option<&str>,
    active_room_after_reduce: Option<&str>,
) -> Option<NavigationReplacementRoomForCleanup> {
    match action {
        AppAction::SelectRoom { room_id } => {
            Some(NavigationReplacementRoomForCleanup::Room(room_id.clone()))
        }
        AppAction::SelectSpace { .. } if active_room_before_reduce != active_room_after_reduce => {
            Some(match active_room_after_reduce {
                Some(room_id) => NavigationReplacementRoomForCleanup::Room(room_id.to_owned()),
                None => NavigationReplacementRoomForCleanup::Cleared,
            })
        }
        AppAction::SelectSpace { .. } => None,
        _ => None,
    }
}

pub(super) fn navigation_session_key(state: &AppState) -> Option<koushi_key::SessionKeyId> {
    composer_draft_session_key(state)
}

pub(super) fn effects_open_focused_timeline(effects: &[AppEffect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, AppEffect::OpenFocusedTimeline { .. }))
}

#[cfg(test)]
mod tests;
