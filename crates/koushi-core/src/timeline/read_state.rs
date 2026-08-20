//! Exact AST extraction draft from immutable timeline baseline.

const READ_NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

const READ_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

const READ_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct ReadPersistenceIngress {
    tx: watch::Sender<Option<ReadPersistenceRequest>>,
}

#[derive(Clone)]
pub(crate) struct ReadPersistenceRequest {
    session_generation: u64,
    save_generation: u64,
    snapshot: ReadPersistenceSnapshot,
}

impl ReadPersistenceRequest {
    pub(crate) fn new(
        session_generation: u64,
        save_generation: u64,
        snapshot: ReadPersistenceSnapshot,
    ) -> Self {
        Self {
            session_generation,
            save_generation,
            snapshot,
        }
    }
}

impl ReadPersistenceRequest {
    pub(crate) fn session_generation(&self) -> u64 {
        self.session_generation
    }
}

impl ReadPersistenceRequest {
    pub(crate) fn save_generation(&self) -> u64 {
        self.save_generation
    }
}

impl ReadPersistenceRequest {
    pub(crate) fn snapshot(&self) -> &ReadPersistenceSnapshot {
        &self.snapshot
    }
}

impl std::fmt::Debug for ReadPersistenceRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadPersistenceRequest")
            .field("session_generation", &self.session_generation)
            .field("save_generation", &self.save_generation)
            .field("entry_count", &self.snapshot.entry_count())
            .field("candidate_count", &self.snapshot.candidate_count())
            .finish()
    }
}

impl ReadPersistenceIngress {
    pub(crate) fn channel() -> (Self, watch::Receiver<Option<ReadPersistenceRequest>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }
}

impl ReadPersistenceIngress {
    pub(crate) fn publish(&self, request: ReadPersistenceRequest) {
        self.tx.send_replace(Some(request));
    }
}

#[derive(Clone)]
enum ReadNetworkContext {
    Matrix(Arc<MatrixClientSession>),
    #[cfg(test)]
    Synthetic {
        requests: mpsc::UnboundedSender<SyntheticReadNetworkRequest>,
    },
}

#[cfg(test)]
struct SyntheticReadNetworkRequest {
    operation: ReadOperation,
    response: oneshot::Sender<Result<(), ()>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadCommandKind {
    Receipt,
    FullyRead,
}

#[derive(Clone, Copy)]
enum ReadRetrySource {
    Backoff,
    Reconnect,
    Checkpoint,
    AuthoritativeReceipt,
    SyncReconciliation,
}

impl ReadRetrySource {
    fn token(self) -> &'static str {
        match self {
            Self::Backoff => "backoff",
            Self::Reconnect => "reconnect",
            Self::Checkpoint => "checkpoint",
            Self::AuthoritativeReceipt => "authoritative_receipt",
            Self::SyncReconciliation => "sync_reconciliation",
        }
    }
}

struct ReadCommandWaiter {
    request_id: RequestId,
    key: TimelineKey,
    event_id: String,
    kind: ReadCommandKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadActorApplyKind {
    ThreadReceipt,
    FullyRead,
}

#[derive(Clone)]
struct ReadRetryToken {
    epoch: Arc<()>,
    serial: u64,
}

impl PartialEq for ReadRetryToken {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial && Arc::ptr_eq(&self.epoch, &other.epoch)
    }
}

impl Eq for ReadRetryToken {}

enum ReadWorkerCompletion {
    Network {
        operation: ReadOperation,
        outcome: ReadNetworkOutcome,
    },
    ActorApplied {
        operation: ReadOperation,
        applied: bool,
    },
    Cancelled {
        operation: ReadOperation,
    },
    RetryWake {
        key: ReadStateKey,
        generation: ReadRetryToken,
        cancelled: bool,
    },
}

impl ReadWorkerCompletion {
    fn fence(&self) -> Option<ReadOperationFence> {
        match self {
            Self::Network { operation, .. }
            | Self::ActorApplied { operation, .. }
            | Self::Cancelled { operation } => Some(operation.fence()),
            Self::RetryWake { .. } => None,
        }
    }
}

type ReadWorkerFuture = Pin<Box<dyn Future<Output = ReadWorkerCompletion> + Send + 'static>>;

struct ReadWorkerSupervisor {
    state: ReadStateEngine,
    network: Option<ReadNetworkContext>,
    network_timeout: Duration,
    tasks: FuturesUnordered<ReadWorkerFuture>,
    retry_tasks: FuturesUnordered<ReadWorkerFuture>,
    cancellations: HashMap<ReadOperationFence, oneshot::Sender<()>>,
    waiters: HashMap<ReadWaiterId, ReadCommandWaiter>,
    next_waiter_id: u64,
    retry_base_delay: Duration,
    retry_max_delay: Duration,
    retry_attempts: HashMap<ReadStateKey, u32>,
    /// Manager-wide token for distinguishing a current retry from cancelled
    /// sleepers without retaining one generation entry per historical key.
    retry_epoch: Arc<()>,
    retry_serial: u64,
    scheduled_retries: HashMap<ReadStateKey, (ReadRetryToken, oneshot::Sender<()>)>,
    reconciliation_pending: HashSet<ReadStateKey>,
    persistence: Option<ReadPersistenceIngress>,
    save_generation: u64,
}

impl ReadWorkerSupervisor {
    fn new(
        session_generation: u64,
        network: Option<ReadNetworkContext>,
        network_timeout: Duration,
    ) -> Self {
        Self {
            state: ReadStateEngine::new(session_generation),
            network,
            network_timeout,
            tasks: FuturesUnordered::new(),
            retry_tasks: FuturesUnordered::new(),
            cancellations: HashMap::new(),
            waiters: HashMap::new(),
            next_waiter_id: 0,
            retry_base_delay: READ_RETRY_BASE_DELAY,
            retry_max_delay: READ_RETRY_MAX_DELAY,
            retry_attempts: HashMap::new(),
            retry_epoch: Arc::new(()),
            retry_serial: 0,
            scheduled_retries: HashMap::new(),
            reconciliation_pending: HashSet::new(),
            persistence: None,
            save_generation: 0,
        }
    }
}

impl ReadWorkerSupervisor {
    fn unavailable() -> Self {
        Self::new(0, None, READ_NETWORK_TIMEOUT)
    }
}

impl ReadWorkerSupervisor {
    fn matrix(
        session: Arc<MatrixClientSession>,
        session_generation: u64,
        restored: ReadPersistenceSnapshot,
        persistence: ReadPersistenceIngress,
    ) -> Self {
        let reconciliation_pending = restored
            .entries()
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let state = ReadStateEngine::restore(session_generation, restored)
            .unwrap_or_else(|| ReadStateEngine::new(session_generation));
        let mut supervisor = Self {
            state,
            network: Some(ReadNetworkContext::Matrix(session)),
            network_timeout: READ_NETWORK_TIMEOUT,
            tasks: FuturesUnordered::new(),
            retry_tasks: FuturesUnordered::new(),
            cancellations: HashMap::new(),
            waiters: HashMap::new(),
            next_waiter_id: 0,
            retry_base_delay: READ_RETRY_BASE_DELAY,
            retry_max_delay: READ_RETRY_MAX_DELAY,
            retry_attempts: HashMap::new(),
            retry_epoch: Arc::new(()),
            retry_serial: 0,
            scheduled_retries: HashMap::new(),
            reconciliation_pending,
            persistence: Some(persistence),
            save_generation: 0,
        };
        for key in supervisor.reconciliation_pending.clone() {
            supervisor.schedule_retry(&key);
        }
        supervisor
    }
}

impl ReadWorkerSupervisor {
    #[cfg(test)]
    fn synthetic(
        requests: mpsc::UnboundedSender<SyntheticReadNetworkRequest>,
        timeout: Duration,
    ) -> Self {
        Self::new(1, Some(ReadNetworkContext::Synthetic { requests }), timeout)
    }
}

impl ReadWorkerSupervisor {
    #[cfg(test)]
    fn synthetic_with_retry(
        requests: mpsc::UnboundedSender<SyntheticReadNetworkRequest>,
        timeout: Duration,
        retry_base_delay: Duration,
        retry_max_delay: Duration,
    ) -> Self {
        let mut supervisor =
            Self::new(1, Some(ReadNetworkContext::Synthetic { requests }), timeout);
        supervisor.retry_base_delay = retry_base_delay;
        supervisor.retry_max_delay = retry_max_delay;
        supervisor
    }
}

impl ReadWorkerSupervisor {
    #[cfg(test)]
    fn synthetic_restored(
        requests: mpsc::UnboundedSender<SyntheticReadNetworkRequest>,
        restored: ReadPersistenceSnapshot,
        persistence: ReadPersistenceIngress,
    ) -> Self {
        let reconciliation_pending = restored
            .entries()
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let mut supervisor = Self {
            state: ReadStateEngine::restore(7, restored)
                .expect("synthetic restored read state must be valid"),
            network: Some(ReadNetworkContext::Synthetic { requests }),
            network_timeout: Duration::from_secs(30),
            tasks: FuturesUnordered::new(),
            retry_tasks: FuturesUnordered::new(),
            cancellations: HashMap::new(),
            waiters: HashMap::new(),
            next_waiter_id: 0,
            retry_base_delay: Duration::from_secs(1),
            retry_max_delay: Duration::from_secs(4),
            retry_attempts: HashMap::new(),
            retry_epoch: Arc::new(()),
            retry_serial: 0,
            scheduled_retries: HashMap::new(),
            reconciliation_pending,
            persistence: Some(persistence),
            save_generation: 0,
        };
        for key in supervisor.reconciliation_pending.clone() {
            supervisor.schedule_retry(&key);
        }
        supervisor
    }
}

impl ReadWorkerSupervisor {
    fn allocate_waiter(&mut self) -> Option<ReadWaiterId> {
        let next = self.next_waiter_id.checked_add(1)?;
        self.next_waiter_id = next;
        Some(ReadWaiterId::new(next))
    }
}

impl ReadWorkerSupervisor {
    fn spawn_network(&mut self, operation: ReadOperation) -> bool {
        let Some(network) = self.network.clone() else {
            return false;
        };
        let timeout = self.network_timeout;
        let fence = operation.fence();
        let cancelled_operation = operation.clone();
        let (cancel, mut cancelled) = oneshot::channel();
        self.cancellations.insert(fence, cancel);
        self.tasks.push(Box::pin(async move {
            tokio::select! {
                biased;
                _ = &mut cancelled => ReadWorkerCompletion::Cancelled {
                    operation: cancelled_operation,
                },
                outcome = executor::timeout(timeout, perform_read_network_operation(
                    network,
                    &operation,
                )) => ReadWorkerCompletion::Network {
                    operation,
                    outcome: match outcome {
                        Ok(Ok(())) => ReadNetworkOutcome::Succeeded,
                        Ok(Err(())) => ReadNetworkOutcome::Failed,
                        Err(_) => ReadNetworkOutcome::TimedOut,
                    },
                },
            }
        }));
        true
    }
}

impl ReadWorkerSupervisor {
    fn spawn_actor_apply<F>(&mut self, operation: ReadOperation, apply: F)
    where
        F: Future<Output = bool> + Send + 'static,
    {
        let timeout = self.network_timeout;
        let fence = operation.fence();
        let cancelled_operation = operation.clone();
        let (cancel, mut cancelled) = oneshot::channel();
        self.cancellations.insert(fence, cancel);
        self.tasks.push(Box::pin(async move {
            tokio::select! {
                biased;
                _ = &mut cancelled => ReadWorkerCompletion::Cancelled {
                    operation: cancelled_operation,
                },
                applied = executor::timeout(timeout, apply) => ReadWorkerCompletion::ActorApplied {
                    operation,
                    applied: applied.unwrap_or(false),
                },
            }
        }));
    }
}

impl ReadWorkerSupervisor {
    fn cancel(&mut self, fence: ReadOperationFence) {
        if let Some(cancel) = self.cancellations.remove(&fence) {
            let _ = cancel.send(());
        }
    }
}

impl ReadWorkerSupervisor {
    fn finish(&mut self, completion: &ReadWorkerCompletion) {
        if let Some(fence) = completion.fence() {
            self.cancellations.remove(&fence);
        }
    }
}

impl ReadWorkerSupervisor {
    fn schedule_retry(&mut self, key: &ReadStateKey) {
        if self.scheduled_retries.contains_key(key) {
            return;
        }
        let attempt = self.retry_attempts.entry(key.clone()).or_default();
        let delay =
            read_retry_delay_for_attempt(self.retry_base_delay, self.retry_max_delay, *attempt);
        *attempt = attempt.saturating_add(1);
        self.retry_serial = match self.retry_serial.checked_add(1) {
            Some(serial) => serial,
            None => {
                // A stale retry future can still own the previous serial.
                // Rotate allocation identity before restarting the scalar so
                // no live stale token can compare equal to a fresh retry.
                self.retry_epoch = Arc::new(());
                1
            }
        };
        let generation = ReadRetryToken {
            epoch: self.retry_epoch.clone(),
            serial: self.retry_serial,
        };
        let cancelled_generation = generation.clone();
        let (cancel, mut cancelled) = oneshot::channel();
        self.scheduled_retries
            .insert(key.clone(), (generation.clone(), cancel));
        let key = key.clone();
        self.retry_tasks.push(Box::pin(async move {
            tokio::select! {
                _ = executor::sleep(delay) => ReadWorkerCompletion::RetryWake {
                    key,
                    generation,
                    cancelled: false,
                },
                _ = &mut cancelled => ReadWorkerCompletion::RetryWake {
                    key,
                    generation: cancelled_generation,
                    cancelled: true,
                },
            }
        }));
    }
}

impl ReadWorkerSupervisor {
    fn accept_retry_wake(&mut self, key: &ReadStateKey, generation: ReadRetryToken) -> bool {
        if self
            .scheduled_retries
            .get(key)
            .is_none_or(|(scheduled, _)| scheduled != &generation)
        {
            return false;
        }
        self.scheduled_retries.remove(key);
        true
    }
}

impl ReadWorkerSupervisor {
    fn invalidate_retry(&mut self, key: &ReadStateKey) {
        if let Some((_, cancel)) = self.scheduled_retries.remove(key) {
            let _ = cancel.send(());
        }
    }
}

impl ReadWorkerSupervisor {
    fn reset_retry(&mut self, key: &ReadStateKey) {
        self.invalidate_retry(key);
        self.retry_attempts.remove(key);
    }
}

impl ReadWorkerSupervisor {
    fn desired_keys(&self) -> Vec<ReadStateKey> {
        self.state
            .persistence_snapshot()
            .entries()
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl ReadWorkerSupervisor {
    fn reconciliation_pending(&self, key: &ReadStateKey) -> bool {
        self.reconciliation_pending.contains(key)
    }
}

impl ReadWorkerSupervisor {
    fn finish_reconciliation(&mut self, key: &ReadStateKey) {
        self.reconciliation_pending.remove(key);
    }
}

impl ReadWorkerSupervisor {
    fn publish_persistence(&mut self) {
        let Some(persistence) = self.persistence.as_ref() else {
            return;
        };
        self.save_generation = self.save_generation.wrapping_add(1).max(1);
        persistence.publish(ReadPersistenceRequest::new(
            self.state.session_generation(),
            self.save_generation,
            self.state.persistence_snapshot(),
        ));
    }
}

impl ReadWorkerSupervisor {
    fn cancel_all(&mut self) {
        for (_, cancel) in self.cancellations.drain() {
            let _ = cancel.send(());
        }
        for (_, (_, cancel)) in self.scheduled_retries.drain() {
            let _ = cancel.send(());
        }
        self.tasks = FuturesUnordered::new();
        self.retry_tasks = FuturesUnordered::new();
        self.retry_attempts.clear();
    }
}

impl ReadWorkerSupervisor {
    #[cfg(test)]
    fn retry_bookkeeping_key_count(&self) -> usize {
        self.retry_attempts
            .keys()
            .chain(self.scheduled_retries.keys())
            .collect::<HashSet<_>>()
            .len()
    }
}

fn read_retry_delay_for_attempt(base: Duration, cap: Duration, attempt: u32) -> Duration {
    let multiplier = 1_u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
    base.saturating_mul(multiplier).min(cap)
}

impl Drop for ReadWorkerSupervisor {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

async fn perform_read_network_operation(
    network: ReadNetworkContext,
    operation: &ReadOperation,
) -> Result<(), ()> {
    match network {
        ReadNetworkContext::Matrix(session) => {
            let room_id = matrix_sdk::ruma::RoomId::parse(match operation.key() {
                ReadStateKey::PublicUnthreaded { room_id }
                | ReadStateKey::ThreadRead { room_id, .. }
                | ReadStateKey::FullyReadAndPrivateUnthreaded { room_id } => room_id.as_str(),
            })
            .map_err(|_| ())?;
            let event_id =
                matrix_sdk::ruma::EventId::parse(operation.target().event_id()).map_err(|_| ())?;
            let room = session.client().get_room(&room_id).ok_or(())?;
            match operation.key() {
                ReadStateKey::PublicUnthreaded { .. } => room
                    .send_multiple_receipts(Receipts::new().public_read_receipt(event_id))
                    .await
                    .map_err(|_| ()),
                ReadStateKey::ThreadRead { root_event_id, .. } => {
                    let root_event_id =
                        matrix_sdk::ruma::EventId::parse(root_event_id).map_err(|_| ())?;
                    room.send_single_receipt(
                        SendReceiptType::Read,
                        ReceiptThread::Thread(root_event_id),
                        event_id,
                    )
                    .await
                    .map_err(|_| ())
                }
                ReadStateKey::FullyReadAndPrivateUnthreaded { .. } => {
                    let private_event_id = private_read_receipt_event_id_from_room_for_fully_read(
                        &room,
                        operation.target().event_id(),
                    );
                    let private_event_id =
                        matrix_sdk::ruma::EventId::parse(private_event_id).map_err(|_| ())?;
                    room.send_multiple_receipts(
                        Receipts::new()
                            .fully_read_marker(event_id)
                            .private_read_receipt(private_event_id),
                    )
                    .await
                    .map_err(|_| ())
                }
            }
        }
        #[cfg(test)]
        ReadNetworkContext::Synthetic { requests } => {
            let (response, outcome) = oneshot::channel();
            requests
                .send(SyntheticReadNetworkRequest {
                    operation: operation.clone(),
                    response,
                })
                .map_err(|_| ())?;
            outcome.await.unwrap_or(Err(()))
        }
    }
}

impl TimelineManagerActor {
    fn route_read_command(
        &mut self,
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        kind: ReadCommandKind,
    ) {
        if self.read_workers.network.is_none() {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        }
        let Some(handle) = self.timelines.get(&key) else {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::NotSubscribed,
                },
            );
            return;
        };
        if matrix_sdk::ruma::RoomId::parse(key.room_id()).is_err()
            || matrix_sdk::ruma::EventId::parse(event_id.as_str()).is_err()
            || matches!(
                &key.kind,
                TimelineKind::Thread { root_event_id, .. }
                    if matrix_sdk::ruma::EventId::parse(root_event_id.as_str()).is_err()
            )
        {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
            return;
        }

        let read_key = read_state_key_for_command(&key, kind);
        let target = match handle.read_position(&event_id) {
            Some(position) => ReadTarget::with_position(event_id.clone(), position),
            None => ReadTarget::new(event_id.clone()),
        };
        let Some(waiter) = self.read_workers.allocate_waiter() else {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::QueueOverflow,
                },
            );
            return;
        };
        let admission = self.read_workers.state.admit(
            self.read_workers.state.session_generation(),
            read_key.clone(),
            target,
            waiter,
        );
        record_read_admission(&read_key, admission.diagnostic());
        match admission.status() {
            ReadAdmissionStatus::Accepted | ReadAdmissionStatus::Coalesced => {
                self.read_workers.waiters.insert(
                    waiter,
                    ReadCommandWaiter {
                        request_id,
                        key,
                        event_id,
                        kind,
                    },
                );
            }
            ReadAdmissionStatus::Rejected(_) => {
                self.emit_failure(
                    request_id,
                    CoreFailure::TimelineOperationFailed {
                        kind: TimelineFailureKind::QueueOverflow,
                    },
                );
                return;
            }
        }
        if let Some(superseded) = admission.superseded_operation() {
            self.read_workers.cancel(superseded);
        }
        self.read_workers.publish_persistence();
        self.wake_read_operation(&read_key);
    }
}

impl TimelineManagerActor {
    fn wake_read_operation(&mut self, key: &ReadStateKey) {
        if self.read_workers.reconciliation_pending(key) {
            self.read_workers.schedule_retry(key);
            return;
        }
        match self.read_workers.state.wake(key) {
            ReadWakeResult::Start(operation) => {
                if !self.read_workers.spawn_network(operation.clone()) {
                    let completion = self.read_workers.state.complete(
                        operation.key(),
                        operation.fence(),
                        ReadNetworkOutcome::Failed,
                    );
                    for settlement in completion.settlements() {
                        if let Some(waiter) = self.read_workers.waiters.remove(&settlement.waiter())
                        {
                            self.emit_failure(
                                waiter.request_id,
                                CoreFailure::TimelineOperationFailed {
                                    kind: TimelineFailureKind::Sdk,
                                },
                            );
                        }
                    }
                }
            }
            ReadWakeResult::AlreadyActive | ReadWakeResult::NoDesired => {}
            ReadWakeResult::OperationGenerationExhausted => {
                let waiter_ids = self
                    .read_workers
                    .waiters
                    .iter()
                    .filter_map(|(waiter_id, waiter)| {
                        (read_state_key_for_command(&waiter.key, waiter.kind) == *key)
                            .then_some(*waiter_id)
                    })
                    .collect::<Vec<_>>();
                for waiter_id in waiter_ids {
                    if let Some(waiter) = self.read_workers.waiters.remove(&waiter_id) {
                        self.emit_failure(
                            waiter.request_id,
                            CoreFailure::TimelineOperationFailed {
                                kind: TimelineFailureKind::QueueOverflow,
                            },
                        );
                    }
                }
            }
        }
    }
}

impl TimelineManagerActor {
    fn wake_all_desired_reads(&mut self, source: ReadRetrySource) {
        for key in self.read_workers.desired_keys() {
            record_read_retry(
                &key,
                source,
                self.read_workers.state.candidate_count(&key),
                self.read_workers.state.waiter_count(&key),
            );
            if self.read_workers.reconciliation_pending(&key) {
                self.read_workers.schedule_retry(&key);
                continue;
            }
            self.read_workers.invalidate_retry(&key);
            self.wake_read_operation(&key);
        }
    }
}

impl TimelineManagerActor {
    fn wake_desired_reads_for_room(&mut self, room_id: &str, source: ReadRetrySource) {
        let keys = self
            .read_workers
            .desired_keys()
            .into_iter()
            .filter(|key| read_state_room_id(key) == room_id)
            .collect::<Vec<_>>();
        for key in keys {
            record_read_retry(
                &key,
                source,
                self.read_workers.state.candidate_count(&key),
                self.read_workers.state.waiter_count(&key),
            );
            if self.read_workers.reconciliation_pending(&key) {
                self.read_workers.schedule_retry(&key);
                continue;
            }
            self.read_workers.invalidate_retry(&key);
            self.wake_read_operation(&key);
        }
    }
}

impl TimelineManagerActor {
    async fn handle_authoritative_read_state_observed(
        &mut self,
        timeline_key: &TimelineKey,
        actor_generation: u64,
        read_key: ReadStateKey,
        event_id: Option<String>,
    ) {
        let Some(position_index) = self
            .timelines
            .get(timeline_key)
            .and_then(TimelineActorHandle::read_position_index)
        else {
            return;
        };
        if position_index.actor_generation() != actor_generation
            || !timeline_key_matches_read_state_key(timeline_key, &read_key)
        {
            return;
        }
        let restored_entries = self.read_workers.state.persistence_snapshot();
        if let Some(entry) = restored_entries
            .entries()
            .iter()
            .find(|entry| entry.key() == &read_key)
        {
            for desired_event_id in entry.event_ids() {
                let Some(position) = position_index.evidence(desired_event_id) else {
                    continue;
                };
                self.read_workers.state.observe_position(
                    self.read_workers.state.session_generation(),
                    &read_key,
                    desired_event_id,
                    position,
                );
            }
        }
        let Some(event_id) = event_id else {
            self.read_workers.finish_reconciliation(&read_key);
            record_read_retry(
                &read_key,
                ReadRetrySource::SyncReconciliation,
                self.read_workers.state.candidate_count(&read_key),
                self.read_workers.state.waiter_count(&read_key),
            );
            self.read_workers.invalidate_retry(&read_key);
            self.wake_read_operation(&read_key);
            return;
        };
        let confirmed_position = position_index.evidence(&event_id);
        let confirmed = confirmed_position
            .map(|position| ReadTarget::with_position(event_id.clone(), position))
            .unwrap_or_else(|| ReadTarget::new(event_id));
        let confirmation = self.read_workers.state.confirm_authoritative(
            self.read_workers.state.session_generation(),
            &read_key,
            confirmed,
        );
        if let Some(superseded) = confirmation.superseded_operation() {
            self.read_workers.cancel(superseded);
        }
        self.settle_read_waiters(confirmation.settlements().to_vec())
            .await;
        let remaining = self.read_workers.state.candidate_count(&read_key);
        if remaining == 0 {
            self.read_workers.finish_reconciliation(&read_key);
            self.read_workers.reset_retry(&read_key);
        } else if self.read_workers.reconciliation_pending(&read_key)
            && (confirmed_position.is_none()
                || self
                    .read_workers
                    .state
                    .persistence_snapshot()
                    .entries()
                    .iter()
                    .find(|entry| entry.key() == &read_key)
                    .is_some_and(|entry| {
                        entry
                            .event_ids()
                            .iter()
                            .any(|event_id| position_index.evidence(event_id).is_none())
                    }))
        {
            // Different targets outside one current canonical position index
            // cannot be ordered safely. Keep the restored intent pending until
            // a later projection or receipt update supplies proof.
        } else {
            self.read_workers.finish_reconciliation(&read_key);
            record_read_retry(
                &read_key,
                ReadRetrySource::AuthoritativeReceipt,
                remaining,
                self.read_workers.state.waiter_count(&read_key),
            );
            self.read_workers.invalidate_retry(&read_key);
            self.wake_read_operation(&read_key);
        }
        self.read_workers.publish_persistence();
    }
}

impl TimelineManagerActor {
    async fn handle_read_worker_completion(&mut self, completion: ReadWorkerCompletion) {
        self.read_workers.finish(&completion);
        match completion {
            ReadWorkerCompletion::RetryWake {
                key,
                generation,
                cancelled,
            } => {
                if !cancelled && self.read_workers.accept_retry_wake(&key, generation) {
                    self.read_workers.finish_reconciliation(&key);
                    record_read_retry(
                        &key,
                        ReadRetrySource::Backoff,
                        self.read_workers.state.candidate_count(&key),
                        self.read_workers.state.waiter_count(&key),
                    );
                    self.wake_read_operation(&key);
                }
            }
            ReadWorkerCompletion::Cancelled { operation } => {
                self.settle_read_operation(operation, ReadNetworkOutcome::Failed)
                    .await;
            }
            ReadWorkerCompletion::Network { operation, outcome } => {
                if outcome == ReadNetworkOutcome::Succeeded
                    && self.read_workers.state.active_operation(operation.key())
                        == Some(operation.fence())
                {
                    match operation.key() {
                        ReadStateKey::PublicUnthreaded { .. } => {
                            let actor_is_current = self
                                .read_timeline_key_for_operation(&operation)
                                .is_some_and(|key| self.timelines.contains_key(&key));
                            if !actor_is_current {
                                self.settle_read_operation(operation, ReadNetworkOutcome::Failed)
                                    .await;
                                return;
                            }
                        }
                        ReadStateKey::ThreadRead { .. }
                        | ReadStateKey::FullyReadAndPrivateUnthreaded { .. } => {
                            if self.spawn_read_actor_apply(operation.clone()) {
                                return;
                            }
                            self.settle_read_operation(operation, ReadNetworkOutcome::Failed)
                                .await;
                            return;
                        }
                    }
                }
                self.settle_read_operation(operation, outcome).await;
            }
            ReadWorkerCompletion::ActorApplied { operation, applied } => {
                self.settle_read_operation(
                    operation,
                    if applied {
                        ReadNetworkOutcome::Succeeded
                    } else {
                        ReadNetworkOutcome::Failed
                    },
                )
                .await;
            }
        }
    }
}

impl TimelineManagerActor {
    fn spawn_read_actor_apply(&mut self, operation: ReadOperation) -> bool {
        let apply_kind = match operation.key() {
            ReadStateKey::PublicUnthreaded { .. } => return false,
            ReadStateKey::ThreadRead { .. } => ReadActorApplyKind::ThreadReceipt,
            ReadStateKey::FullyReadAndPrivateUnthreaded { .. } => ReadActorApplyKind::FullyRead,
        };
        let Some(timeline_key) = self.read_timeline_key_for_operation(&operation) else {
            return false;
        };
        let Some(handle) = self.timelines.get(&timeline_key) else {
            return false;
        };
        let Some(control_tx) = handle.control_tx.clone() else {
            return false;
        };
        let event_id = operation.target().event_id().to_owned();
        self.read_workers.spawn_actor_apply(operation, async move {
            let (acknowledged, acknowledgement) = oneshot::channel();
            if control_tx
                .send(TimelineActorControl::ApplyReadSuccess {
                    kind: apply_kind,
                    event_id,
                    acknowledged,
                })
                .await
                .is_err()
            {
                return false;
            }
            acknowledgement.await.unwrap_or(false)
        });
        true
    }
}

impl TimelineManagerActor {
    fn read_timeline_key_for_operation(&self, operation: &ReadOperation) -> Option<TimelineKey> {
        self.read_workers
            .waiters
            .values()
            .find_map(|waiter| {
                (waiter.event_id == operation.target().event_id()
                    && read_state_key_for_command(&waiter.key, waiter.kind) == *operation.key())
                .then(|| waiter.key.clone())
            })
            .or_else(|| {
                self.timelines
                    .keys()
                    .find(|key| timeline_key_matches_read_state_key(key, operation.key()))
                    .cloned()
            })
    }
}

impl TimelineManagerActor {
    async fn settle_read_operation(
        &mut self,
        operation: ReadOperation,
        outcome: ReadNetworkOutcome,
    ) {
        let read_key = operation.key().clone();
        let completion = self
            .read_workers
            .state
            .complete(&read_key, operation.fence(), outcome);
        record_read_completion(&read_key, completion.diagnostic());
        let disposition = completion.disposition();
        let settlements = completion.settlements().to_vec();
        self.settle_read_waiters(settlements).await;
        match disposition {
            ReadCompletionDisposition::Succeeded => {
                self.read_workers.reset_retry(&read_key);
                self.wake_read_operation(&read_key);
            }
            ReadCompletionDisposition::Failed | ReadCompletionDisposition::TimedOut => {
                self.read_workers.schedule_retry(&read_key);
            }
            ReadCompletionDisposition::StaleDiscarded => {}
        }
        if disposition != ReadCompletionDisposition::StaleDiscarded {
            self.read_workers.publish_persistence();
        }
    }
}

impl TimelineManagerActor {
    async fn settle_read_waiters(
        &mut self,
        settlements: Vec<crate::read_state::ReadWaiterSettlement>,
    ) {
        for settlement in settlements {
            let Some(waiter) = self.read_workers.waiters.remove(&settlement.waiter()) else {
                continue;
            };
            match settlement.terminal() {
                ReadWaiterTerminal::Converged => {
                    if waiter.kind == ReadCommandKind::FullyRead {
                        let room_id = waiter.key.room_id().to_owned();
                        if !self
                            .emit_action_reliable(AppAction::RoomMarkedAsReadSucceeded {
                                request_id: waiter.request_id.sequence,
                                room_id,
                            })
                            .await
                        {
                            self.emit_failure(
                                waiter.request_id,
                                CoreFailure::TimelineOperationFailed {
                                    kind: TimelineFailureKind::Sdk,
                                },
                            );
                            continue;
                        }
                        self.emit(CoreEvent::LiveSignals(LiveSignalsEvent::FullyReadSet {
                            request_id: waiter.request_id,
                            key: waiter.key,
                            event_id: waiter.event_id,
                        }));
                    } else {
                        self.emit(CoreEvent::LiveSignals(LiveSignalsEvent::ReadReceiptSent {
                            request_id: waiter.request_id,
                            key: waiter.key,
                            event_id: waiter.event_id,
                        }));
                    }
                }
                ReadWaiterTerminal::Failed | ReadWaiterTerminal::TimedOut => {
                    self.emit_failure(
                        waiter.request_id,
                        CoreFailure::TimelineOperationFailed {
                            kind: if settlement.terminal() == ReadWaiterTerminal::TimedOut {
                                TimelineFailureKind::Timeout
                            } else {
                                TimelineFailureKind::Sdk
                            },
                        },
                    );
                }
            }
        }
    }
}

impl TimelineActor {
    async fn handle_read_success(&mut self, kind: ReadActorApplyKind, event_id: String) -> bool {
        match kind {
            ReadActorApplyKind::ThreadReceipt => {
                if !matches!(self.key.kind, TimelineKind::Thread { .. }) {
                    return false;
                }
                let authoritative_event_id = newest_provable_receipt_event_id(
                    &self.navigation_items,
                    &event_id,
                    None,
                    self.thread_attention.receipt_event_id.as_deref(),
                );
                if let Some(action) = self.thread_attention.acknowledge(
                    &self.key,
                    &self.navigation_items,
                    authoritative_event_id,
                ) && !self.emit_action_reliable(action).await
                {
                    return false;
                }
                let snapshot = derive_timeline_navigation_snapshot(
                    &self.navigation_items,
                    self.fully_read_event_id.as_deref(),
                    &self.viewport_observation,
                    self.own_user_id.as_ref().map(|user_id| user_id.as_str()),
                );
                record_timeline_unread_consistency(
                    "thread_receipt_applied",
                    &self.key,
                    &self.navigation_items,
                    self.display_projection.display_items(),
                    self.last_navigation_snapshot.as_ref(),
                    &snapshot,
                    &self.thread_attention,
                );
                true
            }
            ReadActorApplyKind::FullyRead => {
                let Some(room_id) = timeline_room_id(&self.key) else {
                    return false;
                };
                if !self
                    .emit_action_reliable(AppAction::FullyReadMarkerUpdated {
                        room_id,
                        event_id: Some(event_id.clone()),
                    })
                    .await
                {
                    return false;
                }
                self.fully_read_event_id = Some(event_id);
                self.emit_navigation_if_changed();
                true
            }
        }
    }
}

impl TimelineActor {
    async fn handle_own_read_receipt_changed(&mut self) {
        let Some(own_user_id) = self.own_user_id.as_deref() else {
            return;
        };
        let Some(event_id) = self
            .timeline
            .latest_user_read_receipt_timeline_event_id(own_user_id)
            .await
            .map(|event_id| event_id.to_string())
        else {
            return;
        };
        if let Some(action) =
            self.thread_attention
                .acknowledge(&self.key, &self.navigation_items, event_id.clone())
        {
            let _ = self.emit_action_reliable(action).await;
        }
        self.publish_authoritative_read_observation(
            read_state_key_for_command(&self.key, ReadCommandKind::Receipt),
            Some(event_id),
        )
        .await;
    }
}

impl TimelineActor {
    async fn publish_authoritative_read_state(&self) {
        let receipt_event_id = if let Some(own_user_id) = self.own_user_id.as_deref() {
            self.timeline
                .latest_user_read_receipt_timeline_event_id(own_user_id)
                .await
                .map(|event_id| event_id.to_string())
        } else {
            None
        };
        self.publish_authoritative_read_observation(
            read_state_key_for_command(&self.key, ReadCommandKind::Receipt),
            receipt_event_id,
        )
        .await;
        let fully_read_event_id = matrix_sdk::ruma::RoomId::parse(self.key.room_id())
            .ok()
            .and_then(|room_id| self.session.client().get_room(&room_id))
            .and_then(|room| {
                room.fully_read_event_id()
                    .map(|event_id| event_id.to_string())
            });
        self.publish_authoritative_read_observation(
            read_state_key_for_command(&self.key, ReadCommandKind::FullyRead),
            fully_read_event_id,
        )
        .await;
    }
}

impl TimelineActor {
    async fn publish_authoritative_read_observation(
        &self,
        read_key: ReadStateKey,
        event_id: Option<String>,
    ) {
        let _ = self
            .manager_tx
            .send(TimelineMessage::AuthoritativeReadStateObserved {
                key: self.key.clone(),
                actor_generation: self.actor_generation,
                read_key,
                event_id,
            })
            .await;
    }
}

impl TimelineActor {
    async fn handle_set_typing(&mut self, request_id: RequestId, is_typing: bool) {
        match self.timeline.room().typing_notice(is_typing).await {
            Ok(()) => {
                self.emit(CoreEvent::LiveSignals(LiveSignalsEvent::TypingSet {
                    request_id,
                    key: self.key.clone(),
                    is_typing,
                }));
            }
            Err(_) => {
                self.emit_failure(
                    request_id,
                    CoreFailure::TimelineOperationFailed {
                        kind: TimelineFailureKind::Sdk,
                    },
                );
            }
        }
    }
}

impl TimelineActor {
    fn live_receipts_action_from_sdk_diffs(
        key: &TimelineKey,
        diffs: &[eyeball_im::VectorDiff<Arc<SdkTimelineItem>>],
    ) -> Option<AppAction> {
        let Some(room_id) = timeline_room_id(key) else {
            return None;
        };
        let mut receipts_by_event = Vec::new();
        for diff in diffs {
            collect_live_event_receipts_from_diff(diff, &mut receipts_by_event);
        }
        if receipts_by_event.is_empty() {
            return None;
        }
        Some(AppAction::LiveRoomReceiptsUpdated {
            room_id,
            receipts_by_event,
        })
    }
}

async fn run_typing_notifications(
    actor_tx: mpsc::Sender<TimelineActorMessage>,
    _guard: matrix_sdk::event_handler::EventHandlerDropGuard,
    mut typing_rx: tokio::sync::broadcast::Receiver<Vec<matrix_sdk::ruma::OwnedUserId>>,
) {
    loop {
        match typing_rx.recv().await {
            Ok(user_ids) => {
                let user_ids = user_ids
                    .into_iter()
                    .map(|user_id| user_id.to_string())
                    .collect();
                if actor_tx
                    .send(TimelineActorMessage::TypingUsersUpdated(user_ids))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

    #[test]
    fn set_fully_read_success_uses_private_read_receipt_before_clearing_room_unread_summary() {
        let source = include_str!("timeline.rs");
        let network = source
            .split("async fn perform_read_network_operation")
            .nth(1)
            .expect("manager read network worker should exist")
            .split("async fn run_send_enqueue_future")
            .next()
            .expect("send worker should follow read worker");
        let actor_success = source
            .split("async fn handle_read_success")
            .nth(1)
            .expect("actor read success handler should exist")
            .split("async fn handle_own_read_receipt_changed")
            .next()
            .expect("own receipt handler should follow actor success");
        let manager_settlement = source
            .split("async fn settle_read_operation")
            .nth(1)
            .expect("manager settlement should exist")
            .split("async fn route_to_actor_or_fail")
            .next()
            .expect("actor route should follow read settlement");

        assert!(
            network.contains("send_multiple_receipts"),
            "set_fully_read must use SDK read-marker batching so the marker and read receipt share one source of truth"
        );
        assert!(
            network.contains("room.send_multiple_receipts"),
            "manager worker must force the room read-marker API; stale server unread counts still need a fresh private receipt"
        );
        assert!(
            network.contains("fully_read_marker"),
            "set_fully_read must continue to update the fully-read marker"
        );
        assert!(
            network.contains("private_read_receipt"),
            "set_fully_read must include a private read receipt so SDK/server unread counts advance without publishing public receipts"
        );
        assert!(
            !network.contains("send_single_receipt(ReceiptType::FullyRead"),
            "fully-read alone must not be used as the persistent unread-count source of truth"
        );
        assert!(
            actor_success.contains("AppAction::FullyReadMarkerUpdated")
                && actor_success.contains("emit_action_reliable"),
            "actor control success must reliably update the fully-read marker before ACK"
        );
        assert!(
            manager_settlement.contains("AppAction::RoomMarkedAsReadSucceeded"),
            "ACKed fully-read success must clear RoomSummary unread counts so sidebar and Activity/Unread agree"
        );
    }

    #[test]
    fn private_read_receipt_target_advances_to_hidden_edit_notification() {
        let target = private_read_receipt_event_id_for_fully_read(FullyReadReceiptContext {
            visible_event_id: "$visible:test",
            latest_event_id: Some("$latest-edit:test"),
            latest_event_relation_type: Some("m.replace"),
            unread_messages: 0,
            notification_count: 1,
        });

        assert_eq!(target, "$latest-edit:test");

        for context in [
            FullyReadReceiptContext {
                visible_event_id: "$visible:test",
                latest_event_id: Some("$latest-message:test"),
                latest_event_relation_type: None,
                unread_messages: 0,
                notification_count: 1,
            },
            FullyReadReceiptContext {
                visible_event_id: "$visible:test",
                latest_event_id: Some("$latest-edit:test"),
                latest_event_relation_type: Some("m.replace"),
                unread_messages: 1,
                notification_count: 1,
            },
            FullyReadReceiptContext {
                visible_event_id: "$visible:test",
                latest_event_id: Some("$latest-edit:test"),
                latest_event_relation_type: Some("m.replace"),
                unread_messages: 0,
                notification_count: 0,
            },
            FullyReadReceiptContext {
                visible_event_id: "$visible:test",
                latest_event_id: None,
                latest_event_relation_type: Some("m.replace"),
                unread_messages: 0,
                notification_count: 1,
            },
        ] {
            assert_eq!(
                private_read_receipt_event_id_for_fully_read(context),
                "$visible:test"
            );
        }
    }

    #[test]
    fn private_read_receipt_target_advances_to_hidden_thread_notification() {
        let target = private_read_receipt_event_id_for_fully_read(FullyReadReceiptContext {
            visible_event_id: "$visible:test",
            latest_event_id: Some("$latest-thread:test"),
            latest_event_relation_type: Some("m.thread"),
            unread_messages: 0,
            notification_count: 1,
        });

        assert_eq!(target, "$latest-thread:test");
    }

    #[test]
    fn send_read_receipt_uses_threaded_receipt_for_thread_timelines() {
        let source = include_str!("timeline.rs");
        let worker = source
            .split("async fn perform_read_network_operation")
            .nth(1)
            .expect("manager read worker should exist")
            .split("async fn run_send_enqueue_future")
            .next()
            .expect("send worker should follow read worker");

        assert!(
            worker.contains("ReadStateKey::ThreadRead"),
            "thread timeline receipts must remain a distinct manager-owned operation"
        );
        assert!(
            worker.contains("ReceiptThread::Thread"),
            "thread timeline read receipts must use ReceiptThread::Thread(root)"
        );
        assert!(
            worker.contains("send_single_receipt"),
            "threaded read receipts must use the SDK single-receipt endpoint that accepts a thread"
        );
    }

    #[tokio::test]
    async fn restored_read_waits_for_authoritative_reconciliation_before_retrying() {
        let key = room_key();
        let read_key = ReadStateKey::PublicUnthreaded {
            room_id: key.room_id().to_owned(),
        };
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: u128::from(7_u64) << 64,
            ranks: HashMap::from([("$desired:test".to_owned(), 5)]),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        let (persistence, mut persistence_rx) = ReadPersistenceIngress::channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_restored(
            read_network_tx,
            restored_public_read_snapshot(key.room_id(), "$desired:test"),
            persistence,
        );

        manager.wake_all_desired_reads(ReadRetrySource::Reconnect);
        assert!(manager.read_workers.tasks.is_empty());
        assert!(read_network_rx.try_recv().is_err());

        manager
            .handle_authoritative_read_state_observed(&key, 7, read_key, None)
            .await;
        let responder = async {
            let retry = read_network_rx
                .recv()
                .await
                .expect("server-behind reconciliation starts retry");
            assert_eq!(retry.operation.target().event_id(), "$desired:test");
            retry.response.send(Ok(())).expect("retry succeeds");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("retry completion"))
            .await;
        persistence_rx
            .changed()
            .await
            .expect("successful retry publishes outbox removal");
        assert!(
            persistence_rx
                .borrow_and_update()
                .as_ref()
                .expect("persistence request")
                .snapshot()
                .is_empty()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_preserves_a_bounded_reconciliation_wake_for_new_read_waiters() {
        let key = room_key();
        let read_key = ReadStateKey::PublicUnthreaded {
            room_id: key.room_id().to_owned(),
        };
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: u128::from(7_u64) << 64,
            ranks: HashMap::new(),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        let (persistence, _persistence_rx) = ReadPersistenceIngress::channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_restored(
            read_network_tx,
            restored_public_read_snapshot(key.room_id(), "$restored:test"),
            persistence,
        );

        manager.wake_all_desired_reads(ReadRetrySource::Reconnect);
        manager.route_read_command(
            fake_rid(29_601),
            key,
            "$new-waiter:test".to_owned(),
            ReadCommandKind::Receipt,
        );

        assert!(
            manager
                .read_workers
                .scheduled_retries
                .contains_key(&read_key),
            "reconnect must not cancel the only bounded reconciliation wake"
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        let completion = manager
            .read_workers
            .retry_tasks
            .next()
            .await
            .expect("bounded reconciliation wake");
        manager.handle_read_worker_completion(completion).await;
        let responder = async {
            let request = read_network_rx
                .recv()
                .await
                .expect("new waiter receives a network attempt after the bound");
            assert_eq!(request.operation.target().event_id(), "$new-waiter:test");
            request.response.send(Err(())).expect("settle retry");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("network completion"))
            .await;
    }

    #[tokio::test]
    async fn invalidating_retry_actively_finishes_the_long_lived_sleeper() {
        let (network_tx, _network_rx) = mpsc::unbounded_channel();
        let mut supervisor = ReadWorkerSupervisor::synthetic_with_retry(
            network_tx,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let key = ReadStateKey::PublicUnthreaded {
            room_id: "!retry-cancel:example.invalid".to_owned(),
        };
        supervisor.schedule_retry(&key);
        assert_eq!(supervisor.retry_tasks.len(), 1);
        assert_eq!(supervisor.scheduled_retries.len(), 1);

        supervisor.invalidate_retry(&key);

        assert!(supervisor.scheduled_retries.is_empty());
        let completion =
            executor::timeout(Duration::from_millis(25), supervisor.retry_tasks.next())
                .await
                .expect("retry invalidation must wake the sleeper promptly")
                .expect("cancelled retry completion");
        assert!(matches!(
            completion,
            ReadWorkerCompletion::RetryWake {
                key: observed,
                cancelled: true,
                ..
            } if observed == key
        ));
        assert!(
            supervisor.retry_tasks.is_empty(),
            "an invalidated retry must not leave a sixty-second task behind"
        );
    }

    #[tokio::test]
    async fn retry_serial_exhaustion_never_reuses_a_live_stale_token() {
        let (network_tx, _network_rx) = mpsc::unbounded_channel();
        let mut supervisor = ReadWorkerSupervisor::synthetic_with_retry(
            network_tx,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let key = ReadStateKey::PublicUnthreaded {
            room_id: "!retry-token-exhaustion:example.invalid".to_owned(),
        };

        supervisor.retry_serial = u64::MAX;
        supervisor.schedule_retry(&key);
        let stale_generation = supervisor
            .scheduled_retries
            .get(&key)
            .map(|(generation, _)| generation.clone())
            .expect("stale retry token");
        supervisor.invalidate_retry(&key);

        // Model the manager-wide serial reaching exhaustion again while the
        // cancelled wake remains queued in `retry_tasks`.
        supervisor.retry_serial = u64::MAX;
        supervisor.schedule_retry(&key);
        let current_generation = supervisor
            .scheduled_retries
            .get(&key)
            .map(|(generation, _)| generation.clone())
            .expect("current retry token");

        let stale = executor::timeout(Duration::from_millis(25), supervisor.retry_tasks.next())
            .await
            .expect("cancelled stale wake must be ready")
            .expect("cancelled stale retry completion");
        assert!(matches!(
            stale,
            ReadWorkerCompletion::RetryWake {
                key: observed,
                generation: observed_generation,
                cancelled: true,
            } if observed == key && observed_generation == stale_generation
        ));
        assert!(
            !supervisor.accept_retry_wake(&key, stale_generation),
            "an exhausted stale token must not settle the current retry"
        );
        assert!(
            supervisor
                .scheduled_retries
                .get(&key)
                .is_some_and(|(generation, _)| generation == &current_generation),
            "the current retry must remain scheduled after the stale wake"
        );
    }

    #[tokio::test]
    async fn completed_retry_keys_do_not_accumulate_generation_bookkeeping() {
        let (network_tx, _network_rx) = mpsc::unbounded_channel();
        let mut supervisor = ReadWorkerSupervisor::synthetic_with_retry(
            network_tx,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(60),
        );

        for index in 0..256 {
            let key = ReadStateKey::PublicUnthreaded {
                room_id: format!("!completed-retry-{index}:example.invalid"),
            };
            supervisor.schedule_retry(&key);
            let generation = supervisor
                .scheduled_retries
                .get(&key)
                .map(|(generation, _)| generation.clone())
                .expect("retry generation");

            supervisor.reset_retry(&key);
            let cancelled =
                executor::timeout(Duration::from_millis(25), supervisor.retry_tasks.next())
                    .await
                    .expect("retry cancellation must be bounded")
                    .expect("cancelled retry completion");
            assert!(matches!(
                cancelled,
                ReadWorkerCompletion::RetryWake {
                    key: observed,
                    generation: observed_generation,
                    cancelled: true,
                } if observed == key && observed_generation == generation
            ));
            assert!(
                !supervisor.accept_retry_wake(&key, generation),
                "a cancelled sleeper must remain stale after its key retires"
            );
        }

        assert_eq!(
            supervisor.retry_bookkeeping_key_count(),
            0,
            "completed historical keys must not remain in retry bookkeeping"
        );
    }

    #[tokio::test]
    async fn authoritative_server_ahead_clears_restored_read_without_network_retry() {
        let key = room_key();
        let read_key = ReadStateKey::PublicUnthreaded {
            room_id: key.room_id().to_owned(),
        };
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: u128::from(7_u64) << 64,
            ranks: HashMap::from([
                ("$desired:test".to_owned(), 5),
                ("$server-ahead:test".to_owned(), 6),
            ]),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        let (persistence, mut persistence_rx) = ReadPersistenceIngress::channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_restored(
            read_network_tx,
            restored_public_read_snapshot(key.room_id(), "$desired:test"),
            persistence,
        );

        manager
            .handle_authoritative_read_state_observed(
                &key,
                7,
                read_key,
                Some("$server-ahead:test".to_owned()),
            )
            .await;

        assert!(read_network_rx.try_recv().is_err());
        assert!(manager.read_workers.tasks.is_empty());
        persistence_rx
            .changed()
            .await
            .expect("server-ahead reconciliation publishes removal");
        assert!(
            persistence_rx
                .borrow_and_update()
                .as_ref()
                .expect("persistence request")
                .snapshot()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn authoritative_reconciliation_keeps_unordered_remaining_candidate_pending() {
        let key = room_key();
        let read_key = ReadStateKey::PublicUnthreaded {
            room_id: key.room_id().to_owned(),
        };
        let mut restored = ReadStateEngine::new(7);
        restored.admit(
            7,
            read_key.clone(),
            ReadTarget::new("$positioned:test".to_owned()),
            ReadWaiterId::new(1),
        );
        restored.admit(
            7,
            read_key.clone(),
            ReadTarget::new("$outside-window:test".to_owned()),
            ReadWaiterId::new(2),
        );
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: u128::from(7_u64) << 64,
            ranks: HashMap::from([
                ("$positioned:test".to_owned(), 5),
                ("$server-ahead:test".to_owned(), 6),
            ]),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        let (persistence, _persistence_rx) = ReadPersistenceIngress::channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_restored(
            read_network_tx,
            restored.persistence_snapshot(),
            persistence,
        );

        manager
            .handle_authoritative_read_state_observed(
                &key,
                7,
                read_key.clone(),
                Some("$server-ahead:test".to_owned()),
            )
            .await;

        assert_eq!(manager.read_workers.state.candidate_count(&read_key), 1);
        assert!(manager.read_workers.reconciliation_pending(&read_key));
        assert!(manager.read_workers.tasks.is_empty());
        assert!(read_network_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn stalled_read_receipt_worker_does_not_block_cached_subscription_replay() {
        let key = room_key();
        let read_request_id = fake_rid(28_480);
        let subscribe_request_id = fake_rid(28_481);
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, mut control_rx) = mpsc::channel(2);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: 11,
            ranks: HashMap::from([("$read-target:test".to_owned(), 7)]),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_secs(30));
        let (manager_tx, manager_rx) = mpsc::channel(4);
        manager.msg_tx = manager_tx.clone();
        manager.msg_rx = manager_rx;
        let run = executor::spawn(manager.run());

        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::SendReadReceipt {
                request_id: read_request_id,
                key: key.clone(),
                event_id: "$read-target:test".to_owned(),
            }))
            .await
            .expect("admit read command");
        let stalled = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("read worker must start")
            .expect("synthetic read request");

        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::Subscribe {
                request_id: subscribe_request_id,
                key,
            }))
            .await
            .expect("queue cached subscribe");

        assert!(matches!(
            executor::timeout(Duration::from_millis(100), control_rx.recv())
                .await
                .expect("cached replay must not wait for read network"),
            Some(TimelineActorControl::ReplayInitialItems { cause_request_id })
                if cause_request_id == subscribe_request_id
        ));

        drop(stalled);
        let (acknowledged, acknowledgement) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::Shutdown {
                acknowledged: Some(acknowledged),
            })
            .await
            .expect("shutdown manager");
        acknowledgement.await.expect("shutdown acknowledgement");
        run.await.expect("manager task");
    }

    #[tokio::test]
    async fn newer_positioned_read_target_cancels_stale_worker_and_settles_both_waiters_once() {
        let key = room_key();
        let older_request_id = fake_rid(28_482);
        let newer_request_id = fake_rid(28_483);
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(2);
        let (_position_tx, position_rx) = watch::channel(Arc::new(TimelinePositionIndex {
            generation: 12,
            ranks: HashMap::from([
                ("$read-old:test".to_owned(), 7),
                ("$read-new:test".to_owned(), 8),
            ]),
        }));
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: Some(position_rx),
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (event_tx, mut event_rx) = broadcast::channel(8);
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_secs(30));
        let (manager_tx, manager_rx) = mpsc::channel(4);
        manager.msg_tx = manager_tx.clone();
        manager.msg_rx = manager_rx;
        let run = executor::spawn(manager.run());

        for (request_id, event_id) in [
            (older_request_id, "$read-old:test"),
            (newer_request_id, "$read-new:test"),
        ] {
            manager_tx
                .send(TimelineMessage::Command(TimelineCommand::SendReadReceipt {
                    request_id,
                    key: key.clone(),
                    event_id: event_id.to_owned(),
                }))
                .await
                .expect("admit read command");
            if request_id == older_request_id {
                break;
            }
        }
        let older = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("older read worker must start")
            .expect("older synthetic read request");
        assert_eq!(older.operation.target().event_id(), "$read-old:test");

        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::SendReadReceipt {
                request_id: newer_request_id,
                key: key.clone(),
                event_id: "$read-new:test".to_owned(),
            }))
            .await
            .expect("admit newer read command");
        let newer = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("newer read worker must start")
            .expect("newer synthetic read request");
        assert_eq!(newer.operation.target().event_id(), "$read-new:test");
        assert!(
            older.response.send(Ok(())).is_err(),
            "dominated worker must be cancelled before its late success"
        );
        newer.response.send(Ok(())).expect("complete newer target");

        let mut settled = HashSet::new();
        while settled.len() < 2 {
            let event = executor::timeout(Duration::from_millis(100), event_rx.recv())
                .await
                .expect("both waiters must settle")
                .expect("event stream");
            if let CoreEvent::LiveSignals(LiveSignalsEvent::ReadReceiptSent {
                request_id, ..
            }) = event
            {
                assert!(settled.insert(request_id), "duplicate waiter success");
            }
        }
        assert_eq!(settled, HashSet::from([older_request_id, newer_request_id]));
        assert!(
            executor::timeout(Duration::from_millis(25), event_rx.recv())
                .await
                .is_err(),
            "stale completion must not emit a second terminal"
        );

        let (acknowledged, acknowledgement) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::Shutdown {
                acknowledged: Some(acknowledged),
            })
            .await
            .expect("shutdown manager");
        acknowledgement.await.expect("shutdown acknowledgement");
        run.await.expect("manager task");
    }

    #[tokio::test]
    async fn coalesced_read_timeout_fails_each_waiter_once_without_retry_storm() {
        let key = room_key();
        let request_ids = [fake_rid(28_484), fake_rid(28_485)];
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: None,
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (event_tx, mut event_rx) = broadcast::channel(8);
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_millis(20));
        let (manager_tx, manager_rx) = mpsc::channel(4);
        manager.msg_tx = manager_tx.clone();
        manager.msg_rx = manager_rx;
        let run = executor::spawn(manager.run());

        for request_id in request_ids {
            manager_tx
                .send(TimelineMessage::Command(TimelineCommand::SendReadReceipt {
                    request_id,
                    key: key.clone(),
                    event_id: "$same-target:test".to_owned(),
                }))
                .await
                .expect("admit coalesced read");
        }
        let stalled = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("one network worker must start")
            .expect("synthetic read request");

        let mut failed = HashSet::new();
        while failed.len() < 2 {
            let event = executor::timeout(Duration::from_millis(100), event_rx.recv())
                .await
                .expect("timeout must settle both waiters")
                .expect("event stream");
            if let CoreEvent::OperationFailed {
                request_id,
                failure:
                    CoreFailure::TimelineOperationFailed {
                        kind: TimelineFailureKind::Timeout,
                    },
            } = event
            {
                assert!(failed.insert(request_id), "duplicate waiter timeout");
            }
        }
        assert_eq!(failed, HashSet::from(request_ids));
        assert!(
            executor::timeout(Duration::from_millis(40), read_network_rx.recv())
                .await
                .is_err(),
            "timeout retains desired state but must not spin an immediate retry"
        );
        assert!(
            executor::timeout(Duration::from_millis(20), event_rx.recv())
                .await
                .is_err(),
            "each waiter receives exactly one timeout"
        );

        drop(stalled);
        let (acknowledged, acknowledgement) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::Shutdown {
                acknowledged: Some(acknowledged),
            })
            .await
            .expect("shutdown manager");
        acknowledgement.await.expect("shutdown acknowledgement");
        run.await.expect("manager task");
    }

    #[tokio::test]
    async fn fully_read_success_waits_for_actor_control_ack_before_terminal_event() {
        let key = room_key();
        let request_id = fake_rid(28_486);
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, mut control_rx) = mpsc::channel(1);
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: None,
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (action_tx, mut action_rx) = mpsc::channel(4);
        let (event_tx, mut event_rx) = broadcast::channel(4);
        manager.action_tx = action_tx;
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_secs(30));
        let (manager_tx, manager_rx) = mpsc::channel(4);
        manager.msg_tx = manager_tx.clone();
        manager.msg_rx = manager_rx;
        let run = executor::spawn(manager.run());

        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::SetFullyRead {
                request_id,
                key: key.clone(),
                event_id: "$fully-read:test".to_owned(),
            }))
            .await
            .expect("admit fully-read command");
        let network = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("fully-read worker must start")
            .expect("synthetic read request");
        network.response.send(Ok(())).expect("SDK success");
        let control = executor::timeout(Duration::from_millis(100), control_rx.recv())
            .await
            .expect("success must enter actor control lane")
            .expect("actor apply control");
        assert!(
            event_rx.try_recv().is_err(),
            "success must wait for actor ACK"
        );
        let TimelineActorControl::ApplyReadSuccess {
            kind: ReadActorApplyKind::FullyRead,
            event_id,
            acknowledged,
        } = control
        else {
            panic!("expected fully-read actor control");
        };
        assert_eq!(event_id, "$fully-read:test");
        acknowledged.send(true).expect("ack actor state update");

        assert!(matches!(
            executor::timeout(Duration::from_millis(100), action_rx.recv())
                .await
                .expect("reducer action after ACK"),
            Some(actions)
                if matches!(actions.as_slice(), [AppAction::RoomMarkedAsReadSucceeded { request_id: sequence, .. }] if *sequence == request_id.sequence)
        ));
        assert!(matches!(
            executor::timeout(Duration::from_millis(100), event_rx.recv())
                .await
                .expect("success after ACK")
                .expect("event stream"),
            CoreEvent::LiveSignals(LiveSignalsEvent::FullyReadSet {
                request_id: settled,
                ..
            }) if settled == request_id
        ));

        let (acknowledged, acknowledgement) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::Shutdown {
                acknowledged: Some(acknowledged),
            })
            .await
            .expect("shutdown manager");
        acknowledgement.await.expect("shutdown acknowledgement");
        run.await.expect("manager task");
    }

    #[tokio::test]
    async fn fully_read_success_after_actor_removal_fails_without_success_terminal() {
        let key = room_key();
        let request_id = fake_rid(28_487);
        let (ordinary_tx, _ordinary_rx) = mpsc::channel(1);
        let (control_tx, mut control_rx) = mpsc::channel(1);
        let actor_handle = TimelineActorHandle {
            tx: ordinary_tx,
            control_tx: Some(control_tx),
            position_rx: None,
            task: None,
            auxiliary_tasks: Vec::new(),
            subscription_generation: None,
            enqueue_context: None,
        };
        let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), actor_handle)]));
        let (action_tx, _action_rx) = mpsc::channel(4);
        let (event_tx, mut event_rx) = broadcast::channel(4);
        manager.action_tx = action_tx;
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_secs(30));
        let (manager_tx, manager_rx) = mpsc::channel(4);
        manager.msg_tx = manager_tx.clone();
        manager.msg_rx = manager_rx;
        let run = executor::spawn(manager.run());

        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::SetFullyRead {
                request_id,
                key: key.clone(),
                event_id: "$fully-read:test".to_owned(),
            }))
            .await
            .expect("admit fully-read command");
        let network = executor::timeout(Duration::from_millis(100), read_network_rx.recv())
            .await
            .expect("fully-read worker must start")
            .expect("synthetic read request");
        manager_tx
            .send(TimelineMessage::Command(TimelineCommand::Unsubscribe {
                request_id: fake_rid(28_488),
                key: key.clone(),
            }))
            .await
            .expect("remove actor");
        assert!(
            executor::timeout(Duration::from_millis(100), control_rx.recv())
                .await
                .expect("actor control sender must close")
                .is_none()
        );
        network
            .response
            .send(Ok(()))
            .expect("late SDK success after actor removal");

        assert!(matches!(
            executor::timeout(Duration::from_millis(100), event_rx.recv())
                .await
                .expect("missing actor must fail waiter")
                .expect("event stream"),
            CoreEvent::OperationFailed {
                request_id: failed,
                failure: CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            } if failed == request_id
        ));
        assert!(
            executor::timeout(Duration::from_millis(20), event_rx.recv())
                .await
                .is_err(),
            "late network success must not emit a success terminal"
        );

        let (acknowledged, acknowledgement) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::Shutdown {
                acknowledged: Some(acknowledged),
            })
            .await
            .expect("shutdown manager");
        acknowledgement.await.expect("shutdown acknowledgement");
        run.await.expect("manager task");
    }

    #[tokio::test]
    async fn read_admission_rejects_missing_session_actor_and_invalid_ids_immediately() {
        let key = room_key();
        let (event_tx, mut event_rx) = broadcast::channel(8);
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.event_tx = event_tx;

        manager
            .handle_command(TimelineCommand::SendReadReceipt {
                request_id: fake_rid(28_489),
                key: key.clone(),
                event_id: "$event:test".to_owned(),
            })
            .await;
        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::OperationFailed {
                failure: CoreFailure::SessionRequired,
                ..
            })
        ));

        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers =
            ReadWorkerSupervisor::synthetic(read_network_tx, Duration::from_secs(30));
        manager.timelines.clear();
        manager
            .handle_command(TimelineCommand::SendReadReceipt {
                request_id: fake_rid(28_490),
                key: key.clone(),
                event_id: "$event:test".to_owned(),
            })
            .await;
        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::OperationFailed {
                failure: CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::NotSubscribed,
                },
                ..
            })
        ));

        manager
            .timelines
            .insert(key.clone(), test_timeline_actor_handle());
        manager
            .handle_command(TimelineCommand::SetFullyRead {
                request_id: fake_rid(28_491),
                key,
                event_id: "not-an-event-id".to_owned(),
            })
            .await;
        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::OperationFailed {
                failure: CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
                ..
            })
        ));
        assert!(manager.read_workers.tasks.is_empty());
        assert!(read_network_rx.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn failed_read_network_settles_waiter_once_then_retries_after_capped_backoff() {
        let key = room_key();
        let request_id = fake_rid(28_492);
        let (event_tx, mut event_rx) = broadcast::channel(4);
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_with_retry(
            read_network_tx,
            Duration::from_secs(30),
            Duration::from_secs(1),
            Duration::from_secs(4),
        );

        manager
            .handle_command(TimelineCommand::SendReadReceipt {
                request_id,
                key: key.clone(),
                event_id: "$event:test".to_owned(),
            })
            .await;
        let responder = async {
            let request = read_network_rx.recv().await.expect("read request");
            request
                .response
                .send(Err(()))
                .expect("fail network request");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("worker completion"))
            .await;

        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::OperationFailed {
                request_id: failed,
                failure: CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            }) if failed == request_id
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(read_network_rx.try_recv().is_err());

        assert!(
            manager
                .read_workers
                .retry_tasks
                .next()
                .now_or_never()
                .is_none(),
            "scheduled retry must begin pending"
        );
        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(
            manager
                .read_workers
                .retry_tasks
                .next()
                .now_or_never()
                .is_none(),
            "retry must not run before the backoff deadline"
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        let retry_wake = manager
            .read_workers
            .retry_tasks
            .next()
            .await
            .expect("backoff wake");
        manager.handle_read_worker_completion(retry_wake).await;
        let responder = async {
            let retried = read_network_rx.recv().await.expect("retry network request");
            assert_eq!(retried.operation.target().event_id(), "$event:test");
            retried.response.send(Ok(())).expect("retry succeeds");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("retry completion"))
            .await;
        assert!(
            event_rx.try_recv().is_err(),
            "background retry must not emit a second user terminal"
        );
        assert!(!manager.read_workers.state.has_candidate(
            &ReadStateKey::PublicUnthreaded {
                room_id: key.room_id().to_owned(),
            },
            "$event:test",
        ));
    }

    #[test]
    fn read_retry_delay_is_exponential_and_capped() {
        assert_eq!(
            read_retry_delay_for_attempt(Duration::from_secs(1), Duration::from_secs(4), 0,),
            Duration::from_secs(1)
        );
        assert_eq!(
            read_retry_delay_for_attempt(Duration::from_secs(1), Duration::from_secs(4), 1,),
            Duration::from_secs(2)
        );
        assert_eq!(
            read_retry_delay_for_attempt(Duration::from_secs(1), Duration::from_secs(4), 64,),
            Duration::from_secs(4)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sync_restart_wakes_failed_read_immediately_and_invalidates_backoff() {
        let key = room_key();
        let request_id = fake_rid(28_493);
        let (event_tx, mut event_rx) = broadcast::channel(4);
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.event_tx = event_tx;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_with_retry(
            read_network_tx,
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(60),
        );

        manager
            .handle_command(TimelineCommand::SendReadReceipt {
                request_id,
                key,
                event_id: "$event:test".to_owned(),
            })
            .await;
        let responder = async {
            let first = read_network_rx.recv().await.expect("initial request");
            first.response.send(Err(())).expect("fail initial request");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("initial completion"))
            .await;
        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::OperationFailed {
                request_id: failed,
                ..
            }) if failed == request_id
        ));

        manager.wake_all_desired_reads(ReadRetrySource::Reconnect);
        let responder = async {
            let retry = read_network_rx
                .recv()
                .await
                .expect("sync restart must wake desired read without waiting for backoff");
            retry.response.send(Ok(())).expect("restart retry succeeds");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("restart retry completion"))
            .await;
        tokio::time::advance(Duration::from_secs(60)).await;
        while let Some(completion) = manager.read_workers.tasks.next().now_or_never().flatten() {
            manager.handle_read_worker_completion(completion).await;
        }
        assert!(
            read_network_rx.try_recv().is_err(),
            "invalidated backoff must not start a duplicate retry"
        );
        assert!(
            event_rx.try_recv().is_err(),
            "restart retry must not emit a second user terminal"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn room_subscription_checkpoint_wakes_failed_read_immediately() {
        let key = room_key();
        let request_id = fake_rid(28_494);
        let (event_tx, mut event_rx) = broadcast::channel(4);
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.event_tx = event_tx;
        manager.room_subscription_service_epoch = 9;
        let (read_network_tx, mut read_network_rx) = mpsc::unbounded_channel();
        manager.read_workers = ReadWorkerSupervisor::synthetic_with_retry(
            read_network_tx,
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(60),
        );

        manager
            .handle_command(TimelineCommand::SendReadReceipt {
                request_id,
                key: key.clone(),
                event_id: "$event:test".to_owned(),
            })
            .await;
        let responder = async {
            let first = read_network_rx.recv().await.expect("initial request");
            first.response.send(Err(())).expect("fail initial request");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("initial completion"))
            .await;
        assert!(event_rx.try_recv().is_ok());

        manager.wake_desired_reads_for_room(key.room_id(), ReadRetrySource::Checkpoint);
        let responder = async {
            let retry = read_network_rx
                .recv()
                .await
                .expect("checkpoint must wake desired read");
            retry
                .response
                .send(Ok(()))
                .expect("checkpoint retry succeeds");
        };
        let (completion, ()) = tokio::join!(manager.read_workers.tasks.next(), responder);
        manager
            .handle_read_worker_completion(completion.expect("checkpoint retry completion"))
            .await;
        assert!(
            event_rx.try_recv().is_err(),
            "checkpoint retry must not emit a second user terminal"
        );
    }

    #[test]
    fn manager_read_completion_lane_precedes_ordinary_mailbox() {
        let source = include_str!("timeline.rs");
        let manager_run = source
            .split("    async fn run(mut self) {")
            .nth(1)
            .expect("timeline manager run loop")
            .split("    async fn handle_navigation_projection")
            .next()
            .expect("manager run boundary");
        let read_completion = manager_run
            .find("completion = self.read_workers.tasks.next()")
            .expect("manager read completion lane");
        let ordinary_mailbox = manager_run
            .find("msg = self.msg_rx.recv()")
            .expect("manager ordinary mailbox");
        assert!(
            read_completion < ordinary_mailbox,
            "biased manager select must poll read completions before ordinary commands"
        );
    }

    #[test]
    fn replaying_thread_initial_items_preserves_semantic_attention_tracker() {
        let source = include_str!("timeline.rs");
        let replay_helper = source
            .split("fn handle_replay_initial_items")
            .nth(1)
            .expect("replay helper should exist")
            .split("async fn handle_paginate")
            .next()
            .expect("pagination handler should follow replay helper");

        assert!(
            replay_helper.contains("ThreadAttentionObservation::Replay")
                && !replay_helper.contains("ThreadAttentionTracker::default()"),
            "thread replay must absorb history without resetting stable-ID deduplication or unread attention"
        );
    }

    #[test]
    fn timeline_builder_does_not_track_state_event_read_receipts() {
        let source = include_str!("timeline.rs");
        let production = source.split("\nmod tests").next().unwrap_or(source);
        let builder_source = production
            .split("fn koushi_timeline_builder")
            .nth(1)
            .expect("timeline builder should exist")
            .split("struct PreparedRelayRecovery")
            .next()
            .expect("relay recovery structs should follow timeline builder");

        assert!(
            builder_source.contains("TimelineReadReceiptTracking::MessageLikeEvents"),
            "timeline read receipts should only track message-like events; state-event tracking exercises SDK event-cache ordering paths that are not needed by Koushi rows"
        );
        assert!(
            !builder_source.contains("TimelineReadReceiptTracking::AllEvents"),
            "do not restore AllEvents for the product timeline builder"
        );
    }

    #[tokio::test]
    async fn koushi_timeline_builder_projects_sdk_read_receipts() {
        use matrix_sdk::assert_next_with_timeout;
        use matrix_sdk::ruma::{event_id, room_id, user_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::{JoinedRoomBuilder, event_factory::EventFactory};

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room_id = room_id!("!receipts:example.test");
        let room = server.sync_joined_room(&client, room_id).await;
        let timeline = koushi_timeline_builder(
            &room,
            TimelineFocus::Live {
                hide_threaded_events: false,
            },
        )
        .build()
        .await
        .expect("timeline");
        let (_initial_items, mut stream) = timeline.subscribe().await;

        let factory = EventFactory::new().room(room_id);
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id)
                    .add_timeline_event(
                        factory
                            .text_msg("first")
                            .event_id(event_id!("$first:example.test"))
                            .sender(user_id!("@alice:example.test"))
                            .into_raw_sync(),
                    )
                    .add_timeline_event(
                        factory
                            .text_msg("second")
                            .event_id(event_id!("$second:example.test"))
                            .sender(user_id!("@bob:example.test"))
                            .into_raw_sync(),
                    ),
            )
            .await;

        let diffs = assert_next_with_timeout!(stream);
        let mut receipts_by_event = Vec::new();
        for diff in &diffs {
            collect_live_event_receipts_from_diff(diff, &mut receipts_by_event);
        }

        let second = receipts_by_event
            .iter()
            .find(|entry| entry.event_id == "$second:example.test")
            .expect("Koushi timeline builder must opt in to SDK read receipt tracking");
        assert!(
            second
                .receipts
                .iter()
                .any(|receipt| receipt.user_id == "@bob:example.test")
        );
    }

    #[test]
    fn live_receipt_observation_action_builder_is_pure_and_orders_profiles_first() {
        let actions = build_live_receipt_observation_actions(
            "!room:example.test",
            vec![LiveEventReceipts {
                event_id: "$event:example.test".to_owned(),
                receipts: vec![LiveReadReceipt {
                    user_id: "@bob:example.test".to_owned(),
                    display_name: None,
                    original_display_label: String::new(),
                    avatar: None,
                    timestamp_ms: Some(1),
                }],
            }],
            vec![MatrixUserProfile {
                user_id: "@bob:example.test".to_owned(),
                display_name: Some("Bob".to_owned()),
                avatar_mxc_uri: None,
            }],
        );

        assert!(matches!(
            actions.as_slice(),
            [
                AppAction::LiveRoomProfilesObserved { profiles, .. },
                AppAction::UserProfilesUpdated { profiles: cached },
                AppAction::LiveRoomReceiptsUpdated { .. },
            ] if profiles[0].display_label == "Bob"
                && cached[0].display_label == "Bob"
        ));
    }

    #[tokio::test]
    async fn local_receipt_observation_helper_builds_profile_then_receipt_actions() {
        use koushi_state::{AppState, SessionInfo, SessionState, reduce};
        use matrix_sdk::assert_next_with_timeout;
        use matrix_sdk::ruma::{event_id, room_id, user_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::{ALICE, JoinedRoomBuilder, event_factory::EventFactory};

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room_id = room_id!("!receipt-profiles:example.test");
        let bob = user_id!("@bob:example.test");
        let room = server.sync_joined_room(&client, room_id).await;
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id).add_state_event(
                    EventFactory::new()
                        .room(room_id)
                        .member(bob)
                        .display_name("Relevant room member")
                        .into_raw_sync_state(),
                ),
            )
            .await;

        let timeline = koushi_timeline_builder(
            &room,
            TimelineFocus::Live {
                hide_threaded_events: false,
            },
        )
        .build()
        .await
        .expect("timeline");
        let (_initial_items, mut stream) = timeline.subscribe().await;
        let factory = EventFactory::new().room(room_id);
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id)
                    .add_timeline_event(
                        factory
                            .text_msg("receipt source")
                            .event_id(event_id!("$receipt-source:example.test"))
                            .sender(bob)
                            .into_raw_sync(),
                    )
                    .add_timeline_event(
                        factory
                            .text_msg("second receipt source")
                            .event_id(event_id!("$receipt-source-two:example.test"))
                            .sender(bob)
                            .into_raw_sync(),
                    ),
            )
            .await;

        let diffs = assert_next_with_timeout!(stream);
        let mut receipts_by_event = Vec::new();
        for diff in &diffs {
            collect_live_event_receipts_from_diff(diff, &mut receipts_by_event);
        }
        let observed_receipts = receipts_by_event
            .iter()
            .find(|entry| {
                entry
                    .receipts
                    .iter()
                    .any(|receipt| receipt.user_id == bob.as_str())
            })
            .cloned()
            .expect("timeline diff should contain a real receipt for the member");

        let session = MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: "http://example.invalid".to_owned(),
                user_id: ALICE.to_string(),
                device_id: "DEVICE".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        );
        let mut state = AppState {
            session: SessionState::Ready(session.info.clone()),
            ..AppState::default()
        };
        reduce(
            &mut state,
            AppAction::LiveRoomReceiptsUpdated {
                room_id: room_id.to_string(),
                receipts_by_event: vec![observed_receipts.clone()],
            },
        );
        assert_eq!(
            state.live_signals.rooms[room_id.as_str()].receipts_by_event
                [&observed_receipts.event_id]
                .readers[0]
                .display_name
                .as_deref(),
            Some("Unknown user")
        );

        let action_batch = live_receipt_observation_actions_from_sdk_receipts(
            &session,
            room_id.as_str(),
            vec![observed_receipts.clone()],
        )
        .await;
        assert!(matches!(
            action_batch.first(),
            Some(AppAction::LiveRoomProfilesObserved {
                room_id: observed_room_id,
                profiles,
            }) if observed_room_id == room_id.as_str()
                && profiles.iter().any(|profile| {
                    profile.user_id == bob.as_str()
                        && profile.display_name.as_deref() == Some("Relevant room member")
                })
        ));
        assert!(matches!(
            action_batch.last(),
            Some(AppAction::LiveRoomReceiptsUpdated { room_id: observed_room_id, .. })
                if observed_room_id == room_id.as_str()
        ));

        for action in action_batch {
            reduce(&mut state, action);
        }

        assert_eq!(
            state.profile.room_users[room_id.as_str()][bob.as_str()]
                .display_name
                .as_deref(),
            Some("Relevant room member")
        );
        assert_eq!(
            state.profile.users[bob.as_str()].display_name.as_deref(),
            Some("Relevant room member")
        );
        assert_eq!(
            state.live_signals.rooms[room_id.as_str()].receipts_by_event
                [&observed_receipts.event_id]
                .readers[0]
                .display_name
                .as_deref(),
            Some("Relevant room member")
        );
    }

    #[tokio::test]
    async fn production_receipt_diff_delivery_refreshes_unknown_with_room_profile() {
        use koushi_state::{AppState, reduce};
        use matrix_sdk::ruma::{event_id, room_id, user_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::{ALICE, JoinedRoomBuilder, event_factory::EventFactory};

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room_id = room_id!("!receipt-production:example.test");
        let bob = user_id!("@bob:example.test");
        server.sync_joined_room(&client, room_id).await;
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id).add_state_event(
                    EventFactory::new()
                        .room(room_id)
                        .member(bob)
                        .display_name("Relevant room member")
                        .into_raw_sync_state(),
                ),
            )
            .await;

        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: "http://example.invalid".to_owned(),
                user_id: ALICE.to_string(),
                device_id: "DEVICE".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        ));
        let receipts = vec![LiveEventReceipts {
            event_id: event_id!("$receipt-production:example.test").to_string(),
            receipts: vec![LiveReadReceipt {
                user_id: bob.to_string(),
                display_name: None,
                original_display_label: String::new(),
                avatar: None,
                timestamp_ms: Some(1),
            }],
        }];
        let mut state = AppState {
            session: SessionState::Ready(session.info.clone()),
            ..AppState::default()
        };
        reduce(
            &mut state,
            AppAction::LiveRoomReceiptsUpdated {
                room_id: room_id.to_string(),
                receipts_by_event: receipts.clone(),
            },
        );
        state.profile.users.insert(
            bob.to_string(),
            UserProfile {
                user_id: bob.to_string(),
                display_name: Some("Global cache".to_owned()),
                display_label: "Global cache".to_owned(),
                original_display_label: "Global cache".to_owned(),
                mention_search_terms: Vec::new(),
                avatar: None,
            },
        );
        assert_eq!(
            state.live_signals.rooms[room_id.as_str()].receipts_by_event[&receipts[0].event_id]
                .readers[0]
                .display_name
                .as_deref(),
            Some("Unknown user"),
            "the production batch must refresh an already-projected Unknown receipt"
        );

        let key = TimelineKey::room(AccountKey(ALICE.to_string()), room_id.to_string());
        let generations = Arc::new(TimelineActorGenerationGate::default());
        let actor_generation = generations.activate_after_quiescence(&key).await.generation;
        let (action_tx, mut action_rx) = mpsc::channel(1);
        assert!(
            emit_live_receipt_observation_actions(
                session.as_ref(),
                &action_tx,
                &generations,
                &key,
                actor_generation,
                room_id.as_str(),
                receipts.clone(),
            )
            .await
        );
        let action_batch = action_rx.recv().await.expect("receipt action batch");
        assert!(matches!(
            action_batch.as_slice(),
            [
                AppAction::LiveRoomProfilesObserved { profiles, .. },
                AppAction::UserProfilesUpdated { profiles: cached },
                AppAction::LiveRoomReceiptsUpdated { .. },
            ] if profiles.iter().any(|profile| {
                profile.user_id == bob.as_str()
                    && profile.display_name.as_deref() == Some("Relevant room member")
            }) && cached.iter().any(|profile| {
                profile.user_id == bob.as_str()
                    && profile.display_name.as_deref() == Some("Relevant room member")
            })
        ));

        for action in action_batch {
            reduce(&mut state, action);
        }
        assert_eq!(
            state.live_signals.rooms[room_id.as_str()].receipts_by_event[&receipts[0].event_id]
                .readers[0]
                .display_name
                .as_deref(),
            Some("Relevant room member"),
            "the relevant room profile must beat the global cache"
        );
    }

    #[tokio::test]
    async fn production_receipt_diff_delivery_uses_global_cache_when_local_lookup_misses() {
        use koushi_state::{AppState, reduce};
        use matrix_sdk::ruma::{event_id, room_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::ALICE;

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room_id = room_id!("!receipt-cache-fallback:example.test");
        server.sync_joined_room(&client, room_id).await;
        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: "http://example.invalid".to_owned(),
                user_id: ALICE.to_string(),
                device_id: "DEVICE".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        ));
        let bob = "@bob:example.test";
        let receipts = vec![LiveEventReceipts {
            event_id: event_id!("$receipt-cache-fallback:example.test").to_string(),
            receipts: vec![LiveReadReceipt {
                user_id: bob.to_owned(),
                display_name: None,
                original_display_label: String::new(),
                avatar: None,
                timestamp_ms: Some(2),
            }],
        }];
        let mut state = AppState {
            session: SessionState::Ready(session.info.clone()),
            ..AppState::default()
        };
        state.profile.users.insert(
            bob.to_owned(),
            UserProfile {
                user_id: bob.to_owned(),
                display_name: Some("Global cache".to_owned()),
                display_label: "Global cache".to_owned(),
                original_display_label: "Global cache".to_owned(),
                mention_search_terms: Vec::new(),
                avatar: None,
            },
        );

        let key = TimelineKey::room(AccountKey(ALICE.to_string()), room_id.to_string());
        let generations = Arc::new(TimelineActorGenerationGate::default());
        let actor_generation = generations.activate_after_quiescence(&key).await.generation;
        let (action_tx, mut action_rx) = mpsc::channel(1);
        assert!(
            emit_live_receipt_observation_actions(
                session.as_ref(),
                &action_tx,
                &generations,
                &key,
                actor_generation,
                room_id.as_str(),
                receipts.clone(),
            )
            .await
        );
        let action_batch = action_rx.recv().await.expect("receipt fallback batch");
        assert!(matches!(
            action_batch.as_slice(),
            [AppAction::LiveRoomReceiptsUpdated { .. }]
        ));
        for action in action_batch {
            reduce(&mut state, action);
        }
        assert_eq!(
            state.live_signals.rooms[room_id.as_str()].receipts_by_event[&receipts[0].event_id]
                .readers[0]
                .display_name
                .as_deref(),
            Some("Global cache")
        );
    }

    #[tokio::test]
    async fn production_receipt_diff_delivery_sends_receipts_when_local_lookup_fails() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        use koushi_state::SessionAuthenticationMethod;
        use matrix_sdk::ruma::event_id;
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::ALICE;

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: "http://example.invalid".to_owned(),
                user_id: ALICE.to_string(),
                device_id: "DEVICE".to_owned(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            },
        ));
        let receipts = vec![LiveEventReceipts {
            event_id: event_id!("$receipt-lookup-failure:example.test").to_string(),
            receipts: vec![LiveReadReceipt {
                user_id: "@bob:example.test".to_owned(),
                display_name: None,
                original_display_label: String::new(),
                avatar: None,
                timestamp_ms: Some(3),
            }],
        }];
        let key = TimelineKey::room(
            AccountKey(ALICE.to_string()),
            "!receipt-failure:example.test",
        );
        let generations = Arc::new(TimelineActorGenerationGate::default());
        let actor_generation = generations.activate_after_quiescence(&key).await.generation;
        let (action_tx, mut action_rx) = mpsc::channel(1);
        let records_before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        assert!(
            emit_live_receipt_observation_actions(
                session.as_ref(),
                &action_tx,
                &generations,
                &key,
                actor_generation,
                "not-a-room-id",
                receipts,
            )
            .await
        );
        let action_batch = action_rx.recv().await.expect("failed lookup receipt batch");
        assert!(matches!(
            action_batch.as_slice(),
            [AppAction::LiveRoomReceiptsUpdated { .. }]
        ));
        assert!(
            koushi_diagnostics::test_support::detail_snapshot()
                .records
                .iter()
                .skip(records_before)
                .any(|record| {
                    record.event.source == "core.read_receipt_profile"
                        && record.event.stage == "local_lookup"
                        && record.event.fields.iter().any(|field| {
                            field.key == "lookup_outcome"
                                && field.value == DiagnosticValue::Token("failed")
                        })
                }),
            "lookup failures must record a sanitized outcome"
        );
    }

    #[tokio::test]
    async fn stale_production_receipt_diff_result_is_discarded_after_generation_replacement() {
        use koushi_state::SessionAuthenticationMethod;
        use matrix_sdk::ruma::event_id;
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::ALICE;

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: "http://example.invalid".to_owned(),
                user_id: ALICE.to_string(),
                device_id: "DEVICE".to_owned(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            },
        ));
        let receipts = vec![LiveEventReceipts {
            event_id: event_id!("$receipt-stale:example.test").to_string(),
            receipts: vec![LiveReadReceipt {
                user_id: "@bob:example.test".to_owned(),
                display_name: None,
                original_display_label: String::new(),
                avatar: None,
                timestamp_ms: Some(4),
            }],
        }];
        let key = TimelineKey::room(AccountKey(ALICE.to_string()), "!receipt-stale:example.test");
        let generations = Arc::new(TimelineActorGenerationGate::default());
        let stale_generation = generations.activate_after_quiescence(&key).await.generation;
        let (action_tx, mut action_rx) = mpsc::channel(1);
        action_tx
            .send(vec![AppAction::TypingUsersUpdated {
                room_id: "!occupied:example.test".to_owned(),
                user_ids: Vec::new(),
            }])
            .await
            .expect("fill action channel");

        let delivery = tokio::spawn({
            let session = Arc::clone(&session);
            let action_tx = action_tx.clone();
            let generations = Arc::clone(&generations);
            let key = key.clone();
            async move {
                emit_live_receipt_observation_actions(
                    session.as_ref(),
                    &action_tx,
                    &generations,
                    &key,
                    stale_generation,
                    "not-a-room-id",
                    receipts,
                )
                .await
            }
        });
        tokio::task::yield_now().await;
        let replacement_generation = generations.activate_after_quiescence(&key).await.generation;
        assert_ne!(replacement_generation, stale_generation);
        assert!(matches!(
            action_rx.recv().await,
            Some(actions) if matches!(
                actions.as_slice(),
                [AppAction::TypingUsersUpdated { room_id, .. }] if room_id == "!occupied:example.test"
            )
        ));
        assert!(!delivery.await.expect("stale delivery task"));
        assert!(
            action_rx.try_recv().is_err(),
            "a stale actor generation must not publish the receipt batch"
        );
    }

    #[test]
    fn production_receipt_diff_path_uses_fenced_ordered_observation_delivery() {
        let source = include_str!("timeline.rs");
        let production = source.split("\nmod tests").next().unwrap_or(source);
        let diff_handler = production
            .split("async fn handle_diff_batch(")
            .nth(1)
            .expect("TimelineActor diff handler exists")
            .split("/// Detect Room thread replies")
            .next()
            .expect("diff handler ends before thread hydration");
        assert!(
            diff_handler.contains("emit_live_receipt_observation_actions"),
            "receipt diffs must use the production profile-observation delivery path"
        );
        let delivery = production
            .split("async fn emit_receipt_observation_actions(")
            .nth(1)
            .expect("production receipt delivery helper exists");
        assert!(
            delivery.contains("send_generation_fenced"),
            "receipt profile actions must use the actor-generation fence"
        );
        assert!(
            !diff_handler.contains("try_send(vec![action])"),
            "receipt action batches must not be dropped through try_send"
        );
    }

    #[test]
    fn initial_receipts_use_the_ordered_local_profile_observation_batch() {
        let source = include_str!("timeline.rs");
        let startup = source
            .split("let initial_receipts = live_event_receipts_from_sdk_items")
            .nth(1)
            .expect("initial receipt projection exists")
            .split("let thread_attention = ThreadAttentionTracker::hydrate")
            .next()
            .expect("initial receipt publication precedes thread attention hydration");

        assert!(
            startup.contains("emit_receipt_observation_actions"),
            "initial receipts must use local profile observation and generation fencing"
        );
        assert!(
            !startup.contains("LiveRoomReceiptsUpdated {"),
            "initial receipts must not bypass the ordered profile/receipt batch"
        );
        assert!(
            !startup.contains("try_send(actions)"),
            "initial receipt publication must be reliable"
        );
    }

    #[test]
    fn authoritative_recovery_receipts_use_the_same_ordered_observation_batch() {
        let source = include_str!("timeline.rs");
        let recovery = source
            .split("async fn handle_relay_overflow")
            .nth(1)
            .expect("authoritative recovery handler exists")
            .split("// ---------------------------------------------------------------------------\n// Relay task")
            .next()
            .expect("authoritative recovery handler boundary exists");

        assert!(
            recovery.contains("emit_receipt_observation_actions"),
            "authoritative recovery must use local profile observation and generation fencing"
        );
        assert!(
            !recovery.contains("if let Some(action) = receipts_action"),
            "authoritative recovery must not publish an unobserved receipt action directly"
        );
    }

