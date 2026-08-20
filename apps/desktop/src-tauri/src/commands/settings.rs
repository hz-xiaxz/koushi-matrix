use super::*;
#[cfg(test)]
use crate::commands::contracts::fake_request_id;
#[cfg(test)]
use koushi_state::{AppearanceSettings, LocaleSettings, TextDirectionPreference, ThemePreference};

#[tauri::command]
pub async fn update_settings(
    patch: SettingsPatch,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_update_settings_command(request_id, patch),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn rebuild_search_index(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_rebuild_search_index_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn set_room_url_preview_override(
    room_id: String,
    enabled: bool,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_set_room_url_preview_override_command(request_id, room_id, enabled),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) fn build_update_settings_command(
    request_id: koushi_core::RequestId,
    patch: SettingsPatch,
) -> CoreCommand {
    CoreCommand::App(AppCommand::UpdateSettings { request_id, patch })
}

pub(super) fn build_rebuild_search_index_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::App(AppCommand::RebuildSearchIndex { request_id })
}

pub(super) fn build_set_room_url_preview_override_command(
    request_id: koushi_core::RequestId,
    room_id: String,
    enabled: bool,
) -> CoreCommand {
    CoreCommand::App(AppCommand::SetRoomUrlPreviewOverride {
        request_id,
        room_id,
        enabled,
    })
}

#[cfg(test)]
fn commands_source() -> String {
    crate::commands::contracts::production_source()
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

    #[test]
    fn update_settings_tauri_command_contract_is_present() {
        let commands_source = commands_source();
        let lib_source = include_str!("../lib.rs");
        for (command_name, builder_name, route_name, registration_name) in [
            (
                "pub async fn update_settings",
                "build_update_settings_command",
                "AppCommand::UpdateSettings",
                "commands::settings::update_settings",
            ),
            (
                "pub async fn set_room_url_preview_override",
                "build_set_room_url_preview_override_command",
                "AppCommand::SetRoomUrlPreviewOverride",
                "commands::settings::set_room_url_preview_override",
            ),
        ] {
            assert!(
                commands_source.contains(command_name),
                "Tauri command should expose {command_name}"
            );
            assert!(
                commands_source.contains(builder_name),
                "Tauri command should keep a testable builder {builder_name}"
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

    #[test]
    fn rebuild_search_index_tauri_command_contract_is_present() {
        let commands_source = commands_source();
        let lib_source = include_str!("../lib.rs");

        assert!(
            commands_source.contains("pub async fn rebuild_search_index"),
            "Tauri command should expose search index rebuild"
        );
        assert!(
            commands_source.contains("build_rebuild_search_index_command"),
            "Tauri command should route through a testable builder"
        );
        assert!(
            commands_source.contains("AppCommand::RebuildSearchIndex"),
            "Tauri command should route through app state"
        );
        assert!(
            lib_source.contains("commands::settings::rebuild_search_index"),
            "Tauri command should be registered in generate_handler"
        );
    }
}
