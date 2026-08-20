use super::credentials::*;
use super::waiters::*;
use super::*;

// ---------------------------------------------------------------------------
// Recovery outcome
// ---------------------------------------------------------------------------

#[cfg(any(debug_assertions, test))]
pub(super) enum RecoveryOutcome {
    Completed,
    Failed(RecoveryFailureKind),
}

/// Tracks the resources the real-homeserver QA run created so the catch-all
/// wrapper can leave/forget rooms/spaces and log out even when an inner step
/// fails via `?`. Without this, a `?`-propagated send/edit/restore failure
/// would leak BOTH the session/device AND the created room/space on the live
/// homeserver (REPOSITORY_RULES: QA must clean up every resource it creates).
#[cfg(any(debug_assertions, test))]
#[derive(Default)]
pub(super) struct RealQaCleanupState {
    pub(super) account_key: Option<AccountKey>,
    pub(super) qa_room_id: Option<String>,
    pub(super) qa_space_id: Option<String>,
    pub(super) logged_out: bool,
}
// ---------------------------------------------------------------------------
// Catch-all cleanup (finally-ish path for `?`-propagated inner failures)
// ---------------------------------------------------------------------------

/// Best-effort cleanup invoked by `run_async` whenever the inner flow returns
/// an error before reaching the happy-path logout. It starts a fresh runtime
/// over the same `data_dir`, restores the session, then leaves/forgets every
/// recorded room and space and logs out so no stale device, room, or space
/// survives a failed run.
///
/// This function MUST NEVER return Err and MUST NEVER panic — every failure is
/// swallowed into a concrete `cleanup_warning=...` token. Matrix identifiers
/// (user/room/event/space ids) are never printed; only token lines are emitted
/// (REPOSITORY_RULES Security; Task 5).
#[cfg(any(debug_assertions, test))]
pub(super) async fn cleanup_real_qa_resources(
    creds: &RealCredentials,
    data_dir: &std::path::Path,
    transcript: &mut Vec<String>,
    cleanup: &mut RealQaCleanupState,
) {
    // No login succeeded -> there is nothing to clean (no session, and rooms /
    // spaces cannot have been created without a session).
    let Some(account_key) = cleanup.account_key.clone() else {
        return;
    };

    // Start a fresh runtime over the same data dir and restore the session so
    // we hold a Matrix-capable connection to leave/forget and log out.
    let runtime = CoreRuntime::start_with_data_dir(data_dir.to_path_buf());
    let mut conn = runtime.attach();

    let restore_id = conn.next_request_id();
    if let Err(e) = conn
        .command(CoreCommand::Account(AccountCommand::RestoreLastSession {
            request_id: restore_id,
        }))
        .await
    {
        let line = format!("cleanup_warning=restore_failed reason={e}");
        transcript.push(line.clone());
        eprintln!("{line}");
        return;
    }

    if let Err(e) = wait_for_session_restored_with_recovery(
        &mut conn,
        restore_id,
        &account_key,
        creds,
        transcript,
        "cleanup restore",
    )
    .await
    {
        let line = format!("cleanup_warning=restore_failed reason={e}");
        transcript.push(line.clone());
        eprintln!("{line}");
        return;
    }

    if let Err(e) = wait_for_ready_snapshot(&mut conn, "cleanup restored Ready").await {
        let line = format!("cleanup_warning=restore_failed reason={e}");
        transcript.push(line.clone());
        eprintln!("{line}");
        return;
    }

    // Leave/forget the QA room. Each sub-step records a concrete warning token
    // on failure and CONTINUES (do not bail) so the space and logout still run.
    if let Some(room_id) = cleanup.qa_room_id.clone() {
        let leave_id = conn.next_request_id();
        match conn
            .command(CoreCommand::Room(RoomCommand::LeaveRoom {
                request_id: leave_id,
                room_id: room_id.clone(),
            }))
            .await
        {
            Ok(()) => {
                if let Err(e) =
                    wait_for_room_left(&mut conn, leave_id, &room_id, "cleanup leave room").await
                {
                    let line = format!("cleanup_warning=leave_room_failed reason={e}");
                    transcript.push(line.clone());
                    eprintln!("{line}");
                }
            }
            Err(e) => {
                let line = format!("cleanup_warning=leave_room_failed reason={e}");
                transcript.push(line.clone());
                eprintln!("{line}");
            }
        }

        let forget_id = conn.next_request_id();
        match conn
            .command(CoreCommand::Room(RoomCommand::ForgetRoom {
                request_id: forget_id,
                room_id: room_id.clone(),
            }))
            .await
        {
            Ok(()) => {
                if let Err(e) =
                    wait_for_room_forgotten(&mut conn, forget_id, &room_id, "cleanup forget room")
                        .await
                {
                    let line = format!("cleanup_warning=forget_room_failed reason={e}");
                    transcript.push(line.clone());
                    eprintln!("{line}");
                }
            }
            Err(e) => {
                let line = format!("cleanup_warning=forget_room_failed reason={e}");
                transcript.push(line.clone());
                eprintln!("{line}");
            }
        }
    }

    // Leave/forget the QA space (spaces are rooms on the homeserver).
    if let Some(space_id) = cleanup.qa_space_id.clone() {
        let leave_id = conn.next_request_id();
        match conn
            .command(CoreCommand::Room(RoomCommand::LeaveRoom {
                request_id: leave_id,
                room_id: space_id.clone(),
            }))
            .await
        {
            Ok(()) => {
                if let Err(e) =
                    wait_for_room_left(&mut conn, leave_id, &space_id, "cleanup leave space").await
                {
                    let line = format!("cleanup_warning=leave_space_failed reason={e}");
                    transcript.push(line.clone());
                    eprintln!("{line}");
                }
            }
            Err(e) => {
                let line = format!("cleanup_warning=leave_space_failed reason={e}");
                transcript.push(line.clone());
                eprintln!("{line}");
            }
        }

        let forget_id = conn.next_request_id();
        match conn
            .command(CoreCommand::Room(RoomCommand::ForgetRoom {
                request_id: forget_id,
                room_id: space_id.clone(),
            }))
            .await
        {
            Ok(()) => {
                if let Err(e) =
                    wait_for_room_forgotten(&mut conn, forget_id, &space_id, "cleanup forget space")
                        .await
                {
                    let line = format!("cleanup_warning=forget_space_failed reason={e}");
                    transcript.push(line.clone());
                    eprintln!("{line}");
                }
            }
            Err(e) => {
                let line = format!("cleanup_warning=forget_space_failed reason={e}");
                transcript.push(line.clone());
                eprintln!("{line}");
            }
        }
    }

    // Finally log out. `do_logout` already prints a `logout_submit=failed` /
    // `logout_wait=failed` token on failure and never propagates errors.
    do_logout(&mut conn, &account_key, transcript).await;
    cleanup.logged_out = true;
}

// ---------------------------------------------------------------------------
// Logout helper (finally-ish path - runs even on earlier failure)
// ---------------------------------------------------------------------------

/// Best-effort logout. Records to transcript but never propagates errors.
/// Called in failure paths so no stale devices accumulate.
#[cfg(any(debug_assertions, test))]
pub(super) async fn do_logout(
    conn: &mut CoreConnection,
    account_key: &AccountKey,
    transcript: &mut Vec<String>,
) {
    let logout_id = conn.next_request_id();
    match conn
        .command(CoreCommand::Account(AccountCommand::Logout {
            request_id: logout_id,
        }))
        .await
    {
        Ok(()) => {}
        Err(e) => {
            let line = format!("logout_submit=failed reason={e}");
            transcript.push(line.clone());
            eprintln!("{line}");
            return;
        }
    }

    match wait_for_logged_out(conn, logout_id, account_key, "logout").await {
        Ok(()) => {
            let line = "logout=ok".to_owned();
            transcript.push(line.clone());
            println!("{line}");
        }
        Err(e) => {
            let line = format!("logout_wait=failed reason={e}");
            transcript.push(line.clone());
            eprintln!("{line}");
        }
    }
}
