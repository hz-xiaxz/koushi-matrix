type QaEventFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + 'a>>;

trait QaEventSource {
    fn recv_event(&mut self) -> QaEventFuture<'_>;
}

trait QaEventSource {
    fn recv_event(&mut self) -> QaEventFuture<'_>;
}

trait QaSnapshotEventSource: QaEventSource {
    fn snapshot(&self) -> AppState;
}

struct QaEventDeadline {
    instant: tokio::time::Instant,
}

enum PairedEventWaitError {
    Deadline,
    Primary(EventStreamLag),
    Secondary(EventStreamLag),
}

async fn wait_for_paired_event_until<Primary, Secondary>(
    primary: &mut Primary,
    secondary: &mut Secondary,
    deadline: tokio::time::Instant,
) -> Result<(), PairedEventWaitError>
where
    Primary: QaEventSource + ?Sized,
    Secondary: QaEventSource + ?Sized,
{
    tokio::select! {
        event = primary.recv_event() => event
            .map(|_| ())
            .map_err(PairedEventWaitError::Primary),
        event = secondary.recv_event() => event
            .map(|_| ())
            .map_err(PairedEventWaitError::Secondary),
        _ = tokio::time::sleep_until(deadline) => Err(PairedEventWaitError::Deadline),
    }
}

async fn wait_for_sync_started_and_running(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    let mut saw_started = false;
    let mut saw_running_before_started = false;
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for SyncEvent::Started/Running"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Sync(SyncEvent::Started { request_id: ev_id })
                if ev_id == Some(request_id) =>
            {
                saw_started = true;
                if saw_running_before_started {
                    return Ok(());
                }
            }
            CoreEvent::Sync(SyncEvent::Running) => {
                if saw_started {
                    return Ok(());
                }
                saw_running_before_started = true;
            }
            CoreEvent::Sync(SyncEvent::Failed) => {
                return Err(format!(
                    "{label}: SyncEvent::Failed received before Running ({})",
                    sync_diagnostic_summary(&koushi_diagnostics::snapshot())
                ));
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_sync_started(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for SyncEvent::Started"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        match event {
            CoreEvent::Sync(SyncEvent::Started {
                request_id: Some(event_request_id),
            }) if event_request_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            } if event_request_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

async fn wait_for_sync_stopped(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for SyncEvent::Stopped"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        if matches!(
            event,
            CoreEvent::Sync(SyncEvent::Stopped {
                request_id: Some(ev_id)
            }) if ev_id == request_id
        ) {
            return Ok(());
        }
        if matches!(
            event,
            CoreEvent::Sync(SyncEvent::Stopped { request_id: None })
        ) {
            return Ok(());
        }
        if let CoreEvent::OperationFailed {
            request_id: ev_id,
            failure,
        } = event
        {
            if ev_id == request_id {
                return Err(format!("{label} failed: {failure:?}"));
            }
        }
    }
}

async fn wait_for_sync_reconnecting(conn: &mut CoreConnection, label: &str) -> Result<(), String> {
    if matches!(
        conn.snapshot().sync,
        koushi_state::SyncState::Reconnecting { .. }
    ) {
        return Ok(());
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for SyncEvent::Reconnecting"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Sync(SyncEvent::Reconnecting) => return Ok(()),
            CoreEvent::StateChanged(snapshot)
                if matches!(snapshot.sync, koushi_state::SyncState::Reconnecting { .. }) =>
            {
                return Ok(());
            }
            CoreEvent::Sync(SyncEvent::Failed) => {
                return Err(format!(
                    "{label}: SyncEvent::Failed received before Reconnecting"
                ));
            }
            _ => {}
        }
    }
}

async fn wait_for_sync_running_after_reconnect(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<(), String> {
    if matches!(conn.snapshot().sync, koushi_state::SyncState::Running) {
        return Ok(());
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for SyncEvent::Running"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Sync(SyncEvent::Running) => return Ok(()),
            CoreEvent::StateChanged(snapshot)
                if matches!(snapshot.sync, koushi_state::SyncState::Running) =>
            {
                return Ok(());
            }
            CoreEvent::Sync(SyncEvent::Failed) => {
                return Err(format!(
                    "{label}: SyncEvent::Failed received before Running"
                ));
            }
            _ => {}
        }
    }
}

async fn wait_for_ready_snapshot(conn: &mut CoreConnection, label: &str) -> Result<(), String> {
    if matches!(conn.snapshot().session, SessionState::Ready(_)) {
        return Ok(());
    }

    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for Ready snapshot"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        if let CoreEvent::StateChanged(snapshot) = event
            && matches!(snapshot.session, SessionState::Ready(_))
        {
            return Ok(());
        }
    }
}

async fn wait_for_logged_in<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<AccountKey, String> {
    if let Some(account_key) = ready_account_key(conn) {
        return Ok(account_key);
    }
    let deadline = QaEventDeadline::after(LOGIN_EVENT_TIMEOUT);
    loop {
        let event = match deadline.recv(conn).await {
            Ok(Ok(event)) => event,
            Ok(Err(lag)) => {
                return Err(format!(
                    "{label}: event stream lagged (skipped={})",
                    lag.skipped
                ));
            }
            Err(_) => {
                // Name the session phase so one failed capture distinguishes
                // "promotion never happened" from "promotion in flight" or
                // "event correlated to another request". Without it the
                // message was identical for every hypothesis (#375).
                let trust_path =
                    trust_admission_diagnostic_summary(&koushi_diagnostics::snapshot());
                return ready_account_key(conn).ok_or_else(|| {
                    format!(
                        "{label}: timed out waiting for LoggedIn event; phase={}; trust_path={trust_path}",
                        gate_session_phase(&conn.snapshot().session),
                    )
                });
            }
        };

        match event {
            CoreEvent::Account(AccountEvent::LoggedIn {
                request_id: ev_id,
                account_key,
            }) if ev_id == request_id => {
                return Ok(account_key);
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => {
                if let Some(account_key) = ready_account_key(conn) {
                    return Ok(account_key);
                }
            }
        }
    }
}

async fn wait_for_session_restored<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    expected_account_key: &AccountKey,
    label: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        if matches!(
            conn.snapshot().session,
            SessionState::AwaitingVerification { .. }
        ) {
            return Err(format!(
                "{label}: trusted restore unexpectedly requires proof; phase={}",
                gate_session_phase(&conn.snapshot().session)
            ));
        }
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for SessionRestored event; phase={}",
                    gate_session_phase(&conn.snapshot().session)
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Account(AccountEvent::SessionRestored {
                request_id: ev_id,
                account_key,
            }) if ev_id == request_id => {
                ensure_session_restored_account_key(&account_key, expected_account_key, label)?;
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_logged_out<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    expected_account_key: &AccountKey,
    label: &str,
) -> Result<(), String> {
    wait_for_logout_barrier(
        conn,
        request_id,
        QaLogoutAccountExpectation::Exact(expected_account_key),
        label,
    )
    .await
}

async fn wait_for_signed_out_after_logout<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    wait_for_logout_barrier(conn, request_id, QaLogoutAccountExpectation::Any, label).await
}

async fn wait_for_logout_barrier<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    account_expectation: QaLogoutAccountExpectation<'_>,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    let mut saw_logged_out = false;
    loop {
        if saw_logged_out && matches!(conn.snapshot().session, SessionState::SignedOut) {
            return Ok(());
        }

        let event = match deadline.recv(conn).await {
            Ok(Ok(event)) => event,
            Err(_) => {
                return if saw_logged_out
                    && matches!(conn.snapshot().session, SessionState::SignedOut)
                {
                    Ok(())
                } else {
                    Err(format!("{label}: timed out waiting for LoggedOut event"))
                };
            }
            Ok(Err(lag)) => {
                return if saw_logged_out
                    && matches!(conn.snapshot().session, SessionState::SignedOut)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "{label}: event stream lagged (skipped={})",
                        lag.skipped
                    ))
                };
            }
        };

        match event {
            CoreEvent::Account(AccountEvent::LoggedOut {
                request_id: ev_id,
                account_key,
            }) if ev_id == request_id => {
                if let QaLogoutAccountExpectation::Exact(expected_account_key) = account_expectation
                    && account_key != *expected_account_key
                {
                    return Err(format!("{label}: LoggedOut account_key mismatch"));
                }
                saw_logged_out = true;
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_operation_failed<S: QaEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<CoreFailure, String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for OperationFailed event"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Ok(failure);
            }
            CoreEvent::Account(account_event) => {
                let matches_request = match &account_event {
                    AccountEvent::LoggedIn { request_id: id, .. }
                    | AccountEvent::SessionRestored { request_id: id, .. }
                    | AccountEvent::SavedSessionsListed { request_id: id, .. }
                    | AccountEvent::RecoveryCompleted { request_id: id, .. }
                    | AccountEvent::ProfileUpdated { request_id: id, .. }
                    | AccountEvent::AvatarThumbnailDownloaded { request_id: id, .. }
                    | AccountEvent::ReportCompleted { request_id: id, .. }
                    | AccountEvent::LoggedOut { request_id: id, .. }
                    | AccountEvent::AccountSwitched { request_id: id, .. } => *id == request_id,
                    AccountEvent::OidcAuthorizationCreated { .. }
                    | AccountEvent::RecoveryRequired { .. } => false,
                };
                if matches_request {
                    return Err(format!(
                        "{label}: expected OperationFailed but the operation succeeded"
                    ));
                }
            }
            _ => continue,
        }
    }
}

async fn wait_for_operation_failed_and_signed_out<S: QaSnapshotEventSource + ?Sized>(
    conn: &mut S,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<CoreFailure, String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    let mut operation_failure = None;
    loop {
        if matches!(conn.snapshot().session, SessionState::SignedOut) {
            if let Some(failure) = operation_failure.take() {
                return Ok(failure);
            }
        }

        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| {
                format!("{label}: timed out waiting for OperationFailed and SignedOut state")
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                operation_failure = Some(failure);
            }
            CoreEvent::Account(account_event) => {
                let matches_request = match &account_event {
                    AccountEvent::LoggedIn { request_id: id, .. }
                    | AccountEvent::SessionRestored { request_id: id, .. }
                    | AccountEvent::SavedSessionsListed { request_id: id, .. }
                    | AccountEvent::RecoveryCompleted { request_id: id, .. }
                    | AccountEvent::ProfileUpdated { request_id: id, .. }
                    | AccountEvent::AvatarThumbnailDownloaded { request_id: id, .. }
                    | AccountEvent::ReportCompleted { request_id: id, .. }
                    | AccountEvent::LoggedOut { request_id: id, .. }
                    | AccountEvent::AccountSwitched { request_id: id, .. } => *id == request_id,
                    AccountEvent::OidcAuthorizationCreated { .. }
                    | AccountEvent::RecoveryRequired { .. } => false,
                };
                if matches_request {
                    return Err(format!(
                        "{label}: expected OperationFailed but the operation succeeded"
                    ));
                }
            }
            _ => continue,
        }
    }
}

async fn subscribe_timeline_for_qa(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    label: &str,
) -> Result<Vec<TimelineItem>, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Timeline(TimelineCommand::Subscribe {
        request_id,
        key: key.clone(),
    }))
    .await
    .map_err(|e| format!("{label}: submit timeline subscribe failed: {e}"))?;
    wait_for_initial_items(conn, key, request_id, label).await
}

struct BodyWaitObserver<'a> {
    expected_body: &'a str,
    saw_decryption_failure: bool,
}

async fn wait_for_initial_items(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<Vec<koushi_core::event::TimelineItem>, String> {
    wait_for_initial_items_from_source(conn, key, request_id, label, TIMELINE_INITIAL_EVENT_TIMEOUT)
        .await
}

async fn wait_for_initial_items_from_source<S: QaEventSource + ?Sized>(
    source: &mut S,
    key: &TimelineKey,
    request_id: koushi_core::ids::RequestId,
    label: &str,
    timeout: Duration,
) -> Result<Vec<koushi_core::event::TimelineItem>, String> {
    let deadline = QaEventDeadline::after(timeout);
    let mut diagnostics = InitialItemsWaitDiagnostics::default();
    loop {
        let event = deadline
            .recv(source)
            .await
            .map_err(|_| diagnostics.timeout_message(label))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        diagnostics.observe(&event, key, request_id);
        match match_initial_items_wait_event(event, key, request_id) {
            InitialItemsWaitMatch::Items(items) => return Ok(items),
            InitialItemsWaitMatch::Failure(failure) => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            InitialItemsWaitMatch::Ignore => continue,
        }
    }
}

fn match_initial_items_wait_event(
    event: CoreEvent,
    key: &TimelineKey,
    request_id: koushi_core::ids::RequestId,
) -> InitialItemsWaitMatch {
    match event {
        CoreEvent::Timeline(TimelineEvent::InitialItems {
            cause_request_id: Some(event_cause_request_id),
            key: event_key,
            items,
            ..
        }) if event_key == *key && event_cause_request_id == request_id => {
            InitialItemsWaitMatch::Items(items)
        }
        CoreEvent::OperationFailed {
            request_id: event_request_id,
            failure,
        } if event_request_id == request_id => InitialItemsWaitMatch::Failure(failure),
        _ => InitialItemsWaitMatch::Ignore,
    }
}

fn find_timeline_item_with_body(
    items: &[koushi_core::event::TimelineItem],
    expected_body: &str,
) -> Option<koushi_core::event::TimelineItem> {
    items
        .iter()
        .find(|item| {
            item.body
                .as_ref()
                .map(|body| body.contains(expected_body))
                .unwrap_or(false)
        })
        .cloned()
}

struct SendFlowOutcome {
    sdk_transaction_id: String,
    send_transaction_id: String,
    event_id: String,
}

struct SendQueueLocalEcho {
    request_id: RequestId,
    client_transaction_id: String,
    sdk_transaction_id: String,
}

struct SendFlowWaiter {
    request_id: koushi_core::ids::RequestId,
    key: TimelineKey,
    expected_client_txn_id: String,
    expected_body: String,
    sdk_transaction_id: Option<String>,
    local_echo_send_state: Option<TimelineSendState>,
    send_transaction_id: Option<String>,
    event_id: Option<String>,
}

async fn wait_for_send_flow_completion(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    key: &TimelineKey,
    client_txn_id: &str,
    expected_body: &str,
    label: &str,
) -> Result<SendFlowOutcome, String> {
    wait_for_send_flow_completion_with_timeout(
        conn,
        request_id,
        key,
        client_txn_id,
        expected_body,
        label,
        EVENT_TIMEOUT,
    )
    .await
}

async fn wait_for_send_flow_completion_with_timeout(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    key: &TimelineKey,
    client_txn_id: &str,
    expected_body: &str,
    label: &str,
    timeout: Duration,
) -> Result<SendFlowOutcome, String> {
    let mut waiter = SendFlowWaiter::new(request_id, key.clone(), client_txn_id, expected_body);

    let deadline = QaEventDeadline::after(timeout);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for send flow completion ({})",
                    waiter.status_summary()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        waiter.observe(event)?;
        if waiter.is_complete() {
            return waiter.finish();
        }
    }
}

async fn send_text_expect_local_echo(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    client_transaction_id: &str,
    body: &str,
    label: &str,
) -> Result<SendQueueLocalEcho, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Timeline(TimelineCommand::SendText {
        request_id,
        key: key.clone(),
        transaction_id: client_transaction_id.to_owned(),
        document: koushi_state::ComposerDocument::from_plain_text(body.to_owned()),
    }))
    .await
    .map_err(|e| format!("{label}: submit SendText failed: {e}"))?;

    let sdk_transaction_id =
        wait_for_local_echo_transaction(conn, key, request_id, body, label).await?;
    Ok(SendQueueLocalEcho {
        request_id,
        client_transaction_id: client_transaction_id.to_owned(),
        sdk_transaction_id,
    })
}

fn timeline_item_body_matches(item: &TimelineItem, expected_body: &str) -> bool {
    item.body
        .as_ref()
        .map(|body| body.contains(expected_body))
        .unwrap_or(false)
}

async fn wait_for_item_with_body(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    expected_body: &str,
    label: &str,
) -> Result<koushi_core::event::TimelineItem, String> {
    let body_matches = |item: &koushi_core::event::TimelineItem| {
        item.body
            .as_ref()
            .map(|body| body.contains(expected_body))
            .unwrap_or(false)
    };

    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for body {expected_body:?}"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                if let Some(item) = find_timeline_item_with_body(&items, expected_body) {
                    return Ok(item);
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                for diff in diffs {
                    let item = match diff {
                        koushi_core::event::TimelineDiff::PushBack { item }
                        | koushi_core::event::TimelineDiff::PushFront { item }
                        | koushi_core::event::TimelineDiff::Insert { item, .. }
                        | koushi_core::event::TimelineDiff::Set { item, .. } => item,
                        koushi_core::event::TimelineDiff::Reset { items } => {
                            if let Some(item) = items.into_iter().find(|item| body_matches(item)) {
                                return Ok(item);
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    if body_matches(&item) {
                        return Ok(item.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

async fn wait_for_item_with_body_or_decryption_failure(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    expected_body: &str,
    label: &str,
) -> Result<koushi_core::event::TimelineItem, String> {
    let deadline = tokio::time::Instant::now() + E2EE_EVENT_TIMEOUT;
    let mut observer = BodyWaitObserver::new(expected_body);
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| observer.timeout_message(label))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                if let Some(item) = observer.observe_items(&items) {
                    return Ok(item);
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                if let Some(item) = observer.observe_diffs(&diffs)? {
                    return Ok(item);
                }
            }
            _ => {}
        }
    }
}

async fn wait_for_bodies_and_pagination_settle(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    initial_items: &[koushi_core::event::TimelineItem],
    expected_bodies: &[&str],
    label: &str,
) -> Result<(), String> {
    // Pre-scan initial items.
    let mut remaining_bodies: Vec<&str> = expected_bodies.to_vec();
    for item in initial_items {
        if let Some(ref body) = item.body {
            remaining_bodies.retain(|expected| !body.contains(expected));
        }
    }

    let mut pagination_settled = false;

    loop {
        if remaining_bodies.is_empty() && pagination_settled {
            return Ok(());
        }

        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out; bodies still needed: {:?}, pagination_settled: {}",
                    remaining_bodies, pagination_settled
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match &event {
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ev_key, diffs, ..
            }) if ev_key == key => {
                for diff in diffs {
                    let item = match diff {
                        koushi_core::event::TimelineDiff::PushBack { item }
                        | koushi_core::event::TimelineDiff::PushFront { item }
                        | koushi_core::event::TimelineDiff::Insert { item, .. }
                        | koushi_core::event::TimelineDiff::Set { item, .. } => item,
                        koushi_core::event::TimelineDiff::Reset { items } => {
                            for it in items {
                                if let Some(ref body) = it.body {
                                    remaining_bodies.retain(|e| !body.contains(e));
                                }
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    if let Some(ref body) = item.body {
                        remaining_bodies.retain(|e| !body.contains(e));
                    }
                }
            }
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ev_key, items, ..
            }) if ev_key == key => {
                for item in items {
                    if let Some(ref body) = item.body {
                        remaining_bodies.retain(|e| !body.contains(e));
                    }
                }
            }
            CoreEvent::Timeline(TimelineEvent::PaginationStateChanged {
                key: ev_key,
                state,
                ..
            }) if ev_key == key => match state {
                PaginationState::Idle
                | PaginationState::EndReached
                | PaginationState::Failed { .. } => {
                    pagination_settled = true;
                }
                PaginationState::Paginating => {}
            },
            _ => {}
        }
    }
}

fn visit_timeline_diff_items(
    diffs: &[TimelineDiff],
    mut visit: impl FnMut(&TimelineItem) -> Result<(), String>,
) -> Result<(), String> {
    for diff in diffs {
        match diff {
            TimelineDiff::PushBack { item }
            | TimelineDiff::PushFront { item }
            | TimelineDiff::Insert { item, .. }
            | TimelineDiff::Set { item, .. } => visit(item)?,
            TimelineDiff::Reset { items } => {
                for item in items {
                    visit(item)?;
                }
            }
            TimelineDiff::Remove { .. } | TimelineDiff::Truncate { .. } | TimelineDiff::Clear => {}
        }
    }
    Ok(())
}
