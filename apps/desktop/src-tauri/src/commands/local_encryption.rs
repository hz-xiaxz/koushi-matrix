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
    let mut wait_conn = state.inner().runtime.attach();
    let baseline_generation = wait_conn.versioned_snapshot().generation;
    let account_key = account_key_from_app_state(&wait_conn.snapshot());
    let request_id = next_request_id(state.inner()).await;
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
    submit_core_command(state.inner(), build_reset_local_data_command(request_id)).await?;
    let outcome = wait_conn
        .wait_for_request_outcome(
            OutcomeCorrelation::Request(request_id),
            RequestOutcomeExpectation::SignedOut {
                request_id,
                account_key,
            },
            baseline_generation,
            tokio::time::Instant::now() + LOCAL_DATA_RESET_EVENT_TIMEOUT,
        )
        .await
        .map_err(|error| invoke_error_from_request_outcome("local data reset", error));
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
    let settled_snapshot = match outcome? {
        RequestOutcome::SignedOut { snapshot, .. } => snapshot,
        _ => return Err("local data reset returned an invalid outcome".to_owned()),
    };
    update_qa_window_title_from_state(&app, state.inner()).await;
    Ok(FrontendDesktopSnapshot::from_versioned(
        settled_snapshot.state,
        settled_snapshot.generation,
    ))
}

const LOCAL_DATA_RESET_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub(super) fn build_probe_local_encryption_health_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ProbeLocalEncryptionHealth { request_id })
}

pub(super) fn build_reset_local_data_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ResetLocalData { request_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::fake_request_id;

    #[test]
    fn credential_health_command_routes_to_account_state_machine() {
        match build_probe_local_encryption_health_command(fake_request_id(47)) {
            CoreCommand::Account(AccountCommand::ProbeLocalEncryptionHealth { request_id }) => {
                assert_eq!(request_id, fake_request_id(47));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn reset_local_data_command_routes_to_account_state_machine() {
        match build_reset_local_data_command(fake_request_id(48)) {
            CoreCommand::Account(AccountCommand::ResetLocalData { request_id }) => {
                assert_eq!(request_id, fake_request_id(48));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
