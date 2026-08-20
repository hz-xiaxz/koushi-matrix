use super::diagnostics::{
    gate_session_phase, sync_diagnostic_summary, trust_admission_diagnostic_summary,
};
use super::registry::{
    E2EE_EVENT_TIMEOUT, EVENT_TIMEOUT, LOGIN_EVENT_TIMEOUT, ROOM_LIST_EVENT_TIMEOUT,
    TIMELINE_INITIAL_EVENT_TIMEOUT,
};
use super::scenario_identity::{
    QaLogoutAccountExpectation, ensure_session_restored_account_key, ready_account_key,
    timeline_item_is_decryption_failure,
};
use super::scenario_timeline::{
    WithheldEventProjectionOrigin, WithheldEventTargetOutcome, withheld_event_target_outcome,
    withheld_event_target_outcome_in_diffs,
};
use super::{
    AccountEvent, AccountKey, AppState, CoreCommand, CoreConnection, CoreEvent, CoreFailure,
    Duration, EventStreamLag, Future, PaginationState, Pin, RequestId, SessionState, SyncCommand,
    SyncEvent, TimelineCommand, TimelineDiff, TimelineEvent, TimelineItem, TimelineItemId,
    TimelineKey, TimelineSendState,
};

pub(super) type QaEventFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CoreEvent, EventStreamLag>> + Send + 'a>>;

pub(super) trait QaEventSource {
    fn recv_event(&mut self) -> QaEventFuture<'_>;
}

pub(super) trait QaSnapshotEventSource: QaEventSource {
    fn snapshot(&self) -> AppState;
}

impl QaEventSource for CoreConnection {
    fn recv_event(&mut self) -> QaEventFuture<'_> {
        Box::pin(CoreConnection::recv_event(self))
    }
}

impl QaSnapshotEventSource for CoreConnection {
    fn snapshot(&self) -> AppState {
        CoreConnection::snapshot(self)
    }
}

#[derive(Clone, Copy)]
pub(super) struct QaEventDeadline {
    pub(super) instant: tokio::time::Instant,
}

impl QaEventDeadline {
    pub(super) fn after(timeout: Duration) -> Self {
        Self {
            instant: tokio::time::Instant::now() + timeout,
        }
    }

    pub(super) async fn recv<S: QaEventSource + ?Sized>(
        self,
        source: &mut S,
    ) -> Result<Result<CoreEvent, EventStreamLag>, tokio::time::error::Elapsed> {
        tokio::time::timeout_at(self.instant, source.recv_event()).await
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PairedEventWaitError {
    Deadline,
    Primary(EventStreamLag),
    Secondary(EventStreamLag),
}

pub(super) async fn wait_for_paired_event_until<Primary, Secondary>(
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

/// Wait for request-scoped `SyncEvent::Started`, then a `Running` state projection.
pub(super) async fn wait_for_sync_started_and_running(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    let mut saw_started = false;
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for Started/Running state"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Sync(SyncEvent::Started { request_id: ev_id })
                if ev_id == Some(request_id) =>
            {
                saw_started = true;
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
            _ => {}
        }

        if saw_started && matches!(conn.snapshot().sync, koushi_state::SyncState::Running) {
            return Ok(());
        }
    }
}

pub(super) async fn wait_for_sync_started(
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

/// Wait for `SyncEvent::Stopped` with the given request_id.
pub(super) async fn wait_for_sync_stopped(
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

pub(super) async fn stop_sync_for_qa(conn: &mut CoreConnection, label: &str) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Sync(SyncCommand::Stop { request_id }))
        .await
        .map_err(|e| format!("{label}: submit Sync stop failed: {e}"))?;
    wait_for_sync_stopped(conn, request_id, label).await
}

pub(super) async fn start_sync_for_qa(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Sync(SyncCommand::Start { request_id }))
        .await
        .map_err(|e| format!("{label}: submit Sync start failed: {e}"))?;
    wait_for_sync_started_and_running(conn, request_id, label).await
}

pub(super) async fn wait_for_sync_reconnecting(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<(), String> {
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

pub(super) async fn wait_for_sync_running_after_reconnect(
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

/// Wait for a `StateChanged` snapshot where `SessionState::Ready`.
pub(super) async fn wait_for_ready_snapshot(
    conn: &mut CoreConnection,
    label: &str,
) -> Result<(), String> {
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

pub(super) async fn wait_for_logged_in<S: QaSnapshotEventSource + ?Sized>(
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

/// Wait for `AccountEvent::SessionRestored` with the given request_id.
pub(super) async fn wait_for_session_restored<S: QaSnapshotEventSource + ?Sized>(
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

/// Wait for `AccountEvent::LoggedOut` with the given request_id.
pub(super) async fn wait_for_logged_out<S: QaSnapshotEventSource + ?Sized>(
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

pub(super) async fn wait_for_signed_out_after_logout<S: QaSnapshotEventSource + ?Sized>(
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

/// Wait for `OperationFailed` with the given request_id and return the failure.
pub(super) async fn wait_for_operation_failed<S: QaEventSource + ?Sized>(
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

pub(super) async fn wait_for_operation_failed_and_signed_out<S: QaSnapshotEventSource + ?Sized>(
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

pub(super) async fn subscribe_timeline_for_qa(
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

#[derive(Debug)]
struct BodyWaitObserver<'a> {
    expected_body: &'a str,
    saw_decryption_failure: bool,
}

impl<'a> BodyWaitObserver<'a> {
    fn new(expected_body: &'a str) -> Self {
        Self {
            expected_body,
            saw_decryption_failure: false,
        }
    }

    fn observe_items(&mut self, items: &[TimelineItem]) -> Option<TimelineItem> {
        if let Some(item) = find_timeline_item_with_body(items, self.expected_body) {
            return Some(item);
        }
        if items.iter().any(timeline_item_is_decryption_failure) {
            self.saw_decryption_failure = true;
        }
        None
    }

    fn observe_diffs(&mut self, diffs: &[TimelineDiff]) -> Result<Option<TimelineItem>, String> {
        let mut found = None;
        visit_timeline_diff_items(diffs, |item| {
            if found.is_none() && timeline_item_body_matches(item, self.expected_body) {
                found = Some(item.clone());
            }
            if timeline_item_is_decryption_failure(item) {
                self.saw_decryption_failure = true;
            }
            Ok(())
        })?;
        Ok(found)
    }

    fn timeout_message(&self, label: &str) -> String {
        if self.saw_decryption_failure {
            format!(
                "{label}: timed out waiting for body {:?} after transient undecryptable events",
                self.expected_body
            )
        } else {
            format!(
                "{label}: timed out waiting for body {:?}",
                self.expected_body
            )
        }
    }
}

/// Wait for `TimelineEvent::InitialItems` for the given key and request_id.
/// Returns the initial item list.
pub(super) async fn wait_for_initial_items(
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

#[derive(Default)]
struct InitialItemsWaitDiagnostics {
    same_key_exact_cause: u64,
    same_key_wrong_cause: u64,
    same_key_causeless: u64,
    wrong_key_initial_items: u64,
    unrelated_events: u64,
}

impl InitialItemsWaitDiagnostics {
    fn observe(&mut self, event: &CoreEvent, key: &TimelineKey, request_id: RequestId) {
        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                cause_request_id,
                key: event_key,
                ..
            }) if event_key == key => match cause_request_id {
                Some(cause_request_id) if *cause_request_id == request_id => {
                    self.same_key_exact_cause += 1;
                }
                Some(_) => self.same_key_wrong_cause += 1,
                None => self.same_key_causeless += 1,
            },
            CoreEvent::Timeline(TimelineEvent::InitialItems { .. }) => {
                self.wrong_key_initial_items += 1;
            }
            _ => self.unrelated_events += 1,
        }
    }

    fn timeout_message(&self, label: &str) -> String {
        format!(
            "{label}: timed out waiting for TimelineEvent::InitialItems \
             (same_key_exact_cause={}, same_key_wrong_cause={}, same_key_causeless={}, \
             wrong_key_initial_items={}, unrelated_events={})",
            self.same_key_exact_cause,
            self.same_key_wrong_cause,
            self.same_key_causeless,
            self.wrong_key_initial_items,
            self.unrelated_events,
        )
    }
}

enum InitialItemsWaitMatch {
    Items(Vec<koushi_core::event::TimelineItem>),
    Failure(CoreFailure),
    Ignore,
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

pub(super) fn find_timeline_item_with_body(
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct SendFlowOutcome {
    pub(super) sdk_transaction_id: String,
    send_transaction_id: String,
    pub(super) event_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct SendQueueLocalEcho {
    pub(super) request_id: RequestId,
    pub(super) client_transaction_id: String,
    pub(super) sdk_transaction_id: String,
}

#[derive(Debug)]
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

impl SendFlowWaiter {
    fn new(
        request_id: koushi_core::ids::RequestId,
        key: TimelineKey,
        expected_client_txn_id: impl Into<String>,
        expected_body: impl Into<String>,
    ) -> Self {
        Self {
            request_id,
            key,
            expected_client_txn_id: expected_client_txn_id.into(),
            expected_body: expected_body.into(),
            sdk_transaction_id: None,
            local_echo_send_state: None,
            send_transaction_id: None,
            event_id: None,
        }
    }

    fn observe(&mut self, event: CoreEvent) -> Result<(), String> {
        match event {
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == &self.key => {
                self.observe_local_echo(diffs);
            }
            CoreEvent::Timeline(TimelineEvent::SendCompleted {
                request_id: ev_id,
                key: ref ev_key,
                transaction_id,
                event_id,
            }) if ev_id == self.request_id && ev_key == &self.key => {
                if transaction_id != self.expected_client_txn_id {
                    return Err(format!(
                        "send completed txn_id mismatch: expected {}, got {}",
                        self.expected_client_txn_id, transaction_id
                    ));
                }
                self.send_transaction_id = Some(transaction_id);
                self.event_id = Some(event_id);
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == self.request_id => {
                return Err(format!("send flow failed: {failure:?}"));
            }
            _ => {}
        }
        if matches!(
            self.local_echo_send_state,
            Some(TimelineSendState::NotSent { .. })
        ) && self.send_transaction_id.is_none()
        {
            return Err(format!("send flow failed: {}", self.status_summary()));
        }
        Ok(())
    }

    fn observe_local_echo(&mut self, diffs: Vec<koushi_core::event::TimelineDiff>) {
        for diff in &diffs {
            let item = match diff {
                koushi_core::event::TimelineDiff::PushBack { item }
                | koushi_core::event::TimelineDiff::PushFront { item }
                | koushi_core::event::TimelineDiff::Insert { item, .. }
                | koushi_core::event::TimelineDiff::Set { item, .. } => item,
                _ => continue,
            };
            if item
                .body
                .as_ref()
                .map(|body| body.contains(&self.expected_body))
                .unwrap_or(false)
            {
                if let Some(state) = item.send_state.as_ref() {
                    self.local_echo_send_state = Some(state.clone());
                }
                if let koushi_core::event::TimelineItemId::Transaction { transaction_id } = &item.id
                {
                    if self.sdk_transaction_id.is_none() {
                        self.sdk_transaction_id = Some(transaction_id.clone());
                    }
                    break;
                }
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.sdk_transaction_id.is_some()
            && self.send_transaction_id.is_some()
            && self.event_id.is_some()
    }

    fn status_summary(&self) -> String {
        format!(
            "local_echo={} local_echo_send_state={} send_completed={} event_id={}",
            self.sdk_transaction_id.is_some(),
            self.local_echo_send_state
                .as_ref()
                .map(timeline_send_state_label)
                .unwrap_or("missing"),
            self.send_transaction_id.is_some(),
            self.event_id.is_some()
        )
    }

    fn finish(self) -> Result<SendFlowOutcome, String> {
        Ok(SendFlowOutcome {
            sdk_transaction_id: self
                .sdk_transaction_id
                .ok_or_else(|| "send flow: missing local echo".to_owned())?,
            send_transaction_id: self
                .send_transaction_id
                .ok_or_else(|| "send flow: missing SendCompleted".to_owned())?,
            event_id: self
                .event_id
                .ok_or_else(|| "send flow: missing SendCompleted event id".to_owned())?,
        })
    }
}

fn timeline_send_state_label(state: &TimelineSendState) -> &'static str {
    match state {
        TimelineSendState::Sending => "Sending",
        TimelineSendState::NotSent {
            reason: koushi_core::event::TimelineSendFailureReason::Recoverable,
        } => "NotSent(recoverable)",
        TimelineSendState::NotSent {
            reason: koushi_core::event::TimelineSendFailureReason::Unrecoverable,
        } => "NotSent(unrecoverable)",
        TimelineSendState::Cancelled => "Cancelled",
        TimelineSendState::Sent => "Sent",
    }
}

/// Wait for both the local echo diff and `TimelineEvent::SendCompleted`
/// for a single send sequence, accepting either order.
pub(super) async fn wait_for_send_flow_completion(
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

pub(super) async fn wait_for_send_flow_completion_with_timeout(
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

pub(super) async fn send_text_expect_local_echo(
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

async fn wait_for_local_echo_transaction(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    request_id: RequestId,
    expected_body: &str,
    label: &str,
) -> Result<String, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for local echo"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                let mut found = None;
                visit_timeline_diff_items(&diffs, |item| {
                    if timeline_item_body_matches(item, expected_body)
                        && let Some(transaction_id) = timeline_item_transaction_id(item)
                    {
                        found = Some(transaction_id.to_owned());
                    }
                    Ok(())
                })?;
                if let Some(transaction_id) = found {
                    return Ok(transaction_id);
                }
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label}: send command failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

pub(super) async fn wait_for_timeline_send_state(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    sdk_transaction_id: &str,
    matches_state: impl Fn(&TimelineSendState) -> bool,
    label: &str,
) -> Result<TimelineSendState, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for send state"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                for item in &items {
                    if timeline_item_transaction_id(item) == Some(sdk_transaction_id)
                        && let Some(state) = item.send_state.as_ref()
                        && matches_state(state)
                    {
                        return Ok(state.clone());
                    }
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                let mut found = None;
                visit_timeline_diff_items(&diffs, |item| {
                    if timeline_item_transaction_id(item) == Some(sdk_transaction_id)
                        && let Some(state) = item.send_state.as_ref()
                        && matches_state(state)
                    {
                        found = Some(state.clone());
                    }
                    Ok(())
                })?;
                if let Some(state) = found {
                    return Ok(state);
                }
            }
            _ => {}
        }
    }
}

pub(super) async fn retry_send_queue_item(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    sdk_transaction_id: &str,
    label: &str,
) -> Result<RequestId, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Timeline(TimelineCommand::RetrySend {
        request_id,
        key: key.clone(),
        transaction_id: sdk_transaction_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit RetrySend failed: {e}"))?;
    Ok(request_id)
}

pub(super) async fn cancel_send_queue_item(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    sdk_transaction_id: &str,
    label: &str,
) -> Result<RequestId, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Timeline(TimelineCommand::CancelSend {
        request_id,
        key: key.clone(),
        transaction_id: sdk_transaction_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit CancelSend failed: {e}"))?;
    Ok(request_id)
}

pub(super) async fn wait_for_event_item_with_body_or_retry_not_sent(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    sdk_transaction_id: &str,
    expected_body: &str,
    mut retry_sent: bool,
    label: &str,
) -> Result<TimelineItem, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for restored send completion"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                for item in items {
                    if timeline_item_body_matches(&item, expected_body)
                        && matches!(item.id, TimelineItemId::Event { .. })
                    {
                        return Ok(item);
                    }
                    if !retry_sent
                        && timeline_item_transaction_id(&item) == Some(sdk_transaction_id)
                        && matches!(item.send_state, Some(TimelineSendState::NotSent { .. }))
                    {
                        retry_send_queue_item(conn, key, sdk_transaction_id, label).await?;
                        retry_sent = true;
                    }
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                let mut found = None;
                let mut should_retry = false;
                visit_timeline_diff_items(&diffs, |item| {
                    if timeline_item_body_matches(item, expected_body)
                        && matches!(item.id, TimelineItemId::Event { .. })
                    {
                        found = Some(item.clone());
                    }
                    if !retry_sent
                        && timeline_item_transaction_id(item) == Some(sdk_transaction_id)
                        && matches!(
                            item.send_state.as_ref(),
                            Some(TimelineSendState::NotSent { .. })
                        )
                    {
                        should_retry = true;
                    }
                    Ok(())
                })?;
                if let Some(item) = found {
                    return Ok(item);
                }
                if should_retry {
                    retry_send_queue_item(conn, key, sdk_transaction_id, label).await?;
                    retry_sent = true;
                }
            }
            _ => {}
        }
    }
}

pub(super) fn timeline_item_body_matches(item: &TimelineItem, expected_body: &str) -> bool {
    item.body
        .as_ref()
        .map(|body| body.contains(expected_body))
        .unwrap_or(false)
}

pub(super) fn timeline_item_transaction_id(item: &TimelineItem) -> Option<&str> {
    match &item.id {
        TimelineItemId::Transaction { transaction_id } => Some(transaction_id.as_str()),
        TimelineItemId::Event { .. } | TimelineItemId::Synthetic { .. } => None,
    }
}

/// Wait for `TimelineEvent::SendCompleted` with the given request_id and key.
/// Returns `(transaction_id, event_id)`.
pub(super) async fn wait_for_send_completed(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    key: &TimelineKey,
    label: &str,
) -> Result<(String, String), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for SendCompleted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::SendCompleted {
                request_id: ev_id,
                key: ref ev_key,
                transaction_id,
                event_id,
            }) if ev_id == request_id && ev_key == key => {
                return Ok((transaction_id, event_id));
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

struct MediaSendWaiter {
    request_id: koushi_core::ids::RequestId,
    key: TimelineKey,
    expected_client_txn_id: String,
    saw_local_media_echo: bool,
    saw_upload_progress: bool,
    event_id: Option<String>,
}

impl MediaSendWaiter {
    fn new(
        request_id: koushi_core::ids::RequestId,
        key: TimelineKey,
        expected_client_txn_id: impl Into<String>,
    ) -> Self {
        Self {
            request_id,
            key,
            expected_client_txn_id: expected_client_txn_id.into(),
            saw_local_media_echo: false,
            saw_upload_progress: false,
            event_id: None,
        }
    }

    fn observe(&mut self, event: CoreEvent) -> Result<(), String> {
        match event {
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == &self.key => {
                if !self.saw_local_media_echo {
                    self.saw_local_media_echo =
                        media_diffs_include_transaction_media(&diffs, &self.expected_client_txn_id);
                }
            }
            CoreEvent::Timeline(TimelineEvent::MediaUploadProgress {
                request_id,
                key: ref ev_key,
                transaction_id,
                progress,
                ..
            }) if ev_key == &self.key && transaction_id == self.expected_client_txn_id => {
                if let Some(request_id) = request_id
                    && request_id != self.request_id
                {
                    return Err("media upload progress request_id mismatch".to_owned());
                }
                if progress.total > 0 && progress.current <= progress.total {
                    self.saw_upload_progress = true;
                }
            }
            CoreEvent::Timeline(TimelineEvent::SendCompleted {
                request_id,
                key: ref ev_key,
                transaction_id,
                event_id,
            }) if request_id == self.request_id && ev_key == &self.key => {
                if transaction_id != self.expected_client_txn_id {
                    return Err("media send transaction_id mismatch".to_owned());
                }
                self.event_id = Some(event_id);
            }
            CoreEvent::OperationFailed {
                request_id,
                failure,
            } if request_id == self.request_id => {
                return Err(format!("media send failed: {failure:?}"));
            }
            _ => {}
        }
        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.saw_local_media_echo && self.saw_upload_progress && self.event_id.is_some()
    }
}

fn media_diffs_include_transaction_media(
    diffs: &[koushi_core::event::TimelineDiff],
    expected_transaction_id: &str,
) -> bool {
    diffs.iter().any(|diff| match diff {
        koushi_core::event::TimelineDiff::PushBack { item }
        | koushi_core::event::TimelineDiff::PushFront { item }
        | koushi_core::event::TimelineDiff::Insert { item, .. }
        | koushi_core::event::TimelineDiff::Set { item, .. } => {
            timeline_item_is_transaction_media(item, expected_transaction_id)
        }
        koushi_core::event::TimelineDiff::Reset { items } => items
            .iter()
            .any(|item| timeline_item_is_transaction_media(item, expected_transaction_id)),
        _ => false,
    })
}

fn timeline_item_is_transaction_media(
    item: &koushi_core::event::TimelineItem,
    expected_transaction_id: &str,
) -> bool {
    item.media.is_some()
        && matches!(
            &item.id,
            koushi_core::event::TimelineItemId::Transaction { transaction_id }
                if transaction_id == expected_transaction_id
        )
}

pub(super) async fn wait_for_media_send_flow_completion(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    key: &TimelineKey,
    client_txn_id: &str,
    label: &str,
) -> Result<String, String> {
    let mut waiter = MediaSendWaiter::new(request_id, key.clone(), client_txn_id);

    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for media send flow completion"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        waiter.observe(event)?;
        if waiter.is_complete() {
            return waiter
                .event_id
                .ok_or_else(|| "media send flow: missing event id".to_owned());
        }
    }
}

pub(super) async fn wait_for_media_item(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    label: &str,
) -> Result<koushi_core::event::TimelineItem, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for media item"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                if let Some(item) = items.into_iter().find(|item| item.media.is_some()) {
                    return Ok(item);
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                for diff in diffs {
                    match diff {
                        koushi_core::event::TimelineDiff::PushBack { item }
                        | koushi_core::event::TimelineDiff::PushFront { item }
                        | koushi_core::event::TimelineDiff::Insert { item, .. }
                        | koushi_core::event::TimelineDiff::Set { item, .. } => {
                            if item.media.is_some() {
                                return Ok(item);
                            }
                        }
                        koushi_core::event::TimelineDiff::Reset { items } => {
                            if let Some(item) = items.into_iter().find(|item| item.media.is_some())
                            {
                                return Ok(item);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) async fn wait_for_media_download_completed(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    key: &TimelineKey,
    expected_event_id: &str,
    expected_byte_count: u64,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for media download completion"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::MediaDownloadCompleted {
                request_id: ev_id,
                key: ref ev_key,
                event_id,
                byte_count,
                ..
            }) if ev_id == request_id && ev_key == key => {
                if event_id != expected_event_id {
                    return Err("media download event_id mismatch".to_owned());
                }
                if byte_count != expected_byte_count {
                    return Err(format!(
                        "media download byte_count mismatch: expected {expected_byte_count}, got {byte_count}"
                    ));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

/// Wait for an item whose body contains `expected_body` and return the item so
/// the caller can assert relation metadata on the projected DTO.
pub(super) async fn wait_for_item_with_body(
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

pub(super) async fn wait_for_event_item_with_body(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    expected_body: &str,
    label: &str,
) -> Result<TimelineItem, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for event body {expected_body:?}"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                if let Some(item) = items.into_iter().find(|item| {
                    timeline_item_body_matches(item, expected_body)
                        && matches!(item.id, TimelineItemId::Event { .. })
                }) {
                    return Ok(item);
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                let mut found = None;
                visit_timeline_diff_items(&diffs, |item| {
                    if found.is_none()
                        && timeline_item_body_matches(item, expected_body)
                        && matches!(item.id, TimelineItemId::Event { .. })
                    {
                        found = Some(item.clone());
                    }
                    Ok(())
                })?;
                if let Some(item) = found {
                    return Ok(item);
                }
            }
            _ => {}
        }
    }
}

pub(super) async fn wait_for_link_preview_item_projection(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    request_id: RequestId,
    expected_body: &str,
    label: &str,
    predicate: impl Fn(&TimelineItem) -> bool,
) -> Result<TimelineItem, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for link-preview projection"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Timeline(TimelineEvent::InitialItems {
                key: ref ev_key,
                items,
                ..
            }) if ev_key == key => {
                if let Some(item) = items
                    .into_iter()
                    .find(|item| timeline_item_body_matches(item, expected_body) && predicate(item))
                {
                    return Ok(item);
                }
            }
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
                key: ref ev_key,
                diffs,
                ..
            }) if ev_key == key => {
                let mut found = None;
                visit_timeline_diff_items(&diffs, |item| {
                    if found.is_none()
                        && timeline_item_body_matches(item, expected_body)
                        && predicate(item)
                    {
                        found = Some(item.clone());
                    }
                    Ok(())
                })?;
                if let Some(item) = found {
                    return Ok(item);
                }
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label}: command failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

pub(super) async fn wait_for_item_with_body_or_decryption_failure(
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

pub(super) async fn wait_for_withheld_event_projection_from_source<S: QaEventSource + ?Sized>(
    source: &mut S,
    key: &TimelineKey,
    target_event_id: &str,
    expected_body: &str,
    initial_items: &[TimelineItem],
    label: &str,
    timeout: Duration,
) -> Result<WithheldEventProjectionOrigin, String> {
    match withheld_event_target_outcome(initial_items, target_event_id, expected_body) {
        WithheldEventTargetOutcome::DecryptionFailure => {
            return Ok(WithheldEventProjectionOrigin::InitialItems);
        }
        WithheldEventTargetOutcome::NonFailure {
            has_body,
            has_typed_decryption_failure,
            matches_expected_body,
        } => {
            return Err(format!(
                "{label}: withheld target projection_outcome=non_failure \
                 projection_origin=initial_items has_body={has_body} \
                 has_typed_decryption_failure={has_typed_decryption_failure} \
                 matches_expected_body={matches_expected_body}"
            ));
        }
        WithheldEventTargetOutcome::Missing => {}
    }

    let deadline = QaEventDeadline::after(timeout);
    let mut matching_update_batches = 0u64;
    loop {
        let event = deadline.recv(source).await.map_err(|_| {
            format!(
                "{label}: withheld target projection_outcome=absent \
                 projection_origin=missing matching_update_batches={matching_update_batches}"
            )
        })?;
        let event = event
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        let CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
            key: event_key,
            diffs,
            ..
        }) = event
        else {
            continue;
        };
        if event_key != *key {
            continue;
        }
        matching_update_batches += 1;
        match withheld_event_target_outcome_in_diffs(&diffs, target_event_id, expected_body)? {
            WithheldEventTargetOutcome::Missing => {}
            WithheldEventTargetOutcome::DecryptionFailure => {
                return Ok(WithheldEventProjectionOrigin::ItemsUpdated);
            }
            WithheldEventTargetOutcome::NonFailure {
                has_body,
                has_typed_decryption_failure,
                matches_expected_body,
            } => {
                return Err(format!(
                    "{label}: withheld target projection_outcome=non_failure \
                     projection_origin=items_updated has_body={has_body} \
                     has_typed_decryption_failure={has_typed_decryption_failure} \
                     matches_expected_body={matches_expected_body}"
                ));
            }
        }
    }
}

/// Wait until all `expected_bodies` are found AND pagination has settled (Idle
/// or EndReached). Scans `initial_items` first, then both ItemsUpdated diffs
/// and PaginationStateChanged events in a single loop. This avoids the race
/// where paginate diffs are consumed before the body scan starts.
pub(super) async fn wait_for_bodies_and_pagination_settle(
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

#[allow(dead_code)]
pub(super) fn visit_timeline_diff_items(
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

#[cfg(test)]
#[path = "event_wait_tests.rs"]
mod tests;
