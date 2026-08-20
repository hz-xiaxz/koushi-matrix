use super::*;

#[tauri::command]
pub async fn set_display_name(
    display_name: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_display_name_command(request_id, display_name),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_local_user_alias(
    user_id: String,
    alias: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_local_user_alias_command(request_id, user_id, alias),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn ignore_user(
    user_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_ignore_user_command(request_id, user_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn unignore_user(
    user_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_unignore_user_command(request_id, user_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn report_user(
    user_id: String,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_report_user_command(request_id, user_id, optional_non_blank(reason)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn report_content(
    room_id: String,
    event_id: String,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_report_content_command(request_id, room_id, event_id, optional_non_blank(reason)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn report_room(
    room_id: String,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_report_room_command(request_id, room_id, optional_non_blank(reason)),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_avatar(
    mime_type: String,
    bytes: Vec<u8>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    if bytes.is_empty() {
        return current_snapshot(state.inner()).await;
    }

    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_avatar_command(request_id, mime_type, bytes),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn download_avatar_thumbnail(
    mxc_uri: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_download_avatar_thumbnail_command(request_id, mxc_uri),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) fn build_set_display_name_command(
    request_id: koushi_core::RequestId,
    display_name: Option<String>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SetDisplayName {
        request_id,
        display_name,
    })
}

pub(super) fn build_set_local_user_alias_command(
    request_id: koushi_core::RequestId,
    user_id: String,
    alias: Option<String>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SetLocalUserAlias {
        request_id,
        user_id,
        alias,
    })
}

pub(super) fn build_ignore_user_command(
    request_id: koushi_core::RequestId,
    user_id: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::IgnoreUser {
        request_id,
        user_id,
    })
}

pub(super) fn build_unignore_user_command(
    request_id: koushi_core::RequestId,
    user_id: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::UnignoreUser {
        request_id,
        user_id,
    })
}

pub(super) fn build_report_user_command(
    request_id: koushi_core::RequestId,
    user_id: String,
    reason: Option<String>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ReportUser {
        request_id,
        user_id,
        reason: reason.unwrap_or_default(),
    })
}

pub(super) fn build_report_content_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    event_id: String,
    reason: Option<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ReportContent {
        request_id,
        room_id,
        event_id,
        reason,
    })
}

pub(super) fn build_report_room_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    reason: Option<String>,
) -> CoreCommand {
    CoreCommand::Room(RoomCommand::ReportRoom {
        request_id,
        room_id,
        reason: reason.unwrap_or_default(),
    })
}

pub(super) fn build_set_avatar_command(
    request_id: koushi_core::RequestId,
    mime_type: String,
    bytes: Vec<u8>,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SetAvatar {
        request_id,
        request: SetAvatarRequest { mime_type, bytes },
    })
}

pub(super) fn build_download_avatar_thumbnail_command(
    request_id: koushi_core::RequestId,
    mxc_uri: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::DownloadAvatarThumbnail {
        request_id,
        mxc_uri,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_tauri_command_contracts_are_present() {
        let commands_source = commands_source();
        let lib_source = include_str!("../lib.rs");
        for (command_name, route_name, registration_name) in [
            (
                "pub async fn set_display_name",
                "build_set_display_name_command",
                "commands::profile::set_display_name",
            ),
            (
                "pub async fn set_local_user_alias",
                "build_set_local_user_alias_command",
                "commands::profile::set_local_user_alias",
            ),
            (
                "pub async fn set_avatar",
                "build_set_avatar_command",
                "commands::profile::set_avatar",
            ),
        ] {
            assert!(
                commands_source.contains(command_name),
                "Tauri command should expose {command_name}"
            );
            assert!(
                commands_source.contains(route_name),
                "Tauri command should route through {route_name}"
            );
            assert!(
                lib_source.contains(registration_name),
                "Tauri command should register {registration_name}"
            );
        }
    }
}
