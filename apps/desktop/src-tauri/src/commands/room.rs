use super::directory::{
    build_accept_invite_command, build_cancel_space_invite_command,
    build_close_invite_workflow_command, build_create_room_command, build_create_space_command,
    build_decline_invite_command, build_invite_targets_command, build_invite_user_command,
    build_invite_user_to_space_command, build_join_room_command,
    build_open_invite_workflow_command, build_remove_invite_target_command,
    build_search_invite_targets_command, build_select_invite_target_command,
    build_set_invite_scope_command, build_set_space_child_command,
    build_start_direct_message_command, wait_for_direct_message_started, wait_for_room_created,
    wait_for_room_in_state, wait_for_room_joined, wait_for_space_created,
};
use super::navigation::SELECT_ROOM_EVENT_TIMEOUT;
use super::*;

const INVITE_WORKFLOW_CONVERGENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const INVITE_WORKFLOW_CONVERGENCE_ERROR: &str = "invite workflow convergence timed out";

enum InviteWorkflowTerminal<'a> {
    Open { room_id: &'a str },
    Search { room_id: &'a str, query: &'a str },
    Closed,
}

fn invite_workflow_snapshot_matches(
    snapshot: &koushi_state::AppState,
    terminal: &InviteWorkflowTerminal<'_>,
) -> bool {
    match terminal {
        InviteWorkflowTerminal::Open { room_id } => {
            snapshot.invite_workflow.query.room_id.as_deref() == Some(*room_id)
        }
        InviteWorkflowTerminal::Search { room_id, query } => {
            snapshot.invite_workflow.query.room_id.as_deref() == Some(*room_id)
                && snapshot.invite_workflow.query.query == *query
        }
        InviteWorkflowTerminal::Closed => {
            snapshot.invite_workflow == koushi_state::InviteWorkflowState::default()
        }
    }
}

#[derive(Debug)]
struct InviteWorkflowVersionedSnapshot {
    state: koushi_state::AppState,
    generation: u64,
}

trait InviteWorkflowSnapshotSource {
    fn versioned_snapshot(&self) -> InviteWorkflowVersionedSnapshot;
    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventStreamLag>> + Send + '_>>;
}

impl InviteWorkflowSnapshotSource for CoreConnection {
    fn versioned_snapshot(&self) -> InviteWorkflowVersionedSnapshot {
        let snapshot = CoreConnection::versioned_snapshot(self);
        InviteWorkflowVersionedSnapshot {
            state: snapshot.state,
            generation: snapshot.generation,
        }
    }

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventStreamLag>> + Send + '_>> {
        Box::pin(async move { CoreConnection::recv_event(self).await.map(|_| ()) })
    }
}

async fn wait_for_invite_workflow_snapshot_from<S: InviteWorkflowSnapshotSource>(
    source: &mut S,
    terminal: InviteWorkflowTerminal<'_>,
    timeout: std::time::Duration,
) -> Result<InviteWorkflowVersionedSnapshot, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let snapshot = source.versioned_snapshot();
        if invite_workflow_snapshot_matches(&snapshot.state, &terminal) {
            return Ok(snapshot);
        }

        match tokio::time::timeout_at(deadline, source.recv_event()).await {
            Err(_) => return Err(INVITE_WORKFLOW_CONVERGENCE_ERROR.to_owned()),
            Ok(Err(lag)) if lag.skipped == 0 => {
                return Err(INVITE_WORKFLOW_CONVERGENCE_ERROR.to_owned());
            }
            Ok(Ok(())) | Ok(Err(_)) => {}
        }
    }
}

async fn wait_for_invite_workflow_snapshot(
    event_conn: &mut CoreConnection,
    terminal: InviteWorkflowTerminal<'_>,
) -> Result<FrontendDesktopSnapshot, String> {
    let snapshot = wait_for_invite_workflow_snapshot_from(
        event_conn,
        terminal,
        INVITE_WORKFLOW_CONVERGENCE_TIMEOUT,
    )
    .await?;
    Ok(FrontendDesktopSnapshot::from_versioned(
        snapshot.state,
        snapshot.generation,
    ))
}

#[tauri::command]
pub async fn open_invite_workflow(
    room_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_open_invite_workflow_command(
            request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|error| format!("command submit failed: {error}"))?;
    wait_for_invite_workflow_snapshot(
        &mut event_conn,
        InviteWorkflowTerminal::Open { room_id: &room_id },
    )
    .await
}

#[tauri::command]
pub async fn close_invite_workflow(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_close_invite_workflow_command(request_id))
        .await
        .map_err(|error| format!("command submit failed: {error}"))?;
    wait_for_invite_workflow_snapshot(&mut event_conn, InviteWorkflowTerminal::Closed).await
}

#[tauri::command]
pub async fn search_invite_targets(
    room_id: String,
    query: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_search_invite_targets_command(
            request_id,
            room_id.clone(),
            query.clone(),
        ))
        .await
        .map_err(|error| format!("command submit failed: {error}"))?;
    wait_for_invite_workflow_snapshot(
        &mut event_conn,
        InviteWorkflowTerminal::Search {
            room_id: &room_id,
            query: &query,
        },
    )
    .await
}

#[tauri::command]
pub async fn set_invite_scope(
    room_id: String,
    scope: InviteScopeSelection,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_invite_scope_command(request_id, room_id, scope),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn select_invite_target(
    room_id: String,
    user_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_select_invite_target_command(request_id, room_id, user_id),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn remove_invite_target(
    user_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_remove_invite_target_command(request_id, user_id),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn select_room_list_filter(
    filter: RoomListFilter,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::App(AppCommand::SelectRoomListFilter { request_id, filter }),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn mark_room_as_read(
    room_id: String,
    event_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::Room(RoomCommand::MarkRoomAsRead {
            request_id,
            room_id,
            event_id,
        }),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn mark_room_as_unread(
    room_id: String,
    unread: bool,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::Room(RoomCommand::MarkRoomAsUnread {
            request_id,
            room_id,
            unread,
        }),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_room_notification_mode(
    room_id: String,
    mode: RoomNotificationMode,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::Room(RoomCommand::SetRoomNotificationMode {
            request_id,
            room_id,
            mode,
        }),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn leave_room(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_leave_room_command(request_id, room_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn forget_room(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_forget_room_command(request_id, room_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_room_tag(
    room_id: String,
    tag: RoomTagKind,
    order: Option<f64>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_room_tag_command(request_id, room_id, tag, order),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn remove_room_tag(
    room_id: String,
    tag: RoomTagKind,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_remove_room_tag_command(request_id, room_id, tag),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn pin_event(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_pin_event_command(request_id, room_id, event_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn unpin_event(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_unpin_event_command(request_id, room_id, event_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn refresh_pinned_events(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_refresh_pinned_events_command(
            request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, _| {
            matches!(
                event,
                RoomEvent::PinnedEventsUpdated {
                    room_id: updated_room_id,
                    ..
                } if updated_room_id == &room_id
            )
        },
        "pinned messages refresh did not complete",
        "pinned messages refresh failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn load_room_settings(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_load_room_settings_command(request_id, room_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::RoomSettingsLoaded { request_id, .. } if *request_id == expected_request_id
            )
        },
        "room settings load did not complete",
        "room settings load failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn load_space_members(
    space_id: String,
    generation: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_load_space_members_command(
            request_id, space_id, generation,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            space_members_loaded_event_matches(event, expected_request_id, generation)
        },
        "Space member load did not complete",
        "Space member load failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn query_mention_candidates(
    room_id: String,
    surface: MentionSurface,
    query: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<(), String> {
    let request_id = next_request_id(state.inner()).await;
    let account_key = account_key_from_snapshot(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::Room(RoomCommand::QueryMentionCandidates {
            request_id,
            account_key,
            room_id,
            surface,
            query,
        }),
    )
    .await
}

#[tauri::command]
pub async fn repair_room_timeline(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_repair_room_timeline_command(request_id, room_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn reshare_room_key(
    room_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<RoomKeyReshareOutcome, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_reshare_room_key_command(request_id, room_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "room key reshare did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::RoomKeyReshared {
                request_id: event_request_id,
                outcome,
                ..
            })) if event_request_id == request_id => return Ok(outcome),
            Ok(CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            }) if event_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "room key reshare failed",
                    failure,
                ));
            }
            Ok(_) | Err(_) => {}
        }
    }
}

/// Temporary dangerous encryption-debug control (issue #538): rotate the
/// outbound Megolm session and confirm the fresh session is at index 0.
#[tauri::command]
pub async fn force_new_outbound_session(
    room_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<EncryptionDebugOperationOutcome, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_force_new_outbound_session_command(
            request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "force new outbound session did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::OutboundSessionForced {
                request_id: event_request_id,
                outcome,
                ..
            })) if event_request_id == request_id => return Ok(outcome),
            Ok(CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            }) if event_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "force new outbound session failed",
                    failure,
                ));
            }
            Ok(_) | Err(_) => {}
        }
    }
}

/// Temporary dangerous encryption-debug control (issue #538): share the
/// current outbound session's index-0 room key to every eligible recipient
/// device.
#[tauri::command]
pub async fn share_index0_room_key(
    room_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<EncryptionDebugOperationOutcome, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_share_index0_room_key_command(
            request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "index-0 room key share did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::Index0RoomKeyShared {
                request_id: event_request_id,
                outcome,
                ..
            })) if event_request_id == request_id => return Ok(outcome),
            Ok(CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            }) if event_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "index-0 room key share failed",
                    failure,
                ));
            }
            Ok(_) | Err(_) => {}
        }
    }
}

/// Temporary dangerous encryption-debug control (issue #541): resend the
/// current session's index-0 recovery material to the immutable original
/// recipient ledger.
#[tauri::command]
pub async fn resend_index0_room_key(
    room_id: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<EncryptionDebugOperationOutcome, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_resend_index0_room_key_command(
            request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "index-0 room key resend did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
                request_id: event_request_id,
                outcome,
                ..
            })) if event_request_id == request_id => return Ok(outcome),
            Ok(CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            }) if event_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "index-0 room key resend failed",
                    failure,
                ));
            }
            Ok(_) | Err(_) => {}
        }
    }
}

#[tauri::command]
pub async fn update_room_setting(
    room_id: String,
    change: RoomSettingChange,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_update_room_setting_command(
            request_id, room_id, change,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::RoomSettingUpdated { request_id, .. } if *request_id == expected_request_id
            )
        },
        "room setting update did not complete",
        "room setting update failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn moderate_room_member(
    room_id: String,
    target_user_id: String,
    action: RoomModerationAction,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_moderate_room_member_command(
            request_id,
            room_id,
            target_user_id,
            action,
            optional_non_blank(reason),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::RoomMemberModerated { request_id, .. } if *request_id == expected_request_id
            )
        },
        "room member moderation did not complete",
        "room member moderation failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn update_room_member_role(
    room_id: String,
    target_user_id: String,
    power_level: i64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_update_room_member_role_command(
            request_id,
            room_id,
            target_user_id,
            power_level,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::RoomMemberRoleUpdated { request_id, .. } if *request_id == expected_request_id
            )
        },
        "room member role update did not complete",
        "room member role update failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn create_room(
    options: koushi_core::CreateRoomOptions,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_create_room_command(request_id, options))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_created(&mut event_conn, request_id, CREATE_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn create_space(
    name: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_create_space_command(request_id, name))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_space_created(&mut event_conn, request_id, CREATE_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_space_child(
    space_id: String,
    child_room_id: String,
    via_server: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_space_child_command(request_id, space_id, child_room_id, via_server),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn join_room(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    let Some(command) = build_join_room_command(request_id, room_id) else {
        update_qa_window_title_from_state(&app, state.inner()).await;
        return current_snapshot(state.inner()).await;
    };

    event_conn
        .command(command)
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_joined(&mut event_conn, request_id, ROOM_OPERATION_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn accept_invite(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_accept_invite_command(request_id, room_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::InviteAccepted { request_id, .. } if *request_id == expected_request_id
            )
        },
        "invite acceptance did not complete",
        "invite acceptance failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn decline_invite(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_decline_invite_command(request_id, room_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::InviteDeclined { request_id, .. } if *request_id == expected_request_id
            )
        },
        "invite decline did not complete",
        "invite decline failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_direct_message(
    user_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_start_direct_message_command(request_id, user_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    // Get-or-create resolves the exact conversation; keep that identity,
    // wait for it to reach the Rust room-list projection, and open it before
    // returning so "Send message" lands in the DM instead of staying on the
    // previous room (#368).
    let room_id =
        wait_for_direct_message_started(&mut event_conn, request_id, ROOM_OPERATION_EVENT_TIMEOUT)
            .await?;
    wait_for_room_in_state(&mut event_conn, &room_id, ROOM_OPERATION_EVENT_TIMEOUT).await?;
    let select_request_id = event_conn.next_request_id();
    event_conn
        .command(super::navigation::build_select_room_command(
            select_request_id,
            room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    super::navigation::wait_for_selected_room(
        &mut event_conn,
        select_request_id,
        &room_id,
        SELECT_ROOM_EVENT_TIMEOUT,
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn invite_user(
    room_id: String,
    user_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_invite_user_command(request_id, room_id, user_id))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::UserInvited { request_id, .. } if *request_id == expected_request_id
            )
        },
        "user invite did not complete",
        "user invite failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn invite_user_to_space(
    space_id: String,
    user_id: String,
    generation: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_invite_user_to_space_command(
            request_id, space_id, user_id, generation,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            space_member_invite_settled_event_matches(event, expected_request_id, generation)
        },
        "Space member invite did not complete",
        "Space member invite failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn update_space_member_role(
    space_id: String,
    user_id: String,
    generation: u64,
    expected_power_levels_revision: Option<String>,
    expected_power_level: i64,
    power_level: i64,
    confirmed: bool,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_update_space_member_role_command(
            request_id,
            space_id,
            user_id,
            generation,
            expected_power_levels_revision,
            expected_power_level,
            power_level,
            confirmed,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_space_member_role_update(&mut event_conn, request_id, generation).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn cancel_space_invite(
    space_id: String,
    user_id: String,
    generation: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_cancel_space_invite_command(
            request_id, space_id, user_id, generation,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            space_member_invite_cancellation_settled_event_matches(
                event,
                expected_request_id,
                generation,
            )
        },
        "Space member invite cancellation did not complete",
        "Space member invite cancellation failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn invite_targets(
    room_id: String,
    user_ids: Vec<String>,
    scope: InviteScopeSelection,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_invite_targets_command(
            request_id, room_id, user_ids, scope,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_invite_batch_completed(&mut event_conn, request_id).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

async fn wait_for_invite_batch_completed(
    event_conn: &mut CoreConnection,
    operation_request_id: RequestId,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "invite batch did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::InviteBatchCompleted { request_id, .. }))
                if request_id == operation_request_id =>
            {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == operation_request_id => {
                return Err(invoke_error_from_core_failure(
                    "invite batch failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) fn snapshot_contains_room(snapshot: &koushi_state::AppState, room_id: &str) -> bool {
    snapshot.rooms.iter().any(|room| room.room_id == room_id)
}

async fn wait_for_space_member_role_update(
    event_conn: &mut CoreConnection,
    operation_request_id: RequestId,
    generation: u64,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ROOM_OPERATION_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "Space member role update did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::SpaceMemberRoleUpdateSettled {
                request_id,
                generation: event_generation,
                outcome,
            })) if request_id == operation_request_id && event_generation == generation => {
                return match outcome {
                    koushi_state::SpaceMemberRoleUpdateOutcome::Succeeded => Ok(()),
                    koushi_state::SpaceMemberRoleUpdateOutcome::Failed(kind) => {
                        Err(format!("Space member role update failed: {kind:?}"))
                    }
                };
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == operation_request_id => {
                return Err(invoke_error_from_core_failure(
                    "Space member role update failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) async fn wait_for_room_operation<F>(
    event_conn: &mut CoreConnection,
    operation_request_id: RequestId,
    timeout: std::time::Duration,
    is_success: F,
    timeout_message: &'static str,
    failure_message: &'static str,
) -> Result<(), String>
where
    F: Fn(&RoomEvent, RequestId) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| timeout_message.to_owned())?;
        match event {
            Ok(CoreEvent::Room(room_event)) if is_success(&room_event, operation_request_id) => {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == operation_request_id => {
                return Err(invoke_error_from_core_failure(failure_message, failure));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) fn build_update_space_member_role_command(
    request_id: koushi_core::RequestId,
    space_id: String,
    user_id: String,
    generation: u64,
    expected_power_levels_revision: Option<String>,
    expected_power_level: i64,
    power_level: i64,
    confirmed: bool,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::UpdateSpaceMemberRole {
        request_id,
        space_id,
        user_id,
        generation,
        expected_power_levels_revision,
        expected_power_level,
        power_level,
        confirmed,
    })
}

pub(super) fn build_leave_room_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::LeaveRoom {
        request_id,
        room_id,
    })
}

pub(super) fn build_forget_room_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ForgetRoom {
        request_id,
        room_id,
    })
}

pub(super) fn build_set_room_tag_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    tag: RoomTagKind,
    order: Option<f64>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::SetTag {
        request_id,
        room_id,
        tag,
        order,
    })
}

pub(super) fn build_remove_room_tag_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    tag: RoomTagKind,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::RemoveTag {
        request_id,
        room_id,
        tag,
    })
}

pub(super) fn build_pin_event_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    event_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::PinEvent {
        request_id,
        room_id,
        event_id,
    })
}

pub(super) fn build_unpin_event_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    event_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::UnpinEvent {
        request_id,
        room_id,
        event_id,
    })
}

pub(super) fn build_refresh_pinned_events_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::RefreshPinnedEvents {
        request_id,
        room_id,
    })
}

pub(super) fn build_load_room_settings_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::LoadRoomSettings {
        request_id,
        room_id,
    })
}

pub(super) fn build_load_space_members_command(
    request_id: koushi_core::RequestId,
    space_id: String,
    generation: u64,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::LoadSpaceMembers {
        request_id,
        space_id,
        generation,
    })
}

pub(super) fn build_repair_room_timeline_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::RepairRoomTimeline {
        request_id,
        room_id,
    })
}

pub(super) fn build_reshare_room_key_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ReshareRoomKey {
        request_id,
        room_id,
    })
}

pub(super) fn build_force_new_outbound_session_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ForceNewOutboundSession {
        request_id,
        room_id,
    })
}

pub(super) fn build_share_index0_room_key_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ShareIndex0RoomKey {
        request_id,
        room_id,
    })
}

pub(super) fn build_resend_index0_room_key_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ResendIndex0RoomKey {
        request_id,
        room_id,
    })
}

pub(super) fn build_update_room_setting_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    change: RoomSettingChange,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::UpdateRoomSetting {
        request_id,
        room_id,
        change,
    })
}

pub(super) fn build_moderate_room_member_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    target_user_id: String,
    action: RoomModerationAction,
    reason: Option<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ModerateRoomMember {
        request_id,
        room_id,
        target_user_id,
        action,
        reason,
    })
}

pub(super) fn build_update_room_member_role_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    target_user_id: String,
    power_level: i64,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::UpdateRoomMemberRole {
        request_id,
        room_id,
        target_user_id,
        power_level,
    })
}

pub(super) const ROOM_OPERATION_EVENT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(60);

const CREATE_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

pub(super) fn space_members_loaded_event_matches(
    event: &RoomEvent,
    expected_request_id: RequestId,
    expected_generation: u64,
) -> bool {
    matches!(
        event,
        RoomEvent::SpaceMembersLoaded {
            request_id,
            generation,
            ..
        } if *request_id == expected_request_id && *generation == expected_generation
    )
}

pub(super) fn space_member_invite_settled_event_matches(
    event: &RoomEvent,
    expected_request_id: RequestId,
    expected_generation: u64,
) -> bool {
    matches!(
        event,
        RoomEvent::SpaceMemberInviteSettled {
            request_id,
            generation,
            ..
        } if *request_id == expected_request_id && *generation == expected_generation
    )
}

pub(super) fn space_member_invite_cancellation_settled_event_matches(
    event: &RoomEvent,
    expected_request_id: RequestId,
    expected_generation: u64,
) -> bool {
    matches!(
        event,
        RoomEvent::SpaceMemberInviteCancellationSettled {
            request_id,
            generation,
            ..
        } if *request_id == expected_request_id && *generation == expected_generation
    )
}

#[cfg(test)]
fn commands_source() -> String {
    crate::commands::contracts::production_source()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::fake_request_id;

    #[test]
    fn invite_workflow_snapshot_terminals_are_exact_and_short_bounded() {
        assert_eq!(
            INVITE_WORKFLOW_CONVERGENCE_TIMEOUT,
            std::time::Duration::from_secs(2)
        );
        let mut state = koushi_state::AppState::default();
        assert!(invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Closed,
        ));
        assert!(!invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Open {
                room_id: "!room:test"
            },
        ));

        state.invite_workflow.query.room_id = Some("!room:test".to_owned());
        state.invite_workflow.query.query = "alice".to_owned();
        assert!(invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Open {
                room_id: "!room:test"
            },
        ));
        assert!(invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Search {
                room_id: "!room:test",
                query: "alice",
            },
        ));
        assert!(!invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Search {
                room_id: "!room:test",
                query: "bob",
            },
        ));
        assert!(!invite_workflow_snapshot_matches(
            &state,
            &InviteWorkflowTerminal::Closed,
        ));
    }

    enum InviteWorkflowWaitStep {
        Snapshot(koushi_state::AppState, u64),
        Lag(u64),
    }

    struct ScriptedInviteWorkflowSource {
        current: InviteWorkflowVersionedSnapshot,
        steps: std::collections::VecDeque<InviteWorkflowWaitStep>,
    }

    impl InviteWorkflowSnapshotSource for ScriptedInviteWorkflowSource {
        fn versioned_snapshot(&self) -> InviteWorkflowVersionedSnapshot {
            InviteWorkflowVersionedSnapshot {
                state: self.current.state.clone(),
                generation: self.current.generation,
            }
        }

        fn recv_event(
            &mut self,
        ) -> Pin<Box<dyn Future<Output = Result<(), EventStreamLag>> + Send + '_>> {
            match self.steps.pop_front() {
                Some(InviteWorkflowWaitStep::Snapshot(state, generation)) => {
                    self.current = InviteWorkflowVersionedSnapshot { state, generation };
                    Box::pin(std::future::ready(Ok(())))
                }
                Some(InviteWorkflowWaitStep::Lag(skipped)) => {
                    Box::pin(std::future::ready(Err(EventStreamLag { skipped })))
                }
                None => Box::pin(std::future::pending()),
            }
        }
    }

    #[tokio::test]
    async fn invite_workflow_wait_rechecks_after_lag_and_times_out_with_fixed_error() {
        let initial = koushi_state::AppState::default();
        let mut matching = initial.clone();
        matching.invite_workflow.query.room_id = Some("!space:test".to_owned());
        matching.invite_workflow.query.query = "alice".to_owned();
        let mut lagged = ScriptedInviteWorkflowSource {
            current: InviteWorkflowVersionedSnapshot {
                state: initial.clone(),
                generation: 1,
            },
            steps: [
                InviteWorkflowWaitStep::Lag(3),
                InviteWorkflowWaitStep::Snapshot(matching, 2),
            ]
            .into(),
        };

        let settled = wait_for_invite_workflow_snapshot_from(
            &mut lagged,
            InviteWorkflowTerminal::Search {
                room_id: "!space:test",
                query: "alice",
            },
            std::time::Duration::from_millis(50),
        )
        .await
        .expect("matching snapshot after lag should settle");
        assert_eq!(settled.generation, 2);

        let mut stalled = ScriptedInviteWorkflowSource {
            current: InviteWorkflowVersionedSnapshot {
                state: initial,
                generation: 1,
            },
            steps: std::collections::VecDeque::new(),
        };
        let error = wait_for_invite_workflow_snapshot_from(
            &mut stalled,
            InviteWorkflowTerminal::Open {
                room_id: "!missing:test",
            },
            std::time::Duration::from_millis(1),
        )
        .await
        .expect_err("non-matching snapshot should hit the fixed deadline");
        assert_eq!(error, INVITE_WORKFLOW_CONVERGENCE_ERROR);
    }

    #[test]
    fn room_management_tauri_commands_wait_for_correlated_core_events() {
        let source = commands_source();

        for (fn_name, event_token) in [
            ("pub async fn load_room_settings", "RoomSettingsLoaded"),
            ("pub async fn update_room_setting", "RoomSettingUpdated"),
            ("pub async fn moderate_room_member", "RoomMemberModerated"),
            (
                "pub async fn update_room_member_role",
                "RoomMemberRoleUpdated",
            ),
        ] {
            let fn_offset = source
                .find(fn_name)
                .unwrap_or_else(|| panic!("{fn_name} command should exist"));
            let rest = &source[fn_offset..];
            let end = rest.find("\n#[tauri::command]").unwrap_or(rest.len());
            let command_source = &rest[..end];

            assert!(
                command_source.contains("wait_for_room_operation"),
                "{fn_name} should wait for the correlated RoomEvent before returning a snapshot"
            );
            assert!(
                command_source.contains(event_token),
                "{fn_name} should wait for {event_token}"
            );
            assert!(
                command_source.contains("update_qa_window_title_from_state"),
                "{fn_name} should refresh the QA title after state changes"
            );
            assert!(
                command_source.contains("current_snapshot"),
                "{fn_name} should return the current snapshot"
            );
        }
    }

    #[test]
    fn load_space_members_and_invite_user_to_space_build_exact_commands_and_wait_for_events() {
        let source = commands_source();
        let lib_source = include_str!("../lib.rs");

        for (fn_name, matcher_token) in [
            (
                "pub async fn load_space_members",
                "space_members_loaded_event_matches",
            ),
            (
                "pub async fn invite_user_to_space",
                "space_member_invite_settled_event_matches",
            ),
            (
                "pub async fn cancel_space_invite",
                "space_member_invite_cancellation_settled_event_matches",
            ),
            (
                "pub async fn update_space_member_role",
                "wait_for_space_member_role_update",
            ),
        ] {
            let fn_offset = source
                .find(fn_name)
                .unwrap_or_else(|| panic!("{fn_name} command should exist"));
            let rest = &source[fn_offset..];
            let end = rest.find("\n#[tauri::command]").unwrap_or(rest.len());
            let command_source = &rest[..end];

            let waiter = if fn_name == "pub async fn update_space_member_role" {
                "wait_for_space_member_role_update"
            } else {
                "wait_for_room_operation"
            };
            assert!(
                command_source.contains(waiter),
                "{fn_name} should wait for the correlated RoomEvent"
            );
            assert!(
                command_source.contains(matcher_token),
                "{fn_name} should wait through {matcher_token}"
            );
            assert!(command_source.contains("current_snapshot"));
        }
        assert!(lib_source.contains("commands::room::cancel_space_invite"));
        assert!(lib_source.contains("commands::room::update_space_member_role"));

        match super::build_load_space_members_command(
            fake_request_id(301),
            "!space:example.org".to_owned(),
            4,
        ) {
            CoreCommand::Room(RoomCommand::LoadSpaceMembers {
                request_id,
                space_id,
                generation,
            }) => {
                assert_eq!(request_id, fake_request_id(301));
                assert_eq!(space_id, "!space:example.org");
                assert_eq!(generation, 4);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match super::build_update_space_member_role_command(
            fake_request_id(306),
            "!space:example.org".to_owned(),
            "@child:example.org".to_owned(),
            4,
            Some("$power:example.org".to_owned()),
            0,
            50,
            false,
        ) {
            CoreCommand::Room(RoomCommand::UpdateSpaceMemberRole {
                request_id,
                space_id,
                user_id,
                generation,
                expected_power_levels_revision,
                expected_power_level,
                power_level,
                confirmed,
            }) => {
                assert_eq!(request_id, fake_request_id(306));
                assert_eq!(space_id, "!space:example.org");
                assert_eq!(user_id, "@child:example.org");
                assert_eq!(generation, 4);
                assert_eq!(
                    expected_power_levels_revision.as_deref(),
                    Some("$power:example.org")
                );
                assert_eq!(expected_power_level, 0);
                assert_eq!(power_level, 50);
                assert!(!confirmed);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match super::build_cancel_space_invite_command(
            fake_request_id(305),
            "!space:example.org".to_owned(),
            "@child:example.org".to_owned(),
            4,
        ) {
            CoreCommand::Room(RoomCommand::CancelSpaceInvite {
                request_id,
                space_id,
                user_id,
                generation,
            }) => {
                assert_eq!(request_id, fake_request_id(305));
                assert_eq!(space_id, "!space:example.org");
                assert_eq!(user_id, "@child:example.org");
                assert_eq!(generation, 4);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        match super::build_invite_user_to_space_command(
            fake_request_id(302),
            "!space:example.org".to_owned(),
            "@child:example.org".to_owned(),
            4,
        ) {
            CoreCommand::Room(RoomCommand::InviteUserToSpace {
                request_id,
                space_id,
                user_id,
                generation,
            }) => {
                assert_eq!(request_id, fake_request_id(302));
                assert_eq!(space_id, "!space:example.org");
                assert_eq!(user_id, "@child:example.org");
                assert_eq!(generation, 4);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn space_member_event_waits_reject_wrong_generation() {
        let wrong_load = koushi_core::RoomEvent::SpaceMembersLoaded {
            request_id: fake_request_id(303),
            generation: 3,
            joined_count: 0,
            invited_count: 0,
            child_room_only_count: 0,
            incomplete_child_room_count: 0,
        };
        let matching_load = koushi_core::RoomEvent::SpaceMembersLoaded {
            request_id: fake_request_id(303),
            generation: 4,
            joined_count: 0,
            invited_count: 0,
            child_room_only_count: 0,
            incomplete_child_room_count: 0,
        };
        assert!(!super::space_members_loaded_event_matches(
            &wrong_load,
            fake_request_id(303),
            4,
        ));
        assert!(super::space_members_loaded_event_matches(
            &matching_load,
            fake_request_id(303),
            4,
        ));

        let wrong_invite = koushi_core::RoomEvent::SpaceMemberInviteSettled {
            request_id: fake_request_id(304),
            generation: 3,
            outcome: koushi_state::SpaceMemberInviteOutcome::Invited,
        };
        let matching_invite = koushi_core::RoomEvent::SpaceMemberInviteSettled {
            request_id: fake_request_id(304),
            generation: 4,
            outcome: koushi_state::SpaceMemberInviteOutcome::Invited,
        };
        assert!(!super::space_member_invite_settled_event_matches(
            &wrong_invite,
            fake_request_id(304),
            4,
        ));
        assert!(super::space_member_invite_settled_event_matches(
            &matching_invite,
            fake_request_id(304),
            4,
        ));

        let wrong_cancel = koushi_core::RoomEvent::SpaceMemberInviteCancellationSettled {
            request_id: fake_request_id(305),
            generation: 3,
            outcome: koushi_state::SpaceMemberInviteOutcome::Cancelled,
        };
        let matching_cancel = koushi_core::RoomEvent::SpaceMemberInviteCancellationSettled {
            request_id: fake_request_id(305),
            generation: 4,
            outcome: koushi_state::SpaceMemberInviteOutcome::Cancelled,
        };
        assert!(
            !super::space_member_invite_cancellation_settled_event_matches(
                &wrong_cancel,
                fake_request_id(305),
                4,
            )
        );
        assert!(
            super::space_member_invite_cancellation_settled_event_matches(
                &matching_cancel,
                fake_request_id(305),
                4,
            )
        );
    }
}
