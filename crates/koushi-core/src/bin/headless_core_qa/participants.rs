async fn complete_new_identity_gate_for_qa(
    conn: &mut CoreConnection,
    password: &str,
    destination_suffix: &str,
) -> Result<Option<AuthSecret>, String> {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        match &conn.snapshot().session {
            SessionState::AwaitingVerification { gate, .. }
                if gate.account_kind == koushi_state::VerificationAccountKind::NewIdentity =>
            {
                break;
            }
            SessionState::Ready(_) => return Ok(None),
            SessionState::Rejecting { .. } => return Err("new identity gate rejected".to_owned()),
            _ => {}
        }
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "timed out waiting for new identity gate; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                )
            })?
            .map_err(|_| "event stream closed while waiting for new identity gate".to_owned())?;
    }
    let request_id = conn.next_request_id();
    let flow_id = request_id.sequence;
    let bootstrap_dir = qa_data_dir(destination_suffix);
    std::fs::create_dir_all(&bootstrap_dir)
        .map_err(|_| "prepare private bootstrap delivery directory".to_owned())?;
    let recovery_key_path = bootstrap_dir.join("recovery-key.txt");
    conn.command(CoreCommand::Account(
        AccountCommand::StartSessionBootstrap {
            request_id,
            flow_id,
            auth: Some(AuthSecret::new(password.to_owned())),
            request: koushi_core::SecureBackupSetupRequest {
                passphrase: Some(AuthSecret::new(password.to_owned())),
                recovery_key_destination_path: Some(recovery_key_path.clone()),
                explicit_reenable_confirmed: false,
            },
        },
    ))
    .await
    .map_err(|error| format!("submit new identity bootstrap: {error}"))?;
    let delivery_deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        match &conn.snapshot().session {
            SessionState::AwaitingBootstrapConfirmation {
                flow_id: active,
                destination_written: true,
                ..
            } if *active == flow_id => break,
            SessionState::AwaitingVerification { gate, .. } if gate.failure.is_some() => {
                return Err(format!(
                    "new identity bootstrap failed; kind={:?}",
                    gate.failure.expect("failure checked above")
                ));
            }
            _ => {}
        }
        tokio::time::timeout_at(delivery_deadline, conn.recv_event())
            .await
            .map_err(|_| "timed out waiting for bootstrap delivery".to_owned())?
            .map_err(|_| "event stream closed during bootstrap delivery".to_owned())?;
    }
    let recovery_secret = AuthSecret::new(
        std::fs::read_to_string(&recovery_key_path)
            .map_err(|_| "read disposable bootstrap recovery key".to_owned())?
            .trim()
            .to_owned(),
    );
    let confirm_id = conn.next_request_id();
    conn.command(CoreCommand::Account(
        AccountCommand::ConfirmSessionBootstrapSaved {
            request_id: confirm_id,
            flow_id,
        },
    ))
    .await
    .map_err(|error| format!("submit bootstrap saved confirmation: {error}"))?;
    std::fs::remove_file(&recovery_key_path)
        .map_err(|_| "remove disposable bootstrap recovery key".to_owned())?;
    std::fs::remove_dir(&bootstrap_dir)
        .map_err(|_| "remove disposable bootstrap delivery directory".to_owned())?;

    // Observe the confirmation's own outcome instead of firing and forgetting
    // it (#375). A failed confirmation leaves the session unpromoted, so
    // `LoggedIn` is never released from the actor's pending-ready events and
    // the run used to surface only `login A: timed out waiting for LoggedIn
    // event` — after this helper had already printed its own success token.
    //
    // Leaving `AwaitingBootstrapConfirmation` is progress, so the loop returns
    // as soon as the session moves on; it deliberately does NOT wait for
    // `Ready`, because the caller owns that wait. Consuming `LoggedIn` here is
    // harmless: the caller resolves the account key from the authoritative
    // snapshot when the session is `Ready`.
    let settle_deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        match &conn.snapshot().session {
            SessionState::AwaitingBootstrapConfirmation {
                flow_id: active, ..
            } if *active == flow_id => {}
            SessionState::Rejecting { .. } => {
                return Err("new identity bootstrap confirmation rejected".to_owned());
            }
            _ => break,
        }
        let event = match tokio::time::timeout_at(settle_deadline, conn.recv_event()).await {
            Ok(event) => event,
            Err(_) => {
                return Err(format!(
                    "timed out settling bootstrap confirmation; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                ));
            }
        };
        match event {
            Ok(CoreEvent::OperationFailed {
                request_id: failed,
                failure,
            }) if failed == confirm_id => {
                return Err(format!("bootstrap confirmation failed: {failure:?}"));
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }

    Ok(Some(recovery_secret))
}

async fn wait_for_existing_identity_gate(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<SessionInfo, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    loop {
        if let SessionState::AwaitingVerification { info, gate } = &conn.snapshot().session {
            if gate.account_kind == koushi_state::VerificationAccountKind::ExistingIdentity
                && gate
                    .methods
                    .contains(&koushi_state::VerificationMethodCapability::ExistingDeviceSas)
            {
                return Ok(info.clone());
            }
            if gate.account_kind == koushi_state::VerificationAccountKind::ExistingIdentity {
                // Device-list/exact-identity refresh may still be populating
                // the SAS capability. Keep waiting rather than turning this
                // transient gate snapshot into a false prerequisite failure.
            }
        }
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                )
            })?
            .map_err(|_| format!("{label}: event stream closed"))?;
    }
}

async fn wait_for_recovery_gate(conn: &mut CoreConnection, label: &str) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + E2EE_EVENT_TIMEOUT;
    loop {
        if let SessionState::AwaitingVerification { gate, .. } = &conn.snapshot().session
            && gate.account_kind == koushi_state::VerificationAccountKind::ExistingIdentity
            && gate.methods.iter().any(|method| {
                matches!(
                    method,
                    koushi_state::VerificationMethodCapability::RecoveryKey
                        | koushi_state::VerificationMethodCapability::SecurityPhrase
                )
            })
        {
            return Ok(());
        }
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for recovery gate; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                )
            })?
            .map_err(|_| format!("{label}: event stream closed"))?;
    }
}

async fn wait_for_matching_recovery_flow(
    conn: &mut CoreConnection,
    flow_id: u64,
    label: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + E2EE_EVENT_TIMEOUT;
    loop {
        if matches!(
            conn.snapshot().session,
            SessionState::Verifying {
                flow_id: active_flow_id,
                method: koushi_state::VerificationMethod::RecoveryKey
                    | koushi_state::VerificationMethod::SecurityPhrase,
                ..
            } if active_flow_id == flow_id
        ) {
            return Ok(());
        }
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for matching recovery flow"))?
            .map_err(|_| format!("{label}: event stream closed"))?;
    }
}

async fn wait_for_locked_snapshot(conn: &mut CoreConnection, label: &str) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    loop {
        if matches!(conn.snapshot().session, SessionState::Locked(_)) {
            return Ok(());
        }
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for Locked; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                )
            })?
            .map_err(|_| format!("{label}: event stream closed"))?;
    }
}

async fn verify_provisional_second_device_for_qa(
    conn_a: &mut CoreConnection,
    conn_a2: &mut CoreConnection,
    session_a: &SessionInfo,
    session_a2: &SessionInfo,
    label: &str,
    outcome: SasQaOutcome,
) -> Result<(), String> {
    if session_a.user_id != session_a2.user_id || session_a.device_id == session_a2.device_id {
        return Err(format!(
            "{label}: expected two distinct devices for one user"
        ));
    }
    // Keep the primary's normal sync continuously running: it owns incoming
    // to-device delivery for the entire SAS flow, including retry outcomes.
    let previous_primary_flow_id =
        verification_state_flow_id(&conn_a.snapshot().e2ee_trust.verification);
    let target_a2 = VerificationTarget {
        user_id: session_a2.user_id.clone(),
        device_id: session_a2.device_id.clone(),
    };
    let flow_request = conn_a2.next_request_id();
    let flow_id_a2 = flow_request.sequence;
    after_receiver_device_known(
        refresh_device_keys_and_assert_known_for_qa(
            conn_a,
            target_a2.clone(),
            &format!("{label}: primary receiver device discovery"),
        ),
        || async {
            conn_a2
                .command(CoreCommand::Account(AccountCommand::StartOwnUserSas {
                    request_id: flow_request,
                    flow_id: flow_id_a2,
                }))
                .await
                .map_err(|error| format!("{label}: submit own-user SAS: {error}"))
        },
    )
    .await?;

    let flow_id_a = wait_for_verification_requested_event_only(
        conn_a,
        Some(&target_a2),
        previous_primary_flow_id,
        &format!("{label}: primary incoming request"),
    )
    .await?;
    let accept_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Account(AccountCommand::AcceptVerification {
            request_id: accept_id,
            flow_id: flow_id_a,
        }))
        .await
        .map_err(|error| format!("{label}: accept primary: {error}"))?;
    wait_for_verification_accepted(
        conn_a,
        flow_id_a,
        Some(accept_id),
        &format!("{label}: primary ready"),
    )
    .await?;

    let deadline = tokio::time::Instant::now() + E2EE_EVENT_TIMEOUT;
    let mut secondary_matching_flow_observed = false;
    let (emojis_a, emojis_a2) = loop {
        let primary = verification_state_sas(
            &conn_a.snapshot().e2ee_trust.verification,
            flow_id_a,
            &format!("{label}: primary SAS"),
        )?;
        let secondary_session = &conn_a2.snapshot().session;
        secondary_matching_flow_observed |= matches!(
            secondary_session,
            SessionState::Verifying { flow_id, .. } if *flow_id == flow_id_a2
        );
        let secondary = match observe_secondary_sas(
            secondary_session,
            flow_id_a2,
            secondary_matching_flow_observed,
        ) {
            SecondarySasObservation::Presented(emojis) => Some(emojis),
            SecondarySasObservation::Failed => {
                return Err(format!("{label}: secondary gate SAS failed"));
            }
            SecondarySasObservation::Pending => None,
        };
        if let (Some(primary), Some(secondary)) = (primary, secondary) {
            break (primary, secondary);
        }
        match wait_for_paired_event_until(conn_a, conn_a2, deadline).await {
            Ok(()) => {}
            Err(PairedEventWaitError::Deadline) => {
                let primary_snapshot = conn_a.snapshot();
                let (primary_phase, primary_flow_matches, primary_emoji_count) =
                    verification_closed_summary(
                        &primary_snapshot.e2ee_trust.verification,
                        flow_id_a,
                    );
                let secondary_snapshot = conn_a2.snapshot();
                let (secondary_phase, secondary_flow_matches, secondary_emoji_count) =
                    session_gate_closed_summary(&secondary_snapshot.session, flow_id_a2);
                return Err(format!(
                    "{label}: timed out waiting for paired SAS; primary_phase={primary_phase};primary_flow_matches={primary_flow_matches};primary_emoji_count={primary_emoji_count};secondary_phase={secondary_phase};secondary_flow_matches={secondary_flow_matches};secondary_emoji_count={secondary_emoji_count}"
                ));
            }
            Err(PairedEventWaitError::Primary(lag)) => {
                return Err(verification_event_stream_error(label, "primary", lag));
            }
            Err(PairedEventWaitError::Secondary(lag)) => {
                return Err(verification_event_stream_error(label, "secondary", lag));
            }
        }
    };
    if emojis_a != emojis_a2 {
        return Err(format!("{label}: SAS emoji mismatch"));
    }
    if outcome == SasQaOutcome::Timeout {
        wait_for_existing_identity_gate(conn_a2, &format!("{label}: timeout retryable")).await?;
        return Ok(());
    }
    if outcome != SasQaOutcome::Success {
        let cancel_a2 = conn_a2.next_request_id();
        conn_a2
            .command(CoreCommand::Account(AccountCommand::CancelVerification {
                request_id: cancel_a2,
                flow_id: flow_id_a2,
                reason: if outcome == SasQaOutcome::Mismatch {
                    koushi_state::VerificationCancelReason::Mismatch
                } else {
                    koushi_state::VerificationCancelReason::User
                },
            }))
            .await
            .map_err(|error| format!("{label}: mismatch secondary: {error}"))?;
        let cancel_a = conn_a.next_request_id();
        conn_a
            .command(CoreCommand::Account(AccountCommand::CancelVerification {
                request_id: cancel_a,
                flow_id: flow_id_a,
                reason: koushi_state::VerificationCancelReason::User,
            }))
            .await
            .map_err(|error| format!("{label}: cancel primary after mismatch: {error}"))?;
        wait_for_existing_identity_gate(conn_a2, &format!("{label}: mismatch retryable")).await?;
        return Ok(());
    }

    let confirm_a = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Account(
            AccountCommand::ConfirmSasVerification {
                request_id: confirm_a,
                flow_id: flow_id_a,
            },
        ))
        .await
        .map_err(|error| format!("{label}: confirm primary: {error}"))?;
    let confirm_a2 = conn_a2.next_request_id();
    conn_a2
        .command(CoreCommand::Account(
            AccountCommand::ConfirmSasVerification {
                request_id: confirm_a2,
                flow_id: flow_id_a2,
            },
        ))
        .await
        .map_err(|error| format!("{label}: confirm secondary: {error}"))?;

    let ready_deadline = tokio::time::Instant::now() + E2EE_EVENT_TIMEOUT;
    loop {
        if matches!(conn_a2.snapshot().session, SessionState::Ready(_)) {
            return Ok(());
        }
        match (QaEventDeadline {
            instant: ready_deadline,
        })
        .recv(conn_a2)
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(lag)) => {
                return Err(verification_event_stream_error(label, "secondary", lag));
            }
            Err(_) => {
                return Err(format!(
                    "{label}: timed out waiting for authoritative Ready"
                ));
            }
        }
    }
}

async fn after_receiver_device_known<Refresh, Start, Started, Output, Error>(
    refresh: Refresh,
    start_once: Start,
) -> Result<Output, Error>
where
    Refresh: Future<Output = Result<(), Error>>,
    Start: FnOnce() -> Started,
    Started: Future<Output = Result<Output, Error>>,
{
    refresh.await?;
    start_once().await
}

fn authenticated_session_info(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<SessionInfo, String> {
    authenticated_session_info_from_state(&conn.snapshot().session)
        .cloned()
        .ok_or_else(|| format!("{label}: session is not authenticated"))
}

fn authenticated_session_info_from_state(session: &SessionState) -> Option<&SessionInfo> {
    match session {
        SessionState::Provisional { info, .. }
        | SessionState::AwaitingVerification { info, .. }
        | SessionState::Verifying { info, .. }
        | SessionState::AwaitingBootstrapConfirmation { info, .. }
        | SessionState::Rejecting { info, .. }
        | SessionState::Ready(info) => Some(info),
        SessionState::SignedOut
        | SessionState::Restoring
        | SessionState::SwitchingAccount { .. }
        | SessionState::Authenticating { .. }
        | SessionState::Locked(_)
        | SessionState::CapabilityBlocked { .. }
        | SessionState::LoggingOut => None,
    }
}

enum QaParticipantLoginGate<'a> {
    BootstrapNewIdentity,
    RecoverExistingIdentity(&'a AuthSecret),
}

struct QaParticipantLoginOutcome {
    runtime: CoreRuntime,
    conn: CoreConnection,
    account_key: AccountKey,
    bootstrap_recovery_secret: Option<AuthSecret>,
}

struct QaOwnedLoggedInRuntime {
    runtime: CoreRuntime,
    conn: CoreConnection,
    account_key: AccountKey,
}

enum QaOwnedRuntimePhase {
    LoginNotSubmitted,
    LoginSubmitted,
    LoggedIn(AccountKey),
}

struct QaOwnedRuntimeParticipant {
    runtime: CoreRuntime,
    conn: CoreConnection,
    phase: QaOwnedRuntimePhase,
}

enum QaE2eeRecipient<'a> {
    Borrowed {
        conn: &'a mut CoreConnection,
        account_key: &'a AccountKey,
    },
    Owned(QaOwnedRuntimeParticipant),
}

async fn finish_e2ee_recipient_stage_with_owned_cleanup<T, Participant, Cleanup, CleanupFuture>(
    stage_result: Result<T, String>,
    owned_participant: Option<Participant>,
    cleanup: Cleanup,
) -> Result<T, String>
where
    Cleanup: FnOnce(Participant) -> CleanupFuture,
    CleanupFuture: Future<Output = Result<(), String>>,
{
    let cleanup_result = match owned_participant {
        Some(participant) => cleanup(participant).await,
        None => Ok(()),
    };

    match (stage_result, cleanup_result) {
        (Err(stage_error), Ok(())) => Err(stage_error),
        (Err(stage_error), Err(_)) => Err(format!(
            "{stage_error}; owned E2EE recipient cleanup also failed"
        )),
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
    }
}

async fn retain_or_cleanup_e2ee_callers_after_stage<Callers, Cleanup, CleanupFuture>(
    stage_result: Result<(), String>,
    callers: Callers,
    cleanup: Cleanup,
) -> Result<Callers, String>
where
    Cleanup: FnOnce(Callers) -> CleanupFuture,
    CleanupFuture: Future<Output = Result<(), String>>,
{
    match stage_result {
        Ok(()) => Ok(callers),
        Err(stage_error) => match cleanup(callers).await {
            Ok(()) => Err(stage_error),
            Err(_) => Err(format!("{stage_error}; E2EE caller cleanup also failed")),
        },
    }
}

async fn retain_or_cleanup_owned_participant_after_stage<T, Participant, Cleanup, CleanupFuture>(
    stage_result: Result<T, String>,
    participant: Participant,
    cleanup: Cleanup,
) -> Result<(T, Participant), String>
where
    Cleanup: FnOnce(Participant) -> CleanupFuture,
    CleanupFuture: Future<Output = Result<(), String>>,
{
    match stage_result {
        Ok(value) => Ok((value, participant)),
        Err(stage_error) => match cleanup(participant).await {
            Ok(()) => Err(stage_error),
            Err(_) => Err(format!(
                "{stage_error}; owned participant login cleanup also failed"
            )),
        },
    }
}

async fn login_synced_participant_for_qa(
    homeserver: &str,
    data_dir: std::path::PathBuf,
    username: &str,
    password: &str,
    device_display_name: &str,
    label: &str,
    gate_label: &str,
    gate: QaParticipantLoginGate<'_>,
) -> Result<QaParticipantLoginOutcome, String> {
    let runtime = CoreRuntime::start_with_data_dir(data_dir);
    let conn = runtime.attach();
    let mut participant = QaOwnedRuntimeParticipant::new(runtime, conn);
    let login_stage_result: Result<Option<AuthSecret>, String> = async {
        let login_id = participant.conn.next_request_id();
        participant
            .conn
            .command(CoreCommand::Account(AccountCommand::LoginPassword {
                request_id: login_id,
                request: koushi_state::LoginRequest {
                    homeserver: homeserver.to_owned(),
                    username: username.to_owned(),
                    password: AuthSecret::new(password.to_owned()),
                    device_display_name: Some(device_display_name.to_owned()),
                },
                platform: koushi_state::DisplayPlatform::Linux,
            }))
            .await
            .map_err(|e| format!("{label}: submit login failed: {e}"))?;
        participant.mark_login_submitted();
        let bootstrap_recovery_secret = match gate {
            QaParticipantLoginGate::BootstrapNewIdentity => {
                complete_new_identity_gate_for_qa(&mut participant.conn, password, gate_label)
                    .await?
            }
            QaParticipantLoginGate::RecoverExistingIdentity(recovery_secret) => {
                wait_for_recovery_gate(&mut participant.conn, gate_label).await?;
                let recovery_request_id = participant.conn.next_request_id();
                participant
                    .conn
                    .command(CoreCommand::Account(AccountCommand::SubmitRecovery {
                        request_id: recovery_request_id,
                        request: RecoveryRequest {
                            secret: recovery_secret.clone(),
                        },
                    }))
                    .await
                    .map_err(|e| format!("{gate_label}: submit recovery failed: {e}"))?;
                None
            }
        };
        let account_key = wait_for_logged_in(&mut participant.conn, login_id, label).await?;
        participant.mark_logged_in(account_key);
        wait_for_ready_snapshot(&mut participant.conn, label).await?;
        start_sync_for_qa(&mut participant.conn, label).await?;
        Ok(bootstrap_recovery_secret)
    }
    .await;
    let (bootstrap_recovery_secret, participant) = retain_or_cleanup_owned_participant_after_stage(
        login_stage_result,
        participant,
        |participant| async move {
            cleanup_owned_e2ee_participant_best_effort(participant, "participant login cleanup")
                .await
        },
    )
    .await?;
    let QaOwnedLoggedInRuntime {
        runtime,
        conn,
        account_key,
    } = participant.into_logged_in_runtime();

    Ok(QaParticipantLoginOutcome {
        runtime,
        conn,
        account_key,
        bootstrap_recovery_secret,
    })
}

fn qa_data_dir(suffix: &str) -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("KOUSHI_QA_DATA_DIR") {
        return std::path::PathBuf::from(dir).join(suffix);
    }
    std::env::temp_dir()
        .join("koushi-core-qa")
        .join(format!("{}_{}", std::process::id(), suffix))
}
