//! Runtime navigation persistence and projection helpers.

use super::{AppActor, composer_draft_session_key};
use crate::event::{CoreEvent, IntentNoOpReason, IntentOutcome};
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

fn take_committed_focused_navigation(
    pending: &mut Option<PendingFocusedNavigation>,
    commit: &crate::timeline::FocusedProjectionCommitted,
) -> Option<PendingFocusedNavigation> {
    let matches = pending.as_ref().is_some_and(|candidate| {
        candidate.projection_request_id == commit.projection_request_id
            && candidate.key == commit.key
    });
    matches.then(|| {
        pending
            .take()
            .expect("matching pending navigation must exist")
    })
}

pub(super) fn admit_focused_projection_generation(
    latest: &mut std::collections::HashMap<TimelineKey, (u64, crate::TimelineGeneration)>,
    commit: &crate::timeline::FocusedProjectionCommitted,
) -> bool {
    let generation = (commit.actor_generation, commit.timeline_generation);
    if latest
        .get(&commit.key)
        .is_some_and(|current| generation < *current)
    {
        return false;
    }
    latest.insert(commit.key.clone(), generation);
    true
}

pub(super) fn focused_navigation_action_after_projection_commit(
    pending: &mut Option<PendingFocusedNavigation>,
    commit: &crate::timeline::FocusedProjectionCommitted,
) -> Option<AppAction> {
    let accepted = take_committed_focused_navigation(pending, commit)?;
    if commit.target_present {
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
    pub(super) async fn handle_focused_projection_commit(
        &mut self,
        commit: crate::timeline::FocusedProjectionCommitted,
    ) {
        if !admit_focused_projection_generation(
            &mut self.latest_focused_projection_generation,
            &commit,
        ) {
            return;
        }

        let Some(navigation) = self
            .pending_focused_navigation
            .as_ref()
            .filter(|pending| {
                pending.projection_request_id == commit.projection_request_id
                    && pending.key == commit.key
            })
            .cloned()
        else {
            return;
        };
        let Some(action) = focused_navigation_action_after_projection_commit(
            &mut self.pending_focused_navigation,
            &commit,
        ) else {
            return;
        };
        let target_found = commit.target_present;
        record(
            DiagnosticEvent::new(
                DiagnosticLevel::Debug,
                "core.activity_navigation",
                if target_found {
                    "anchor_committed"
                } else {
                    "live_fallback"
                },
            )
            .field(DiagnosticField::count("item_count", commit.item_count))
            .field(DiagnosticField::count(
                "actor_generation",
                commit.actor_generation,
            ))
            .field(DiagnosticField::count(
                "timeline_generation",
                commit.timeline_generation.0,
            )),
        );

        let focused_key = (!target_found)
            .then(|| self.current_focused_context_timeline_key())
            .flatten();
        let before_state = self.snapshot_tx.borrow().state.clone();
        let (effects, deferred_reducer_side_effects) = self.reduce_app_action_state(action);
        let published_generation = self
            .publish_state_delta(&before_state)
            .unwrap_or(self.state_generation);
        let lifecycle_outcome =
            focused_navigation_outcome_after_reduce(&self.state, &navigation, target_found);
        self.emit(CoreEvent::IntentLifecycle {
            request_id: commit.projection_request_id,
            outcome: lifecycle_outcome,
            published_generation,
        });
        self.apply_deferred_reducer_side_effects(deferred_reducer_side_effects)
            .await;
        if let Some(key) = focused_key {
            self.send_timeline_command_or_fail(
                commit.projection_request_id,
                crate::command::TimelineCommand::Unsubscribe {
                    request_id: commit.projection_request_id,
                    key,
                },
            )
            .await;
        }
        self.handle_app_effects(commit.projection_request_id, effects)
            .await;
    }

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
