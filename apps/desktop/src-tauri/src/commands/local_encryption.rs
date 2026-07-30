use super::*;

#[tauri::command]
pub async fn probe_local_encryption_health(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_probe_local_encryption_health_command(request_id),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn reset_local_data(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.inner().runtime.attach();
    let request_id = event_conn.next_request_id();
    koushi_diagnostics::record_and_stderr(
        DiagnosticEvent::new(
            DiagnosticLevel::Info,
            "desktop.local_data_reset",
            "submitted",
        )
        .field(DiagnosticField::request_id(
            "request_id",
            request_id.connection_id.0,
            request_id.sequence,
        )),
    );
    event_conn
        .command(build_reset_local_data_command(request_id))
        .await
        .map_err(|error| format!("command submit failed: {error}"))?;
    let outcome =
        wait_for_local_data_reset(&mut event_conn, request_id, LOCAL_DATA_RESET_EVENT_TIMEOUT)
            .await;
    koushi_diagnostics::record_and_stderr(
        DiagnosticEvent::new(
            if outcome.is_ok() {
                DiagnosticLevel::Info
            } else {
                DiagnosticLevel::Error
            },
            "desktop.local_data_reset",
            "settled",
        )
        .field(DiagnosticField::request_id(
            "request_id",
            request_id.connection_id.0,
            request_id.sequence,
        ))
        .field(DiagnosticField::token(
            "outcome",
            if outcome.is_ok() { "success" } else { "failed" },
        )),
    );
    outcome?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}
