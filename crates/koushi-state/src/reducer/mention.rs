use crate::{
    AppEffect, AppState, MentionCandidate, MentionCandidatesCompleteness,
    MentionCandidatesFailureKind, MentionCandidatesTarget, MentionSurface, RoomMentionPermission,
    UiEvent,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_demanded(
    state: &mut AppState,
    request_id: u64,
    generation: u64,
    room_id: String,
    surface: MentionSurface,
    query: String,
) -> Vec<AppEffect> {
    if state
        .mention_candidates
        .target(&room_id, surface)
        .is_some_and(|target| generation <= target.generation)
    {
        return Vec::new();
    }

    state
        .mention_candidates
        .replace_target(MentionCandidatesTarget {
            room_id,
            generation,
            request_id,
            query,
            surface,
            completeness: MentionCandidatesCompleteness::Loading,
            candidates: Vec::new(),
            room_mention_allowed: RoomMentionPermission::Unknown,
            failure_kind: None,
        });
    changed()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_projected(
    state: &mut AppState,
    request_id: u64,
    generation: u64,
    room_id: String,
    surface: MentionSurface,
    query: String,
    completeness: MentionCandidatesCompleteness,
    candidates: Vec<MentionCandidate>,
    room_mention_allowed: RoomMentionPermission,
) -> Vec<AppEffect> {
    let Some(target) = matching_target(state, request_id, generation, &room_id, surface, &query)
    else {
        return Vec::new();
    };
    if completeness == MentionCandidatesCompleteness::Failed {
        return Vec::new();
    }
    target.completeness = completeness;
    target.candidates = candidates;
    target.room_mention_allowed = room_mention_allowed;
    target.failure_kind = None;
    changed()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_failed(
    state: &mut AppState,
    request_id: u64,
    generation: u64,
    room_id: String,
    surface: MentionSurface,
    query: String,
    kind: MentionCandidatesFailureKind,
) -> Vec<AppEffect> {
    let Some(target) = matching_target(state, request_id, generation, &room_id, surface, &query)
    else {
        return Vec::new();
    };
    target.completeness = MentionCandidatesCompleteness::Failed;
    target.failure_kind = Some(kind);
    changed()
}

fn matching_target<'a>(
    state: &'a mut AppState,
    request_id: u64,
    generation: u64,
    room_id: &str,
    surface: MentionSurface,
    query: &str,
) -> Option<&'a mut MentionCandidatesTarget> {
    let target = state.mention_candidates.target_mut(room_id, surface)?;
    (target.request_id == request_id && target.generation == generation && target.query == query)
        .then_some(target)
}

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(UiEvent::MentionCandidatesChanged)]
}
