use super::*;

struct SpaceMemberDemand {
    space_id: String,
    generation: u64,
    child_room_ids: BTreeSet<String>,
    demand_generation: u64,
}
struct SpaceMemberRefreshFence {
    request_id: RequestId,
    session_generation: u64,
    demand_generation: u64,
    refresh_generation: u64,
}

impl RoomActor {
    async fn handle_space_membership_changed(&mut self, room_ids: Option<&BTreeSet<String>>) {
        let Some(demand) = self.space_member_demand.clone() else {
            return;
        };
        if !space_members_update_affects_demand(&demand.space_id, &demand.child_room_ids, room_ids)
        {
            return;
        }
        if self.space_member_refresh_in_flight.is_some() {
            self.space_member_refresh_pending = true;
            return;
        }
        self.start_space_member_refresh(demand);
    }
    fn start_space_member_refresh(&mut self, demand: SpaceMemberDemand) {
        let Some(session) = self.session.clone() else {
            return;
        };

        self.space_member_refresh_sequence =
            self.space_member_refresh_sequence.wrapping_add(1).max(1);
        let refresh_generation = self.space_member_refresh_sequence;
        let request_id = RequestId {
            connection_id: SPACE_MEMBER_REFRESH_CONNECTION_ID,
            sequence: refresh_generation,
        };
        let session_generation = self.space_member_session_generation;
        let fence = SpaceMemberRefreshFence {
            request_id,
            session_generation,
            demand_generation: demand.demand_generation,
            refresh_generation,
        };
        self.space_member_refresh_in_flight = Some(fence);

        let room_tx = self.self_tx.clone();
        let space_id = demand.space_id.clone();
        let generation = demand.generation;
        let demand_generation = demand.demand_generation;
        let _ = executor::spawn(async move {
            let result = koushi_sdk::matrix_space_members_projection(&session, &space_id).await;
            let _ = room_tx
                .send(RoomMessage::SpaceMembersProjectionRefreshed {
                    request_id,
                    session_generation,
                    demand_generation,
                    refresh_generation,
                    space_id,
                    generation,
                    result,
                })
                .await;
        });
    }
    async fn handle_space_members_projection_refreshed(
        &mut self,
        request_id: RequestId,
        session_generation: u64,
        demand_generation: u64,
        refresh_generation: u64,
        space_id: String,
        generation: u64,
        result: Result<MatrixSpaceMembersProjection, MatrixRoomOperationError>,
    ) {
        let Some(demand) = self.space_member_demand.clone() else {
            return;
        };
        let is_current = space_member_refresh_fence_is_current(
            self.space_member_refresh_in_flight,
            SpaceMemberRefreshFence {
                request_id,
                session_generation,
                demand_generation,
                refresh_generation,
            },
            self.space_member_session_generation,
            demand.demand_generation,
            &space_id,
            generation,
            &demand.space_id,
            demand.generation,
        );
        if !is_current {
            record_space_member_refresh_event("stale_completion_ignored", false);
            return;
        }

        self.space_member_refresh_in_flight = None;
        let should_refresh_again = std::mem::take(&mut self.space_member_refresh_pending);
        match result {
            Ok(raw_projection) => {
                let profiles = user_profiles_from_space_projection(&raw_projection);
                let projection = state_space_members_projection(raw_projection.clone(), generation);
                record_core_space_members_projection_with_raw(
                    "sync_refresh",
                    generation,
                    &raw_projection,
                    &projection,
                    "success",
                );
                self.reduce_reliable(vec![
                    AppAction::SpaceMembersBackgroundProjectionReconciled {
                        request_id: request_id.sequence,
                        space_id: space_id.clone(),
                        generation,
                        projection,
                        profiles,
                    },
                ])
                .await;
                self.install_space_member_demand(
                    &space_id,
                    generation,
                    &raw_projection.child_room_ids,
                );
            }
            Err(_error) => {
                // A background lookup failure is deliberately silent at the
                // state layer: the last-known projection remains visible and
                // the next relevant sync update may retry it.
                record_core_space_members_load_failure("sync_refresh", generation);
            }
        }

        if should_refresh_again {
            if let Some(demand) = self.space_member_demand.clone() {
                self.start_space_member_refresh(demand);
            }
        }
    }
    async fn handle_invite_user_to_space(
        &self,
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    ) {
        let (outcome, reconciliation) = match &self.session {
            None => (
                SpaceMemberInviteOutcome::Failed(OperationFailureKind::Sdk),
                None,
            ),
            Some(session) => {
                match koushi_sdk::invite_user_to_room(session, &space_id, &user_id).await {
                    Ok(()) => {
                        let fallback = SpaceMemberInviteOutcome::Invited;
                        match reconcile_space_invite_outcome(
                            session,
                            &space_id,
                            &user_id,
                            generation,
                            fallback.clone(),
                        )
                        .await
                        {
                            Some(reconciliation) => {
                                (reconciliation.outcome.clone(), Some(reconciliation))
                            }
                            None => (fallback, None),
                        }
                    }
                    Err(error) => {
                        let failure_kind = operation_failure_kind(classify_room_error(&error));
                        let fallback = SpaceMemberInviteOutcome::Failed(failure_kind);
                        match reconcile_space_invite_outcome(
                            session,
                            &space_id,
                            &user_id,
                            generation,
                            fallback.clone(),
                        )
                        .await
                        {
                            Some(reconciliation) => {
                                (reconciliation.outcome.clone(), Some(reconciliation))
                            }
                            None => (fallback, None),
                        }
                    }
                }
            }
        };
        record_core_space_members_operation("invite", generation, &outcome);
        let mut actions = Vec::new();
        if let Some(reconciliation) = reconciliation {
            actions.push(AppAction::SpaceMembersProjectionReconciled {
                request_id: request_id.sequence,
                projection: reconciliation.projection,
                profiles: reconciliation.profiles,
            });
        }
        actions.push(AppAction::SpaceMemberInviteSettled {
            request_id: request_id.sequence,
            space_id,
            user_id,
            generation,
            outcome: outcome.clone(),
        });
        self.reduce_reliable(actions).await;
        self.emit(CoreEvent::Room(RoomEvent::SpaceMemberInviteSettled {
            request_id,
            generation,
            outcome,
        }));
    }
    async fn handle_cancel_space_invite(
        &self,
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    ) {
        let (outcome, reconciliation) = match &self.session {
            None => (
                SpaceMemberInviteOutcome::Failed(OperationFailureKind::Sdk),
                None,
            ),
            Some(session) => {
                let outcome =
                    match koushi_sdk::cancel_space_invite(session, &space_id, &user_id).await {
                        Ok(koushi_sdk::MatrixSpaceInviteCancellationOutcome::Cancelled) => {
                            SpaceMemberInviteOutcome::Cancelled
                        }
                        Ok(koushi_sdk::MatrixSpaceInviteCancellationOutcome::NotInvited) => {
                            SpaceMemberInviteOutcome::NotInvited
                        }
                        Err(error) => SpaceMemberInviteOutcome::Failed(operation_failure_kind(
                            classify_room_error(&error),
                        )),
                    };
                let reconciliation =
                    reconcile_space_invite_cancellation(session, &space_id, generation).await;
                (outcome, reconciliation)
            }
        };
        record_core_space_members_operation("cancel", generation, &outcome);
        let mut actions = Vec::new();
        if let Some(reconciliation) = reconciliation {
            actions.push(AppAction::SpaceMembersProjectionReconciled {
                request_id: request_id.sequence,
                projection: reconciliation.projection,
                profiles: reconciliation.profiles,
            });
        }
        actions.push(AppAction::SpaceMemberInviteCancellationSettled {
            request_id: request_id.sequence,
            space_id,
            user_id,
            generation,
            outcome: outcome.clone(),
        });
        self.reduce_reliable(actions).await;
        self.emit(CoreEvent::Room(
            RoomEvent::SpaceMemberInviteCancellationSettled {
                request_id,
                generation,
                outcome,
            },
        ));
    }
    fn reset_space_member_session(&mut self) {
        self.space_member_session_generation =
            self.space_member_session_generation.wrapping_add(1).max(1);
        self.clear_space_member_demand();
    }
    fn clear_space_member_demand(&mut self) {
        self.space_member_demand = None;
        self.space_member_demand_generation =
            self.space_member_demand_generation.wrapping_add(1).max(1);
        self.space_member_refresh_in_flight = None;
        self.space_member_refresh_pending = false;
        self.space_member_refresh_sequence = 0;
    }
    fn install_space_member_demand(
        &mut self,
        space_id: &str,
        generation: u64,
        child_room_ids: &[String],
    ) {
        self.space_member_demand_generation =
            self.space_member_demand_generation.wrapping_add(1).max(1);
        let child_room_ids = child_room_ids.iter().cloned().collect::<BTreeSet<_>>();
        let child_room_count = child_room_ids.len();
        self.space_member_demand = Some(SpaceMemberDemand {
            space_id: space_id.to_owned(),
            generation,
            child_room_ids,
            demand_generation: self.space_member_demand_generation,
        });
        record_space_member_demand_event("installed", generation, child_room_count);
    }
}

fn state_space_members_projection(
    projection: MatrixSpaceMembersProjection,
    generation: u64,
) -> SpaceMembersProjection {
    SpaceMembersProjection {
        space_id: projection.space_id,
        generation,
        space_joined: projection
            .space_joined
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::SpaceJoined))
            .collect(),
        space_invited: projection
            .space_invited
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::SpaceInvited))
            .collect(),
        child_room_only: projection
            .child_room_only
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::ChildRoomOnly))
            .collect(),
        child_room_count: projection.child_room_count,
        complete_child_room_count: projection.complete_child_room_count,
        incomplete_child_room_count: projection.incomplete_child_room_count,
    }
}
fn state_space_member_entry(
    entry: MatrixSpaceMemberEntry,
    membership: SpaceMemberMembership,
) -> SpaceMemberEntry {
    let display_name = entry
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let display_label = display_name
        .clone()
        .unwrap_or_else(|| "Unknown user".to_owned());
    SpaceMemberEntry {
        user_id: entry.user_id,
        display_name,
        display_label: display_label.clone(),
        original_display_label: display_label,
        avatar_url: entry.avatar_url,
        power_level: entry.power_level,
        role: match entry.role {
            koushi_sdk::MatrixRoomMemberRole::Creator => koushi_state::RoomMemberRole::Creator,
            koushi_sdk::MatrixRoomMemberRole::Administrator => {
                koushi_state::RoomMemberRole::Administrator
            }
            koushi_sdk::MatrixRoomMemberRole::Moderator => koushi_state::RoomMemberRole::Moderator,
            koushi_sdk::MatrixRoomMemberRole::User => koushi_state::RoomMemberRole::User,
        },
        membership,
        child_room_ids: entry.child_room_ids,
        invite_pending: false,
    }
}
fn user_profiles_from_space_projection(
    projection: &MatrixSpaceMembersProjection,
) -> Vec<UserProfile> {
    let mut profiles = BTreeMap::<String, UserProfile>::new();
    for entry in projection
        .space_joined
        .iter()
        .chain(projection.space_invited.iter())
        .chain(projection.child_room_only.iter())
        .chain(projection.child_room_profiles.iter())
    {
        let has_display_name = entry
            .display_name
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if !has_display_name && entry.avatar_url.is_none() {
            continue;
        }
        let next = UserProfile {
            user_id: entry.user_id.clone(),
            display_name: entry
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            display_label: entry
                .display_name
                .clone()
                .unwrap_or_else(|| "Unknown user".to_owned()),
            original_display_label: entry
                .display_name
                .clone()
                .unwrap_or_else(|| "Unknown user".to_owned()),
            mention_search_terms: Vec::new(),
            avatar: avatar_from_mxc_uri(entry.avatar_url.as_deref()),
        };
        profiles
            .entry(entry.user_id.clone())
            .and_modify(|existing| {
                if existing.display_name.is_none() && next.display_name.is_some() {
                    existing.display_name = next.display_name.clone();
                    existing.display_label = next.display_label.clone();
                    existing.original_display_label = next.original_display_label.clone();
                }
                if existing.avatar.is_none() && next.avatar.is_some() {
                    existing.avatar = next.avatar.clone();
                }
            })
            .or_insert(next);
    }
    profiles.into_values().collect()
}
async fn reconcile_space_invite_outcome(
    session: &MatrixClientSession,
    space_id: &str,
    user_id: &str,
    generation: u64,
    fallback: SpaceMemberInviteOutcome,
) -> Option<SpaceInviteReconciliation> {
    let Ok(raw_projection) = koushi_sdk::matrix_space_members_projection(session, space_id).await
    else {
        return None;
    };
    let outcome = if raw_projection
        .space_joined
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        SpaceMemberInviteOutcome::AlreadyJoined
    } else if raw_projection
        .space_invited
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        SpaceMemberInviteOutcome::AlreadyInvited
    } else {
        fallback
    };
    let profiles = user_profiles_from_space_projection(&raw_projection);
    let projection = state_space_members_projection(raw_projection, generation);
    Some(SpaceInviteReconciliation {
        projection,
        profiles,
        outcome,
    })
}
async fn reconcile_space_invite_cancellation(
    session: &MatrixClientSession,
    space_id: &str,
    generation: u64,
) -> Option<SpaceInviteProjectionReconciliation> {
    let Ok(raw_projection) = koushi_sdk::matrix_space_members_projection(session, space_id).await
    else {
        return None;
    };
    let profiles = user_profiles_from_space_projection(&raw_projection);
    let projection = state_space_members_projection(raw_projection, generation);
    Some(SpaceInviteProjectionReconciliation {
        projection,
        profiles,
    })
}
fn record_core_space_members_projection(
    trigger: &'static str,
    generation: u64,
    projection: &SpaceMembersProjection,
    outcome: &'static str,
) {
    record_core_space_members_projection_with_metrics(
        trigger, generation, projection, None, outcome,
    );
}
fn record_core_space_members_projection_with_raw(
    trigger: &'static str,
    generation: u64,
    raw_projection: &MatrixSpaceMembersProjection,
    projection: &SpaceMembersProjection,
    outcome: &'static str,
) {
    record_core_space_members_projection_with_metrics(
        trigger,
        generation,
        projection,
        Some(raw_projection),
        outcome,
    );
}
fn record_core_space_members_projection_with_metrics(
    trigger: &'static str,
    generation: u64,
    projection: &SpaceMembersProjection,
    raw_projection: Option<&MatrixSpaceMembersProjection>,
    outcome: &'static str,
) {
    let output_count = projection.space_joined.len()
        + projection.space_invited.len()
        + projection.child_room_only.len();
    let mut event = DiagnosticEvent::new(
        DiagnosticLevel::Debug,
        "core.space_members_projection",
        "projection",
    )
    .field(DiagnosticField::token("trigger", trigger))
    .field(DiagnosticField::count("generation", generation))
    .field(DiagnosticField::count(
        "space_joined_count",
        projection.space_joined.len() as u64,
    ))
    .field(DiagnosticField::count(
        "space_invited_count",
        projection.space_invited.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_only_count",
        projection.child_room_only.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_count",
        projection.child_room_count as u64,
    ))
    .field(DiagnosticField::count(
        "complete_child_room_count",
        projection.complete_child_room_count as u64,
    ))
    .field(DiagnosticField::count(
        "incomplete_child_room_count",
        projection.incomplete_child_room_count as u64,
    ))
    .field(DiagnosticField::count("output_count", output_count as u64))
    .field(DiagnosticField::count(
        "space_joined_output_count",
        projection.space_joined.len() as u64,
    ))
    .field(DiagnosticField::count(
        "space_invited_output_count",
        projection.space_invited.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_only_output_count",
        projection.child_room_only.len() as u64,
    ))
    .field(DiagnosticField::boolean(
        "incomplete",
        projection.incomplete_child_room_count > 0,
    ))
    .field(DiagnosticField::token("outcome", outcome));

    if let Some(raw_projection) = raw_projection {
        let input_count = raw_projection.space_joined_input_count
            + raw_projection.space_invited_input_count
            + raw_projection.child_join_input_count;
        event = event
            .field(DiagnosticField::count("input_count", input_count as u64))
            .field(DiagnosticField::count(
                "space_joined_input_count",
                raw_projection.space_joined_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "space_invited_input_count",
                raw_projection.space_invited_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "child_join_input_count",
                raw_projection.child_join_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "deduplicated_count",
                raw_projection.duplicate_child_membership_count as u64,
            ))
            .field(DiagnosticField::count(
                "child_join_union_count",
                raw_projection.child_join_union_count as u64,
            ))
            .field(DiagnosticField::token("input_tracking_status", "tracked"));
    } else {
        event = event
            .field(DiagnosticField::token("input_count", "not_tracked"))
            .field(DiagnosticField::token("deduplicated_count", "not_tracked"))
            .field(DiagnosticField::token(
                "input_tracking_status",
                "not_tracked",
            ));
    }

    record(event.field(DiagnosticField::token("freshness_status", "not_tracked")));
}
fn space_members_update_affects_demand(
    space_id: &str,
    child_room_ids: &BTreeSet<String>,
    updated_room_ids: Option<&BTreeSet<String>>,
) -> bool {
    updated_room_ids.map_or(true, |updated| {
        updated.contains(space_id)
            || updated
                .iter()
                .any(|room_id| child_room_ids.contains(room_id))
    })
}
fn should_clear_space_member_demand(
    demand: Option<&SpaceMemberDemand>,
    space_id: &str,
    generation: u64,
) -> bool {
    demand.is_some_and(|demand| demand.space_id != space_id || demand.generation != generation)
}
fn space_members_refresh_is_current(
    result_space_id: &str,
    result_generation: u64,
    demanded_space_id: &str,
    demanded_generation: u64,
) -> bool {
    result_space_id == demanded_space_id && result_generation == demanded_generation
}
fn space_member_refresh_fence_is_current(
    active_fence: Option<SpaceMemberRefreshFence>,
    expected_fence: SpaceMemberRefreshFence,
    current_session_generation: u64,
    current_demand_generation: u64,
    result_space_id: &str,
    result_generation: u64,
    demanded_space_id: &str,
    demanded_generation: u64,
) -> bool {
    active_fence == Some(expected_fence)
        && current_session_generation == expected_fence.session_generation
        && current_demand_generation == expected_fence.demand_generation
        && space_members_refresh_is_current(
            result_space_id,
            result_generation,
            demanded_space_id,
            demanded_generation,
        )
}
fn record_space_member_demand_event(
    outcome: &'static str,
    generation: u64,
    child_room_count: usize,
) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "demand",
        )
        .field(DiagnosticField::token("outcome", outcome))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::count(
            "child_room_count",
            child_room_count as u64,
        )),
    );
}
fn record_space_member_refresh_event(outcome: &'static str, applied: bool) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "background_refresh",
        )
        .field(DiagnosticField::token("outcome", outcome))
        .field(DiagnosticField::boolean("applied", applied)),
    );
}
fn record_core_space_members_load_failure(trigger: &'static str, generation: u64) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "projection",
        )
        .field(DiagnosticField::token("trigger", trigger))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::token("outcome", "lookup_failed"))
        .field(DiagnosticField::token(
            "space_joined_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token(
            "space_invited_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token(
            "child_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token("input_count", "counts_unavailable"))
        .field(DiagnosticField::token("output_count", "counts_unavailable"))
        .field(DiagnosticField::token("freshness_status", "not_tracked")),
    );
}
fn record_core_space_members_operation(
    trigger: &'static str,
    generation: u64,
    outcome: &SpaceMemberInviteOutcome,
) {
    let outcome_token = match outcome {
        SpaceMemberInviteOutcome::Invited => "invited",
        SpaceMemberInviteOutcome::AlreadyInvited => "already_invited",
        SpaceMemberInviteOutcome::AlreadyJoined => "already_joined",
        SpaceMemberInviteOutcome::Cancelled => "cancelled",
        SpaceMemberInviteOutcome::NotInvited => "not_invited",
        SpaceMemberInviteOutcome::Failed(_) => "failed",
    };
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "invite_settled",
        )
        .field(DiagnosticField::token("trigger", trigger))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::token("outcome", outcome_token)),
    );
}

#[cfg(test)]
mod tests {

    #[test]
    fn space_member_sync_updates_are_relevant_only_for_the_demanded_scope() {
        let child_room_ids = BTreeSet::from([
            "!child-a:example.invalid".to_owned(),
            "!child-b:example.invalid".to_owned(),
        ]);
        let space_update = BTreeSet::from(["!space:example.invalid".to_owned()]);
        let child_update = BTreeSet::from(["!child-a:example.invalid".to_owned()]);
        let unrelated_update = BTreeSet::from(["!unrelated:example.invalid".to_owned()]);

        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&space_update),
        ));
        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&child_update),
        ));
        assert!(!space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&unrelated_update),
        ));
        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            None,
        ));
        assert!(!space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&BTreeSet::new()),
        ));
    }

    #[test]
    fn space_member_reload_clears_only_a_different_demand() {
        let demand = SpaceMemberDemand {
            space_id: "!space:example.invalid".to_owned(),
            generation: 4,
            child_room_ids: BTreeSet::new(),
            demand_generation: 1,
        };

        assert!(!should_clear_space_member_demand(
            Some(&demand),
            "!space:example.invalid",
            4,
        ));
        assert!(should_clear_space_member_demand(
            Some(&demand),
            "!other-space:example.invalid",
            4,
        ));
        assert!(should_clear_space_member_demand(
            Some(&demand),
            "!space:example.invalid",
            5,
        ));
        assert!(!should_clear_space_member_demand(
            None,
            "!space:example.invalid",
            4,
        ));
    }

    #[test]
    fn space_members_projection_load_path_emits_non_empty_child_profile_observations() {
        let raw = MatrixSpaceMembersProjection {
            space_id: "!space:example.invalid".to_owned(),
            child_room_ids: vec!["!child:example.invalid".to_owned()],
            space_joined: Vec::new(),
            space_invited: Vec::new(),
            child_room_only: vec![MatrixSpaceMemberEntry {
                user_id: "@child:example.invalid".to_owned(),
                display_name: Some("Child room profile".to_owned()),
                avatar_url: None,
                power_level: Some(0),
                role: MatrixRoomMemberRole::User,
                child_room_ids: vec!["!child:example.invalid".to_owned()],
            }],
            child_room_profiles: Vec::new(),
            space_joined_input_count: 0,
            space_invited_input_count: 0,
            child_join_input_count: 1,
            child_join_union_count: 1,
            duplicate_child_membership_count: 0,
            child_room_count: 1,
            complete_child_room_count: 1,
            incomplete_child_room_count: 0,
        };

        let profiles = user_profiles_from_space_projection(&raw);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].user_id, "@child:example.invalid");
        assert_eq!(
            profiles[0].display_name.as_deref(),
            Some("Child room profile")
        );

        let projection = state_space_members_projection(raw, 4);
        assert_eq!(
            projection.child_room_only[0].display_name.as_deref(),
            Some("Child room profile")
        );
    }

    #[test]
    fn space_member_load_failure_does_not_construct_an_empty_projection() {
        let source = include_str!("room.rs");
        let failure_path = source
            .split("async fn handle_load_space_members")
            .nth(1)
            .expect("Space load error branch exists")
            .split("async fn handle_invite_user_to_space")
            .next()
            .expect("Space load handler boundary exists")
            .split("Err(error) =>")
            .nth(1)
            .expect("Space load error branch exists")
            .split("self.reduce_reliable")
            .next()
            .expect("Space load failure must reduce a structured failure action");

        assert!(
            !failure_path.contains("SpaceMembersProjection {"),
            "a failed Space lookup must not be represented by an empty projection"
        );
        assert!(
            failure_path.contains("record_core_space_members_load_failure"),
            "core failure diagnostics must preserve unavailable-count semantics"
        );
    }

    #[test]
    fn background_space_member_lookup_failure_preserves_state_and_only_records_diagnostic() {
        let source = include_str!("room.rs");
        let failure_path = source
            .split("async fn handle_space_members_projection_refreshed")
            .nth(1)
            .expect("background refresh handler exists")
            .split("async fn handle_invite_user_to_space")
            .next()
            .expect("background refresh handler boundary exists")
            .split("Err(_error) =>")
            .nth(1)
            .expect("background lookup failure branch exists");

        assert!(failure_path.contains("record_core_space_members_load_failure"));
        assert!(!failure_path.contains("SpaceMembersBackgroundProjectionReconciled"));
        assert!(!failure_path.contains("SpaceMembersLoadFailed"));
    }

    #[test]
    fn cancel_space_invite_reconciles_a_fresh_projection_before_settling() {
        let source = include_str!("room.rs");
        let handler = source
            .split("async fn handle_cancel_space_invite")
            .nth(1)
            .expect("Space invite cancellation handler exists")
            .split("async fn handle_invite_targets")
            .next()
            .expect("Space invite cancellation handler boundary exists");
        let sdk_call = handler
            .find("koushi_sdk::cancel_space_invite")
            .expect("core must call the SDK cancellation helper");
        let reconcile = handler
            .find("reconcile_space_invite_cancellation")
            .expect("core must request a fresh Space projection");
        let settlement = handler
            .find("SpaceMemberInviteCancellationSettled")
            .expect("core must settle the cancellation action");
        assert!(sdk_call < reconcile);
        assert!(reconcile < settlement);

        let reconciliation = source
            .split("async fn reconcile_space_invite_cancellation")
            .nth(1)
            .expect("cancellation reconciliation helper exists")
            .split("fn record_core_space_members_projection")
            .next()
            .expect("cancellation reconciliation helper boundary exists");
        assert!(reconciliation.contains("koushi_sdk::matrix_space_members_projection"));
    }

    #[test]
    fn failed_space_member_diagnostics_do_not_fabricate_member_counts() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        record_core_space_members_load_failure("sync_refresh", 7);
        let record = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .skip(before)
            .find(|record| {
                record.event.source == "core.space_members_projection"
                    && record.event.fields.iter().any(|field| {
                        field.key == "outcome"
                            && field.value
                                == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
                    })
            })
            .expect("Space load failure diagnostic");

        assert!(record.event.fields.iter().any(|field| {
            field.key == "outcome"
                && field.value == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
        }));
        for field in &record.event.fields {
            if matches!(
                field.key,
                "space_joined_count"
                    | "space_invited_count"
                    | "child_room_count"
                    | "child_room_only_count"
                    | "input_count"
                    | "output_count"
            ) {
                assert_ne!(
                    field.value,
                    koushi_diagnostics::DiagnosticValue::Count(0),
                    "failed Space diagnostics must not report member counts as zero"
                );
            }
        }
    }

    #[test]
    fn core_space_members_diagnostics_are_private_data_free() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let projection = SpaceMembersProjection {
            space_id: "!private:example.invalid".to_owned(),
            generation: 4,
            space_joined: vec![SpaceMemberEntry {
                user_id: "@alice:example.invalid".to_owned(),
                display_name: Some("Alice private".to_owned()),
                display_label: "Alice private".to_owned(),
                original_display_label: "Alice private".to_owned(),
                avatar_url: Some("mxc://example.invalid/avatar".to_owned()),
                power_level: Some(100),
                role: RoomMemberRole::Administrator,
                membership: SpaceMemberMembership::SpaceJoined,
                child_room_ids: Vec::new(),
                invite_pending: false,
            }],
            space_invited: Vec::new(),
            child_room_only: Vec::new(),
            child_room_count: 0,
            complete_child_room_count: 0,
            incomplete_child_room_count: 0,
        };
        record_core_space_members_projection("load", 4, &projection, "success");
        record_core_profile_resolution(&projection);

        let snapshot = koushi_diagnostics::test_support::detail_snapshot();
        let encoded = serde_json::to_string(&snapshot).expect("diagnostics serialize");
        assert!(!encoded.contains("@alice:example.invalid"));
        assert!(!encoded.contains("Alice private"));
        assert!(!encoded.contains("mxc://example.invalid/avatar"));
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.space_members_projection")
        );
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.profile_resolution")
        );
    }
}

    #[test]
    fn background_space_member_lookup_failure_preserves_state_and_only_records_diagnostic() {
        let source = include_str!("room.rs");
        let failure_path = source
            .split("async fn handle_space_members_projection_refreshed")
            .nth(1)
            .expect("background refresh handler exists")
            .split("async fn handle_invite_user_to_space")
            .next()
            .expect("background refresh handler boundary exists")
            .split("Err(_error) =>")
            .nth(1)
            .expect("background lookup failure branch exists");

        assert!(failure_path.contains("record_core_space_members_load_failure"));
        assert!(!failure_path.contains("SpaceMembersBackgroundProjectionReconciled"));
        assert!(!failure_path.contains("SpaceMembersLoadFailed"));
    }

    #[test]
    fn cancel_space_invite_reconciles_a_fresh_projection_before_settling() {
        let source = include_str!("room.rs");
        let handler = source
            .split("async fn handle_cancel_space_invite")
            .nth(1)
            .expect("Space invite cancellation handler exists")
            .split("async fn handle_invite_targets")
            .next()
            .expect("Space invite cancellation handler boundary exists");
        let sdk_call = handler
            .find("koushi_sdk::cancel_space_invite")
            .expect("core must call the SDK cancellation helper");
        let reconcile = handler
            .find("reconcile_space_invite_cancellation")
            .expect("core must request a fresh Space projection");
        let settlement = handler
            .find("SpaceMemberInviteCancellationSettled")
            .expect("core must settle the cancellation action");
        assert!(sdk_call < reconcile);
        assert!(reconcile < settlement);

        let reconciliation = source
            .split("async fn reconcile_space_invite_cancellation")
            .nth(1)
            .expect("cancellation reconciliation helper exists")
            .split("fn record_core_space_members_projection")
            .next()
            .expect("cancellation reconciliation helper boundary exists");
        assert!(reconciliation.contains("koushi_sdk::matrix_space_members_projection"));
    }

    #[test]
    fn failed_space_member_diagnostics_do_not_fabricate_member_counts() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        record_core_space_members_load_failure("sync_refresh", 7);
        let record = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .skip(before)
            .find(|record| {
                record.event.source == "core.space_members_projection"
                    && record.event.fields.iter().any(|field| {
                        field.key == "outcome"
                            && field.value
                                == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
                    })
            })
            .expect("Space load failure diagnostic");

        assert!(record.event.fields.iter().any(|field| {
            field.key == "outcome"
                && field.value == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
        }));
        for field in &record.event.fields {
            if matches!(
                field.key,
                "space_joined_count"
                    | "space_invited_count"
                    | "child_room_count"
                    | "child_room_only_count"
                    | "input_count"
                    | "output_count"
            ) {
                assert_ne!(
                    field.value,
                    koushi_diagnostics::DiagnosticValue::Count(0),
                    "failed Space diagnostics must not report member counts as zero"
                );
            }
        }
    }

    #[test]
    fn core_space_members_diagnostics_are_private_data_free() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let projection = SpaceMembersProjection {
            space_id: "!private:example.invalid".to_owned(),
            generation: 4,
            space_joined: vec![SpaceMemberEntry {
                user_id: "@alice:example.invalid".to_owned(),
                display_name: Some("Alice private".to_owned()),
                display_label: "Alice private".to_owned(),
                original_display_label: "Alice private".to_owned(),
                avatar_url: Some("mxc://example.invalid/avatar".to_owned()),
                power_level: Some(100),
                role: RoomMemberRole::Administrator,
                membership: SpaceMemberMembership::SpaceJoined,
                child_room_ids: Vec::new(),
                invite_pending: false,
            }],
            space_invited: Vec::new(),
            child_room_only: Vec::new(),
            child_room_count: 0,
            complete_child_room_count: 0,
            incomplete_child_room_count: 0,
        };
        record_core_space_members_projection("load", 4, &projection, "success");
        record_core_profile_resolution(&projection);

        let snapshot = koushi_diagnostics::test_support::detail_snapshot();
        let encoded = serde_json::to_string(&snapshot).expect("diagnostics serialize");
        assert!(!encoded.contains("@alice:example.invalid"));
        assert!(!encoded.contains("Alice private"));
        assert!(!encoded.contains("mxc://example.invalid/avatar"));
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.space_members_projection")
        );
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.profile_resolution")
        );
    }
}
