use super::*;

#[tauri::command]
pub async fn open_activity(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_open_activity_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn close_activity(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_close_activity_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_activity_tab(
    tab: ActivityTab,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_activity_tab_command(request_id, tab),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn paginate_activity(
    tab: ActivityTab,
    cursor: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_paginate_activity_command(request_id, tab, cursor),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn retry_activity_resolution(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_retry_activity_resolution_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn mark_activity_read(
    target: ActivityMarkReadTarget,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_mark_activity_read_command(request_id, target),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) fn build_open_activity_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::App(AppCommand::OpenActivity { request_id })
}

pub(super) fn build_close_activity_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::App(AppCommand::CloseActivity { request_id })
}

pub(super) fn build_set_activity_tab_command(
    request_id: koushi_core::RequestId,
    tab: ActivityTab,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SetActivityTab { request_id, tab })
}

pub(super) fn build_paginate_activity_command(
    request_id: koushi_core::RequestId,
    tab: ActivityTab,
    cursor: Option<String>,
) -> CoreCommand {
    CoreCommand::App(AppCommand::PaginateActivity {
        request_id,
        tab,
        cursor: optional_non_blank(cursor),
    })
}

pub(super) fn build_mark_activity_read_command(
    request_id: koushi_core::RequestId,
    target: ActivityMarkReadTarget,
) -> CoreCommand {
    CoreCommand::App(AppCommand::MarkActivityRead { request_id, target })
}

pub(super) fn build_retry_activity_resolution_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::App(AppCommand::RetryActivityResolution { request_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::fake_request_id;

    #[test]
    fn open_activity_event_opens_anchored_main_timeline_without_room_resubscribe() {
        let source = commands_source();
        let fn_name = "async fn open_anchored_timeline";
        let open_token = "OpenAnchoredTimeline";

        let fn_offset = source
            .find(fn_name)
            .expect("open_activity_event command should exist");
        let rest = &source[fn_offset..];
        let end = rest
            .find("pub async fn acknowledge_timeline_projection")
            .expect("projection acknowledgement command should follow the shared helper");
        let command_source = &rest[..end];

        assert!(
            command_source.contains("build_select_room_command"),
            "activity event navigation should select the destination room"
        );
        assert!(
                !command_source.contains("build_subscribe_timeline_command"),
                "activity event navigation should rely on room selection reducers for timeline subscription"
            );
        assert!(
            command_source.contains(open_token),
            "activity event navigation should subscribe the focused event timeline"
        );
        assert!(!command_source.contains("EnterAnchoredTimeline"));
        assert!(!command_source.contains("wait_for_focused_timeline_event"));
        assert!(
            command_source.contains("wait_for_main_timeline_anchor"),
            "activity event navigation should wait for the main anchored timeline"
        );
        assert!(
                !command_source.contains("build_update_navigation_scroll_anchor_command"),
                "activity event navigation must not anchor an event that may be absent from the live timeline"
            );
    }

    #[test]
    fn open_activity_event_waits_before_opening_anchored_event_timeline() {
        let source = commands_source();
        let fn_name = "async fn open_anchored_timeline";
        let open_token = "OpenAnchoredTimeline";

        let fn_offset = source
            .find(fn_name)
            .expect("open_activity_event command should exist");
        let rest = &source[fn_offset..];
        let end = rest
            .find("pub async fn acknowledge_timeline_projection")
            .expect("projection acknowledgement command should follow the shared helper");
        let command_source = &rest[..end];

        let close_offset = command_source
            .find("CloseFocusedContext")
            .expect("activity event navigation should close any focused main timeline first");
        let wait_close_offset = command_source
            .find("wait_for_focused_context_closed")
            .expect(
                "activity event navigation must wait until focused context/main anchor is closed",
            );
        let select_offset = command_source
            .find("build_select_room_command")
            .expect("activity event navigation should select the destination room");
        let wait_select_offset = command_source
            .find("wait_for_selected_room")
            .expect("activity event navigation should wait for the selected room state");
        let open_offset = command_source
            .find(open_token)
            .expect("activity event navigation should open the focused event timeline");
        let wait_anchor_offset = command_source[open_offset..]
            .find("wait_for_main_timeline_anchor")
            .map(|offset| open_offset + offset)
            .expect("activity navigation should wait for the acknowledged Core anchor");

        assert!(
                close_offset < wait_close_offset
                    && wait_close_offset < select_offset
                    && select_offset < wait_select_offset
                    && wait_select_offset < open_offset
                    && open_offset < wait_anchor_offset,
                "activity event navigation must clear the previous owner, select the room, start one Core-owned focused navigation, then wait for its acknowledged anchor"
            );
    }
}
