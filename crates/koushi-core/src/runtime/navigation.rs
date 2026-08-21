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

pub(super) fn unsubscribe_replaced_focused_context_timeline_key(
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
mod tests {
    use super::*;
    use crate::ids::{AccountKey, RuntimeConnectionId};
    use koushi_state::SessionInfo;

    fn focused_projection_fixture(sequence: u64) -> PendingFocusedNavigation {
        PendingFocusedNavigation {
            projection_request_id: RequestId {
                connection_id: RuntimeConnectionId(3),
                sequence,
            },
            key: TimelineKey {
                account_key: AccountKey("@qa:example.invalid".to_owned()),
                kind: TimelineKind::Focused {
                    room_id: "!room:example.invalid".to_owned(),
                    event_id: "$target".to_owned(),
                },
            },
            room_id: "!room:example.invalid".to_owned(),
            event_id: "$target".to_owned(),
            allow_live_fallback: true,
        }
    }
    #[test]
    fn focused_projection_ack_requires_same_owner_and_key_and_is_idempotent() {
        let expected = focused_projection_fixture(9);
        let mut pending = Some(expected.clone());
        let stale_id = RequestId {
            connection_id: RuntimeConnectionId(3),
            sequence: 8,
        };
        assert!(
            take_acknowledged_focused_navigation(&mut pending, stale_id, &expected.key).is_none()
        );
        assert_eq!(pending, Some(expected.clone()));

        let wrong_key = TimelineKey::room(
            AccountKey("@qa:example.invalid".to_owned()),
            "!room:example.invalid",
        );
        assert!(
            take_acknowledged_focused_navigation(
                &mut pending,
                expected.projection_request_id,
                &wrong_key,
            )
            .is_none()
        );
        assert_eq!(pending, Some(expected.clone()));

        assert_eq!(
            take_acknowledged_focused_navigation(
                &mut pending,
                expected.projection_request_id,
                &expected.key,
            ),
            Some(expected.clone())
        );
        assert!(pending.is_none());
        assert!(
            take_acknowledged_focused_navigation(
                &mut pending,
                expected.projection_request_id,
                &expected.key,
            )
            .is_none()
        );
    }
    #[test]
    fn focused_anchor_action_is_impossible_before_actor_acceptance() {
        let expected = focused_projection_fixture(12);
        let mut pending = Some(expected.clone());
        assert!(
            anchored_action_after_projection_ack(
                &mut pending,
                expected.projection_request_id,
                &expected.key,
                false,
                true,
                true,
            )
            .is_none()
        );
        assert_eq!(pending, Some(expected.clone()));

        let action = anchored_action_after_projection_ack(
            &mut pending,
            expected.projection_request_id,
            &expected.key,
            true,
            true,
            true,
        )
        .expect("accepted exact projection advances the anchor");
        assert!(matches!(
            action,
            AppAction::EnterAnchoredTimeline { room_id, event_id }
                if room_id == expected.room_id && event_id == expected.event_id
        ));
        assert!(pending.is_none());

        let mut target_missing = Some(expected.clone());
        assert_eq!(
            anchored_action_after_projection_ack(
                &mut target_missing,
                expected.projection_request_id,
                &expected.key,
                true,
                false,
                true,
            ),
            Some(AppAction::CloseFocusedContext)
        );
        assert!(
            target_missing.is_none(),
            "an accepted target-missing projection must terminate the focused attempt"
        );

        let mut actor_missing = Some(expected.clone());
        assert_eq!(
            anchored_action_after_projection_ack(
                &mut actor_missing,
                expected.projection_request_id,
                &expected.key,
                true,
                true,
                false,
            ),
            Some(AppAction::CloseFocusedContext)
        );
        assert!(
            actor_missing.is_none(),
            "the frontend and actor must both prove that the target is present"
        );

        let thread_key = TimelineKey {
            account_key: expected.key.account_key.clone(),
            kind: TimelineKind::Thread {
                room_id: expected.room_id,
                root_event_id: "$thread-root".to_owned(),
            },
        };
        assert!(
            anchored_action_after_projection_ack(
                &mut pending,
                expected.projection_request_id,
                &thread_key,
                true,
                true,
                true,
            )
            .is_none()
        );
    }
    #[test]
    fn focused_navigation_lifecycle_uses_the_reduced_state() {
        let expected = focused_projection_fixture(13);
        let mut state = AppState {
            session: SessionState::Ready(SessionInfo {
                homeserver: "https://example.invalid".to_owned(),
                user_id: "@synthetic:example.invalid".to_owned(),
                device_id: "SYNTHETIC".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            }),
            focused_context: FocusedContextState::Open {
                room_id: expected.room_id.clone(),
                event_id: expected.event_id.clone(),
                is_subscribed: true,
            },
            ..AppState::default()
        };
        state.navigation.active_room_id = Some(expected.room_id.clone());
        state.navigation.main_timeline_anchor = Some(koushi_state::MainTimelineAnchor {
            event_id: expected.event_id.clone(),
        });
        assert_eq!(
            focused_navigation_outcome_after_reduce(&state, &expected, true),
            IntentOutcome::Committed
        );

        state.navigation.main_timeline_anchor = None;
        state.focused_context = FocusedContextState::Closed;
        assert_eq!(
            focused_navigation_outcome_after_reduce(&state, &expected, false),
            IntentOutcome::BenignNoOp(IntentNoOpReason::TimelineTargetMissing)
        );

        let mut pinned_navigation = expected.clone();
        pinned_navigation.allow_live_fallback = false;
        assert_eq!(
            focused_navigation_outcome_after_reduce(&state, &pinned_navigation, false),
            IntentOutcome::FailedNoOp(IntentNoOpReason::TimelineTargetMissing)
        );

        state.navigation.active_room_id = Some("!other:example.invalid".to_owned());
        assert_eq!(
            focused_navigation_outcome_after_reduce(&state, &expected, true),
            IntentOutcome::FailedNoOp(IntentNoOpReason::RoomNotInState)
        );
    }
    #[test]
    fn replacement_focused_helper_preserves_same_key_and_unsubscribes_different_key() {
        let account_key = AccountKey("@alice:example.invalid".to_owned());
        let current = TimelineKey {
            account_key: account_key.clone(),
            kind: TimelineKind::Focused {
                room_id: "!room:example.invalid".to_owned(),
                event_id: "$event-a:example.invalid".to_owned(),
            },
        };
        let same = current.clone();
        let different = TimelineKey {
            account_key,
            kind: TimelineKind::Focused {
                room_id: "!room:example.invalid".to_owned(),
                event_id: "$event-b:example.invalid".to_owned(),
            },
        };

        assert_eq!(
            unsubscribe_replaced_focused_context_timeline_key(Some(current.clone()), same),
            None
        );
        assert_eq!(
            unsubscribe_replaced_focused_context_timeline_key(Some(current.clone()), different),
            Some(current)
        );
        assert_eq!(
            unsubscribe_replaced_focused_context_timeline_key(
                None,
                focused_key("$event-c:example.invalid")
            ),
            None
        );
    }
    #[test]
    fn select_space_cleanup_targets_previous_room_only_when_active_room_changes() {
        let action = AppAction::SelectSpace {
            space_id: Some("!space:example.invalid".to_owned()),
        };

        assert_eq!(
            navigation_replacement_room_for_cleanup(
                &action,
                Some("!old:example.invalid"),
                Some("!next:example.invalid"),
            ),
            Some(NavigationReplacementRoomForCleanup::Room(
                "!next:example.invalid".to_owned()
            ))
        );
        assert_eq!(
            navigation_replacement_room_for_cleanup(&action, Some("!old:example.invalid"), None,),
            Some(NavigationReplacementRoomForCleanup::Cleared)
        );
        assert_eq!(
            navigation_replacement_room_for_cleanup(
                &action,
                Some("!same:example.invalid"),
                Some("!same:example.invalid"),
            ),
            None
        );
        assert_eq!(
            navigation_replacement_room_for_cleanup(&action, None, None),
            None
        );
    }
    #[test]
    fn select_room_cleanup_still_uses_explicit_target_room() {
        let action = AppAction::SelectRoom {
            room_id: "!target:example.invalid".to_owned(),
        };

        assert_eq!(
            navigation_replacement_room_for_cleanup(
                &action,
                Some("!old:example.invalid"),
                Some("!target:example.invalid"),
            ),
            Some(NavigationReplacementRoomForCleanup::Room(
                "!target:example.invalid".to_owned()
            ))
        );
    }
    fn focused_key(event_id: &str) -> TimelineKey {
        TimelineKey {
            account_key: AccountKey("@alice:example.invalid".to_owned()),
            kind: TimelineKind::Focused {
                room_id: "!room:example.invalid".to_owned(),
                event_id: event_id.to_owned(),
            },
        }
    }
}
