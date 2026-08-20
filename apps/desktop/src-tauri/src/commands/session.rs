use super::*;

#[derive(serde::Serialize)]
pub struct OidcAuthorizationResponse {
    pub authorization_url: String,
    pub state: String,
}

#[tauri::command]
pub async fn get_snapshot(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn discover_login_methods(
    homeserver: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.inner().runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_discover_login_command(request_id, homeserver))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_auth_changed(&mut event_conn, LOGIN_EVENT_TIMEOUT).await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_oidc_login(
    homeserver: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<OidcAuthorizationResponse, String> {
    let mut event_conn = state.inner().runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_start_oidc_login_command(request_id, homeserver))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_oidc_authorization(&mut event_conn, request_id, LOGIN_EVENT_TIMEOUT).await
}

#[tauri::command]
pub async fn complete_oidc_login(
    _homeserver: String,
    callback_url: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.inner().runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_complete_oidc_login_command(
            request_id,
            callback_url,
            crate::dto::frontend_display_platform(),
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;
    wait_for_logged_in_authenticated(&mut event_conn, request_id, LOGIN_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

async fn wait_for_oidc_authorization(
    event_conn: &mut koushi_core::CoreConnection,
    request_id: koushi_core::RequestId,
    timeout: std::time::Duration,
) -> Result<OidcAuthorizationResponse, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "OIDC login did not start".to_owned())?;
        match event {
            Ok(koushi_core::CoreEvent::Account(
                koushi_core::AccountEvent::OidcAuthorizationCreated {
                    request_id: ev_id,
                    authorization_url,
                    state,
                },
            )) if ev_id == request_id => {
                return Ok(OidcAuthorizationResponse {
                    authorization_url,
                    state,
                });
            }
            Ok(koushi_core::CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            }) if ev_id == request_id => {
                return Err(invoke_error_from_core_failure("OIDC login failed", failure));
            }
            Err(_) => continue,
            _ => {}
        }
    }
}

#[tauri::command]
pub async fn submit_login(
    homeserver: String,
    username: String,
    password: String,
    device_display_name: Option<String>,
    platform: DisplayPlatform,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let login_request = LoginRequest {
        homeserver,
        username,
        password: AuthSecret::new(password),
        device_display_name,
    };
    submit_login_request(app, state.inner(), login_request, platform).await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn submit_soft_logout_reauth(
    password: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    submit_soft_logout_reauth_request(app, state.inner(), AuthSecret::new(password)).await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn list_saved_sessions(
    state: State<'_, CoreRuntimeState>,
) -> Result<Vec<SessionInfo>, String> {
    // GUI-smoke toggle: skip the keychain-backed query entirely.
    if crate::saved_sessions_disabled_from_env() {
        return Ok(Vec::new());
    }

    // Attach a dedicated connection so (a) the request id belongs to this
    // connection and (b) the broadcast cursor starts BEFORE the command is
    // submitted — the correlated answer cannot be missed.
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(CoreCommand::Account(AccountCommand::QuerySavedSessions {
            request_id,
        }))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;

    let deadline = tokio::time::Instant::now() + SAVED_SESSIONS_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "saved sessions could not be loaded".to_owned())?;
        match event {
            Ok(koushi_core::CoreEvent::Account(
                koushi_core::AccountEvent::SavedSessionsListed {
                    request_id: ev_id,
                    sessions,
                },
            )) if ev_id == request_id => return Ok(sessions),
            Ok(koushi_core::CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            }) if ev_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "saved sessions could not be loaded",
                    failure,
                ));
            }
            // Unrelated events / lag: keep waiting until the deadline.
            _ => {}
        }
    }
}

#[tauri::command]
pub async fn switch_account(
    homeserver: String,
    user_id: String,
    device_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_switch_account_command(request_id, user_id),
    )
    .await?;
    // AccountKey canonically identifies the account by user_id.
    let _ = (homeserver, device_id);
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn submit_recovery(
    secret: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    submit_recovery_request(app, state.inner(), AuthSecret::new(secret)).await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_device_cleanup(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_start_device_cleanup_command(request_id),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn submit_device_cleanup_uia(
    flow_id: u64,
    password: String,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_submit_device_cleanup_uia_command(request_id, flow_id, AuthSecret::new(password)),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn erase_local_data_anyway(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_erase_device_cleanup_local_data_anyway_command(request_id),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn logout(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_logout_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn retry_sliding_sync_capability(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        build_retry_sliding_sync_capability_command(request_id),
    )
    .await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn change_homeserver(
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_change_homeserver_command(request_id)).await?;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn restart_sync(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_restart_sync_command(request_id)).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

pub(super) async fn submit_login_request(
    app: AppHandle,
    state: &CoreRuntimeState,
    login_request: LoginRequest,
    platform: DisplayPlatform,
) -> Result<(), String> {
    submit_login_and_wait_for_authenticated(app, state, login_request, platform).await?;
    Ok(())
}

pub(super) async fn submit_soft_logout_reauth_request(
    app: AppHandle,
    state: &CoreRuntimeState,
    password: AuthSecret,
) -> Result<(), String> {
    let mut event_conn = state.runtime.attach();
    let request_id = event_conn.next_request_id();
    event_conn
        .command(build_submit_soft_logout_reauth_command(
            request_id, password,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;

    wait_for_logged_in_authenticated(&mut event_conn, request_id, LOGIN_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state).await;
    Ok(())
}

const LOGIN_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn submit_login_and_wait_for_authenticated(
    app: AppHandle,
    state: &CoreRuntimeState,
    login_request: LoginRequest,
    platform: DisplayPlatform,
) -> Result<(), String> {
    // Use a dedicated connection so the event cursor is attached before the
    // login command is submitted and the correlated LoggedIn event cannot be
    // missed by this product path.
    let mut event_conn = state.runtime.attach();
    let login_request_id = event_conn.next_request_id();
    event_conn
        .command(build_submit_login_command(
            login_request_id,
            login_request,
            platform,
        ))
        .await
        .map_err(|e| format!("command submit failed: {e}"))?;

    wait_for_logged_in_authenticated(&mut event_conn, login_request_id, LOGIN_EVENT_TIMEOUT)
        .await?;
    update_qa_window_title_from_state(&app, state).await;
    Ok(())
}

async fn wait_for_logged_in_authenticated(
    event_conn: &mut CoreConnection,
    login_request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_login_transport_terminal(&event_conn.snapshot()) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "login did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::Account(AccountEvent::LoggedIn { request_id, .. }))
                if request_id == login_request_id =>
            {
                if snapshot_has_login_transport_terminal(&event_conn.snapshot()) {
                    return Ok(());
                }
            }
            Ok(CoreEvent::OperationFailed {
                request_id,
                failure,
            }) if request_id == login_request_id => {
                return Err(invoke_error_from_core_failure("login failed", failure));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

async fn wait_for_auth_changed(
    event_conn: &mut CoreConnection,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_auth_discovery_answer(&event_conn.snapshot()) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "login discovery did not complete".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(snapshot))
                if snapshot_has_auth_discovery_answer(&snapshot) =>
            {
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
}

fn snapshot_has_auth_discovery_answer(snapshot: &koushi_state::AppState) -> bool {
    matches!(
        snapshot.auth,
        koushi_state::AuthDiscoveryState::Ready { .. }
            | koushi_state::AuthDiscoveryState::Failed { .. }
    )
}

fn snapshot_has_authenticated_session(snapshot: &koushi_state::AppState) -> bool {
    matches!(snapshot.session, koushi_state::SessionState::Ready(_))
}

fn snapshot_has_login_transport_terminal(snapshot: &koushi_state::AppState) -> bool {
    matches!(
        &snapshot.session,
        koushi_state::SessionState::Provisional {
            phase: koushi_state::ProvisionalPhase::RecheckingTrust { failure: Some(_) },
            ..
        } | koushi_state::SessionState::AwaitingVerification { .. }
            | koushi_state::SessionState::Verifying { .. }
            | koushi_state::SessionState::AwaitingBootstrapConfirmation { .. }
            | koushi_state::SessionState::Rejecting { .. }
    ) || snapshot_has_authenticated_session(snapshot)
}

/// How long the adapter waits for the `SavedSessionsListed` answer before
/// reporting a transport error. The query is a local credential-store read in
/// core, so 5 seconds is generous.
const SAVED_SESSIONS_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) async fn submit_recovery_request(
    app: AppHandle,
    state: &CoreRuntimeState,
    secret: AuthSecret,
) -> Result<(), String> {
    let request_id = next_request_id(state).await;
    submit_core_command(state, build_submit_recovery_command(request_id, secret)).await?;
    update_qa_window_title_from_state(&app, state).await;
    Ok(())
}

pub(super) fn build_submit_login_command(
    request_id: koushi_core::RequestId,
    login_request: LoginRequest,
    platform: DisplayPlatform,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::LoginPassword {
        request_id,
        request: login_request,
        platform,
    })
}

pub(super) fn build_submit_soft_logout_reauth_command(
    request_id: koushi_core::RequestId,
    password: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SoftLogoutReauth {
        request_id,
        password,
    })
}

pub(super) fn build_discover_login_command(
    request_id: koushi_core::RequestId,
    homeserver: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::DiscoverLogin {
        request_id,
        homeserver,
    })
}

pub(super) fn build_start_oidc_login_command(
    request_id: koushi_core::RequestId,
    homeserver: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::StartOidcLogin {
        request_id,
        homeserver,
    })
}

pub(crate) fn build_complete_oidc_login_command(
    request_id: koushi_core::RequestId,
    callback_url: String,
    platform: DisplayPlatform,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::CompleteOidcLogin {
        request_id,
        callback_url,
        platform,
    })
}

pub(super) fn build_switch_account_command(
    request_id: koushi_core::RequestId,
    user_id: String,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SwitchAccount {
        request_id,
        account_key: AccountKey(user_id),
    })
}

pub(super) fn build_submit_recovery_command(
    request_id: koushi_core::RequestId,
    secret: AuthSecret,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::SubmitRecovery {
        request_id,
        request: RecoveryRequest { secret },
    })
}

pub(super) fn build_logout_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::Logout { request_id })
}

pub(super) fn build_retry_sliding_sync_capability_command(
    request_id: koushi_core::RequestId,
) -> CoreCommand {
    CoreCommand::Account(AccountCommand::RetrySlidingSyncCapability { request_id })
}

pub(super) fn build_change_homeserver_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Account(AccountCommand::ChangeHomeserver { request_id })
}

pub(super) fn build_restart_sync_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::Sync(SyncCommand::Restart { request_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_login_request_waits_for_authenticated_session_and_leaves_sync_to_runtime_effects() {
        let source = commands_source();
        let helper_name = concat!("async fn submit_login", "_and_wait_for_authenticated");
        let wait_call_token = concat!("wait_for_logged", "_in_authenticated");
        let logged_in_token = concat!("AccountEvent::", "LoggedIn");
        let start_sync_token = concat!("build_start", "_sync_command");
        let failed_token = concat!("Operation", "Failed");
        let timeout_token = concat!("LOGIN_EVENT", "_TIMEOUT");
        let helper_offset = source
            .find(helper_name)
            .expect("shared login helper should exist");
        let helper_source = &source[helper_offset..];
        let helper_source = helper_source
            .split(concat!("async fn wait_for_logged", "_in_authenticated"))
            .next()
            .expect("login wait helper should follow shared helper");
        let wait_call_offset = helper_source
            .find(wait_call_token)
            .expect("helper should wait for an authenticated session");

        assert!(wait_call_offset > 0);
        assert!(
            !helper_source.contains(start_sync_token),
            "sync startup belongs to AppEffect::StartSync in core runtime, not the Tauri adapter"
        );
        assert!(helper_source.contains(timeout_token));
        let wait_helper_offset = source
            .find(concat!("async fn wait_for_logged", "_in_authenticated"))
            .expect("login wait helper should exist");
        let wait_helper_source = &source[wait_helper_offset..];
        assert!(wait_helper_source.contains(logged_in_token));
        assert!(wait_helper_source.contains(failed_token));
        assert!(wait_helper_source.contains("timeout_at"));
    }

    #[test]
    fn login_transport_completes_at_interactive_verification_gate() {
        let mut state = koushi_state::AppState::default();
        state.session = koushi_state::SessionState::AwaitingVerification {
            info: koushi_state::SessionInfo {
                homeserver: "https://example.invalid".into(),
                user_id: "@u:example.invalid".into(),
                device_id: "D".into(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
            gate: koushi_state::VerificationGateState {
                methods: vec![],
                account_kind: koushi_state::VerificationAccountKind::ExistingIdentity,
                failure: None,
            },
        };
        assert!(snapshot_has_login_transport_terminal(&state));
    }

    #[test]
    fn login_transport_does_not_complete_while_discovering_verification_methods() {
        let mut state = koushi_state::AppState::default();
        state.session = koushi_state::SessionState::Provisional {
            info: koushi_state::SessionInfo {
                homeserver: "https://example.invalid".into(),
                user_id: "@u:example.invalid".into(),
                device_id: "D".into(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
            phase: koushi_state::ProvisionalPhase::DiscoveringMethods,
        };

        assert!(!snapshot_has_login_transport_terminal(&state));
    }

    #[test]
    fn login_wait_does_not_treat_verification_gate_as_ready_session() {
        let mut state = AppState::default();
        assert!(!super::snapshot_has_authenticated_session(&state));

        let info = SessionInfo {
            homeserver: "https://matrix.example.org".to_owned(),
            user_id: "@user:example.org".to_owned(),
            device_id: "DEVICE".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        };

        state.session = SessionState::AwaitingVerification {
            info: info.clone(),
            gate: koushi_state::VerificationGateState {
                methods: vec![],
                account_kind: koushi_state::VerificationAccountKind::Unknown,
                failure: None,
            },
        };
        assert!(!super::snapshot_has_authenticated_session(&state));

        state.session = SessionState::Verifying {
            info,
            gate: koushi_state::VerificationGateState {
                methods: vec![],
                account_kind: koushi_state::VerificationAccountKind::Unknown,
                failure: None,
            },
            method: koushi_state::VerificationMethod::RecoveryKey,
            flow_id: 1,
            sas_emojis: vec![],
        };
        assert!(!super::snapshot_has_authenticated_session(&state));
    }
}
