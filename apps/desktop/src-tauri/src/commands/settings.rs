use super::*;
#[cfg(test)]
use crate::commands::contracts::fake_request_id;
#[cfg(test)]
use koushi_state::{AppearanceSettings, LocaleSettings, TextDirectionPreference, ThemePreference};

#[tauri::command]
pub async fn update_settings(
    account_tab_id: Option<String>,
    patch: SettingsPatch,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_update_settings_command(request_id, patch),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn import_legacy_settings(
    account_tab_id: Option<String>,
    patch: SettingsPatch,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_import_legacy_settings_command(request_id, patch),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn rebuild_search_index(
    account_tab_id: Option<String>,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_rebuild_search_index_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn set_room_url_preview_override(
    account_tab_id: Option<String>,
    room_id: String,
    enabled: bool,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_set_room_url_preview_override_command(request_id, room_id, enabled),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn set_notification_delivery_secret(
    account_tab_id: Option<String>,
    sink_id: String,
    secret: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_set_notification_delivery_secret_command(request_id, sink_id, secret),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

/// #1468: the settings screen's single Save for one destination. The secret, when present,
/// crosses this boundary once and is moved straight into the core command.
#[tauri::command]
pub async fn save_notification_sink(
    account_tab_id: Option<String>,
    sink: koushi_state::NotificationSinkSettings,
    secret: Option<String>,
    accept_content_consent: bool,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_save_notification_sink_command(request_id, sink, secret, accept_content_consent),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

/// What the destination editor's preview shows (#1492): the sender name, the subject or
/// ServerChan title, and the body, exactly as the digest renderer writes them.
#[derive(Clone, serde::Serialize)]
pub struct NotificationDeliveryPreviewDto {
    pub sender_name: String,
    pub subject: String,
    pub body: String,
}

/// #1492: render the catalog's sample direct message through the digest renderer under
/// the unsaved draft. It needs no account, stores nothing, and reads no credential; it
/// reads only the selected locale, so the preview is worded as a send would be (#1476).
#[tauri::command]
pub async fn preview_notification_delivery(
    sink: koushi_state::NotificationSinkSettings,
    partner_user_id: String,
    partner_alias: String,
    message_text: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<NotificationDeliveryPreviewDto, String> {
    use koushi_core::notification_delivery::digest::{
        NotificationDeliveryPreviewSample, preview_digest,
    };
    let locale = koushi_state::resolve_catalog_locale(
        &state
            .connection
            .lock()
            .await
            .snapshot()
            .settings
            .values
            .locale,
    );
    let preview = preview_digest(
        &sink,
        &NotificationDeliveryPreviewSample {
            partner_user_id,
            partner_alias,
            message_text,
        },
        locale,
    );
    Ok(NotificationDeliveryPreviewDto {
        sender_name: preview.sender_name,
        subject: preview.subject,
        body: preview.body,
    })
}

#[tauri::command]
pub async fn clear_notification_delivery_secret(
    account_tab_id: Option<String>,
    sink_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_clear_notification_delivery_secret_command(request_id, sink_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn test_notification_delivery(
    account_tab_id: Option<String>,
    sink_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id_for(state.inner(), account_tab_id.as_deref()).await?;
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_test_notification_delivery_command(request_id, sink_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

pub(super) fn build_update_settings_command(
    request_id: koushi_protocol::RequestId,
    patch: SettingsPatch,
) -> CoreCommand {
    CoreCommand::App(AppCommand::UpdateSettings { request_id, patch })
}

pub(super) fn build_import_legacy_settings_command(
    request_id: koushi_protocol::RequestId,
    patch: SettingsPatch,
) -> CoreCommand {
    CoreCommand::App(AppCommand::ImportLegacySettings { request_id, patch })
}

pub(super) fn build_rebuild_search_index_command(
    request_id: koushi_protocol::RequestId,
) -> CoreCommand {
    CoreCommand::App(AppCommand::RebuildSearchIndex { request_id })
}

pub(super) fn build_set_room_url_preview_override_command(
    request_id: koushi_protocol::RequestId,
    room_id: String,
    enabled: bool,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SetRoomUrlPreviewOverride {
        request_id,
        room_id,
        enabled,
    })
}

/// #1427: the sink secret crosses this boundary once, in the command, and is moved straight into
/// the core command. Nothing here logs it, returns it, or copies it into an event.
pub(super) fn build_set_notification_delivery_secret_command(
    request_id: koushi_protocol::RequestId,
    sink_id: String,
    secret: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SetNotificationDeliverySecret {
        request_id,
        sink_id,
        secret: koushi_state::AuthSecret::new(secret),
    })
}

pub(super) fn build_save_notification_sink_command(
    request_id: koushi_protocol::RequestId,
    sink: koushi_state::NotificationSinkSettings,
    secret: Option<String>,
    accept_content_consent: bool,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SaveNotificationSink {
        request_id,
        sink,
        secret: secret.map(koushi_state::AuthSecret::new),
        accept_content_consent,
    })
}

pub(super) fn build_clear_notification_delivery_secret_command(
    request_id: koushi_protocol::RequestId,
    sink_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::ClearNotificationDeliverySecret {
        request_id,
        sink_id,
    })
}

pub(super) fn build_test_notification_delivery_command(
    request_id: koushi_protocol::RequestId,
    sink_id: String,
) -> CoreCommand {
    CoreCommand::App(AppCommand::TestNotificationDelivery {
        request_id,
        sink_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_settings_command_routes_patch_to_app_update_settings() {
        let command = build_update_settings_command(
            fake_request_id(23),
            SettingsPatch {
                appearance: Some(AppearanceSettings {
                    theme: ThemePreference::Dark,
                    ..AppearanceSettings::default()
                }),
                ..SettingsPatch::default()
            },
        );

        match command {
            CoreCommand::App(AppCommand::UpdateSettings { request_id, patch }) => {
                assert_eq!(request_id, fake_request_id(23));
                assert_eq!(
                    patch.appearance.expect("appearance patch").theme,
                    ThemePreference::Dark
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let debug = format!(
            "{:?}",
            build_update_settings_command(
                fake_request_id(24),
                SettingsPatch {
                    locale: Some(LocaleSettings {
                        language_tag: Some("ja-JP-private".to_owned()),
                        text_direction: TextDirectionPreference::Auto,
                    }),
                    ..SettingsPatch::default()
                },
            )
        );
        assert!(debug.contains("locale"), "{debug}");
        assert!(!debug.contains("ja-JP-private"), "{debug}");
    }

    #[test]
    fn import_legacy_settings_command_routes_to_the_typed_app_boundary() {
        let patch = SettingsPatch {
            appearance: Some(AppearanceSettings {
                density: koushi_state::DisplayDensity::Compact,
                ..AppearanceSettings::default()
            }),
            ..SettingsPatch::default()
        };
        let command = build_import_legacy_settings_command(fake_request_id(24), patch.clone());
        assert!(matches!(
            command,
            CoreCommand::App(AppCommand::ImportLegacySettings {
                request_id,
                patch: routed
            }) if request_id == fake_request_id(24) && routed == patch
        ));
    }

    #[test]
    fn set_room_url_preview_override_command_routes_to_app_state() {
        let command = build_set_room_url_preview_override_command(
            fake_request_id(24),
            "!room:example.invalid".to_owned(),
            false,
        );

        match command {
            CoreCommand::App(AppCommand::SetRoomUrlPreviewOverride {
                request_id,
                room_id,
                enabled,
            }) => {
                assert_eq!(request_id, fake_request_id(24));
                assert_eq!(room_id, "!room:example.invalid");
                assert!(!enabled);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let debug = format!(
            "{:?}",
            build_set_room_url_preview_override_command(
                fake_request_id(25),
                "!private-room:example.invalid".to_owned(),
                true,
            )
        );
        assert!(debug.contains("SetRoomUrlPreviewOverride"), "{debug}");
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
    }
}
