use std::collections::HashMap;

use crate::{
    effect::{AppEffect, UiEvent},
    state::{AppState, NativeAttentionObservationKind, RoomNotificationMode},
};

use super::is_session_ready;

pub(crate) fn handle_dispatch_started(
    state: &mut AppState,
    dispatch_id: crate::state::NativeAttentionDispatchId,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    if state.native_attention.summary.candidate.is_none() {
        return Vec::new();
    }
    if matches!(
        state.native_attention.dispatch,
        crate::state::NativeAttentionDispatchState::Dispatching { .. }
            | crate::state::NativeAttentionDispatchState::Suppressed { .. }
    ) {
        return Vec::new();
    }
    state.native_attention.dispatch =
        crate::state::NativeAttentionDispatchState::Dispatching { dispatch_id };
    vec![AppEffect::EmitUiEvent(UiEvent::NativeAttentionChanged)]
}

pub(crate) fn handle_dispatch_settled(
    state: &mut AppState,
    dispatch_id: crate::state::NativeAttentionDispatchId,
    outcome: crate::state::NativeAttentionSoundOutcome,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    if state.native_attention.dispatch
        != (crate::state::NativeAttentionDispatchState::Dispatching { dispatch_id })
    {
        return Vec::new();
    }
    state.native_attention.dispatch = match outcome {
        crate::state::NativeAttentionSoundOutcome::Played => {
            crate::state::NativeAttentionDispatchState::Delivered { dispatch_id }
        }
        crate::state::NativeAttentionSoundOutcome::Unsupported => {
            crate::state::NativeAttentionDispatchState::Unsupported { dispatch_id }
        }
        crate::state::NativeAttentionSoundOutcome::Failed => {
            crate::state::NativeAttentionDispatchState::Failed {
                dispatch_id,
                kind: crate::state::OperationFailureKind::Sdk,
            }
        }
        crate::state::NativeAttentionSoundOutcome::Skipped => return Vec::new(),
    };
    vec![AppEffect::EmitUiEvent(UiEvent::NativeAttentionChanged)]
}

pub(crate) fn handle_native_attention_updated(
    state: &mut AppState,
    mut attention: crate::state::NativeAttentionState,
) -> Vec<AppEffect> {
    if !is_session_ready(state) {
        return Vec::new();
    }
    if !state.settings.values.notifications.badges {
        attention.summary.badge_count = 0;
    }
    attention.dispatch = state.native_attention.dispatch.clone();
    if state.native_attention == attention {
        return Vec::new();
    }

    state.native_attention = attention;
    vec![AppEffect::EmitUiEvent(UiEvent::NativeAttentionChanged)]
}

fn projected_native_attention_from_rooms(
    state: &AppState,
    observation: NativeAttentionObservationKind,
) -> crate::state::NativeAttentionState {
    let room_notification_modes: HashMap<String, RoomNotificationMode> = state
        .room_notification_settings
        .iter()
        .map(|(room_id, settings)| (room_id.clone(), settings.mode))
        .collect();
    let previous_candidate = state.native_attention.summary.candidate.as_ref();
    let mut next = crate::state::native_attention_state_from_rooms(
        crate::state::NativeAttentionProjectionInput {
            rooms: &state.rooms,
            active_room_id: state.navigation.active_room_id.as_deref(),
            muted_room_ids: &[],
            room_notification_modes: &room_notification_modes,
            ignored_user_ids: &state.profile.ignored_user_ids,
            window_focused: state.native_attention_context.window_focused,
            observation,
            previous_candidate,
            capabilities: state.native_attention.summary.capabilities,
        },
    );
    if !state.settings.values.notifications.badges {
        next.summary.badge_count = 0;
    }
    next
}

pub(crate) fn recompute_native_attention_from_rooms(
    state: &mut AppState,
    observation: NativeAttentionObservationKind,
) -> bool {
    let next = projected_native_attention_from_rooms(state, observation);
    if state.native_attention == next {
        return false;
    }
    state.native_attention = next;
    true
}

pub(crate) fn handle_native_window_focus_changed(
    state: &mut AppState,
    focused: bool,
) -> Vec<AppEffect> {
    if state.native_attention_context.window_focused == focused {
        return Vec::new();
    }
    state.native_attention_context.window_focused = focused;

    let mut next =
        projected_native_attention_from_rooms(state, NativeAttentionObservationKind::InitialSync);
    next.summary.candidate = None;
    next.dispatch = crate::state::NativeAttentionDispatchState::Idle;
    if state.native_attention == next {
        return Vec::new();
    }

    state.native_attention = next;
    vec![AppEffect::EmitUiEvent(UiEvent::NativeAttentionChanged)]
}

pub(crate) fn apply_badge_setting(state: &mut AppState) -> bool {
    let next = if state.settings.values.notifications.badges
        && state.native_attention.summary.capabilities.badge
            != crate::state::NativeAttentionCapability::Unavailable
    {
        state.native_attention.summary.unread_count
    } else {
        0
    };
    if state.native_attention.summary.badge_count == next {
        return false;
    }
    state.native_attention.summary.badge_count = next;
    true
}

pub(crate) fn handle_japanese_catalog_profile_changed(
    state: &mut AppState,
    profile: crate::state::JapaneseCatalogProfile,
) -> Vec<AppEffect> {
    if state.cjk_text_policy.japanese_catalog == profile {
        return Vec::new();
    }

    state.cjk_text_policy.japanese_catalog = profile;
    vec![AppEffect::EmitUiEvent(UiEvent::CjkTextPolicyChanged)]
}
