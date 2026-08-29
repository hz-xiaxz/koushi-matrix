use super::room::{
    ROOM_OPERATION_EVENT_TIMEOUT, build_refresh_pinned_events_command, wait_for_room_operation,
};
use super::timeline::{
    build_observe_timeline_viewport_command, build_open_timeline_at_timestamp_command,
    build_update_navigation_scroll_anchor_command,
};
use super::*;
#[tauri::command]
pub async fn select_space(
    space_id: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let started = std::time::Instant::now();
    let requested_space_id = space_id.clone();
    let request_id = next_request_id(state.inner()).await;
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "desktop.space.transition", "submit")
            .field(DiagnosticField::request_id(
                "request_id",
                request_id.connection_id.0,
                request_id.sequence,
            ))
            .field(DiagnosticField::boolean(
                "target_present",
                requested_space_id.is_some(),
            )),
    );
    submit_core_command(
        state.inner(),
        build_select_space_command(request_id, space_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    let snapshot = current_snapshot(state.inner()).await?;
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "desktop.space.transition",
            "snapshot",
        )
        .field(DiagnosticField::request_id(
            "request_id",
            request_id.connection_id.0,
            request_id.sequence,
        ))
        .field(DiagnosticField::milliseconds(
            "elapsed_ms",
            started.elapsed().as_millis(),
        ))
        .field(DiagnosticField::boolean(
            "active_space_selected",
            snapshot.state.ui.navigation.active_space_id.as_deref()
                == requested_space_id.as_deref(),
        ))
        .field(DiagnosticField::boolean(
            "active_room_present",
            snapshot.state.ui.navigation.active_room_id.is_some(),
        )),
    );
    Ok(snapshot)
}

#[tauri::command]
pub async fn reorder_spaces(
    space_ids: Vec<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_reorder_spaces_command(request_id, space_ids),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn select_room(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let selected_room_id = room_id.clone();
    let mut event_conn = state.runtime.attach();
    let selected_snapshot = event_conn
        .select_room_and_wait(selected_room_id.clone(), SELECT_ROOM_EVENT_TIMEOUT)
        .await
        .map_err(invoke_error_from_select_room_error)?;
    let refresh_request_id = event_conn.next_request_id();
    event_conn
        .command(build_refresh_pinned_events_command(
            refresh_request_id,
            selected_room_id.clone(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_room_operation(
        &mut event_conn,
        refresh_request_id,
        ROOM_OPERATION_EVENT_TIMEOUT,
        |event, _| {
            matches!(
                event,
                RoomEvent::PinnedEventsUpdated {
                    room_id: updated_room_id,
                    ..
                } if updated_room_id == &selected_room_id
            )
        },
        "pinned messages refresh did not complete",
        "pinned messages refresh failed",
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(FrontendDesktopSnapshot::from_versioned(
        selected_snapshot.state,
        selected_snapshot.generation,
    ))
}

#[tauri::command]
pub async fn open_activity_event(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    open_anchored_timeline(room_id, event_id, app, state, true).await
}

#[tauri::command]
pub async fn open_pinned_event(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    open_anchored_timeline(room_id, event_id, app, state, false).await
}

#[tauri::command]
pub async fn select_search_result(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    open_anchored_timeline(room_id, event_id, app, state, true).await
}

async fn open_anchored_timeline(
    room_id: String,
    event_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
    allow_live_fallback: bool,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();

    let close_request_id = event_conn.next_request_id();
    event_conn
        .command(CoreCommand::App(AppCommand::CloseFocusedContext {
            request_id: close_request_id,
        }))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_focused_context_closed(
        &mut event_conn,
        close_request_id,
        FOCUSED_CONTEXT_EVENT_TIMEOUT,
    )
    .await?;

    event_conn
        .select_room_and_wait(room_id.clone(), SELECT_ROOM_EVENT_TIMEOUT)
        .await
        .map_err(invoke_error_from_select_room_error)?;

    let open_request_id = event_conn.next_request_id();
    event_conn
        .command(CoreCommand::App(AppCommand::OpenAnchoredTimeline {
            request_id: open_request_id,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
            allow_live_fallback,
        }))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    let anchored_snapshot = wait_for_main_timeline_anchor(
        &mut event_conn,
        open_request_id,
        &room_id,
        &event_id,
        allow_live_fallback,
        FOCUSED_CONTEXT_EVENT_TIMEOUT,
    )
    .await?;

    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(FrontendDesktopSnapshot::from_versioned(
        anchored_snapshot.state,
        anchored_snapshot.generation,
    ))
}

#[tauri::command]
pub async fn acknowledge_timeline_projection(
    projection_request_id: RequestId,
    key: TimelineKey,
    generation: TimelineGeneration,
    item_count: u64,
    target_present: bool,
    state: State<'_, CoreRuntimeState>,
) -> Result<(), String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::App(AppCommand::AcknowledgeTimelineProjection {
            request_id,
            projection_request_id,
            key,
            generation,
            item_count,
            target_present,
        }),
    )
    .await
}

#[tauri::command]
pub async fn acknowledge_timeline_batch_rendered(
    key: TimelineKey,
    actor_generation: u64,
    timeline_generation: TimelineGeneration,
    repair_generation: u64,
    batch_id: TimelineBatchId,
    state: State<'_, CoreRuntimeState>,
) -> Result<(), String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::App(AppCommand::AcknowledgeTimelineBatchRendered {
            request_id,
            key,
            actor_generation,
            timeline_generation,
            repair_generation,
            batch_id,
        }),
    )
    .await
}

#[tauri::command]
pub async fn close_focused_context(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(CoreCommand::App(AppCommand::CloseFocusedContext {
            request_id,
        }))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_focused_context_closed(&mut event_conn, request_id, FOCUSED_CONTEXT_EVENT_TIMEOUT)
        .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn open_timeline_at_timestamp(
    room_id: String,
    timestamp_ms: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let focused_room_id = room_id.clone();
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_open_timeline_at_timestamp_command(
            request_id,
            room_id,
            timestamp_ms,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_focused_context(
        &mut event_conn,
        request_id,
        &focused_room_id,
        FOCUSED_CONTEXT_EVENT_TIMEOUT,
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn update_navigation_scroll_anchor(
    room_id: String,
    anchor: koushi_state::TimelineScrollAnchor,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<(), String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_update_navigation_scroll_anchor_command(request_id, room_id, anchor),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(())
}

pub(super) fn invoke_error_from_select_room_error(error: koushi_core::SelectRoomError) -> String {
    match error {
        koushi_core::SelectRoomError::CommandSubmit(error) => {
            format!("command submit failed: {error}")
        }
        koushi_core::SelectRoomError::SessionNotReady => "session not ready".to_owned(),
        koushi_core::SelectRoomError::RoomNotInState => "room not yet loaded".to_owned(),
        koushi_core::SelectRoomError::FailedNoOp(IntentNoOpReason::TimelineTargetMissing) => {
            "timeline target not available".to_owned()
        }
        koushi_core::SelectRoomError::FailedNoOp(IntentNoOpReason::AlreadyActive) => {
            "room selection did not complete".to_owned()
        }
        koushi_core::SelectRoomError::FailedNoOp(
            IntentNoOpReason::SessionNotReady
            | IntentNoOpReason::RoomNotInState
            | IntentNoOpReason::Superseded,
        ) => "room selection did not complete".to_owned(),
        koushi_core::SelectRoomError::OperationFailed(failure) => {
            invoke_error_from_core_failure("room selection failed", failure)
        }
        koushi_core::SelectRoomError::EventStreamClosed | koushi_core::SelectRoomError::Timeout => {
            "room selection did not complete".to_owned()
        }
    }
}

#[tauri::command]
pub async fn observe_timeline_viewport(
    room_id: String,
    first_visible_event_id: Option<String>,
    last_visible_event_id: Option<String>,
    visible_gap_ids: Vec<TimelineGapId>,
    at_bottom: bool,
    thread_root_event_id: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<(), String> {
    let account_key = account_key_from_snapshot(state.inner()).await;
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_observe_timeline_viewport_command(
            request_id,
            account_key,
            room_id,
            first_visible_event_id,
            last_visible_event_id,
            visible_gap_ids,
            at_bottom,
            thread_root_event_id,
        ),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(())
}

pub(super) trait SelectEventSource {
    fn snapshot(&self) -> koushi_state::AppState;

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + '_>>;
}

impl SelectEventSource for CoreConnection {
    fn snapshot(&self) -> koushi_state::AppState {
        CoreConnection::snapshot(self)
    }

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + '_>> {
        Box::pin(CoreConnection::recv_event(self))
    }
}

fn snapshot_has_focused_context(snapshot: &koushi_state::AppState, room_id: &str) -> bool {
    match &snapshot.focused_context {
        FocusedContextState::Opening {
            room_id: focused_room_id,
            ..
        }
        | FocusedContextState::Open {
            room_id: focused_room_id,
            ..
        } => focused_room_id == room_id,
        FocusedContextState::Closed => false,
    }
}

fn snapshot_has_no_focused_context(snapshot: &koushi_state::AppState) -> bool {
    snapshot.focused_context == FocusedContextState::Closed
        && snapshot.navigation.main_timeline_anchor.is_none()
}

fn snapshot_has_main_timeline_anchor(
    snapshot: &koushi_state::AppState,
    room_id: &str,
    event_id: &str,
) -> bool {
    snapshot.navigation.active_room_id.as_deref() == Some(room_id)
        && snapshot
            .navigation
            .main_timeline_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.event_id == event_id)
}

fn snapshot_has_live_main_timeline(snapshot: &koushi_state::AppState, room_id: &str) -> bool {
    snapshot.navigation.active_room_id.as_deref() == Some(room_id)
        && snapshot.focused_context == FocusedContextState::Closed
        && snapshot.navigation.main_timeline_anchor.is_none()
}

fn snapshot_matches_main_timeline_settlement(
    snapshot: &koushi_state::AppState,
    room_id: &str,
    event_id: &str,
    settlement: Option<MainTimelineSettlement>,
) -> bool {
    match settlement {
        Some(MainTimelineSettlement::Anchor) | None => {
            snapshot_has_main_timeline_anchor(snapshot, room_id, event_id)
        }
        Some(MainTimelineSettlement::LiveFallback) => {
            snapshot_has_live_main_timeline(snapshot, room_id)
        }
    }
}

async fn wait_for_focused_context_closed(
    event_conn: &mut CoreConnection,
    request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_no_focused_context(&event_conn.snapshot()) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "focused context did not close".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(snapshot)) if snapshot_has_no_focused_context(&snapshot) => {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "focused context close failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) if snapshot_has_no_focused_context(&event_conn.snapshot()) => {
                return Ok(());
            }
            Err(_) => continue,
        }
    }
}

async fn wait_for_focused_context(
    event_conn: &mut CoreConnection,
    request_id: RequestId,
    room_id: &str,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_focused_context(&event_conn.snapshot(), room_id) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "focused context did not open".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(snapshot))
                if snapshot_has_focused_context(&snapshot, room_id) =>
            {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "focused context open failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) if snapshot_has_focused_context(&event_conn.snapshot(), room_id) => {
                return Ok(());
            }
            Err(_) => continue,
        }
    }
}

async fn wait_for_main_timeline_anchor(
    event_conn: &mut CoreConnection,
    request_id: RequestId,
    room_id: &str,
    event_id: &str,
    allow_live_fallback: bool,
    timeout: std::time::Duration,
) -> Result<koushi_core::event::VersionedAppStateSnapshot, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut settlement = None;

    loop {
        let current = event_conn.versioned_snapshot();
        if snapshot_matches_main_timeline_settlement(&current.state, room_id, event_id, settlement)
        {
            return Ok(current);
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "main timeline anchor did not open".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(_)) => {}
            Ok(CoreEvent::IntentLifecycle {
                request_id: settled_request_id,
                outcome: IntentOutcome::Committed,
                ..
            }) if settled_request_id == request_id => {
                settlement = Some(MainTimelineSettlement::Anchor);
            }
            Ok(CoreEvent::IntentLifecycle {
                request_id: settled_request_id,
                outcome: IntentOutcome::BenignNoOp(IntentNoOpReason::TimelineTargetMissing),
                ..
            }) if settled_request_id == request_id => {
                if allow_live_fallback {
                    settlement = Some(MainTimelineSettlement::LiveFallback);
                } else {
                    return Err("pinned event is not available in the timeline".to_owned());
                }
            }
            Ok(CoreEvent::IntentLifecycle {
                request_id: settled_request_id,
                outcome: IntentOutcome::FailedNoOp(_),
                ..
            }) if settled_request_id == request_id => {
                return Err("main timeline anchor open failed".to_owned());
            }
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "main timeline anchor open failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) => {
                let current = event_conn.versioned_snapshot();
                if snapshot_matches_main_timeline_settlement(
                    &current.state,
                    room_id,
                    event_id,
                    settlement,
                ) {
                    return Ok(current);
                }
            }
        }
    }
}

pub(super) fn build_select_space_command(
    request_id: koushi_core::RequestId,
    space_id: Option<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::SelectSpace {
        request_id,
        space_id,
    })
}

pub(super) fn build_reorder_spaces_command(
    request_id: koushi_core::RequestId,
    space_ids: Vec<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ReorderSpaces {
        request_id,
        space_ids,
    })
}

#[cfg(test)]
pub(super) fn build_select_room_command(
    request_id: koushi_core::RequestId,
    room_id: String,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::SelectRoom {
        request_id,
        room_id,
    })
}

pub(super) const SELECT_ROOM_EVENT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10);

const FOCUSED_CONTEXT_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone, Copy)]
enum MainTimelineSettlement {
    Anchor,
    LiveFallback,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::fake_request_id;

    #[test]
    fn open_timeline_at_timestamp_command_routes_through_app_command() {
        let command = build_open_timeline_at_timestamp_command(
            fake_request_id(40),
            "!room:example.org".to_owned(),
            1_718_000_000_000,
        );

        match command {
            CoreCommand::App(AppCommand::OpenTimelineAtTimestamp {
                request_id,
                room_id,
                timestamp_ms,
            }) => {
                assert_eq!(request_id, fake_request_id(40));
                assert_eq!(room_id, "!room:example.org");
                assert_eq!(timestamp_ms, 1_718_000_000_000);
                let debug = format!(
                    "{:?}",
                    AppCommand::OpenTimelineAtTimestamp {
                        request_id,
                        room_id,
                        timestamp_ms,
                    }
                );
                assert!(!debug.contains("!room:example.org"), "{debug}");
                assert!(!debug.contains("1718000000000"), "{debug}");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn observe_timeline_viewport_command_routes_viewport_facts_only() {
        let account_key = AccountKey("@alice:example.org".to_owned());
        let command = build_observe_timeline_viewport_command(
            fake_request_id(41),
            account_key.clone(),
            "!room:example.org".to_owned(),
            Some("$first".to_owned()),
            Some("$last".to_owned()),
            vec![koushi_core::TimelineGapId {
                topology_revision: 7,
                ordinal: 2,
            }],
            false,
            None,
        );
        let debug = format!("{command:?}");
        assert!(!debug.contains("!room:example.org"), "{debug}");
        assert!(!debug.contains("$first"), "{debug}");
        assert!(!debug.contains("$last"), "{debug}");

        match command {
            CoreCommand::Timeline(TimelineCommand::ObserveViewport {
                request_id,
                key,
                observation,
            }) => {
                assert_eq!(request_id, fake_request_id(41));
                assert_eq!(key.account_key, account_key);
                assert_eq!(
                    key.kind,
                    koushi_core::TimelineKind::Room {
                        room_id: "!room:example.org".to_owned()
                    }
                );
                assert_eq!(
                    observation.first_visible_event_id.as_deref(),
                    Some("$first")
                );
                assert_eq!(observation.last_visible_event_id.as_deref(), Some("$last"));
                assert_eq!(
                    observation.visible_gap_ids,
                    vec![koushi_core::TimelineGapId {
                        topology_revision: 7,
                        ordinal: 2,
                    }]
                );
                assert!(!observation.at_bottom);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn observe_timeline_viewport_routes_thread_identity() {
        let command = build_observe_timeline_viewport_command(
            fake_request_id(43),
            AccountKey("@alice:example.org".to_owned()),
            "!room:example.org".to_owned(),
            Some("$reply".to_owned()),
            Some("$reply".to_owned()),
            Vec::new(),
            true,
            Some("$root".to_owned()),
        );
        let CoreCommand::Timeline(TimelineCommand::ObserveViewport { key, .. }) = command else {
            panic!("expected observe viewport command");
        };
        assert_eq!(
            key.kind,
            koushi_core::TimelineKind::Thread {
                room_id: "!room:example.org".to_owned(),
                root_event_id: "$root".to_owned(),
            }
        );
    }

    #[test]
    fn observe_timeline_viewport_parses_full_range_topology_revision() {
        let visible_gap_ids: Vec<koushi_core::TimelineGapId> =
            serde_json::from_value(serde_json::json!([{
                "topology_revision": "14695981039346656037",
                "ordinal": 0,
            }]))
            .expect("Tauri viewport gap ids parse from their JSON wire shape");

        let command = build_observe_timeline_viewport_command(
            fake_request_id(42),
            AccountKey("@alice:example.org".to_owned()),
            "!room:example.org".to_owned(),
            None,
            None,
            visible_gap_ids,
            false,
            None,
        );

        let CoreCommand::Timeline(TimelineCommand::ObserveViewport { observation, .. }) = command
        else {
            panic!("expected observe viewport command");
        };
        assert_eq!(
            observation.visible_gap_ids,
            vec![koushi_core::TimelineGapId {
                topology_revision: 14_695_981_039_346_656_037,
                ordinal: 0,
            }]
        );
    }
}

#[cfg(test)]
mod issue551_moved_tests {
    use koushi_state::AppState;

    #[test]
    fn main_timeline_lifecycle_requires_the_matching_settled_snapshot() {
        let room_id = "!room:example.invalid";
        let event_id = "$event:example.invalid";
        let mut state = AppState::default();
        state.navigation.active_room_id = Some(room_id.to_owned());

        assert!(!super::snapshot_matches_main_timeline_settlement(
            &state,
            room_id,
            event_id,
            Some(super::MainTimelineSettlement::Anchor),
        ));
        state.navigation.main_timeline_anchor = Some(koushi_state::MainTimelineAnchor {
            event_id: event_id.to_owned(),
        });
        assert!(super::snapshot_matches_main_timeline_settlement(
            &state,
            room_id,
            event_id,
            Some(super::MainTimelineSettlement::Anchor),
        ));

        state.navigation.main_timeline_anchor = None;
        state.focused_context = koushi_state::FocusedContextState::Opening {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        };
        assert!(!super::snapshot_matches_main_timeline_settlement(
            &state,
            room_id,
            event_id,
            Some(super::MainTimelineSettlement::LiveFallback),
        ));
        state.focused_context = koushi_state::FocusedContextState::Closed;
        assert!(super::snapshot_matches_main_timeline_settlement(
            &state,
            room_id,
            event_id,
            Some(super::MainTimelineSettlement::LiveFallback),
        ));
    }
}
