use super::navigation::SelectEventSource;
use super::*;
#[cfg(test)]
use crate::commands::contracts::fake_request_id;

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

const LOCAL_DATA_RESET_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn snapshot_has_completed_local_data_reset(snapshot: &koushi_state::AppState) -> bool {
    matches!(snapshot.session, SessionState::SignedOut)
        && !matches!(
            snapshot.local_encryption,
            koushi_state::LocalEncryptionState::Resetting { .. }
        )
}

async fn wait_for_local_data_reset(
    event_conn: &mut impl SelectEventSource,
    request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_completed_local_data_reset(&event_conn.snapshot()) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "local data reset did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(snapshot))
                if snapshot_has_completed_local_data_reset(&snapshot) =>
            {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "local data reset failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) if snapshot_has_completed_local_data_reset(&event_conn.snapshot()) => {
                return Ok(());
            }
            Err(_) => continue,
        }
    }
}

pub(super) fn build_probe_local_encryption_health_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ProbeLocalEncryptionHealth { request_id })
}

pub(super) fn build_reset_local_data_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ResetLocalData { request_id })
}

#[cfg(test)]
fn commands_source() -> String {
    crate::commands::contracts::production_source()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn credential_health_tauri_command_contract_is_present() {
        let commands_source = commands_source();
        let lib_source = include_str!("../lib.rs");
        let command_name = "pub async fn probe_local_encryption_health";
        let builder_name = "build_probe_local_encryption_health_command";
        let route_name = "AccountCommand::ProbeLocalEncryptionHealth";
        let registration_name = "commands::local_encryption::probe_local_encryption_health";

        assert!(
            commands_source.contains(command_name),
            "Tauri command should expose probe_local_encryption_health"
        );
        assert!(
            commands_source.contains(builder_name),
            "Tauri command should keep a testable local encryption probe builder"
        );
        assert!(
            commands_source.contains(route_name),
            "Tauri command should route through the Rust credential health state machine"
        );
        assert!(
            lib_source.contains(registration_name),
            "Tauri command should be registered in generate_handler"
        );
    }

    #[test]
    fn reset_local_data_tauri_command_contract_is_present() {
        let commands_source = commands_source();
        let reset_command_source = include_str!("local_encryption.rs");
        let lib_source = include_str!("../lib.rs");
        let command_name = "pub async fn reset_local_data";
        let builder_name = "build_reset_local_data_command";
        let route_name = "AccountCommand::ResetLocalData";
        let registration_name = "commands::local_encryption::reset_local_data";

        assert!(
            commands_source.contains(command_name),
            "Tauri command should expose reset_local_data"
        );
        assert!(
            commands_source.contains(builder_name),
            "Tauri command should keep a testable local data reset builder"
        );
        assert!(
            commands_source.contains(route_name),
            "Tauri command should route through the Rust local-encryption state machine"
        );
        assert!(
            lib_source.contains(registration_name),
            "Tauri command should be registered in generate_handler"
        );
        assert!(
            reset_command_source.contains("wait_for_local_data_reset"),
            "Tauri reset must not return the pre-reset snapshot before the correlated signed-out projection"
        );
    }
}

#[cfg(test)]
mod issue551_moved_tests {
    use super::*;
    use crate::commands::contracts::{ScriptedSelectSource, fake_request_id};
    use koushi_core::CoreEvent;
    use koushi_state::AppState;
    use std::collections::VecDeque;
    #[tokio::test]
    async fn reset_local_data_waits_for_the_correlated_signed_out_projection() {
        let request_id = fake_request_id(48);
        let mut signed_out = AppState::default();
        signed_out.session = SessionState::SignedOut;
        let mut before_reset = AppState::default();
        before_reset.session = SessionState::Restoring;
        let mut source = ScriptedSelectSource {
            snapshot: before_reset,
            events: VecDeque::from([Ok(CoreEvent::StateChanged(signed_out))]),
        };

        super::wait_for_local_data_reset(
            &mut source,
            request_id,
            std::time::Duration::from_millis(10),
        )
        .await
        .expect("reset should settle only after signed-out is projected");
    }
}
