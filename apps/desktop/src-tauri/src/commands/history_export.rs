//! History export: the platform half of the Rust-owned archive export.
//!
//! The adapter owns the native folder dialog, the directory registration, and
//! the civil-date resolution. React sends the civil dates it showed, the IANA
//! time zone it named, and the catalog labels for the pages; this module turns
//! them into the Core request. The directory path never reaches the WebView.

use super::*;
use jiff::{Timestamp, civil::Date, tz::TimeZone};
use koushi_protocol::{HistoryExportLabels, HistoryExportRequest};
use koushi_state::{HistoryExportRange, HistoryExportScope};
use tauri_plugin_dialog::DialogExt;

#[cfg(any(debug_assertions, test))]
const QA_HISTORY_EXPORT_DIR_ENV: &str = "KOUSHI_QA_HISTORY_EXPORT_DIR";

const FALLBACK_TIME_ZONE: &str = "UTC";

/// What React asked to export.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryExportScopeInput {
    #[serde(rename_all = "camelCase")]
    Room { room_id: String },
    #[serde(rename_all = "camelCase")]
    Space { space_id: String },
}

impl HistoryExportScopeInput {
    fn into_scope(self) -> Result<HistoryExportScope, String> {
        let scope = match self {
            Self::Room { room_id } => HistoryExportScope::Room { room_id },
            Self::Space { space_id } => HistoryExportScope::Space { space_id },
        };
        let (HistoryExportScope::Room { room_id: id } | HistoryExportScope::Space { space_id: id }) =
            &scope;
        if id.trim().is_empty() {
            return Err("export target must not be blank".to_owned());
        }
        Ok(scope)
    }
}

/// The range the dialog showed: civil dates are `YYYY-MM-DD`, the end date is
/// inclusive, and `time_zone` is the IANA name displayed with them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryExportRangeInput {
    AllAvailable,
    #[serde(rename_all = "camelCase")]
    Period {
        start_date: String,
        end_date: String,
        time_zone: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FrontendHistoryExportStart {
    /// The user closed the folder dialog; nothing was submitted.
    Dismissed,
    #[serde(rename_all = "camelCase")]
    Submitted {
        request_id: u64,
        admission: FrontendCommandAdmission,
    },
}

/// The IANA name of the platform time zone, or `UTC` when the platform does
/// not name one. The export dialog shows it and sends it back unchanged.
#[tauri::command]
pub async fn history_export_time_zone() -> Result<String, String> {
    Ok(platform_time_zone_name())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_history(
    scope: HistoryExportScopeInput,
    range: HistoryExportRangeInput,
    labels: HistoryExportLabels,
    dialog_title: String,
    folder_name_stem: String,
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendHistoryExportStart, String> {
    let scope = scope.into_scope()?;
    let range = resolve_range(range)?;
    let time_zone = platform_time_zone();
    let Some(directory) = choose_directory(&app, &window, dialog_title).await? else {
        return Ok(FrontendHistoryExportStart::Dismissed);
    };
    let request_id = next_request_id(state.inner()).await;
    let command = build_export_history_command(
        request_id,
        HistoryExportRequest {
            scope,
            range,
            display_time_zone: platform_time_zone_name(),
            export_date_utc_offset_minutes: utc_offset_minutes(&time_zone, Timestamp::now()),
            folder_name_stem,
            labels,
        },
    );
    let admission = submit_core_command_with_native_artifact_path(
        state.inner(),
        request_id,
        NativeArtifactKind::HistoryExportDirectory,
        directory,
        command,
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(FrontendHistoryExportStart::Submitted {
        request_id: request_id.sequence,
        admission,
    })
}

#[tauri::command]
pub async fn stop_history_export(
    target_request_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendCommandAdmission, String> {
    let request_id = next_request_id(state.inner()).await;
    let target_request_id = koushi_protocol::RequestId {
        connection_id: request_id.connection_id,
        sequence: target_request_id,
    };
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_stop_history_export_command(request_id, target_request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(admission)
}

#[tauri::command]
pub async fn retry_history_export(
    target_request_id: u64,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendHistoryExportStart, String> {
    let request_id = next_request_id(state.inner()).await;
    let target_request_id = koushi_protocol::RequestId {
        connection_id: request_id.connection_id,
        sequence: target_request_id,
    };
    let admission = submit_core_command_with_admission(
        state.inner(),
        build_retry_history_export_command(request_id, target_request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(FrontendHistoryExportStart::Submitted {
        request_id: request_id.sequence,
        admission,
    })
}

pub(super) fn build_export_history_command(
    request_id: koushi_protocol::RequestId,
    request: HistoryExportRequest,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ExportHistory {
        request_id,
        request,
    })
}

pub(super) fn build_stop_history_export_command(
    request_id: koushi_protocol::RequestId,
    target_request_id: koushi_protocol::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::StopHistoryExport {
        request_id,
        target_request_id,
    })
}

pub(super) fn build_retry_history_export_command(
    request_id: koushi_protocol::RequestId,
    target_request_id: koushi_protocol::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::RetryHistoryExport {
        request_id,
        target_request_id,
    })
}

/// A folder: an earlier export folder to resume, or the parent to create a
/// new export folder in (Core decides from its manifest).
async fn choose_directory(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    dialog_title: String,
) -> Result<Option<PathBuf>, String> {
    #[cfg(any(debug_assertions, test))]
    if let Some(directory) = qa_history_export_dir() {
        // Unattended GUI QA cannot drive a native folder dialog.
        return Ok(Some(directory));
    }
    let mut dialog = app
        .dialog()
        .file()
        .set_parent(window)
        .set_title(dialog_title)
        .set_can_create_directories(true);
    if let Ok(downloads) = app.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |selected| {
        let _ = sender.send(selected);
    });
    match receiver.await {
        Ok(Some(selected)) => selected
            .into_path()
            .map(Some)
            .map_err(|_| "export folder is not a local directory".to_owned()),
        Ok(None) | Err(_) => Ok(None),
    }
}

#[cfg(any(debug_assertions, test))]
fn qa_history_export_dir() -> Option<PathBuf> {
    std::env::var_os(QA_HISTORY_EXPORT_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn platform_time_zone() -> TimeZone {
    TimeZone::try_system()
        .ok()
        .filter(|zone| zone.iana_name().is_some())
        .unwrap_or(TimeZone::UTC)
}

fn platform_time_zone_name() -> String {
    platform_time_zone()
        .iana_name()
        .unwrap_or(FALLBACK_TIME_ZONE)
        .to_owned()
}

/// Resolve the dialog's civil dates: the start is 00:00 of the start day and
/// the exclusive end is 00:00 of the day after the end day, both in the named
/// zone. Rust Core still validates the resulting range.
pub(super) fn resolve_range(input: HistoryExportRangeInput) -> Result<HistoryExportRange, String> {
    match input {
        HistoryExportRangeInput::AllAvailable => Ok(HistoryExportRange::AllAvailable),
        HistoryExportRangeInput::Period {
            start_date,
            end_date,
            time_zone,
        } => {
            let zone = TimeZone::get(time_zone.trim())
                .map_err(|_| "export time zone is not a known IANA name".to_owned())?;
            let start = parse_civil_date(&start_date)?;
            let end_exclusive = parse_civil_date(&end_date)?
                .tomorrow()
                .map_err(|_| "export end date is out of range".to_owned())?;
            Ok(HistoryExportRange::Period {
                start_ms: start_of_day_ms(start, &zone)?,
                end_exclusive_ms: start_of_day_ms(end_exclusive, &zone)?,
                time_zone: time_zone.trim().to_owned(),
            })
        }
    }
}

fn parse_civil_date(value: &str) -> Result<Date, String> {
    value
        .trim()
        .parse::<Date>()
        .map_err(|_| "export date must be YYYY-MM-DD".to_owned())
}

fn start_of_day_ms(date: Date, zone: &TimeZone) -> Result<u64, String> {
    // A midnight that falls in a DST gap resolves to the first instant after
    // the gap, which is still the first instant of that civil day.
    let instant = date
        .to_zoned(zone.clone())
        .map_err(|_| "export date is out of range".to_owned())?
        .timestamp()
        .as_millisecond();
    u64::try_from(instant).map_err(|_| "export date is before 1970".to_owned())
}

pub(super) fn utc_offset_minutes(zone: &TimeZone, now: Timestamp) -> i32 {
    zone.to_offset(now).seconds() / 60
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period(start: &str, end: &str, zone: &str) -> HistoryExportRangeInput {
        HistoryExportRangeInput::Period {
            start_date: start.to_owned(),
            end_date: end.to_owned(),
            time_zone: zone.to_owned(),
        }
    }

    #[test]
    fn period_resolves_to_midnights_in_the_named_zone_with_an_inclusive_end_day() {
        assert_eq!(
            resolve_range(period("2026-09-01", "2026-09-30", "Asia/Tokyo")),
            Ok(HistoryExportRange::Period {
                // 2026-08-31T15:00Z and 2026-09-30T15:00Z.
                start_ms: 1_788_188_400_000,
                end_exclusive_ms: 1_790_780_400_000,
                time_zone: "Asia/Tokyo".to_owned(),
            })
        );
        let Ok(HistoryExportRange::Period {
            start_ms,
            end_exclusive_ms,
            ..
        }) = resolve_range(period("2026-09-23", "2026-09-23", "UTC"))
        else {
            panic!("single-day period must resolve");
        };
        assert_eq!(end_exclusive_ms - start_ms, 86_400_000);
    }

    #[test]
    fn period_days_follow_daylight_saving_transitions() {
        // New York springs forward on 2026-03-08: that civil day has 23 hours.
        let Ok(HistoryExportRange::Period {
            start_ms,
            end_exclusive_ms,
            ..
        }) = resolve_range(period("2026-03-08", "2026-03-08", "America/New_York"))
        else {
            panic!("DST day must resolve");
        };
        assert_eq!(end_exclusive_ms - start_ms, 23 * 3_600_000);
    }

    #[test]
    fn invalid_period_inputs_are_rejected_before_any_dialog() {
        assert!(resolve_range(period("2026-09-01", "2026-09-30", "Not/AZone")).is_err());
        assert!(resolve_range(period("2026-9-1", "2026-09-30", "UTC")).is_err());
        assert!(resolve_range(period("2026-02-30", "2026-03-01", "UTC")).is_err());
        assert!(resolve_range(period("1969-12-31", "1970-01-01", "UTC")).is_err());
        assert_eq!(
            resolve_range(HistoryExportRangeInput::AllAvailable),
            Ok(HistoryExportRange::AllAvailable)
        );
    }

    #[test]
    fn an_end_before_the_start_resolves_to_a_range_core_rejects() {
        let range = resolve_range(period("2026-09-10", "2026-09-01", "UTC")).unwrap();
        assert!(!range.is_valid());
    }

    #[test]
    fn range_input_uses_the_frontend_wire_shape() {
        let input: HistoryExportRangeInput = serde_json::from_value(serde_json::json!({
            "kind": "period",
            "startDate": "2026-09-01",
            "endDate": "2026-09-02",
            "timeZone": "UTC"
        }))
        .unwrap();
        assert_eq!(input, period("2026-09-01", "2026-09-02", "UTC"));
        let all: HistoryExportRangeInput =
            serde_json::from_value(serde_json::json!({ "kind": "allAvailable" })).unwrap();
        assert_eq!(all, HistoryExportRangeInput::AllAvailable);
    }

    #[test]
    fn utc_offset_is_taken_at_the_given_instant() {
        let zone = TimeZone::get("America/New_York").unwrap();
        let winter: Timestamp = "2026-01-15T12:00:00Z".parse().unwrap();
        let summer: Timestamp = "2026-07-15T12:00:00Z".parse().unwrap();
        assert_eq!(utc_offset_minutes(&zone, winter), -300);
        assert_eq!(utc_offset_minutes(&zone, summer), -240);
        assert_eq!(
            utc_offset_minutes(&TimeZone::get("Asia/Kolkata").unwrap(), winter),
            330
        );
    }

    fn labels() -> HistoryExportLabels {
        HistoryExportLabels {
            edited: "(edited)".to_owned(),
            ..HistoryExportLabels::default()
        }
    }

    #[test]
    fn scope_and_labels_use_the_frontend_wire_shape() {
        let space: HistoryExportScopeInput =
            serde_json::from_value(serde_json::json!({ "kind": "space", "spaceId": "!s:x" }))
                .unwrap();
        assert_eq!(
            space.into_scope(),
            Ok(HistoryExportScope::Space {
                space_id: "!s:x".to_owned()
            })
        );
        let room: HistoryExportScopeInput =
            serde_json::from_value(serde_json::json!({ "kind": "room", "roomId": "!r:x" }))
                .unwrap();
        assert!(room.into_scope().is_ok());
        let blank: HistoryExportScopeInput =
            serde_json::from_value(serde_json::json!({ "kind": "room", "roomId": " " })).unwrap();
        assert!(blank.into_scope().is_err());
        let labels: HistoryExportLabels = serde_json::from_value(serde_json::json!({
            "edited": "(編集済み)", "inReplyTo": "{name} への返信", "timesInZone": "{timeZone}"
        }))
        .unwrap();
        assert_eq!(labels.edited, "(編集済み)");
        assert_eq!(labels.in_reply_to, "{name} への返信");
        assert_eq!(labels.times_in_zone, "{timeZone}");
    }

    #[test]
    fn export_stop_and_retry_build_correlated_commands_without_leaking_private_data() {
        let request_id = crate::commands::contracts::fake_request_id(51);
        let range = resolve_range(period("2026-09-01", "2026-09-02", "Asia/Tokyo")).unwrap();
        let command = build_export_history_command(
            request_id,
            HistoryExportRequest {
                scope: HistoryExportScope::Space {
                    space_id: "!private-space:example.invalid".to_owned(),
                },
                range: range.clone(),
                display_time_zone: "Asia/Tokyo".to_owned(),
                export_date_utc_offset_minutes: 540,
                folder_name_stem: "Private Lab".to_owned(),
                labels: labels(),
            },
        );
        assert_eq!(command.request_id(), request_id);
        let debug = format!("{command:?}");
        for private in ["private-space", "Asia/Tokyo", "Private Lab", "(edited)"] {
            assert!(!debug.contains(private), "{private} leaked: {debug}");
        }
        match command {
            CoreCommand::Account(AccountCommand::ExportHistory { request, .. }) => {
                assert_eq!(request.range, range);
                assert_eq!(request.export_date_utc_offset_minutes, 540);
                assert_eq!(request.labels.edited, "(edited)");
            }
            other => panic!("unexpected command: {other:?}"),
        }
        let target = crate::commands::contracts::fake_request_id(51);
        match build_stop_history_export_command(crate::commands::contracts::fake_request_id(52), target)
        {
            CoreCommand::Account(AccountCommand::StopHistoryExport {
                request_id,
                target_request_id,
            }) => {
                assert_eq!(request_id.sequence, 52);
                assert_eq!(target_request_id, target);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        match build_retry_history_export_command(crate::commands::contracts::fake_request_id(53), target)
        {
            CoreCommand::Account(AccountCommand::RetryHistoryExport {
                request_id,
                target_request_id,
            }) => {
                assert_eq!(request_id.sequence, 53);
                assert_eq!(target_request_id, target);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn start_result_uses_the_frontend_wire_shape() {
        assert_eq!(
            serde_json::to_value(FrontendHistoryExportStart::Dismissed).unwrap(),
            serde_json::json!({ "kind": "dismissed" })
        );
    }

    #[test]
    fn platform_time_zone_name_is_always_a_resolvable_iana_name() {
        assert!(TimeZone::get(&platform_time_zone_name()).is_ok());
    }
}
