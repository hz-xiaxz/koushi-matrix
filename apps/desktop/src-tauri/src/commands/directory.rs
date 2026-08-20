use super::navigation::{SELECT_ROOM_EVENT_TIMEOUT, SelectEventSource};
use super::room::{ROOM_OPERATION_EVENT_TIMEOUT, snapshot_contains_room};
use super::*;
#[tauri::command]
pub async fn query_directory(
    term: Option<String>,
    server_name: Option<String>,
    limit: Option<u32>,
    since: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_query_directory_command(
            request_id,
            term,
            server_name,
            limit,
            since,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    super::room::wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::DirectoryQueryCompleted { request_id, .. } if *request_id == expected_request_id
            )
        },
        "directory query did not complete",
        "directory query failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn join_directory_room(
    room_id_or_alias: String,
    via_servers: Vec<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    let Some(command) =
        build_join_directory_room_command(request_id, room_id_or_alias, via_servers)
    else {
        update_qa_window_title_from_state(&app, state.inner()).await;
        return current_snapshot(state.inner()).await;
    };

    event_conn
        .command(command)
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let joined_room_id =
        wait_for_room_joined(&mut event_conn, request_id, ROOM_OPERATION_EVENT_TIMEOUT).await?;
    super::navigation::wait_for_selected_room(
        &mut event_conn,
        request_id,
        &joined_room_id,
        SELECT_ROOM_EVENT_TIMEOUT,
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn preview_join_target(
    room_id_or_alias: String,
    via_servers: Vec<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    let Some(command) =
        build_preview_join_target_command(request_id, room_id_or_alias, via_servers)
    else {
        update_qa_window_title_from_state(&app, state.inner()).await;
        return current_snapshot(state.inner()).await;
    };

    event_conn
        .command(command)
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    super::room::wait_for_room_operation(
        &mut event_conn,
        request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, expected_request_id| {
            matches!(
                event,
                RoomEvent::DirectoryPreviewLoaded { request_id, .. } if *request_id == expected_request_id
            )
        },
        "directory preview did not complete",
        "directory preview failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn dismiss_directory_preview(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_dismiss_directory_preview_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) async fn wait_for_room_created(
    event_conn: &mut CoreConnection,
    create_request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "room creation did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::RoomCreated { request_id, .. }))
                if request_id == create_request_id =>
            {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == create_request_id => {
                return Err(invoke_error_from_core_failure(
                    "room creation failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) async fn wait_for_space_created(
    event_conn: &mut CoreConnection,
    create_request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "space creation did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(RoomEvent::SpaceCreated { request_id, .. }))
                if request_id == create_request_id =>
            {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == create_request_id => {
                return Err(invoke_error_from_core_failure(
                    "space creation failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) async fn wait_for_direct_message_started<S: SelectEventSource + ?Sized>(
    event_conn: &mut S,
    operation_request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "direct message start did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(koushi_core::RoomEvent::DirectMessageStarted {
                request_id,
                room_id,
            })) if request_id == operation_request_id => {
                return Ok(room_id);
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == operation_request_id => {
                return Err(invoke_error_from_core_failure(
                    "direct message start failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

/// Wait until the room-list projection carries `room_id`. A freshly created
/// DM is announced by `DirectMessageStarted` before the asynchronous
/// room-list refresh lands, so selecting it immediately would race the
/// known-room state (#368).
pub(super) async fn wait_for_room_in_state<S: SelectEventSource + ?Sized>(
    event_conn: &mut S,
    room_id: &str,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if snapshot_contains_room(&event_conn.snapshot(), room_id) {
            return Ok(());
        }
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "direct message room did not reach the room list".to_owned())?;
        match event {
            Ok(_) => {}
            Err(_) if snapshot_contains_room(&event_conn.snapshot(), room_id) => {
                return Ok(());
            }
            Err(_) => continue,
        }
    }
}

/// Preview and join name the same target, so they normalize it the same way.
///
/// A blank server name is not a routing hint, and keeping duplicates would make
/// the homeserver retry the same server.
fn normalize_join_target(
    room_id_or_alias: String,
    via_servers: Vec<String>,
) -> Option<(String, Vec<String>)> {
    let room_id_or_alias = room_id_or_alias.trim().to_owned();
    if room_id_or_alias.is_empty() {
        return None;
    }
    let mut seen = std::collections::BTreeSet::new();
    let via_servers = via_servers
        .into_iter()
        .filter_map(|server| optional_non_blank(Some(server)))
        .filter(|server| seen.insert(server.clone()))
        .collect::<Vec<_>>();
    Some((room_id_or_alias, via_servers))
}

pub(super) fn build_create_room_command(
    request_id: koushi_core::RequestId,
    options: CreateRoomOptions,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::CreateRoom {
        request_id,
        options,
    })
}

pub(super) fn build_create_space_command(
    request_id: koushi_core::RequestId,
    name: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::CreateSpace { request_id, name })
}

pub(super) fn build_join_room_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> Option<CoreCommand> {
    let room_id = room_id.trim().to_owned();
    if room_id.is_empty() {
        return None;
    }
    Some(CoreCommand::Room(RoomCommand::JoinRoom {
        request_id,
        room_id,
    }))
}

pub(super) fn build_set_space_child_command(
    request_id: koushi_core::RequestId,
    space_id: String,
    child_room_id: String,
    via_server: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::SetSpaceChild {
        request_id,
        space_id,
        child_room_id,
        via_server,
    })
}

pub(super) fn build_accept_invite_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::AcceptInvite {
        request_id,
        room_id,
    })
}

pub(super) fn build_decline_invite_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::DeclineInvite {
        request_id,
        room_id,
    })
}

pub(super) fn build_start_direct_message_command(
    request_id: koushi_core::RequestId,
    user_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::StartDirectMessage {
        request_id,
        user_id,
    })
}

pub(super) fn build_invite_user_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    user_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::InviteUser {
        request_id,
        room_id,
        user_id,
    })
}

pub(super) fn build_invite_user_to_space_command(
    request_id: koushi_core::RequestId,
    space_id: String,
    user_id: String,
    generation: u64,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::InviteUserToSpace {
        request_id,
        space_id,
        user_id,
        generation,
    })
}

pub(super) fn build_cancel_space_invite_command(
    request_id: koushi_core::RequestId,
    space_id: String,
    user_id: String,
    generation: u64,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::CancelSpaceInvite {
        request_id,
        space_id,
        user_id,
        generation,
    })
}

pub(super) fn build_open_invite_workflow_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::OpenInviteWorkflow {
        request_id,
        room_id,
    })
}

pub(super) fn build_close_invite_workflow_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::App(AppCommand::CloseInviteWorkflow { request_id })
}

pub(super) fn build_search_invite_targets_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    query: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SearchInviteTargets {
        request_id,
        room_id,
        query,
    })
}

pub(super) fn build_set_invite_scope_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    scope: InviteScopeSelection,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SetInviteScope {
        request_id,
        room_id,
        scope,
    })
}

pub(super) fn build_select_invite_target_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    user_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SelectInviteTarget {
        request_id,
        room_id,
        user_id,
    })
}

pub(super) fn build_remove_invite_target_command(
    request_id: koushi_core::RequestId,
    user_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::RemoveInviteTarget {
        request_id,
        user_id,
    })
}

pub(super) fn build_invite_targets_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    user_ids: Vec<String>,
    scope: InviteScopeSelection,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::InviteTargets {
        request_id,
        room_id,
        user_ids,
        scope,
    })
}

pub(super) async fn wait_for_room_joined(
    event_conn: &mut CoreConnection,
    operation_request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "room join did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Room(koushi_core::RoomEvent::RoomJoined {
                request_id,
                room_id,
            })) if request_id == operation_request_id => {
                return Ok(room_id);
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == operation_request_id => {
                return Err(invoke_error_from_core_failure("room join failed", failure));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

pub(super) fn build_query_directory_command(
    request_id: koushi_core::RequestId,
    term: Option<String>,
    server_name: Option<String>,
    limit: Option<u32>,
    since: Option<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::QueryDirectory {
        request_id,
        query: DirectoryQuery {
            term: optional_non_blank(term),
            server_name: optional_non_blank(server_name),
            limit,
            since: optional_non_blank(since),
        },
    })
}

pub(super) fn build_preview_join_target_command(
    request_id: koushi_core::RequestId,
    room_id_or_alias: String,
    via_servers: Vec<String>,
) -> Option<CoreCommand> {
    let (room_id_or_alias, via_servers) = normalize_join_target(room_id_or_alias, via_servers)?;
    Some(CoreCommand::Room(RoomCommand::PreviewJoinTarget {
        request_id,
        room_id_or_alias,
        via_servers,
    }))
}

pub(super) fn build_dismiss_directory_preview_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::DismissDirectoryPreview { request_id })
}

pub(super) fn build_join_directory_room_command(
    request_id: koushi_core::RequestId,
    room_id_or_alias: String,
    via_servers: Vec<String>,
) -> Option<CoreCommand> {
    let (room_id_or_alias, via_servers) = normalize_join_target(room_id_or_alias, via_servers)?;
    Some(CoreCommand::Room(RoomCommand::JoinDirectoryRoom {
        request_id,
        room_id_or_alias,
        via_servers,
    }))
}

#[cfg(test)]
fn commands_source() -> String {
    crate::commands::contracts::production_source()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::{ScriptedSelectSource, fake_request_id};
    use koushi_state::{AppState, RoomSummary, RoomTags};
    use std::collections::VecDeque;

    fn dm_room_summary(room_id: &str) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: "DM".to_owned(),
            display_label: "DM".to_owned(),
            original_display_label: "DM".to_owned(),
            avatar: None,
            is_dm: true,
            dm_user_ids: vec!["@dm-target:example.invalid".to_owned()],
            tags: RoomTags::default(),
            unread_count: 0,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: None,
            conversation_activity: None,
            latest_event: None,
            parent_space_ids: vec![],
            dm_space_ids: vec![],
            is_encrypted: false,
            joined_members: 2,
        }
    }

    struct SequencedSelectSource {
        snapshots: std::sync::Mutex<VecDeque<AppState>>,
        events: VecDeque<Result<CoreEvent, koushi_core::EventStreamLag>>,
    }

    impl SelectEventSource for SequencedSelectSource {
        fn snapshot(&self) -> AppState {
            let mut snapshots = self.snapshots.lock().expect("snapshots lock");
            if snapshots.len() > 1 {
                snapshots.pop_front().expect("non-empty snapshot queue")
            } else {
                snapshots
                    .front()
                    .cloned()
                    .expect("at least one scripted snapshot")
            }
        }

        fn recv_event(
            &mut self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<CoreEvent, koushi_core::EventStreamLag>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(std::future::ready(self.events.pop_front().unwrap_or_else(
                || Err(koushi_core::EventStreamLag { skipped: 0 }),
            )))
        }
    }

    #[tokio::test]
    async fn direct_message_start_returns_the_started_room_id() {
        let request_id = fake_request_id(91);
        let mut source = ScriptedSelectSource {
            snapshot: AppState::default(),
            events: VecDeque::from([
                Ok(CoreEvent::Room(koushi_core::RoomEvent::RoomJoined {
                    request_id: fake_request_id(90),
                    room_id: "!other:example.invalid".to_owned(),
                })),
                Ok(CoreEvent::Room(
                    koushi_core::RoomEvent::DirectMessageStarted {
                        request_id,
                        room_id: "!dm:example.invalid".to_owned(),
                    },
                )),
            ]),
        };

        let room_id = super::wait_for_direct_message_started(
            &mut source,
            request_id,
            std::time::Duration::from_millis(50),
        )
        .await
        .expect("started event should resolve the room id");
        assert_eq!(room_id, "!dm:example.invalid");
    }

    #[tokio::test]
    async fn direct_message_start_surfaces_the_correlated_failure() {
        let request_id = fake_request_id(92);
        let mut source = ScriptedSelectSource {
            snapshot: AppState::default(),
            events: VecDeque::from([Ok(CoreEvent::OperationFailed {
                request_id,
                failure: koushi_core::CoreFailure::SessionRequired,
            })]),
        };

        let error = super::wait_for_direct_message_started(
            &mut source,
            request_id,
            std::time::Duration::from_millis(50),
        )
        .await
        .expect_err("correlated failure must fail the wait");
        assert!(error.contains("direct message start failed"));
    }

    #[tokio::test]
    async fn direct_message_room_wait_settles_after_the_room_projection_arrives() {
        let room_id = "!dm:example.invalid";
        let mut with_room = AppState::default();
        with_room.rooms = vec![dm_room_summary(room_id)];
        let mut source = SequencedSelectSource {
            snapshots: std::sync::Mutex::new(VecDeque::from([AppState::default(), with_room])),
            events: VecDeque::from([Ok(CoreEvent::StateChanged(AppState::default()))]),
        };

        super::wait_for_room_in_state(&mut source, room_id, std::time::Duration::from_millis(50))
            .await
            .expect("the wait settles once the room-list projection has the room");
    }

    #[test]
    fn start_direct_message_selects_the_resolved_room_before_returning() {
        let source = include_str!("room.rs");
        let body = source
            .split("pub async fn start_direct_message")
            .nth(1)
            .expect("start_direct_message body")
            .split("#[tauri::command]")
            .next()
            .expect("body ends at the next command");
        let started = body
            .find("wait_for_direct_message_started")
            .expect("resolves the started room id");
        let in_state = body
            .find("wait_for_room_in_state")
            .expect("waits for the room-list projection");
        let select = body
            .find("build_select_room_command")
            .expect("selects the resolved room");
        let selected = body
            .find("wait_for_selected_room")
            .expect("waits for the selection to settle");
        assert!(
            started < in_state && in_state < select && select < selected,
            "DM start must resolve the room, wait for its projection, then select it"
        );
    }

    #[test]
    fn join_directory_room_waits_for_backend_selected_room() {
        let source = commands_source();
        let fn_name = "pub async fn join_directory_room";
        let fn_offset = source
            .find(fn_name)
            .expect("join_directory_room command should exist");
        let rest = &source[fn_offset..];
        let end = rest
            .find("pub async fn set_space_child")
            .expect("next command should exist");
        let join_source = &rest[..end];
        let joined_offset = join_source
            .find("wait_for_room_joined")
            .expect("directory join should wait for RoomJoined");
        let selected_offset = join_source
            .find("wait_for_selected_room")
            .expect("directory join should wait for selected-room state");

        assert!(
            joined_offset < selected_offset,
            "join should learn the joined room id before waiting for selection"
        );
        assert!(
            join_source.contains("joined_room_id"),
            "joined room id should be carried into selected-room wait"
        );
        assert!(
            join_source.contains("SELECT_ROOM_EVENT_TIMEOUT"),
            "selected-room wait should be bounded"
        );
    }
}
