use super::*;

    async fn handle_load_room_settings(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
                    room_id,
                    settings: settings.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomSettingsLoaded {
                    request_id,
                    settings,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }


    async fn handle_update_room_setting(
        &self,
        request_id: RequestId,
        room_id: String,
        change: RoomSettingChange,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !settings.permissions.can_edit_settings {
            self.reduce_reliable(vec![AppAction::RoomSettingUpdateRequested {
                request_id: request_id.sequence,
                room_id,
                change,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomSettingUpdateRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            change: change.clone(),
        }])
        .await;

        match koushi_sdk::update_room_setting(session, &room_id, room_setting_change_to_sdk(change))
            .await
        {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![AppAction::RoomSettingUpdateSucceeded {
                    request_id: request_id.sequence,
                    room_id,
                    settings: settings.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomSettingUpdated {
                    request_id,
                    settings,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomSettingUpdateFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }


    async fn handle_moderate_room_member(
        &self,
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        action: RoomModerationAction,
        reason: Option<String>,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !room_moderation_allowed(&settings.permissions, action) {
            self.reduce_reliable(vec![AppAction::RoomModerationRequested {
                request_id: request_id.sequence,
                room_id,
                target_user_id,
                action,
                reason,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomModerationRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            target_user_id: target_user_id.clone(),
            action,
            reason: reason.clone(),
        }])
        .await;

        match koushi_sdk::moderate_room_member(
            session,
            &room_id,
            &target_user_id,
            room_moderation_action_to_sdk(action),
            reason.as_deref(),
        )
        .await
        {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::RoomModerationSucceeded {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                    target_user_id: target_user_id.clone(),
                    action,
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomMemberModerated {
                    request_id,
                    room_id,
                    target_user_id,
                    action,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomModerationFailed {
                    request_id: request_id.sequence,
                    room_id,
                    target_user_id,
                    action,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }


    async fn handle_update_room_member_role(
        &self,
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        power_level: i64,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !settings.permissions.can_edit_roles {
            self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateRequested {
                request_id: request_id.sequence,
                room_id,
                target_user_id,
                power_level,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            target_user_id: target_user_id.clone(),
            power_level,
        }])
        .await;

        match koushi_sdk::update_room_member_power_level(
            session,
            &room_id,
            &target_user_id,
            power_level,
        )
        .await
        {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![
                    AppAction::RoomSettingsSnapshotLoaded {
                        room_id: room_id.clone(),
                        settings,
                    },
                    AppAction::RoomMemberRoleUpdateRequested {
                        request_id: request_id.sequence,
                        room_id: room_id.clone(),
                        target_user_id: target_user_id.clone(),
                        power_level,
                    },
                    AppAction::RoomMemberRoleUpdateSucceeded {
                        request_id: request_id.sequence,
                        room_id: room_id.clone(),
                        target_user_id: target_user_id.clone(),
                        power_level,
                    },
                ])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomMemberRoleUpdated {
                    request_id,
                    room_id,
                    target_user_id,
                    power_level,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateFailed {
                    request_id: request_id.sequence,
                    room_id,
                    target_user_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }


fn room_settings_snapshot_from_sdk(settings: MatrixRoomSettingsSnapshot) -> RoomSettingsSnapshot {
    let share_link = koushi_state::room_settings_share_link(
        &settings.room_id,
        settings.canonical_alias.as_deref(),
        &settings.alternate_aliases,
    );
    RoomSettingsSnapshot {
        room_id: settings.room_id,
        name: settings.name,
        topic: settings.topic,
        avatar_url: settings.avatar_url,
        canonical_alias: settings.canonical_alias,
        alternate_aliases: settings.alternate_aliases,
        share_link,
        join_rule: room_join_rule_from_sdk(settings.join_rule),
        history_visibility: room_history_visibility_from_sdk(settings.history_visibility),
        permissions: room_permission_facts_from_sdk(settings.permissions),
        members: settings
            .members
            .into_iter()
            .map(room_member_summary_from_sdk)
            .collect(),
    }
}


fn room_join_rule_from_sdk(join_rule: MatrixRoomJoinRule) -> RoomJoinRule {
    match join_rule {
        MatrixRoomJoinRule::Public => RoomJoinRule::Public,
        MatrixRoomJoinRule::Invite => RoomJoinRule::Invite,
        MatrixRoomJoinRule::Knock => RoomJoinRule::Knock,
        MatrixRoomJoinRule::Restricted => RoomJoinRule::Restricted,
        MatrixRoomJoinRule::Private => RoomJoinRule::Private,
    }
}


fn room_history_visibility_from_sdk(
    history_visibility: MatrixRoomHistoryVisibility,
) -> RoomHistoryVisibility {
    match history_visibility {
        MatrixRoomHistoryVisibility::WorldReadable => RoomHistoryVisibility::WorldReadable,
        MatrixRoomHistoryVisibility::Shared => RoomHistoryVisibility::Shared,
        MatrixRoomHistoryVisibility::Invited => RoomHistoryVisibility::Invited,
        MatrixRoomHistoryVisibility::Joined => RoomHistoryVisibility::Joined,
    }
}


fn room_permission_facts_from_sdk(permissions: MatrixRoomPermissionFacts) -> RoomPermissionFacts {
    RoomPermissionFacts {
        can_edit_settings: permissions.can_edit_settings,
        can_edit_roles: permissions.can_edit_roles,
        can_invite: permissions.can_invite,
        can_kick: permissions.can_kick,
        can_ban: permissions.can_ban,
        can_unban: permissions.can_unban,
    }
}


fn room_member_summary_from_sdk(member: MatrixRoomMemberSummary) -> RoomMemberSummary {
    let display_label = member
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .unwrap_or(member.user_id.as_str())
        .to_owned();
    RoomMemberSummary {
        user_id: member.user_id,
        display_name: member.display_name,
        display_label: display_label.clone(),
        original_display_label: display_label,
        avatar_url: member.avatar_url,
        power_level: member.power_level,
        role: room_member_role_from_sdk(member.role),
        user_trust: member.user_trust.map(user_trust_state_from_sdk),
    }
}


fn room_moderation_allowed(
    permissions: &RoomPermissionFacts,
    action: RoomModerationAction,
) -> bool {
    match action {
        RoomModerationAction::Kick => permissions.can_kick,
        RoomModerationAction::Ban => permissions.can_ban,
        RoomModerationAction::Unban => permissions.can_unban,
    }
}


