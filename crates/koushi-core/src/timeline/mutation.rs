//! Bounded session-owned mutation execution, independent of presentation and
//! manager mailbox backpressure (#1482). Active SDK futures are cancellation
//! sensitive: client expiry settles the request but never abandons SDK rollback.
use crate::executor;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_MUTATION_CORRELATION: AtomicU64 = AtomicU64::new(1);
use futures_util::{StreamExt, stream::FuturesUnordered};
use koushi_protocol::{
    event::CoreEvent,
    failure::{CoreFailure, TimelineFailureKind},
    ids::{RequestId, TimelineKey},
};
use koushi_state::{ComposerDocument, ComposerFormattingOptions};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, watch};
mod context;
pub(super) use context::{MutationContext, SentEventTransactions};

pub(super) const MUTATION_CAPACITY: usize = 64;
pub(super) const MUTATION_QUEUE_CAPACITY: usize = 8;
pub(super) const MUTATION_DEADLINE: Duration = Duration::from_secs(30);

type MutationFuture =
    Pin<Box<dyn Future<Output = (Target, u64, Result<(), TimelineFailureKind>)> + Send>>;
#[derive(Clone, Hash, PartialEq, Eq)]
struct Target {
    key: TimelineKey,
    generation: u64,
    event_id: String,
}

pub(super) enum Mutation {
    Edit {
        event_id: String,
        document: ComposerDocument,
        formatting: ComposerFormattingOptions,
    },
    Redact {
        event_id: String,
    },
    AddReaction {
        event_id: String,
        reaction_key: String,
    },
    RemoveReaction {
        event_id: String,
        reaction_key: String,
        reaction_event_id: String,
    },
    #[cfg(test)]
    Synthetic {
        event_id: String,
        future: Pin<Box<dyn Future<Output = Result<(), TimelineFailureKind>> + Send>>,
    },
}
impl Mutation {
    fn kind(&self) -> &'static str {
        match self {
            Self::Edit { .. } => "edit_text",
            Self::Redact { .. } => "redact",
            Self::AddReaction { .. } => "send_reaction",
            Self::RemoveReaction { .. } => "redact_reaction",
            #[cfg(test)]
            Self::Synthetic { .. } => "synthetic",
        }
    }
    fn event_id(&self) -> &str {
        match self {
            Self::Edit { event_id, .. }
            | Self::Redact { event_id }
            | Self::AddReaction { event_id, .. }
            | Self::RemoveReaction { event_id, .. } => event_id,
            #[cfg(test)]
            Self::Synthetic { event_id, .. } => event_id,
        }
    }
}

/// Sole result owner. Ingress, queues and active execution all retain this guard.
struct Request {
    request_id: RequestId,
    correlation: u64,
    kind: &'static str,
    admitted_at: tokio::time::Instant,
    context: MutationContext,
    mutation: Option<Mutation>,
    deadline: tokio::time::Instant,
    events: broadcast::Sender<CoreEvent>,
    settled: bool,
    _permit: OwnedSemaphorePermit,
}
impl Request {
    fn trace(&self, stage: &'static str, outcome: &'static str) {
        record(
            DiagnosticEvent::new(DiagnosticLevel::Info, "core.timeline_mutation", stage)
                .field(DiagnosticField::correlation(
                    "correlation",
                    self.correlation,
                ))
                .field(DiagnosticField::token("kind", self.kind))
                .field(DiagnosticField::token("outcome", outcome))
                .field(DiagnosticField::count(
                    "elapsed_ms",
                    self.admitted_at
                        .elapsed()
                        .as_millis()
                        .try_into()
                        .unwrap_or(u64::MAX),
                )),
        );
    }
    fn target(&self) -> Target {
        Target {
            key: self.context.key.clone(),
            generation: self.context.generation,
            event_id: self
                .mutation
                .as_ref()
                .expect("not started")
                .event_id()
                .to_owned(),
        }
    }
    fn settle(&mut self, result: Result<(), TimelineFailureKind>) {
        if self.settled {
            return;
        }
        self.settled = true;
        self.trace("settled", if result.is_ok() { "success" } else { "failed" });
        if let Err(kind) = result {
            let _ = self.events.send(CoreEvent::OperationFailed {
                request_id: self.request_id,
                failure: CoreFailure::TimelineOperationFailed { kind },
            });
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.settle(Err(TimelineFailureKind::Sdk));
    }
}
struct TargetState {
    active: Request,
    queue: VecDeque<Request>,
}
struct Owner {
    tx: mpsc::Sender<Request>,
    stopping: watch::Sender<bool>,
    task: executor::JoinHandle<()>,
}

pub(super) struct MutationLane {
    owner: Option<Owner>,
    slots: Arc<Semaphore>,
    deadline: Duration,
}
impl Default for MutationLane {
    fn default() -> Self {
        Self {
            owner: None,
            slots: Arc::new(Semaphore::new(MUTATION_CAPACITY)),
            deadline: MUTATION_DEADLINE,
        }
    }
}
impl Drop for MutationLane {
    fn drop(&mut self) {
        if let Some(owner) = &self.owner {
            owner.task.abort();
        }
    }
}
impl MutationLane {
    #[cfg(test)]
    pub(super) fn with_deadline(deadline: Duration) -> Self {
        let mut lane = Self::default();
        lane.deadline = deadline;
        lane
    }

    pub(super) fn admit(
        &mut self,
        context: MutationContext,
        request_id: RequestId,
        mutation: Mutation,
        events: broadcast::Sender<CoreEvent>,
    ) {
        let permit = match self.slots.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let _ = events.send(CoreEvent::OperationFailed {
                    request_id,
                    failure: CoreFailure::TimelineOperationFailed {
                        kind: TimelineFailureKind::QueueOverflow,
                    },
                });
                return;
            }
        };
        let mut request = Request {
            request_id,
            correlation: NEXT_MUTATION_CORRELATION.fetch_add(1, Ordering::Relaxed),
            kind: mutation.kind(),
            admitted_at: tokio::time::Instant::now(),
            context,
            mutation: Some(mutation),
            deadline: tokio::time::Instant::now() + self.deadline,
            events,
            settled: false,
            _permit: permit,
        };
        if !request.context.is_current() {
            request.settle(Err(TimelineFailureKind::NotSubscribed));
            return;
        }
        request.trace("admitted", "accepted");
        let owner = self.owner.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel(MUTATION_CAPACITY);
            let (stopping, stop) = watch::channel(false);
            Owner {
                tx,
                stopping,
                task: executor::spawn(run(rx, stop)),
            }
        });
        if let Err(error) = owner.tx.try_send(request) {
            error
                .into_inner()
                .settle(Err(TimelineFailureKind::QueueOverflow));
        }
    }
    pub(super) fn begin_shutdown(&mut self) {
        self.slots.close();
        if let Some(owner) = &self.owner {
            owner.stopping.send_replace(true);
        }
    }
    /// Called only after presentation actors have been retired. Abandoning an
    /// optimistic SDK controller is safe only when no presentation can reuse it.
    pub(super) async fn stop(&mut self) {
        self.begin_shutdown();
        if let Some(owner) = self.owner.take() {
            owner.task.abort();
            let _ = owner.task.await;
        }
    }
}
fn start(request: &mut Request, target: Target, tasks: &FuturesUnordered<MutationFuture>) {
    request.trace("started", "running");
    let context = request.context.clone();
    let mutation = request
        .mutation
        .take()
        .expect("one execution per accepted operation");
    let request_id = request.request_id;
    let correlation = request.correlation;
    tasks.push(Box::pin(async move {
        let result = Box::pin(context.execute(request_id, mutation)).await;
        (target, correlation, result)
    }));
}
fn next_deadline(states: &HashMap<Target, TargetState>) -> tokio::time::Instant {
    states
        .values()
        .flat_map(|state| std::iter::once(&state.active).chain(state.queue.iter()))
        .filter(|request| !request.settled)
        .map(|request| request.deadline)
        .min()
        .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400))
}
fn expire(states: &mut HashMap<Target, TargetState>) {
    let now = tokio::time::Instant::now();
    for state in states.values_mut() {
        if state.active.deadline <= now {
            state.active.settle(Err(TimelineFailureKind::Sdk));
        }
        state.queue.retain_mut(|request| {
            if request.deadline <= now {
                request.settle(Err(TimelineFailureKind::Sdk));
                false
            } else if !request.context.is_current() {
                request.settle(Err(TimelineFailureKind::NotSubscribed));
                false
            } else {
                true
            }
        });
    }
}
async fn run(mut ingress: mpsc::Receiver<Request>, mut stop: watch::Receiver<bool>) {
    let tasks: FuturesUnordered<MutationFuture> = FuturesUnordered::new();
    let mut tasks = tasks;
    let mut states: HashMap<Target, TargetState> = HashMap::new();
    let mut stopping = false;
    loop {
        let deadline = next_deadline(&states);
        tokio::select! {
            // Unbiased: sustained admissions cannot starve execution or expiry.
            changed = stop.changed(), if !stopping => {
                if changed.is_err() || *stop.borrow_and_update() {
                    stopping = true; ingress.close();
                    while let Ok(mut request) = ingress.try_recv() { request.settle(Err(TimelineFailureKind::Sdk)); }
                    for state in states.values_mut() {
                        state.queue.clear();
                        state.active.settle(Err(TimelineFailureKind::Sdk));
                    }
                }
            }
            request = ingress.recv(), if !stopping => {
                let Some(mut request) = request else { break; };
                let target = request.target();
                if !request.context.is_current() { request.settle(Err(TimelineFailureKind::NotSubscribed)); continue; }
                if request.deadline <= tokio::time::Instant::now() { request.settle(Err(TimelineFailureKind::Sdk)); continue; }
                if let Some(state) = states.get_mut(&target) {
                    if state.queue.len() >= MUTATION_QUEUE_CAPACITY { request.settle(Err(TimelineFailureKind::QueueOverflow)); }
                    else { state.queue.push_back(request); }
                } else {
                    start(&mut request, target.clone(), &tasks);
                    states.insert(target, TargetState { active: request, queue: VecDeque::new() });
                }
            }
            completion = tasks.next(), if !tasks.is_empty() => {
                let Some((target, correlation, result)) = completion else { continue; };
                if states.get(&target).is_none_or(|state| state.active.correlation != correlation) { continue; }
                let Some(mut state) = states.remove(&target) else { continue; };
                state.active.trace("execution_finished", if result.is_ok() { "success" } else { "failed" });
                if state.active.deadline <= tokio::time::Instant::now() {
                    state.active.settle(Err(TimelineFailureKind::Sdk));
                } else {
                    state.active.settle(result);
                }
                drop(state.active);
                while let Some(mut next) = state.queue.pop_front() {
                    if next.deadline <= tokio::time::Instant::now() { next.settle(Err(TimelineFailureKind::Sdk)); continue; }
                    if !next.context.is_current() { next.settle(Err(TimelineFailureKind::NotSubscribed)); continue; }
                    start(&mut next, target.clone(), &tasks);
                    states.insert(target, TargetState { active: next, queue: state.queue });
                    break;
                }
            }
            _ = tokio::time::sleep_until(deadline), if !states.is_empty() => { expire(&mut states); }
        }
        if stopping && tasks.is_empty() {
            break;
        }
    }
}
