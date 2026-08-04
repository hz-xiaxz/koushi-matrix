//! SyncActor: the single production Simplified Sliding Sync owner.
//!
//! `AccountActor` owns this actor and supplies the encryption-sync permit that
//! was previously held by provisional verification. A normal session creates
//! exactly one SDK `SyncService`; that service owns exactly one unfiltered
//! `all_rooms` list and the encryption connection.
//!
//! `SyncService::State::Running` means only that the SDK engine is running. It
//! is not connectivity evidence. Koushi becomes connected and hands the live
//! room-list service to dependent actors after the SDK reports a committed
//! `all_rooms` response with a Sliding Sync position, and RoomActor
//! acknowledges projecting that exact committed response. The SDK may omit a
//! room count for an empty account, so complete-range status remains a
//! projection diagnostic rather than a startup gate.

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_sdk::MatrixClientSession;
use koushi_state::{AppAction, RoomListSource, SyncLifecycleStatus};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::command::SyncCommand;
use crate::event::{CoreEvent, SyncEvent};
use crate::executor;
#[cfg(any(test, feature = "test-hooks", feature = "qa-bin"))]
use crate::failure::CoreFailure;
use crate::failure::SyncFailureKind;
use crate::ids::RequestId;
use crate::room::{RoomListReconcileAck, RoomMessage};
use crate::{
    SlidingSyncDiagnostics, SlidingSyncFailureDiagnostic, SlidingSyncFailureKind,
    SlidingSyncFailureOrigin, SlidingSyncFailureRetryability, SlidingSyncFailureStage,
    SlidingSyncHttpErrorSource, SlidingSyncHttpStatus, SlidingSyncMatrixErrorKind,
    SlidingSyncSdkVersion,
};

const SYNC_ACTOR_SHUTDOWN_SEND_TIMEOUT: Duration = Duration::from_secs(1);
const SYNC_ACTOR_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(10);
const SYNC_SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const ROOM_OBSERVATION_ACK_TIMEOUT: Duration = Duration::from_secs(10);

macro_rules! trace_sync {
    ($stage:expr, [$($field:expr),* $(,)?], $($arg:tt)*) => {{
        let event = DiagnosticEvent::new(DiagnosticLevel::Debug, "core.sync", $stage)
            $(.field($field))*;
        record(event);
    }};
}

pub enum SyncMessage {
    Command(SyncCommand),
    Shutdown,
}

pub struct SyncActorHandle {
    tx: mpsc::Sender<SyncMessage>,
    task: executor::JoinHandle<()>,
}

impl SyncActorHandle {
    pub async fn send(&self, msg: SyncMessage) -> bool {
        self.tx.send(msg).await.is_ok()
    }

    pub async fn shutdown(self) -> bool {
        self.shutdown_with_timeout(SYNC_ACTOR_SHUTDOWN_JOIN_TIMEOUT)
            .await
    }

    async fn shutdown_with_timeout(mut self, timeout: Duration) -> bool {
        let _ = executor::timeout(
            SYNC_ACTOR_SHUTDOWN_SEND_TIMEOUT,
            self.tx.send(SyncMessage::Shutdown),
        )
        .await;
        match executor::timeout(timeout, &mut self.task).await {
            Ok(_) => true,
            Err(_) => {
                self.task.abort();
                let _ = self.task.await;
                false
            }
        }
    }

    pub async fn join(self) {
        let _ = self.task.await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncLifecycle {
    Stopped,
    Starting,
    Running,
    Reconnecting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncActorControl {
    FirstResponseCommitted {
        run_generation: u64,
    },
    Reconnecting {
        run_generation: u64,
        reason: &'static str,
    },
    Recovered {
        run_generation: u64,
    },
}

fn accepts_control(
    lifecycle: SyncLifecycle,
    active_generation: u64,
    observed_generation: u64,
    expected: &[SyncLifecycle],
) -> bool {
    active_generation == observed_generation && expected.contains(&lifecycle)
}

#[derive(Debug)]
enum SyncTaskOutcome {
    Failed {
        kind: SyncFailureKind,
        ever_connected: bool,
    },
    Panicked,
}

fn internal_observer_failure(ever_connected: bool) -> SyncTaskOutcome {
    SyncTaskOutcome::Failed {
        kind: SyncFailureKind::Internal,
        ever_connected,
    }
}

fn internal_observer_failure_at(reason: &'static str, ever_connected: bool) -> SyncTaskOutcome {
    trace_sync!(
        "observer_exit",
        [
            DiagnosticField::token("reason", reason),
            DiagnosticField::boolean("ever_connected", ever_connected),
        ],
        "reason={} ever_connected={}",
        reason,
        ever_connected
    );
    internal_observer_failure(ever_connected)
}

#[cfg(any(test, feature = "test-hooks", feature = "qa-bin"))]
fn sync_once_admitted(
    lifecycle: SyncLifecycle,
    sync_task_active: bool,
    sync_service_active: bool,
) -> bool {
    matches!(lifecycle, SyncLifecycle::Stopped | SyncLifecycle::Failed)
        && !sync_task_active
        && !sync_service_active
}

fn sync_lifecycle_label(lifecycle: SyncLifecycle) -> &'static str {
    match lifecycle {
        SyncLifecycle::Stopped => "stopped",
        SyncLifecycle::Starting => "starting",
        SyncLifecycle::Running => "running",
        SyncLifecycle::Reconnecting => "reconnecting",
        SyncLifecycle::Failed => "failed",
    }
}

fn sync_status_trace_label(status: &SyncLifecycleStatus) -> &'static str {
    match status {
        SyncLifecycleStatus::Stopped => "stopped",
        SyncLifecycleStatus::Starting => "starting",
        SyncLifecycleStatus::Running => "running",
        SyncLifecycleStatus::Failed { .. } => "failed",
        SyncLifecycleStatus::Reconnecting { .. } => "reconnecting",
    }
}

async fn send_sync_status(
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    generation: &AtomicU64,
    status: SyncLifecycleStatus,
) {
    let label = sync_status_trace_label(&status);
    let generation = generation.fetch_add(1, Ordering::Relaxed) + 1;
    trace_sync!(
        "status_projected",
        [
            DiagnosticField::count("generation", generation),
            DiagnosticField::token("lifecycle", label),
        ],
        "generation={} lifecycle={}",
        generation,
        label
    );
    let _ = action_tx
        .send(vec![AppAction::SyncStatusChanged { generation, status }])
        .await;
}

fn sync_command_trace_parts(command: &SyncCommand) -> (&'static str, RequestId) {
    match command {
        SyncCommand::Start { request_id } => ("start", *request_id),
        SyncCommand::Stop { request_id } => ("stop", *request_id),
        SyncCommand::Restart { request_id } => ("restart", *request_id),
        #[cfg(any(test, feature = "test-hooks", feature = "qa-bin"))]
        SyncCommand::SyncOnce { request_id } => ("sync_once", *request_id),
    }
}

fn sync_service_state_trace_label(state: &matrix_sdk_ui::sync_service::State) -> &'static str {
    match state {
        matrix_sdk_ui::sync_service::State::Idle => "idle",
        matrix_sdk_ui::sync_service::State::Running => "running",
        matrix_sdk_ui::sync_service::State::Offline(_) => "offline",
        matrix_sdk_ui::sync_service::State::Error(_) => "error",
        matrix_sdk_ui::sync_service::State::Terminated => "terminated",
    }
}

fn committed_response_is_handoff_evidence(
    pos_present: bool,
    response_sequence: u64,
    last_committed_sequence: u64,
) -> bool {
    pos_present && response_sequence > last_committed_sequence
}

pub struct SyncActor {
    session: Arc<MatrixClientSession>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    command_rx: mpsc::Receiver<SyncMessage>,
    control_tx: mpsc::Sender<SyncActorControl>,
    control_rx: mpsc::Receiver<SyncActorControl>,
    room_tx: mpsc::Sender<RoomMessage>,
    timeline_tx: mpsc::Sender<crate::timeline::TimelineMessage>,
    lifecycle: SyncLifecycle,
    sync_generation: Arc<AtomicU64>,
    encryption_sync_permit: koushi_sdk::EncryptionSyncPermitOwner,
    run_generation: u64,
    sync_task: Option<executor::JoinHandle<SyncTaskOutcome>>,
    sync_service: Option<Arc<matrix_sdk_ui::sync_service::SyncService>>,
    active_start_request_id: Option<RequestId>,
    ignored_user_list_handler: Option<matrix_sdk::event_handler::EventHandlerHandle>,
    diagnostics: SlidingSyncDiagnostics,
}

impl SyncActor {
    pub(crate) fn spawn(
        session: Arc<MatrixClientSession>,
        action_tx: mpsc::Sender<Vec<AppAction>>,
        event_tx: broadcast::Sender<CoreEvent>,
        room_tx: mpsc::Sender<RoomMessage>,
        timeline_tx: mpsc::Sender<crate::timeline::TimelineMessage>,
        sync_generation: Arc<AtomicU64>,
        encryption_sync_permit: koushi_sdk::EncryptionSyncPermitOwner,
        diagnostics: SlidingSyncDiagnostics,
    ) -> SyncActorHandle {
        let (tx, command_rx) = mpsc::channel(16);
        let (control_tx, control_rx) = mpsc::channel(4);
        let actor = Self {
            session,
            action_tx,
            event_tx,
            command_rx,
            control_tx,
            control_rx,
            room_tx,
            timeline_tx,
            lifecycle: SyncLifecycle::Stopped,
            sync_generation,
            encryption_sync_permit,
            run_generation: 0,
            sync_task: None,
            sync_service: None,
            active_start_request_id: None,
            ignored_user_list_handler: None,
            diagnostics,
        };
        let task = executor::spawn(actor.run());
        SyncActorHandle { tx, task }
    }

    async fn run(mut self) {
        loop {
            if self.sync_task.is_some() {
                tokio::select! {
                    biased;
                    outcome = async { self.sync_task.as_mut().unwrap().await } => {
                        let outcome = outcome.unwrap_or(SyncTaskOutcome::Panicked);
                        self.sync_task = None;
                        self.handle_sync_task_ended(outcome).await;
                    }
                    msg = self.command_rx.recv() => match msg {
                        None | Some(SyncMessage::Shutdown) => {
                            self.do_stop(None).await;
                            break;
                        }
                        Some(SyncMessage::Command(command)) => self.handle_command(command).await,
                    },
                    control = self.control_rx.recv() => {
                        if let Some(control) = control {
                            self.handle_control(control).await;
                        }
                    }
                }
            } else {
                match self.command_rx.recv().await {
                    None | Some(SyncMessage::Shutdown) => break,
                    Some(SyncMessage::Command(command)) => self.handle_command(command).await,
                }
            }
        }
        if self.sync_task.is_some() {
            self.do_stop(None).await;
        }
    }

    async fn handle_control(&mut self, control: SyncActorControl) {
        match control {
            SyncActorControl::FirstResponseCommitted { run_generation }
                if accepts_control(
                    self.lifecycle,
                    self.run_generation,
                    run_generation,
                    &[SyncLifecycle::Starting, SyncLifecycle::Reconnecting],
                ) =>
            {
                self.lifecycle = SyncLifecycle::Running;
            }
            SyncActorControl::Reconnecting {
                run_generation,
                reason,
            } if accepts_control(
                self.lifecycle,
                self.run_generation,
                run_generation,
                &[SyncLifecycle::Starting, SyncLifecycle::Running],
            ) =>
            {
                self.lifecycle = SyncLifecycle::Reconnecting;
                self.emit(CoreEvent::Sync(SyncEvent::Reconnecting));
                self.project_sync_status(SyncLifecycleStatus::Reconnecting {
                    reason: reason.to_owned(),
                })
                .await;
            }
            SyncActorControl::Recovered { run_generation }
                if accepts_control(
                    self.lifecycle,
                    self.run_generation,
                    run_generation,
                    &[SyncLifecycle::Reconnecting],
                ) =>
            {
                self.lifecycle = SyncLifecycle::Running;
                self.emit(CoreEvent::Sync(SyncEvent::Running));
                self.project_sync_status(SyncLifecycleStatus::Running).await;
            }
            _ => {}
        }
    }

    async fn handle_sync_task_ended(&mut self, outcome: SyncTaskOutcome) {
        let run_generation = self.run_generation;
        notify_room_runtime_stopped(self.room_tx.clone(), run_generation).await;
        self.cleanup_runtime().await;
        match outcome {
            SyncTaskOutcome::Failed {
                kind,
                ever_connected,
            } => {
                trace_sync!(
                    "task_ended",
                    [
                        DiagnosticField::token("outcome", "failed"),
                        DiagnosticField::token("kind", sync_failure_kind_label(kind)),
                        DiagnosticField::boolean("ever_connected", ever_connected),
                    ],
                    "outcome=failed kind={} ever_connected={}",
                    sync_failure_kind_label(kind),
                    ever_connected
                );
                self.fail(kind).await;
            }
            SyncTaskOutcome::Panicked => self.fail(SyncFailureKind::Internal).await,
        }
    }

    async fn fail(&mut self, kind: SyncFailureKind) {
        self.lifecycle = SyncLifecycle::Failed;
        self.emit(CoreEvent::Sync(SyncEvent::Failed));
        self.project_sync_status(SyncLifecycleStatus::Failed {
            reason: sync_failure_kind_label(kind).to_owned(),
        })
        .await;
    }

    async fn handle_command(&mut self, command: SyncCommand) {
        let (kind, request_id) = sync_command_trace_parts(&command);
        trace_sync!(
            "command",
            [
                DiagnosticField::token("kind", kind),
                DiagnosticField::request_id(
                    "request_id",
                    request_id.connection_id.0,
                    request_id.sequence,
                ),
                DiagnosticField::token("lifecycle", sync_lifecycle_label(self.lifecycle)),
            ],
            "kind={} lifecycle={}",
            kind,
            sync_lifecycle_label(self.lifecycle)
        );
        match command {
            SyncCommand::Start { request_id } => self.handle_start(request_id).await,
            SyncCommand::Stop { request_id } => self.do_stop(Some(request_id)).await,
            SyncCommand::Restart { request_id } => {
                self.do_stop(None).await;
                self.handle_start(request_id).await;
            }
            #[cfg(any(test, feature = "test-hooks", feature = "qa-bin"))]
            SyncCommand::SyncOnce { request_id } => self.handle_sync_once(request_id).await,
        }
    }

    async fn handle_start(&mut self, request_id: RequestId) {
        if matches!(
            self.lifecycle,
            SyncLifecycle::Starting | SyncLifecycle::Running | SyncLifecycle::Reconnecting
        ) {
            self.emit(CoreEvent::Sync(SyncEvent::Started {
                request_id: Some(request_id),
            }));
            if self.lifecycle == SyncLifecycle::Running {
                self.project_sync_status(SyncLifecycleStatus::Running).await;
                self.emit(CoreEvent::Sync(SyncEvent::Running));
            }
            return;
        }

        self.lifecycle = SyncLifecycle::Starting;
        self.active_start_request_id = Some(request_id);
        self.project_sync_status(SyncLifecycleStatus::Starting)
            .await;
        self.emit(CoreEvent::Sync(SyncEvent::Started {
            request_id: Some(request_id),
        }));

        if self.start_sync_service().await.is_err() {
            self.diagnostics.failed(SlidingSyncFailureDiagnostic {
                origin: SlidingSyncFailureOrigin::Supervisor,
                kind: SlidingSyncFailureKind::Internal,
                stage: SlidingSyncFailureStage::Supervisor,
                ..SlidingSyncFailureDiagnostic::default()
            });
            self.fail(SyncFailureKind::Internal).await;
        }
    }

    async fn start_sync_service(&mut self) -> Result<(), ()> {
        let client = self.session.client();
        self.diagnostics
            .runtime_profile(match client.sliding_sync_version() {
                matrix_sdk::sliding_sync::Version::None => SlidingSyncSdkVersion::None,
                matrix_sdk::sliding_sync::Version::Native => SlidingSyncSdkVersion::Native,
            });
        self.register_ignored_user_list_handler(&client);
        self.run_generation = self.run_generation.wrapping_add(1).max(1);
        let run_generation = self.run_generation;

        let service = matrix_sdk_ui::sync_service::SyncService::builder(client.clone())
            .with_offline_mode()
            .with_encryption_sync_permit(self.encryption_sync_permit.clone())
            .build()
            .await
            .map_err(|_| {
                record(DiagnosticEvent::new(
                    DiagnosticLevel::Error,
                    "core.sync",
                    "service_build_failed",
                ));
                if let Some(handle) = self.ignored_user_list_handler.take() {
                    client.remove_event_handler(handle);
                }
            })?;
        let service = Arc::new(service);
        let room_list_service = service.room_list_service();
        let state_sub = service.state();
        let committed_all_rooms_response = room_list_service.committed_all_rooms_response();
        let task = executor::spawn(observe_sync_service(
            state_sub,
            committed_all_rooms_response,
            self.event_tx.clone(),
            self.action_tx.clone(),
            self.sync_generation.clone(),
            self.session.clone(),
            self.room_tx.clone(),
            self.timeline_tx.clone(),
            room_list_service,
            self.control_tx.clone(),
            run_generation,
            self.diagnostics.clone(),
        ));

        self.diagnostics.sync_started(run_generation);
        service.start().await;
        self.sync_service = Some(service);
        self.sync_task = Some(task);
        Ok(())
    }

    async fn cleanup_runtime(&mut self) {
        if let Some(service) = self.sync_service.take() {
            let _ = executor::timeout(SYNC_SERVICE_STOP_TIMEOUT, service.stop()).await;
        }
        if let Some(handle) = self.ignored_user_list_handler.take() {
            self.session.client().remove_event_handler(handle);
        }
        self.active_start_request_id = None;
    }

    async fn do_stop(&mut self, request_id: Option<RequestId>) {
        let run_generation = self.run_generation;
        if let Some(service) = self.sync_service.take() {
            let _ = executor::timeout(SYNC_SERVICE_STOP_TIMEOUT, service.stop()).await;
        }
        if let Some(handle) = self.ignored_user_list_handler.take() {
            self.session.client().remove_event_handler(handle);
        }
        if let Some(mut task) = self.sync_task.take() {
            if executor::timeout(SYNC_ACTOR_SHUTDOWN_JOIN_TIMEOUT, &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        stop_room_observation(self.room_tx.clone(), run_generation).await;
        self.active_start_request_id = None;
        self.lifecycle = SyncLifecycle::Stopped;
        self.diagnostics.stopped();
        self.emit(CoreEvent::Sync(SyncEvent::Stopped { request_id }));
        self.project_sync_status(SyncLifecycleStatus::Stopped).await;
    }

    fn register_ignored_user_list_handler(&mut self, client: &matrix_sdk::Client) {
        use matrix_sdk::ruma::events::{
            GlobalAccountDataEvent, ignored_user_list::IgnoredUserListEventContent,
        };

        let action_tx = self.action_tx.clone();
        let timeline_tx = self.timeline_tx.clone();
        let handle = client.add_event_handler(
            move |ev: GlobalAccountDataEvent<IgnoredUserListEventContent>| {
                let action_tx = action_tx.clone();
                let timeline_tx = timeline_tx.clone();
                async move {
                    let user_ids: BTreeSet<String> = ev
                        .content
                        .ignored_users
                        .keys()
                        .map(ToString::to_string)
                        .collect();
                    let _ = action_tx.try_send(vec![AppAction::IgnoredUsersLoaded {
                        user_ids: user_ids.clone(),
                    }]);
                    let _ = timeline_tx.try_send(
                        crate::timeline::TimelineMessage::IgnoredUsersUpdated { user_ids },
                    );
                }
            },
        );
        self.ignored_user_list_handler = Some(handle);
    }

    #[cfg(any(test, feature = "test-hooks", feature = "qa-bin"))]
    async fn handle_sync_once(&self, request_id: RequestId) {
        if !sync_once_admitted(
            self.lifecycle,
            self.sync_task.is_some(),
            self.sync_service.is_some(),
        ) {
            self.emit(CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::SyncFailed {
                    kind: SyncFailureKind::Internal,
                },
            });
            return;
        }
        match koushi_sdk::sync_once(&self.session).await {
            Ok(()) => self.emit(CoreEvent::Sync(SyncEvent::Stopped {
                request_id: Some(request_id),
            })),
            Err(_) => self.emit(CoreEvent::OperationFailed {
                request_id,
                failure: CoreFailure::SyncFailed {
                    kind: SyncFailureKind::Http,
                },
            }),
        }
    }

    fn emit(&self, event: CoreEvent) {
        let _ = self.event_tx.send(event);
    }

    async fn project_sync_status(&self, status: SyncLifecycleStatus) {
        send_sync_status(&self.action_tx, &self.sync_generation, status).await;
    }
}

async fn start_room_observation(
    session: Arc<MatrixClientSession>,
    room_tx: mpsc::Sender<RoomMessage>,
    room_list_service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
    run_generation: u64,
) -> bool {
    room_tx
        .send(RoomMessage::SyncStarted {
            session,
            room_list_service,
            source: RoomListSource::Live,
            backend_generation: run_generation,
        })
        .await
        .is_ok()
}

async fn start_timeline_observation(
    timeline_tx: &mpsc::Sender<crate::timeline::TimelineMessage>,
    room_list_service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
    core_generation: u64,
) {
    let _ = timeline_tx
        .send(crate::timeline::TimelineMessage::SyncStarted {
            room_list_service,
            core_generation,
        })
        .await;
}

async fn forward_latest_timeline_response_commit(
    timeline_tx: &mpsc::Sender<crate::timeline::TimelineMessage>,
    core_generation: u64,
    response_sequence: u64,
) -> bool {
    timeline_tx
        .send(
            crate::timeline::TimelineMessage::AllRoomsResponseCommitted {
                core_generation,
                response_sequence,
            },
        )
        .await
        .is_ok()
}

async fn reconcile_committed_room_list(
    room_tx: &mpsc::Sender<RoomMessage>,
    run_generation: u64,
    response_sequence: u64,
) -> RoomListReconcileResult {
    let (ack_tx, ack_rx) = oneshot::channel();
    if room_tx
        .send(RoomMessage::ReconcileCommittedRange {
            source: RoomListSource::Live,
            backend_generation: run_generation,
            response_sequence,
            ack: ack_tx,
        })
        .await
        .is_err()
    {
        return RoomListReconcileResult::Failed;
    }
    match executor::timeout(ROOM_OBSERVATION_ACK_TIMEOUT, ack_rx).await {
        Ok(Ok(ack)) => classify_room_list_reconcile_ack(run_generation, response_sequence, ack),
        _ => RoomListReconcileResult::Failed,
    }
}

fn classify_room_list_reconcile_ack(
    run_generation: u64,
    requested_sequence: u64,
    ack: RoomListReconcileAck,
) -> RoomListReconcileResult {
    match ack {
        RoomListReconcileAck::Projected {
            backend_generation,
            room_generation,
            response_sequence: acknowledged_sequence,
        } if backend_generation == run_generation
            && room_generation > 0
            && acknowledged_sequence >= requested_sequence =>
        {
            RoomListReconcileResult::Projected {
                response_sequence: acknowledged_sequence,
            }
        }
        RoomListReconcileAck::Reconciled {
            backend_generation,
            room_generation,
            response_sequence: acknowledged_sequence,
        } if backend_generation == run_generation
            && room_generation > 0
            && acknowledged_sequence >= requested_sequence =>
        {
            RoomListReconcileResult::Reconciled {
                response_sequence: acknowledged_sequence,
            }
        }
        RoomListReconcileAck::Superseded {
            backend_generation,
            room_generation,
            response_sequence: acknowledged_sequence,
        } if backend_generation == run_generation
            && room_generation > 0
            && acknowledged_sequence > requested_sequence =>
        {
            RoomListReconcileResult::Superseded {
                response_sequence: acknowledged_sequence,
            }
        }
        _ => RoomListReconcileResult::Failed,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RoomListReconcileResult {
    Projected { response_sequence: u64 },
    Reconciled { response_sequence: u64 },
    Superseded { response_sequence: u64 },
    Failed,
}

async fn stop_room_observation(room_tx: mpsc::Sender<RoomMessage>, run_generation: u64) {
    let (ack_tx, ack_rx) = oneshot::channel();
    if room_tx
        .send(RoomMessage::StopSyncObservation {
            backend_generation: run_generation,
            ack: ack_tx,
        })
        .await
        .is_ok()
    {
        let _ = executor::timeout(ROOM_OBSERVATION_ACK_TIMEOUT, ack_rx).await;
    }
}

async fn notify_room_runtime_stopped(room_tx: mpsc::Sender<RoomMessage>, run_generation: u64) {
    let _ = room_tx
        .send(RoomMessage::BackendSyncStopped {
            source: RoomListSource::Live,
            backend_generation: run_generation,
        })
        .await;
}

async fn observe_sync_service(
    mut state_sub: eyeball::Subscriber<matrix_sdk_ui::sync_service::State>,
    mut committed_all_rooms_response: eyeball::Subscriber<
        matrix_sdk_ui::room_list_service::CommittedAllRoomsResponse,
    >,
    event_tx: broadcast::Sender<CoreEvent>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    sync_generation: Arc<AtomicU64>,
    session: Arc<MatrixClientSession>,
    room_tx: mpsc::Sender<RoomMessage>,
    timeline_tx: mpsc::Sender<crate::timeline::TimelineMessage>,
    room_list_service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
    control_tx: mpsc::Sender<SyncActorControl>,
    run_generation: u64,
    diagnostics: SlidingSyncDiagnostics,
) -> SyncTaskOutcome {
    let mut connected = false;
    let mut room_observation_started = false;
    let mut reconnecting = false;
    let mut last_committed_sequence = 0;
    let mut pending_state = Some(state_sub.get());
    let mut pending_commit = Some(committed_all_rooms_response.get());

    loop {
        enum Signal {
            State(matrix_sdk_ui::sync_service::State),
            Committed(matrix_sdk_ui::room_list_service::CommittedAllRoomsResponse),
        }

        let signal = if let Some(state) = pending_state.take() {
            Signal::State(state)
        } else if let Some(committed) = pending_commit.take() {
            Signal::Committed(committed)
        } else {
            tokio::select! {
                state = state_sub.next() => match state {
                    Some(state) => Signal::State(state),
                    None => return internal_observer_failure_at("state_subscription_closed", connected),
                },
                committed = committed_all_rooms_response.next() => match committed {
                    Some(committed) => Signal::Committed(committed),
                    None => return internal_observer_failure_at("response_subscription_closed", connected),
                },
            }
        };

        match signal {
            Signal::Committed(committed)
                if committed_response_is_handoff_evidence(
                    committed.pos_present(),
                    committed.sequence(),
                    last_committed_sequence,
                ) =>
            {
                last_committed_sequence = committed.sequence();
                trace_sync!(
                    "committed_response",
                    [
                        DiagnosticField::count("sequence", committed.sequence()),
                        DiagnosticField::boolean(
                            "range_fully_loaded",
                            committed.range_fully_loaded()
                        ),
                        DiagnosticField::count(
                            "rooms_from_response",
                            committed.rooms_from_response_count() as u64,
                        ),
                        DiagnosticField::boolean("pos_present", committed.pos_present()),
                    ],
                    "sequence={} range_fully_loaded={} rooms_from_response={} pos_present={}",
                    committed.sequence(),
                    committed.range_fully_loaded(),
                    committed.rooms_from_response_count(),
                    committed.pos_present()
                );
                if room_observation_started && committed.rooms_from_response_count() > 0 {
                    let _ = room_tx
                        .send(RoomMessage::RefreshCommittedProjection {
                            source: RoomListSource::Live,
                            backend_generation: run_generation,
                        })
                        .await;
                }
                if !room_observation_started {
                    if !start_room_observation(
                        session.clone(),
                        room_tx.clone(),
                        room_list_service.clone(),
                        run_generation,
                    )
                    .await
                    {
                        return internal_observer_failure_at(
                            "room_observation_start_failed",
                            connected,
                        );
                    }
                    start_timeline_observation(
                        &timeline_tx,
                        room_list_service.clone(),
                        run_generation,
                    )
                    .await;
                    room_observation_started = true;
                }
                if !forward_latest_timeline_response_commit(
                    &timeline_tx,
                    run_generation,
                    committed.sequence(),
                )
                .await
                {
                    trace_sync!(
                        "timeline_commit_forward_unavailable",
                        [
                            DiagnosticField::count("sequence", committed.sequence()),
                            DiagnosticField::boolean("connected", connected),
                        ],
                        "sequence={} connected={}",
                        committed.sequence(),
                        connected
                    );
                }
                if !connected {
                    match reconcile_committed_room_list(
                        &room_tx,
                        run_generation,
                        committed.sequence(),
                    )
                    .await
                    {
                        RoomListReconcileResult::Projected { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                        }
                        RoomListReconcileResult::Reconciled { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                        }
                        RoomListReconcileResult::Superseded { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                            pending_commit = Some(committed_all_rooms_response.get());
                            continue;
                        }
                        RoomListReconcileResult::Failed => {
                            return internal_observer_failure_at(
                                "initial_room_reconcile_failed",
                                connected,
                            );
                        }
                    }
                    connected = true;
                    reconnecting = false;
                    let _ = control_tx
                        .send(SyncActorControl::FirstResponseCommitted { run_generation })
                        .await;
                    let _ = event_tx.send(CoreEvent::Sync(SyncEvent::Running));
                    send_sync_status(&action_tx, &sync_generation, SyncLifecycleStatus::Running)
                        .await;
                } else if reconnecting {
                    match reconcile_committed_room_list(
                        &room_tx,
                        run_generation,
                        committed.sequence(),
                    )
                    .await
                    {
                        RoomListReconcileResult::Projected { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                        }
                        RoomListReconcileResult::Reconciled { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                        }
                        RoomListReconcileResult::Superseded { response_sequence } => {
                            last_committed_sequence =
                                last_committed_sequence.max(response_sequence);
                            pending_commit = Some(committed_all_rooms_response.get());
                            continue;
                        }
                        RoomListReconcileResult::Failed => {
                            return internal_observer_failure_at(
                                "reconnect_room_reconcile_failed",
                                connected,
                            );
                        }
                    }
                    reconnecting = false;
                    let _ = control_tx
                        .send(SyncActorControl::Recovered { run_generation })
                        .await;
                }
                diagnostics.response_committed(run_generation, committed.pos_present());
            }
            Signal::Committed(_) => {}
            Signal::State(state) => {
                let state_label = sync_service_state_trace_label(&state);
                trace_sync!(
                    "sync_service_state",
                    [
                        DiagnosticField::token("state", state_label),
                        DiagnosticField::boolean("connected", connected),
                        DiagnosticField::boolean("reconnecting", reconnecting),
                    ],
                    "state={} connected={} reconnecting={}",
                    state_label,
                    connected,
                    reconnecting
                );
                match state {
                    matrix_sdk_ui::sync_service::State::Offline(error) if !reconnecting => {
                        diagnostics.sync_offline(classify_sync_service_error(&error));
                        reconnecting = true;
                        let _ = control_tx
                            .send(SyncActorControl::Reconnecting {
                                run_generation,
                                reason: "network_offline",
                            })
                            .await;
                    }
                    matrix_sdk_ui::sync_service::State::Error(error) if !reconnecting => {
                        diagnostics.failed(classify_sync_service_error(&error));
                        reconnecting = true;
                        let _ = control_tx
                            .send(SyncActorControl::Reconnecting {
                                run_generation,
                                reason: "network_error",
                            })
                            .await;
                    }
                    matrix_sdk_ui::sync_service::State::Terminated => {
                        diagnostics.failed(SlidingSyncFailureDiagnostic {
                            origin: SlidingSyncFailureOrigin::Supervisor,
                            kind: SlidingSyncFailureKind::Internal,
                            stage: SlidingSyncFailureStage::Supervisor,
                            ..SlidingSyncFailureDiagnostic::default()
                        });
                        return SyncTaskOutcome::Failed {
                            kind: SyncFailureKind::Http,
                            ever_connected: connected,
                        };
                    }
                    _ => {}
                }
            }
        }
    }
}

fn classify_http_status(code: Option<u16>) -> SlidingSyncHttpStatus {
    match code {
        Some(400) => SlidingSyncHttpStatus::BadRequest,
        Some(401) => SlidingSyncHttpStatus::Unauthorized,
        Some(403) => SlidingSyncHttpStatus::Forbidden,
        Some(404) => SlidingSyncHttpStatus::NotFound,
        Some(429) => SlidingSyncHttpStatus::RateLimited,
        Some(400..=499) => SlidingSyncHttpStatus::ClientError,
        Some(500..=599) => SlidingSyncHttpStatus::ServerError,
        Some(_) => SlidingSyncHttpStatus::Other,
        None => SlidingSyncHttpStatus::None,
    }
}

fn classify_matrix_error_kind(
    kind: Option<&matrix_sdk::ruma::api::error::ErrorKind>,
) -> SlidingSyncMatrixErrorKind {
    use matrix_sdk::ruma::api::error::ErrorKind;

    match kind {
        Some(ErrorKind::MissingToken) => SlidingSyncMatrixErrorKind::MissingToken,
        Some(ErrorKind::Unknown) => SlidingSyncMatrixErrorKind::Unknown,
        Some(ErrorKind::BadJson) => SlidingSyncMatrixErrorKind::BadJson,
        Some(ErrorKind::InvalidParam) => SlidingSyncMatrixErrorKind::InvalidParam,
        Some(ErrorKind::MissingParam) => SlidingSyncMatrixErrorKind::MissingParam,
        Some(ErrorKind::NotJson) => SlidingSyncMatrixErrorKind::NotJson,
        Some(ErrorKind::NotFound) => SlidingSyncMatrixErrorKind::NotFound,
        Some(ErrorKind::Unauthorized) => SlidingSyncMatrixErrorKind::Unauthorized,
        Some(ErrorKind::UnknownToken { .. }) => SlidingSyncMatrixErrorKind::UnknownToken,
        Some(ErrorKind::Forbidden) => SlidingSyncMatrixErrorKind::Forbidden,
        Some(ErrorKind::UnknownPos) => SlidingSyncMatrixErrorKind::UnknownPos,
        Some(ErrorKind::Unrecognized) => SlidingSyncMatrixErrorKind::Unrecognized,
        Some(ErrorKind::LimitExceeded(_)) => SlidingSyncMatrixErrorKind::LimitExceeded,
        Some(_) => SlidingSyncMatrixErrorKind::Other,
        None => SlidingSyncMatrixErrorKind::None,
    }
}

fn retryability_for_http(
    status: SlidingSyncHttpStatus,
    matrix_kind: SlidingSyncMatrixErrorKind,
    default: SlidingSyncFailureRetryability,
) -> SlidingSyncFailureRetryability {
    match matrix_kind {
        SlidingSyncMatrixErrorKind::UnknownPos | SlidingSyncMatrixErrorKind::LimitExceeded => {
            SlidingSyncFailureRetryability::Transient
        }
        SlidingSyncMatrixErrorKind::MissingToken
        | SlidingSyncMatrixErrorKind::UnknownToken
        | SlidingSyncMatrixErrorKind::Forbidden
        | SlidingSyncMatrixErrorKind::Unrecognized => SlidingSyncFailureRetryability::Permanent,
        _ => match status {
            SlidingSyncHttpStatus::RateLimited | SlidingSyncHttpStatus::ServerError => {
                SlidingSyncFailureRetryability::Transient
            }
            SlidingSyncHttpStatus::Unauthorized
            | SlidingSyncHttpStatus::Forbidden
            | SlidingSyncHttpStatus::NotFound
            | SlidingSyncHttpStatus::BadRequest
            | SlidingSyncHttpStatus::ClientError => SlidingSyncFailureRetryability::Permanent,
            _ => default,
        },
    }
}

fn classify_http_error(
    http_error: &matrix_sdk::HttpError,
) -> (
    SlidingSyncHttpErrorSource,
    SlidingSyncHttpStatus,
    SlidingSyncMatrixErrorKind,
    SlidingSyncFailureRetryability,
) {
    use matrix_sdk::ruma::api::error::FromHttpResponseError;

    match http_error {
        matrix_sdk::HttpError::Reqwest(error) => {
            let status = classify_http_status(error.status().map(|status| status.as_u16()));
            let retryability = retryability_for_http(
                status,
                SlidingSyncMatrixErrorKind::None,
                SlidingSyncFailureRetryability::Transient,
            );
            (
                SlidingSyncHttpErrorSource::Transport,
                status,
                SlidingSyncMatrixErrorKind::None,
                retryability,
            )
        }
        matrix_sdk::HttpError::Api(error) => match error.as_ref() {
            FromHttpResponseError::Deserialization(_) => (
                SlidingSyncHttpErrorSource::ResponseDecode,
                SlidingSyncHttpStatus::None,
                SlidingSyncMatrixErrorKind::None,
                SlidingSyncFailureRetryability::Permanent,
            ),
            FromHttpResponseError::Server(_) => {
                let status = classify_http_status(
                    http_error
                        .as_client_api_error()
                        .map(|error| error.status_code.as_u16()),
                );
                let matrix_kind = classify_matrix_error_kind(http_error.client_api_error_kind());
                let retryability = retryability_for_http(
                    status,
                    matrix_kind,
                    SlidingSyncFailureRetryability::Unknown,
                );
                (
                    SlidingSyncHttpErrorSource::ServerResponse,
                    status,
                    matrix_kind,
                    retryability,
                )
            }
            _ => (
                SlidingSyncHttpErrorSource::NotHttp,
                SlidingSyncHttpStatus::None,
                SlidingSyncMatrixErrorKind::None,
                SlidingSyncFailureRetryability::Unknown,
            ),
        },
        matrix_sdk::HttpError::IntoHttp(_) => (
            SlidingSyncHttpErrorSource::RequestBuild,
            SlidingSyncHttpStatus::None,
            SlidingSyncMatrixErrorKind::None,
            SlidingSyncFailureRetryability::Permanent,
        ),
        matrix_sdk::HttpError::RefreshToken(_) => (
            SlidingSyncHttpErrorSource::TokenRefresh,
            SlidingSyncHttpStatus::None,
            SlidingSyncMatrixErrorKind::None,
            SlidingSyncFailureRetryability::Unknown,
        ),
        matrix_sdk::HttpError::Cached(inner) => {
            let (_, status, matrix_kind, retryability) = classify_http_error(inner);
            (
                SlidingSyncHttpErrorSource::Cached,
                status,
                matrix_kind,
                retryability,
            )
        }
        #[cfg(target_os = "android")]
        matrix_sdk::HttpError::VerifierBuilder(_) => (
            SlidingSyncHttpErrorSource::Tls,
            SlidingSyncHttpStatus::None,
            SlidingSyncMatrixErrorKind::None,
            SlidingSyncFailureRetryability::Permanent,
        ),
    }
}

fn classify_matrix_sync_error(
    error: &matrix_sdk::Error,
    origin: SlidingSyncFailureOrigin,
    stage: SlidingSyncFailureStage,
) -> SlidingSyncFailureDiagnostic {
    let mut diagnostic = SlidingSyncFailureDiagnostic {
        origin,
        stage,
        ..SlidingSyncFailureDiagnostic::default()
    };
    match error {
        matrix_sdk::Error::AuthenticationRequired => {
            diagnostic.kind = SlidingSyncFailureKind::Auth;
            diagnostic.retryability = SlidingSyncFailureRetryability::Permanent;
        }
        matrix_sdk::Error::Http(error) => {
            let (source, status, matrix_kind, retryability) = classify_http_error(error);
            diagnostic.kind = if matches!(
                matrix_kind,
                SlidingSyncMatrixErrorKind::MissingToken
                    | SlidingSyncMatrixErrorKind::UnknownToken
                    | SlidingSyncMatrixErrorKind::Forbidden
            ) || matches!(
                status,
                SlidingSyncHttpStatus::Unauthorized | SlidingSyncHttpStatus::Forbidden
            ) {
                SlidingSyncFailureKind::Auth
            } else {
                SlidingSyncFailureKind::Http
            };
            diagnostic.http_error_source = source;
            diagnostic.http_status = status;
            diagnostic.matrix_error_kind = matrix_kind;
            diagnostic.retryability = retryability;
        }
        matrix_sdk::Error::Timeout => {
            diagnostic.kind = SlidingSyncFailureKind::Http;
            diagnostic.http_error_source = SlidingSyncHttpErrorSource::Transport;
            diagnostic.retryability = SlidingSyncFailureRetryability::Transient;
        }
        matrix_sdk::Error::StateStore(_)
        | matrix_sdk::Error::EventCacheStore(_)
        | matrix_sdk::Error::MediaStore(_)
        | matrix_sdk::Error::BadCryptoStoreState
        | matrix_sdk::Error::CryptoStoreError(_) => {
            diagnostic.kind = SlidingSyncFailureKind::Store;
            diagnostic.retryability = SlidingSyncFailureRetryability::Unknown;
        }
        matrix_sdk::Error::SerdeJson(_)
        | matrix_sdk::Error::Identifier(_)
        | matrix_sdk::Error::Url(_)
        | matrix_sdk::Error::SlidingSync(_) => {
            diagnostic.kind = SlidingSyncFailureKind::Protocol;
            diagnostic.retryability = SlidingSyncFailureRetryability::Permanent;
        }
        _ => {
            diagnostic.kind = SlidingSyncFailureKind::Internal;
            diagnostic.retryability = SlidingSyncFailureRetryability::Unknown;
        }
    }
    diagnostic
}

fn classify_sync_service_error(
    error: &matrix_sdk_ui::sync_service::Error,
) -> SlidingSyncFailureDiagnostic {
    use matrix_sdk_ui::{encryption_sync_service, room_list_service, sync_service};

    match error {
        sync_service::Error::RoomList(room_list_service::Error::SlidingSync(error)) => {
            classify_matrix_sync_error(
                error,
                SlidingSyncFailureOrigin::RoomList,
                SlidingSyncFailureStage::RoomListSlidingSync,
            )
        }
        sync_service::Error::RoomList(room_list_service::Error::EventCache(_)) => {
            SlidingSyncFailureDiagnostic {
                origin: SlidingSyncFailureOrigin::RoomList,
                kind: SlidingSyncFailureKind::Store,
                stage: SlidingSyncFailureStage::RoomListEventCache,
                retryability: SlidingSyncFailureRetryability::Unknown,
                ..SlidingSyncFailureDiagnostic::default()
            }
        }
        sync_service::Error::RoomList(
            room_list_service::Error::UnknownList(_) | room_list_service::Error::RoomNotFound(_),
        ) => SlidingSyncFailureDiagnostic {
            origin: SlidingSyncFailureOrigin::RoomList,
            kind: SlidingSyncFailureKind::Internal,
            stage: SlidingSyncFailureStage::RoomListProjection,
            retryability: SlidingSyncFailureRetryability::Permanent,
            ..SlidingSyncFailureDiagnostic::default()
        },
        sync_service::Error::EncryptionSync(encryption_sync_service::Error::SlidingSync(error)) => {
            classify_matrix_sync_error(
                error,
                SlidingSyncFailureOrigin::Encryption,
                SlidingSyncFailureStage::EncryptionSlidingSync,
            )
        }
        sync_service::Error::EncryptionSync(encryption_sync_service::Error::ClientError(error)) => {
            classify_matrix_sync_error(
                error,
                SlidingSyncFailureOrigin::Encryption,
                SlidingSyncFailureStage::EncryptionClient,
            )
        }
        sync_service::Error::EncryptionSync(encryption_sync_service::Error::LockError(error)) => {
            classify_matrix_sync_error(
                error,
                SlidingSyncFailureOrigin::Encryption,
                SlidingSyncFailureStage::EncryptionLock,
            )
        }
        sync_service::Error::Supervisor => SlidingSyncFailureDiagnostic {
            origin: SlidingSyncFailureOrigin::Supervisor,
            kind: SlidingSyncFailureKind::Internal,
            stage: SlidingSyncFailureStage::Supervisor,
            retryability: SlidingSyncFailureRetryability::Permanent,
            ..SlidingSyncFailureDiagnostic::default()
        },
    }
}

pub(crate) fn sync_failure_kind_label(kind: SyncFailureKind) -> &'static str {
    match kind {
        SyncFailureKind::Http => "sync_failed_http",
        SyncFailureKind::Auth => "sync_failed_auth",
        SyncFailureKind::Store => "sync_failed_store",
        SyncFailureKind::Internal => "sync_failed_internal",
    }
}

#[cfg(test)]
pub mod tests {
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn sync_service_has_one_all_rooms_owner() {
        let sync_source = include_str!("sync.rs");
        let production = sync_source
            .split("#[cfg(test)]\npub mod tests")
            .next()
            .expect("production sync source");

        assert_eq!(production.matches("SyncService::builder").count(), 1);
        assert!(production.contains("committed_all_rooms_response"));
        assert!(!production.contains("KOUSHI_QA_FORCE_SYNC_BACKEND"));
        assert!(!production.contains("probe_backend"));
        assert!(!production.contains("run_legacy_sync_loop"));
        assert!(production.contains("room_list_service: Arc<"));
        assert!(production.contains("room_list_service,"));
    }

    #[test]
    fn running_state_is_not_the_committed_response_handoff() {
        let source = include_str!("sync.rs");
        let observer = source
            .split("async fn observe_sync_service")
            .nth(1)
            .expect("observer body")
            .split("\n#[cfg(test)]")
            .next()
            .expect("observer production body");
        let committed = observer
            .find("Signal::Committed(committed)")
            .expect("committed response branch");
        let handoff = observer
            .find("reconcile_committed_room_list")
            .expect("RoomActor reconciliation handoff");
        assert!(handoff > committed);
        assert!(!observer.contains("if !committed.range_fully_loaded()"));
    }

    #[test]
    fn latest_observed_commit_is_forwarded_to_timeline_before_range_readiness() {
        let source = include_str!("sync.rs");
        let observer = source
            .split("async fn observe_sync_service")
            .nth(1)
            .expect("observer body")
            .split("\n#[cfg(test)]")
            .next()
            .expect("observer production body");
        let committed = observer
            .find("Signal::Committed(committed)")
            .expect("committed response branch");
        let forwarding = observer
            .find("forward_latest_timeline_response_commit(")
            .expect("global timeline commit handoff");

        assert!(forwarding > committed);
        let handoff = &observer[forwarding..];
        assert!(handoff.contains("run_generation"));
        assert!(handoff.contains("committed.sequence()"));
        assert!(!handoff.contains("backend"));
    }

    #[test]
    fn any_new_positioned_commit_is_startup_handoff_evidence() {
        assert!(committed_response_is_handoff_evidence(true, 1, 0));
        assert!(committed_response_is_handoff_evidence(true, 2, 1));
        assert!(!committed_response_is_handoff_evidence(false, 1, 0));
        assert!(!committed_response_is_handoff_evidence(true, 1, 1));
    }

    #[test]
    fn controls_are_generation_fenced() {
        assert!(accepts_control(
            SyncLifecycle::Starting,
            7,
            7,
            &[SyncLifecycle::Starting, SyncLifecycle::Reconnecting],
        ));
        assert!(!accepts_control(
            SyncLifecycle::Starting,
            8,
            7,
            &[SyncLifecycle::Starting],
        ));
        assert!(!accepts_control(
            SyncLifecycle::Running,
            7,
            7,
            &[SyncLifecycle::Starting],
        ));
    }

    #[test]
    fn failure_kind_labels_are_private_safe_tokens() {
        assert_eq!(
            sync_failure_kind_label(SyncFailureKind::Http),
            "sync_failed_http"
        );
        assert_eq!(
            sync_failure_kind_label(SyncFailureKind::Auth),
            "sync_failed_auth"
        );
        assert_eq!(
            sync_failure_kind_label(SyncFailureKind::Store),
            "sync_failed_store"
        );
        assert_eq!(
            sync_failure_kind_label(SyncFailureKind::Internal),
            "sync_failed_internal"
        );
    }

    #[test]
    fn bad_request_and_schema_errcodes_are_actionable_diagnostics() {
        use matrix_sdk::ruma::api::error::ErrorKind;

        assert_eq!(
            classify_http_status(Some(400)),
            SlidingSyncHttpStatus::BadRequest
        );
        for (kind, expected) in [
            (ErrorKind::Unknown, SlidingSyncMatrixErrorKind::Unknown),
            (ErrorKind::BadJson, SlidingSyncMatrixErrorKind::BadJson),
            (
                ErrorKind::InvalidParam,
                SlidingSyncMatrixErrorKind::InvalidParam,
            ),
            (
                ErrorKind::MissingParam,
                SlidingSyncMatrixErrorKind::MissingParam,
            ),
            (ErrorKind::NotJson, SlidingSyncMatrixErrorKind::NotJson),
            (ErrorKind::NotFound, SlidingSyncMatrixErrorKind::NotFound),
            (
                ErrorKind::Unauthorized,
                SlidingSyncMatrixErrorKind::Unauthorized,
            ),
        ] {
            assert_eq!(classify_matrix_error_kind(Some(&kind)), expected);
        }
    }

    #[test]
    fn observer_infrastructure_loss_is_not_a_normal_stop() {
        assert!(matches!(
            internal_observer_failure(true),
            SyncTaskOutcome::Failed {
                kind: SyncFailureKind::Internal,
                ever_connected: true,
            }
        ));
    }

    #[test]
    fn newer_room_snapshot_supersedes_without_becoming_an_internal_failure() {
        assert_eq!(
            classify_room_list_reconcile_ack(
                7,
                11,
                RoomListReconcileAck::Superseded {
                    backend_generation: 7,
                    room_generation: 3,
                    response_sequence: 12,
                },
            ),
            RoomListReconcileResult::Superseded {
                response_sequence: 12,
            }
        );
        assert_eq!(
            classify_room_list_reconcile_ack(
                7,
                11,
                RoomListReconcileAck::Superseded {
                    backend_generation: 8,
                    room_generation: 3,
                    response_sequence: 12,
                },
            ),
            RoomListReconcileResult::Failed
        );
    }

    #[test]
    fn projected_room_list_ack_is_connectivity_evidence() {
        assert_eq!(
            classify_room_list_reconcile_ack(
                7,
                11,
                RoomListReconcileAck::Projected {
                    backend_generation: 7,
                    room_generation: 3,
                    response_sequence: 11,
                },
            ),
            RoomListReconcileResult::Projected {
                response_sequence: 11,
            }
        );
        assert_eq!(
            classify_room_list_reconcile_ack(
                7,
                11,
                RoomListReconcileAck::Projected {
                    backend_generation: 8,
                    room_generation: 3,
                    response_sequence: 11,
                },
            ),
            RoomListReconcileResult::Failed
        );
    }

    #[tokio::test]
    async fn action_channel_accepts_projected_sync_statuses_with_generations() {
        let (tx, mut rx) = mpsc::channel(4);
        let generation = AtomicU64::new(0);
        send_sync_status(&tx, &generation, SyncLifecycleStatus::Starting).await;
        send_sync_status(&tx, &generation, SyncLifecycleStatus::Running).await;
        assert!(matches!(
            rx.recv().await,
            Some(actions) if matches!(
                actions.as_slice(),
                [AppAction::SyncStatusChanged { generation: 1, status: SyncLifecycleStatus::Starting }]
            )
        ));
        assert!(matches!(
            rx.recv().await,
            Some(actions) if matches!(
                actions.as_slice(),
                [AppAction::SyncStatusChanged { generation: 2, status: SyncLifecycleStatus::Running }]
            )
        ));
    }

    #[test]
    fn sync_once_requires_no_continuous_owner() {
        assert!(sync_once_admitted(SyncLifecycle::Stopped, false, false));
        assert!(!sync_once_admitted(SyncLifecycle::Running, true, true));
        assert!(!sync_once_admitted(SyncLifecycle::Failed, false, true));
    }
}
