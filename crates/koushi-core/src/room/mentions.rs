#[derive(Clone)]
struct MentionDemand {
    request_id: RequestId,
    generation: u64,
    query: String,
}

impl RoomActor {
    async fn handle_mention_members_refreshed(
        &mut self,
        room_id: String,
        session_generation: u64,
        refresh_generation: u64,
        result: Result<MatrixJoinedMemberSnapshot, MatrixRoomOperationError>,
    ) {
        if session_generation != self.mention_session_generation
            || self.mention_refresh_generations.get(&room_id) != Some(&refresh_generation)
            || self.session.is_none()
        {
            record_mention_candidate_event(
                "member_refresh_settled",
                MentionSurface::Main,
                MentionCandidatesCompleteness::Failed,
                0,
                "stale",
            );
            return;
        }
        self.mention_refresh_generations.remove(&room_id);
        let demanded_surfaces = self
            .mention_demands
            .keys()
            .filter_map(|(demanded_room_id, surface)| {
                (demanded_room_id == &room_id).then_some(*surface)
            })
            .collect::<Vec<_>>();
        match result {
            Ok(snapshot) => {
                self.mention_member_snapshots
                    .insert(room_id.clone(), snapshot.clone());
                for surface in demanded_surfaces {
                    self.publish_mention_projection(&room_id, surface, &snapshot)
                        .await;
                }
            }
            Err(error) => {
                let kind = mention_failure_kind(&error);
                for surface in demanded_surfaces {
                    self.publish_mention_failure(&room_id, surface, kind).await;
                }
            }
        }
    }

    fn start_mention_member_refresh(&mut self, session: Arc<MatrixClientSession>, room_id: String) {
        if self.mention_refresh_generations.contains_key(&room_id) {
            return;
        }
        self.mention_refresh_sequence = self.mention_refresh_sequence.wrapping_add(1).max(1);
        let refresh_generation = self.mention_refresh_sequence;
        self.mention_refresh_generations
            .insert(room_id.clone(), refresh_generation);
        let session_generation = self.mention_session_generation;
        let self_tx = self.self_tx.clone();
        record_mention_candidate_event(
            "member_refresh_started",
            MentionSurface::Main,
            MentionCandidatesCompleteness::Loading,
            0,
            "started",
        );
        executor::spawn(async move {
            let result = session.refresh_joined_member_snapshot(&room_id).await;
            let _ = self_tx
                .send(RoomMessage::MentionMembersRefreshed {
                    room_id,
                    session_generation,
                    refresh_generation,
                    result,
                })
                .await;
        });
    }

    async fn handle_mention_membership_changed(&mut self, room_ids: Option<BTreeSet<String>>) {
        self.handle_space_membership_changed(room_ids.as_ref())
            .await;

        let demanded_rooms = self
            .mention_demands
            .keys()
            .filter_map(|(room_id, _)| {
                room_ids
                    .as_ref()
                    .is_none_or(|updated| updated.contains(room_id))
                    .then_some(room_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let Some(session) = self.session.clone() else {
            return;
        };
        for room_id in demanded_rooms {
            self.mention_member_snapshots.remove(&room_id);
            // An update supersedes an in-flight refresh for this room. Its
            // completion is fenced by the replacement refresh generation.
            self.mention_refresh_generations.remove(&room_id);
            match session.joined_member_snapshot_no_sync(&room_id).await {
                Ok(snapshot) => {
                    self.mention_member_snapshots
                        .insert(room_id.clone(), snapshot.clone());
                    let surfaces = self
                        .mention_demands
                        .keys()
                        .filter_map(|(demanded_room_id, surface)| {
                            (demanded_room_id == &room_id).then_some(*surface)
                        })
                        .collect::<Vec<_>>();
                    for surface in surfaces {
                        self.publish_mention_projection(&room_id, surface, &snapshot)
                            .await;
                    }
                    if !snapshot.complete {
                        self.start_mention_member_refresh(session.clone(), room_id);
                    }
                }
                Err(error) => {
                    let kind = mention_failure_kind(&error);
                    let surfaces = self
                        .mention_demands
                        .keys()
                        .filter_map(|(demanded_room_id, surface)| {
                            (demanded_room_id == &room_id).then_some(*surface)
                        })
                        .collect::<Vec<_>>();
                    for surface in surfaces {
                        self.publish_mention_failure(&room_id, surface, kind).await;
                    }
                }
            }
        }
    }

    async fn handle_mention_local_aliases_updated(&mut self, aliases: BTreeMap<String, String>) {
        self.mention_local_aliases = aliases;
        let demanded_targets = self.mention_demands.keys().cloned().collect::<Vec<_>>();
        for (room_id, surface) in demanded_targets {
            if let Some(snapshot) = self.mention_member_snapshots.get(&room_id).cloned() {
                self.publish_mention_projection(&room_id, surface, &snapshot)
                    .await;
            }
        }
    }

    async fn publish_mention_projection(
        &self,
        room_id: &str,
        surface: MentionSurface,
        snapshot: &MatrixJoinedMemberSnapshot,
    ) {
        let Some(demand) = self
            .mention_demands
            .get(&(room_id.to_owned(), surface))
            .cloned()
        else {
            return;
        };
        let permission = match snapshot.room_mention_allowed {
            Some(true) => RoomMentionPermission::Allowed,
            Some(false) => RoomMentionPermission::Denied,
            None => RoomMentionPermission::Unknown,
        };
        let projection = project_candidates(
            &demand.query,
            snapshot
                .members
                .iter()
                .map(|member| MentionMemberInput {
                    user_id: member.user_id.clone(),
                    room_display_name: member.display_name.clone(),
                    profile_display_name: None,
                    local_alias: self.mention_local_aliases.get(&member.user_id).cloned(),
                    avatar_mxc_uri: member.avatar_url.clone(),
                })
                .collect(),
            permission,
        );
        let room_mention_allowed = if projection.room_mention_included {
            RoomMentionPermission::Allowed
        } else if permission == RoomMentionPermission::Unknown {
            RoomMentionPermission::Unknown
        } else {
            RoomMentionPermission::Denied
        };
        let candidate_count = projection.candidates.len();
        self.reduce_reliable(vec![AppAction::MentionCandidatesProjected {
            request_id: demand.request_id.sequence,
            generation: demand.generation,
            room_id: room_id.to_owned(),
            surface,
            query: demand.query,
            completeness: if snapshot.complete {
                MentionCandidatesCompleteness::Complete
            } else {
                MentionCandidatesCompleteness::Partial
            },
            candidates: projection.candidates,
            room_mention_allowed,
        }])
        .await;
        record_mention_candidate_event(
            "projected",
            surface,
            if snapshot.complete {
                MentionCandidatesCompleteness::Complete
            } else {
                MentionCandidatesCompleteness::Partial
            },
            candidate_count,
            "success",
        );
    }

    async fn publish_mention_failure(
        &self,
        room_id: &str,
        surface: MentionSurface,
        kind: MentionCandidatesFailureKind,
    ) {
        let Some(demand) = self
            .mention_demands
            .get(&(room_id.to_owned(), surface))
            .cloned()
        else {
            return;
        };
        self.reduce_reliable(vec![AppAction::MentionCandidatesFailed {
            request_id: demand.request_id.sequence,
            generation: demand.generation,
            room_id: room_id.to_owned(),
            surface,
            query: demand.query,
            kind,
        }])
        .await;
        record_mention_candidate_event(
            "projected",
            surface,
            MentionCandidatesCompleteness::Failed,
            0,
            match kind {
                MentionCandidatesFailureKind::Network => "network",
                MentionCandidatesFailureKind::Forbidden => "forbidden",
                MentionCandidatesFailureKind::Sdk => "sdk",
            },
        );
    }
}
