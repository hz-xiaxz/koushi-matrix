//! Runtime navigation persistence and projection helpers.

use super::{AppActor, composer_draft_session_key};
use crate::event::{CoreEvent, IntentNoOpReason, IntentOutcome};
use crate::executor;
use crate::failure::CoreFailure;
use crate::ids::{RequestId, TimelineKey, TimelineKind};
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::{
    AppAction, AppEffect, AppState, FocusedContextState, HomeSelection,
    MAX_SPACE_LOCAL_PRESENTATIONS, NavigationPreferenceUpdate, NavigationState, SessionState,
    SpaceLocalPresentation, SpaceLocalPresentations, reduce,
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
    ) -> bool {
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
                true
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
                false
            }
        }
    }

    pub(super) async fn handle_navigation_preference_command(
        &mut self,
        request_id: RequestId,
        update: NavigationPreferenceUpdate,
    ) {
        self.load_navigation_for_current_session().await;
        let Some(key_id) = navigation_session_key(&self.state) else {
            self.emit(CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::SessionRequired,
            });
            return;
        };
        if navigation_preference_exceeds_capacity(&self.state.navigation, &update) {
            self.emit(CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::PreferenceRejected,
            });
            return;
        }
        let Ok(update) = normalize_navigation_preference_update(update) else {
            self.emit(CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::PreferenceRejected,
            });
            return;
        };

        if matches!(update, NavigationPreferenceUpdate::ImportLegacy { .. }) {
            if self.navigation_persistence_status
                != NavigationPersistenceStatus::Loaded(key_id.clone())
            {
                self.emit(CoreEvent::OperationFailed {
                    request_id,
                    failure: CoreFailure::StoreUnavailable,
                });
                return;
            }
            if self.state.navigation.legacy_frontend_preferences_imported {
                return;
            }
            let mut navigation = self.state.navigation.clone();
            navigation.apply_preference_update(update);
            if !self.persist_navigation(key_id, navigation.clone()).await {
                self.emit(CoreEvent::OperationFailed {
                    request_id,
                    failure: CoreFailure::StoreUnavailable,
                });
                return;
            }
            let effects = self
                .reduce_app_action(AppAction::NavigationLoaded { navigation })
                .await;
            self.handle_app_effects(request_id, effects).await;
            return;
        }

        let effects = self
            .reduce_app_action(AppAction::NavigationPreferenceUpdated { update })
            .await;
        self.handle_app_effects(request_id, effects).await;
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

const MAX_MATRIX_ID_SCALARS: usize = 255;
const MAX_LOCAL_SPACE_NAME_SCALARS: usize = 128;
const MAX_LOCAL_SPACE_ICON_SCALARS: usize = 12;

fn navigation_preference_exceeds_capacity(
    navigation: &NavigationState,
    update: &NavigationPreferenceUpdate,
) -> bool {
    matches!(
        update,
        NavigationPreferenceUpdate::SetSpacePresentation {
            space_id,
            presentation: Some(_),
        } if !navigation.space_local_presentations.0.contains_key(space_id)
            && navigation.space_local_presentations.0.len() >= MAX_SPACE_LOCAL_PRESENTATIONS
    )
}

fn normalize_navigation_preference_update(
    update: NavigationPreferenceUpdate,
) -> Result<NavigationPreferenceUpdate, ()> {
    match update {
        NavigationPreferenceUpdate::SetHomeSelection { selection } => {
            validate_home_selection(&selection)?;
            Ok(NavigationPreferenceUpdate::SetHomeSelection { selection })
        }
        NavigationPreferenceUpdate::SetSpacePresentation {
            space_id,
            presentation,
        } => {
            validate_matrix_id(&space_id)?;
            Ok(NavigationPreferenceUpdate::SetSpacePresentation {
                space_id,
                presentation: presentation.and_then(normalize_space_presentation),
            })
        }
        NavigationPreferenceUpdate::ImportLegacy {
            home_selection,
            space_local_presentations,
        } => {
            if space_local_presentations.0.len() > MAX_SPACE_LOCAL_PRESENTATIONS {
                return Err(());
            }
            if let Some(selection) = home_selection.as_ref() {
                validate_home_selection(selection)?;
            }
            let mut normalized = std::collections::BTreeMap::new();
            for (space_id, presentation) in space_local_presentations.0 {
                validate_matrix_id(&space_id)?;
                if let Some(presentation) = normalize_space_presentation(presentation) {
                    normalized.insert(space_id, presentation);
                }
            }
            Ok(NavigationPreferenceUpdate::ImportLegacy {
                home_selection,
                space_local_presentations: SpaceLocalPresentations(normalized),
            })
        }
    }
}

fn validate_home_selection(selection: &HomeSelection) -> Result<(), ()> {
    if let HomeSelection::DirectMessage { room_id } = selection {
        validate_matrix_id(room_id)?;
    }
    Ok(())
}

fn validate_matrix_id(value: &str) -> Result<(), ()> {
    (value.starts_with('!')
        && value.chars().count() <= MAX_MATRIX_ID_SCALARS
        && !value.chars().any(char::is_control))
    .then_some(())
    .ok_or(())
}

fn normalize_space_presentation(
    presentation: SpaceLocalPresentation,
) -> Option<SpaceLocalPresentation> {
    let name = normalize_bounded_text(presentation.name, MAX_LOCAL_SPACE_NAME_SCALARS);
    let icon = normalize_bounded_text(presentation.icon, MAX_LOCAL_SPACE_ICON_SCALARS);
    (name.is_some() || icon.is_some()).then_some(SpaceLocalPresentation { name, icon })
}

fn normalize_bounded_text(value: Option<String>, max_scalars: usize) -> Option<String> {
    let value = value?.trim().to_owned();
    (!value.is_empty()
        && value.chars().count() <= max_scalars
        && !value.chars().any(char::is_control))
    .then_some(value)
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
