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
        serde_json::from_value(serde_json::json!({ "kind": "space", "spaceId": "!s:x" })).unwrap();
    assert_eq!(
        space.into_scope(),
        Ok(HistoryExportScope::Space {
            space_id: "!s:x".to_owned()
        })
    );
    let room: HistoryExportScopeInput =
        serde_json::from_value(serde_json::json!({ "kind": "room", "roomId": "!r:x" })).unwrap();
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
    match build_retry_history_export_command(
        crate::commands::contracts::fake_request_id(53),
        target,
    ) {
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
