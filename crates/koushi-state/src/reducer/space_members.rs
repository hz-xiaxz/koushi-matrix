use crate::{
    effect::{AppEffect, UiEvent},
    state::{
        AppState, SpaceMemberEntry, SpaceMemberInviteOutcome, SpaceMemberMembership,
        SpaceMemberRoleUpdateOutcome, SpaceMembersOperationState, SpaceMembersProjection,
        SpaceMembersState, admit_space_member_role, refresh_space_member_display_projection,
        resolve_space_members_projection, sort_entries,
    },
};

use super::is_session_ready;

pub(crate) fn handle_selected(state: &mut AppState, selected_space_id: Option<String>) -> bool {
    if !is_session_ready(state) || state.space_members.selected_space_id == selected_space_id {
        return false;
    }

    state.space_members.selected_space_id = selected_space_id;
    state.space_members.generation = state.space_members.generation.wrapping_add(1);
    clear_projection(&mut state.space_members);
    state.space_members.operation = SpaceMembersOperationState::Idle;
    true
}

pub(crate) fn handle_load_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    generation: u64,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || matches!(
            state.space_members.operation,
            SpaceMembersOperationState::UpdatingRole { .. }
        )
        || generation < state.space_members.generation
        || state
            .space_members
            .selected_space_id
            .as_deref()
            .is_some_and(|selected| selected != space_id)
    {
        return Vec::new();
    }

    let new_selection = state.space_members.selected_space_id.as_deref() != Some(&space_id);
    let new_generation = state.space_members.generation != generation;
    state.space_members.selected_space_id = Some(space_id.clone());
    state.space_members.generation = generation;
    if new_selection || new_generation {
        clear_projection(&mut state.space_members);
    }
    state.space_members.operation = SpaceMembersOperationState::Loading {
        request_id: Some(request_id),
        space_id,
        generation,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_loaded(
    state: &mut AppState,
    request_id: u64,
    projection: SpaceMembersProjection,
) -> Vec<AppEffect> {
    if !is_session_ready(state) || !projection_matches(state, request_id, &projection) {
        return Vec::new();
    }

    let pending = pending_operation(state);
    let pending_entry = pending.as_ref().and_then(|(_, _, user_id, _)| {
        state
            .space_members
            .space_invited
            .iter()
            .find(|entry| entry.user_id == *user_id)
            .cloned()
    });

    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    if resolved.incomplete_child_room_count > 0 {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    let mut operation_after_load = pending
        .clone()
        .map(
            |(request_id, space_id, user_id, generation)| SpaceMembersOperationState::Inviting {
                request_id,
                space_id,
                user_id,
                generation,
            },
        )
        .unwrap_or(SpaceMembersOperationState::Idle);
    if let Some((request_id, space_id, user_id, generation)) = pending {
        if let Some(entry) = resolved
            .space_joined
            .iter_mut()
            .find(|entry| entry.user_id == user_id)
        {
            entry.membership = SpaceMemberMembership::SpaceJoined;
            entry.invite_pending = false;
            operation_after_load = SpaceMembersOperationState::Idle;
        } else if let Some(entry) = resolved
            .space_invited
            .iter_mut()
            .find(|entry| entry.user_id == user_id)
        {
            entry.membership = SpaceMemberMembership::SpaceInvited;
            entry.invite_pending = false;
            operation_after_load = SpaceMembersOperationState::Idle;
        } else if let Some(mut entry) = pending_entry {
            remove_projection_entry(&mut resolved, &user_id);
            entry.membership = SpaceMemberMembership::SpaceInvited;
            entry.invite_pending = true;
            resolved.space_invited.push(entry);
            operation_after_load = SpaceMembersOperationState::Inviting {
                request_id,
                space_id,
                user_id,
                generation,
            };
        }
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
    state.space_members.operation = operation_after_load;
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_profiles_observed(
    state: &mut AppState,
    request_id: u64,
    profiles: Vec<crate::state::UserProfile>,
) -> Vec<AppEffect> {
    if !is_session_ready(state) || !operation_matches_request(state, request_id) {
        return Vec::new();
    }
    super::profile::handle_user_profiles_updated(state, profiles)
}

pub(crate) fn handle_projection_reconciled(
    state: &mut AppState,
    request_id: u64,
    projection: SpaceMembersProjection,
    profiles: Vec<crate::state::UserProfile>,
) -> Vec<AppEffect> {
    if !is_session_ready(state) || !operation_matches_request(state, request_id) {
        return Vec::new();
    }
    if matches!(
        &state.space_members.operation,
        SpaceMembersOperationState::CancellingInvite {
            request_id: active_request_id,
            space_id: active_space_id,
            generation: active_generation,
            ..
        } if *active_request_id == request_id
            && active_space_id == &projection.space_id
            && *active_generation == projection.generation
            && state.space_members.selected_space_id.as_deref()
                == Some(projection.space_id.as_str())
            && state.space_members.generation == projection.generation
    ) {
        let mut effects = super::profile::handle_user_profiles_updated(state, profiles);
        apply_reconciled_projection_during_cancellation(state, projection);
        effects.push(AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged));
        return effects;
    }
    if matches!(
        state.space_members.operation,
        SpaceMembersOperationState::CancellingInvite { .. }
    ) {
        return Vec::new();
    }
    let mut effects = super::profile::handle_user_profiles_updated(state, profiles);
    if matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Loading {
            request_id: Some(active_request_id),
            ..
        } if active_request_id == request_id
    ) {
        effects.extend(handle_loaded(state, request_id, projection));
    } else if matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Inviting {
            request_id: active_request_id,
            ..
        } if active_request_id == request_id
    ) {
        apply_reconciled_projection_during_invite(state, projection);
        effects.push(AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged));
    }
    effects
}

pub(crate) fn handle_background_projection_reconciled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    generation: u64,
    projection: SpaceMembersProjection,
    profiles: Vec<crate::state::UserProfile>,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
        || projection.space_id != space_id
        || projection.generation != generation
        || !background_reconciliation_operation_matches(
            &state.space_members.operation,
            &space_id,
            generation,
        )
    {
        return Vec::new();
    }

    let operation = state.space_members.operation.clone();
    let mut effects = super::profile::handle_user_profiles_updated(state, profiles);
    match operation {
        SpaceMembersOperationState::Idle => {
            apply_reconciled_projection_during_invite(state, projection);
        }
        SpaceMembersOperationState::Inviting { .. } => {
            if apply_reconciled_projection_during_invite(state, projection) {
                state.space_members.operation = SpaceMembersOperationState::Idle;
            }
        }
        SpaceMembersOperationState::CancellingInvite { .. } => {
            apply_reconciled_projection_during_cancellation(state, projection);
        }
        SpaceMembersOperationState::Failed {
            user_id: Some(user_id),
            ..
        } => {
            if apply_failed_invite_background_projection(state, projection, &user_id) {
                state.space_members.operation = SpaceMembersOperationState::Idle;
            }
        }
        SpaceMembersOperationState::Failed { user_id: None, .. } => {
            if apply_failed_load_background_projection(state, projection) {
                state.space_members.operation = SpaceMembersOperationState::Idle;
            }
        }
        SpaceMembersOperationState::UpdatingRole { .. } => {
            apply_projection_during_role_update(state, projection);
        }
        SpaceMembersOperationState::RoleUpdateFailed {
            user_id,
            power_level: requested_power_level,
            expected_power_levels_revision,
            ..
        } => {
            let projection_revision = projection.power_levels_revision.clone();
            apply_projection_during_role_update(state, projection);
            let target_matches = state.space_members.space_joined.iter().any(|entry| {
                entry.user_id == user_id && entry.power_level == Some(requested_power_level)
            });
            let revision_is_authoritative =
                projection_revision.as_deref().is_some_and(|revision| {
                    Some(revision) != expected_power_levels_revision.as_deref()
                });
            if target_matches && revision_is_authoritative {
                state.space_members.operation = SpaceMembersOperationState::Idle;
            }
        }
        SpaceMembersOperationState::Loading { .. } => unreachable!(
            "background Space member projections are fenced while an explicit load is active"
        ),
    }
    let _ = request_id;
    effects.push(AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged));
    effects
}

fn apply_failed_invite_background_projection(
    state: &mut AppState,
    projection: SpaceMembersProjection,
    failed_user_id: &str,
) -> bool {
    let failed_entry = state
        .space_members
        .all_entries()
        .find(|entry| entry.user_id == failed_user_id)
        .cloned()
        .unwrap_or_else(|| fallback_entry(failed_user_id, SpaceMemberMembership::ChildRoomOnly));
    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    let authoritative = resolved
        .space_joined
        .iter()
        .chain(resolved.space_invited.iter())
        .any(|entry| entry.user_id == failed_user_id);
    if resolved.incomplete_child_room_count > 0 {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    if !authoritative {
        remove_projection_entry(&mut resolved, failed_user_id);
        let mut failed_entry = failed_entry;
        failed_entry.membership = SpaceMemberMembership::ChildRoomOnly;
        failed_entry.invite_pending = false;
        resolved.child_room_only.push(failed_entry);
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
    if !authoritative {
        refresh_space_member_display_projection(&mut state.space_members, &state.profile);
    }
    authoritative
}

fn apply_failed_load_background_projection(
    state: &mut AppState,
    projection: SpaceMembersProjection,
) -> bool {
    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    let complete = resolved.incomplete_child_room_count == 0;
    if !complete {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
    complete
}

fn apply_reconciled_projection_during_invite(
    state: &mut AppState,
    projection: SpaceMembersProjection,
) -> bool {
    let pending = pending_operation(state);
    let pending_entry = pending.as_ref().and_then(|(_, _, user_id, _)| {
        state
            .space_members
            .space_invited
            .iter()
            .find(|entry| entry.user_id == *user_id)
            .cloned()
    });
    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    let authoritative = pending.as_ref().is_some_and(|(_, _, user_id, _)| {
        resolved
            .space_joined
            .iter()
            .chain(resolved.space_invited.iter())
            .any(|entry| entry.user_id == *user_id)
    });
    if resolved.incomplete_child_room_count > 0 {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    if let Some((_, _, user_id, _)) = pending {
        if !authoritative {
            if let Some(mut entry) = pending_entry {
                remove_projection_entry(&mut resolved, &user_id);
                entry.membership = SpaceMemberMembership::SpaceInvited;
                entry.invite_pending = true;
                resolved.space_invited.push(entry);
            }
        }
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
    authoritative
}

fn apply_reconciled_projection_during_cancellation(
    state: &mut AppState,
    projection: SpaceMembersProjection,
) {
    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    if resolved.incomplete_child_room_count > 0 {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
}

fn apply_projection_during_role_update(state: &mut AppState, projection: SpaceMembersProjection) {
    let mut resolved = resolve_space_members_projection(projection, &state.profile);
    if resolved.incomplete_child_room_count > 0 {
        merge_incomplete_projection(&state.space_members, &mut resolved);
    }
    sort_projection(&mut resolved);
    apply_projection(&mut state.space_members, resolved);
}

pub(crate) fn handle_load_failed(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    generation: u64,
    kind: crate::state::OperationFailureKind,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
        || !matches!(
            state.space_members.operation,
            SpaceMembersOperationState::Loading {
                request_id: Some(active_request_id),
                ..
            } if active_request_id == request_id
        )
    {
        return Vec::new();
    }

    state.space_members.operation = SpaceMembersOperationState::Failed {
        request_id,
        space_id,
        user_id: None,
        generation,
        kind,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_role_update_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
    expected_power_levels_revision: Option<String>,
    expected_power_level: i64,
    power_level: i64,
    confirmed: bool,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || admit_space_member_role(
            state,
            &space_id,
            &user_id,
            generation,
            expected_power_levels_revision.as_deref(),
            expected_power_level,
            power_level,
            confirmed,
        )
        .is_err()
    {
        return Vec::new();
    }

    state.space_members.operation = SpaceMembersOperationState::UpdatingRole {
        request_id,
        space_id,
        user_id,
        generation,
        expected_power_levels_revision,
        expected_power_level,
        power_level,
        confirmed,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_role_update_settled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
    outcome: SpaceMemberRoleUpdateOutcome,
    sent_revision: Option<String>,
    projection: Option<SpaceMembersProjection>,
) -> Vec<AppEffect> {
    let SpaceMembersOperationState::UpdatingRole {
        request_id: active_request_id,
        space_id: active_space_id,
        user_id: active_user_id,
        generation: active_generation,
        expected_power_levels_revision,
        expected_power_level,
        power_level,
        confirmed: _,
    } = state.space_members.operation.clone()
    else {
        return Vec::new();
    };
    if !is_session_ready(state)
        || active_request_id != request_id
        || active_space_id != space_id
        || active_user_id != user_id
        || active_generation != generation
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
    {
        return Vec::new();
    }

    if let Some(projection) = projection {
        let mut resolved = resolve_space_members_projection(projection, &state.profile);
        if resolved.incomplete_child_room_count > 0 {
            merge_incomplete_projection(&state.space_members, &mut resolved);
        }
        sort_projection(&mut resolved);
        apply_projection(&mut state.space_members, resolved);
    }

    state.space_members.operation = match outcome {
        SpaceMemberRoleUpdateOutcome::Succeeded => SpaceMembersOperationState::Idle,
        SpaceMemberRoleUpdateOutcome::Failed(kind) => {
            SpaceMembersOperationState::RoleUpdateFailed {
                request_id,
                space_id,
                user_id,
                generation,
                expected_power_levels_revision,
                expected_power_level,
                power_level,
                sent_revision,
                kind,
            }
        }
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_invite_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
) -> Vec<AppEffect> {
    if !is_session_ready(state)
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
        || matches!(
            state.space_members.operation,
            SpaceMembersOperationState::Inviting { .. }
                | SpaceMembersOperationState::CancellingInvite { .. }
                | SpaceMembersOperationState::UpdatingRole { .. }
                | SpaceMembersOperationState::RoleUpdateFailed { .. }
        )
    {
        return Vec::new();
    }

    if state
        .space_members
        .space_joined
        .iter()
        .chain(state.space_members.space_invited.iter())
        .any(|entry| entry.user_id == user_id)
    {
        return Vec::new();
    }

    let Some(mut entry) = state
        .space_members
        .child_room_only
        .iter()
        .find(|entry| entry.user_id == user_id)
        .cloned()
    else {
        return Vec::new();
    };
    state
        .space_members
        .child_room_only
        .retain(|entry| entry.user_id != user_id);
    entry.membership = SpaceMemberMembership::SpaceInvited;
    entry.invite_pending = true;
    state.space_members.space_invited.push(entry);
    sort_entries(&mut state.space_members.space_invited);
    state.space_members.operation = SpaceMembersOperationState::Inviting {
        request_id,
        space_id,
        user_id,
        generation,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_invite_settled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
    outcome: SpaceMemberInviteOutcome,
) -> Vec<AppEffect> {
    let matches_operation = matches!(
        &state.space_members.operation,
        SpaceMembersOperationState::Inviting {
            request_id: active_request_id,
            space_id: active_space_id,
            user_id: active_user_id,
            generation: active_generation,
        } if *active_request_id == request_id
            && active_space_id == &space_id
            && active_user_id == &user_id
            && *active_generation == generation
    );
    if !is_session_ready(state)
        || !matches_operation
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
    {
        return Vec::new();
    }

    let entry = remove_entry(&mut state.space_members, &user_id)
        .unwrap_or_else(|| fallback_entry(&user_id, SpaceMemberMembership::ChildRoomOnly));
    match outcome {
        SpaceMemberInviteOutcome::Invited | SpaceMemberInviteOutcome::AlreadyInvited => {
            let mut entry = entry;
            entry.membership = SpaceMemberMembership::SpaceInvited;
            entry.invite_pending = false;
            state.space_members.space_invited.push(entry);
            sort_entries(&mut state.space_members.space_invited);
            state.space_members.operation = SpaceMembersOperationState::Idle;
        }
        SpaceMemberInviteOutcome::AlreadyJoined => {
            let mut entry = entry;
            entry.membership = SpaceMemberMembership::SpaceJoined;
            entry.invite_pending = false;
            state.space_members.space_joined.push(entry);
            sort_entries(&mut state.space_members.space_joined);
            state.space_members.operation = SpaceMembersOperationState::Idle;
        }
        SpaceMemberInviteOutcome::Failed(kind) => {
            let mut entry = entry;
            entry.membership = SpaceMemberMembership::ChildRoomOnly;
            entry.invite_pending = false;
            state.space_members.child_room_only.push(entry);
            sort_entries(&mut state.space_members.child_room_only);
            state.space_members.operation = SpaceMembersOperationState::Failed {
                request_id,
                space_id,
                user_id: Some(user_id),
                generation,
                kind,
            };
        }
        SpaceMemberInviteOutcome::Cancelled | SpaceMemberInviteOutcome::NotInvited => {
            let mut entry = entry;
            entry.membership = SpaceMemberMembership::SpaceInvited;
            entry.invite_pending = false;
            state.space_members.space_invited.push(entry);
            sort_entries(&mut state.space_members.space_invited);
            state.space_members.operation = SpaceMembersOperationState::Idle;
        }
    }
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_cancellation_requested(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
) -> Vec<AppEffect> {
    let cancellation_context_is_retryable = matches!(
        &state.space_members.operation,
        SpaceMembersOperationState::Idle
    ) || matches!(
        &state.space_members.operation,
        SpaceMembersOperationState::Failed {
            space_id: failed_space_id,
            user_id: Some(failed_user_id),
            generation: failed_generation,
            ..
        } if failed_space_id == &space_id
            && failed_user_id == &user_id
            && *failed_generation == generation
    );
    if !is_session_ready(state)
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
        || !cancellation_context_is_retryable
        || !state
            .space_members
            .space_invited
            .iter()
            .any(|entry| entry.user_id == user_id)
    {
        return Vec::new();
    }

    state.space_members.operation = SpaceMembersOperationState::CancellingInvite {
        request_id,
        space_id,
        user_id,
        generation,
    };
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn handle_cancellation_settled(
    state: &mut AppState,
    request_id: u64,
    space_id: String,
    user_id: String,
    generation: u64,
    outcome: SpaceMemberInviteOutcome,
) -> Vec<AppEffect> {
    let matches_operation = matches!(
        &state.space_members.operation,
        SpaceMembersOperationState::CancellingInvite {
            request_id: active_request_id,
            space_id: active_space_id,
            user_id: active_user_id,
            generation: active_generation,
        } if *active_request_id == request_id
            && active_space_id == &space_id
            && active_user_id == &user_id
            && *active_generation == generation
    );
    if !is_session_ready(state)
        || !matches_operation
        || state.space_members.selected_space_id.as_deref() != Some(space_id.as_str())
        || state.space_members.generation != generation
    {
        return Vec::new();
    }

    match outcome {
        SpaceMemberInviteOutcome::Cancelled => {
            state
                .space_members
                .space_invited
                .retain(|entry| entry.user_id != user_id);
            state.space_members.operation = SpaceMembersOperationState::Idle;
        }
        SpaceMemberInviteOutcome::NotInvited => {
            state.space_members.operation = SpaceMembersOperationState::Idle;
        }
        SpaceMemberInviteOutcome::Failed(kind) => {
            if !state
                .space_members
                .space_invited
                .iter()
                .any(|entry| entry.user_id == user_id)
                && !state
                    .space_members
                    .space_joined
                    .iter()
                    .any(|entry| entry.user_id == user_id)
            {
                let mut entry = state
                    .space_members
                    .child_room_only
                    .iter()
                    .find(|entry| entry.user_id == user_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        fallback_entry(&user_id, SpaceMemberMembership::SpaceInvited)
                    });
                state
                    .space_members
                    .child_room_only
                    .retain(|entry| entry.user_id != user_id);
                entry.membership = SpaceMemberMembership::SpaceInvited;
                entry.invite_pending = false;
                state.space_members.space_invited.push(entry);
                sort_entries(&mut state.space_members.space_invited);
            }
            state.space_members.operation = SpaceMembersOperationState::Failed {
                request_id,
                space_id,
                user_id: Some(user_id),
                generation,
                kind,
            };
        }
        SpaceMemberInviteOutcome::Invited
        | SpaceMemberInviteOutcome::AlreadyInvited
        | SpaceMemberInviteOutcome::AlreadyJoined => {
            state.space_members.operation = SpaceMembersOperationState::Failed {
                request_id,
                space_id,
                user_id: Some(user_id),
                generation,
                kind: crate::state::OperationFailureKind::Sdk,
            };
        }
    }
    vec![AppEffect::EmitUiEvent(UiEvent::SpaceMembersChanged)]
}

pub(crate) fn refresh_member_display_projection(state: &mut AppState) -> bool {
    refresh_space_member_display_projection(&mut state.space_members, &state.profile)
}

fn projection_matches(
    state: &AppState,
    request_id: u64,
    projection: &SpaceMembersProjection,
) -> bool {
    state.space_members.generation == projection.generation
        && state.space_members.selected_space_id.as_deref() == Some(projection.space_id.as_str())
        && matches!(
            state.space_members.operation,
            SpaceMembersOperationState::Loading {
                request_id: Some(active_request_id),
                ..
            } if active_request_id == request_id
        )
}

fn operation_matches_request(state: &AppState, request_id: u64) -> bool {
    matches!(
        state.space_members.operation,
        SpaceMembersOperationState::Loading {
            request_id: Some(active_request_id),
            ..
        }
            | SpaceMembersOperationState::Inviting {
                request_id: active_request_id,
                ..
            }
            | SpaceMembersOperationState::CancellingInvite {
                request_id: active_request_id,
                ..
            }
            | SpaceMembersOperationState::UpdatingRole {
                request_id: active_request_id,
                ..
            }
            | SpaceMembersOperationState::RoleUpdateFailed {
                request_id: active_request_id,
                ..
            } if active_request_id == request_id
    )
}

fn background_reconciliation_operation_matches(
    operation: &SpaceMembersOperationState,
    space_id: &str,
    generation: u64,
) -> bool {
    match operation {
        SpaceMembersOperationState::Idle => true,
        SpaceMembersOperationState::Inviting {
            space_id: operation_space_id,
            generation: operation_generation,
            ..
        }
        | SpaceMembersOperationState::CancellingInvite {
            space_id: operation_space_id,
            generation: operation_generation,
            ..
        }
        | SpaceMembersOperationState::Failed {
            space_id: operation_space_id,
            generation: operation_generation,
            ..
        }
        | SpaceMembersOperationState::UpdatingRole {
            space_id: operation_space_id,
            generation: operation_generation,
            ..
        }
        | SpaceMembersOperationState::RoleUpdateFailed {
            space_id: operation_space_id,
            generation: operation_generation,
            ..
        } => operation_space_id == space_id && *operation_generation == generation,
        SpaceMembersOperationState::Loading { .. } => false,
    }
}

fn pending_operation(state: &AppState) -> Option<(u64, String, String, u64)> {
    match &state.space_members.operation {
        SpaceMembersOperationState::Inviting {
            request_id,
            space_id,
            user_id,
            generation,
        } => Some((*request_id, space_id.clone(), user_id.clone(), *generation)),
        _ => None,
    }
}

fn apply_projection(state: &mut SpaceMembersState, projection: SpaceMembersProjection) {
    state.space_joined = projection.space_joined;
    state.space_invited = projection.space_invited;
    state.child_room_only = projection.child_room_only;
    state.child_room_count = projection.child_room_count;
    state.complete_child_room_count = projection.complete_child_room_count;
    state.incomplete_child_room_count = projection.incomplete_child_room_count;
    state.power_levels_revision = projection.power_levels_revision;
    state.can_edit_roles = projection.can_edit_roles;
}

fn merge_incomplete_projection(previous: &SpaceMembersState, next: &mut SpaceMembersProjection) {
    let mut observed_user_ids = next
        .space_joined
        .iter()
        .chain(next.space_invited.iter())
        .chain(next.child_room_only.iter())
        .map(|entry| entry.user_id.clone())
        .collect::<std::collections::BTreeSet<_>>();

    for entry in previous
        .space_joined
        .iter()
        .chain(previous.space_invited.iter())
        .chain(previous.child_room_only.iter())
    {
        if observed_user_ids.insert(entry.user_id.clone()) {
            match entry.membership {
                SpaceMemberMembership::SpaceJoined => next.space_joined.push(entry.clone()),
                SpaceMemberMembership::SpaceInvited => next.space_invited.push(entry.clone()),
                SpaceMemberMembership::ChildRoomOnly => next.child_room_only.push(entry.clone()),
            }
        }
    }
}

fn remove_projection_entry(projection: &mut SpaceMembersProjection, user_id: &str) {
    projection
        .space_joined
        .retain(|entry| entry.user_id != user_id);
    projection
        .space_invited
        .retain(|entry| entry.user_id != user_id);
    projection
        .child_room_only
        .retain(|entry| entry.user_id != user_id);
}

fn sort_projection(projection: &mut SpaceMembersProjection) {
    sort_entries(&mut projection.space_joined);
    sort_entries(&mut projection.space_invited);
    sort_entries(&mut projection.child_room_only);
}

fn clear_projection(state: &mut SpaceMembersState) {
    state.space_joined.clear();
    state.space_invited.clear();
    state.child_room_only.clear();
    state.child_room_count = 0;
    state.complete_child_room_count = 0;
    state.incomplete_child_room_count = 0;
    state.power_levels_revision = None;
    state.can_edit_roles = false;
}

fn remove_entry(state: &mut SpaceMembersState, user_id: &str) -> Option<SpaceMemberEntry> {
    for entries in [
        &mut state.space_joined,
        &mut state.space_invited,
        &mut state.child_room_only,
    ] {
        if let Some(index) = entries.iter().position(|entry| entry.user_id == user_id) {
            return Some(entries.remove(index));
        }
    }
    None
}

fn fallback_entry(user_id: &str, membership: SpaceMemberMembership) -> SpaceMemberEntry {
    SpaceMemberEntry {
        user_id: user_id.to_owned(),
        display_name: None,
        display_label: "Unknown user".to_owned(),
        original_display_label: "Unknown user".to_owned(),
        avatar_url: None,
        power_level: None,
        role: crate::state::RoomMemberRole::User,
        membership,
        child_room_ids: Vec::new(),
        invite_pending: false,
        role_options: Vec::new(),
    }
}
