//! Room-scoped typing-notice lane (issue #1481).
//!
//! `Room::typing_notice` performs outbound HTTP in the vendored SDK. Awaiting it
//! in a `TimelineActor` mailbox handler stalled that actor's pending-send
//! publication and diff batches behind an unrelated request, so a
//! session/manager-owned lane owns it instead. The lane is keyed by room id, so
//! the room, thread, and focused views of one room share one ordered sequence.
//!
//! The manager polls [`TypingLane::tasks`] alongside its other worker sets. One
//! task per room is in flight; further admissions wait in a bounded FIFO. The
//! SDK's own throttle and HTTP retry stay authoritative, so the lane is only an
//! owner and an ordering fence, not a second retry state machine.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::stream::FuturesUnordered;
use koushi_sdk::MatrixClientSession;
#[cfg(test)]
use tokio::sync::{mpsc, oneshot};

use crate::executor;
use koushi_protocol::failure::TimelineFailureKind;
use koushi_protocol::ids::{RequestId, TimelineKey};

use super::diagnostics::trace_typing_operation;

/// Bounded number of admitted-but-not-yet-submitted typing requests per room.
/// React sends one command per typing-state transition, not per keystroke, so
/// this only rejects a pathological backlog.
pub(super) const TYPING_LANE_QUEUE_CAPACITY: usize = 8;

/// One absolute deadline per typing HTTP request.
pub(super) const TYPING_LANE_TIMEOUT: Duration = Duration::from_secs(30);

/// Session-wide bound on rooms with unsettled typing requests. Room entries are
/// removed as soon as a room goes idle, so this only bounds how many distinct
/// rooms can have in-flight or queued notices at once (engineering rules
/// "Async and Runtime" 2: no unbounded map of owned tasks).
pub(super) const TYPING_LANE_MAX_ROOMS: usize = 64;

#[derive(Clone)]
enum TypingNetworkContext {
    Matrix(Arc<MatrixClientSession>),
    #[cfg(test)]
    Synthetic {
        requests: mpsc::UnboundedSender<SyntheticTypingRequest>,
    },
}

#[cfg(test)]
pub(super) struct SyntheticTypingRequest {
    pub(super) is_typing: bool,
    pub(super) response: oneshot::Sender<bool>,
}

#[derive(Clone)]
pub(super) struct TypingRequest {
    correlation: u64,
    request_id: RequestId,
    key: TimelineKey,
    room_id: String,
    is_typing: bool,
    admitted_at: Instant,
}

/// The terminal for one admitted typing request. Every admitted request produces
/// exactly one of these, so the owner can always emit one correlated result.
pub(super) enum TypingLaneSettlement {
    Succeeded(TypingRequest),
    Failed {
        request: TypingRequest,
        kind: TimelineFailureKind,
    },
}

impl TypingLaneSettlement {
    /// `(request_id, key, is_typing, failure)`: `failure` is `Some` for a
    /// correlated `OperationFailed` and `None` for a `TypingSet` success.
    pub(super) fn into_parts(self) -> (RequestId, TimelineKey, bool, Option<TimelineFailureKind>) {
        match self {
            Self::Succeeded(request) => (request.request_id, request.key, request.is_typing, None),
            Self::Failed { request, kind } => (
                request.request_id,
                request.key,
                request.is_typing,
                Some(kind),
            ),
        }
    }
}

#[derive(Clone)]
pub(super) enum TypingOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone)]
pub(super) struct TypingCompletion {
    correlation: u64,
    request: TypingRequest,
    submitted_at: Instant,
    outcome: TypingOutcome,
}

type TypingFuture = Pin<Box<dyn Future<Output = TypingCompletion> + Send + 'static>>;

#[derive(Default)]
struct RoomTypingState {
    active: Option<TypingRequest>,
    queue: VecDeque<TypingRequest>,
}

/// Session/manager-owned, room-scoped typing lane. Ownership and cancellation
/// live with its only poller, `TimelineManagerActor`.
pub(super) struct TypingLane {
    network: Option<TypingNetworkContext>,
    /// Account the owning session may type as. A key for another account is
    /// rejected instead of typing on this session.
    account_key: Option<String>,
    timeout: Duration,
    pub(super) tasks: FuturesUnordered<TypingFuture>,
    rooms: HashMap<String, RoomTypingState>,
    next_correlation: u64,
}

impl TypingLane {
    pub(super) fn unavailable() -> Self {
        Self::new(None, None, TYPING_LANE_TIMEOUT)
    }

    pub(super) fn matrix(session: Arc<MatrixClientSession>) -> Self {
        let account_key = session
            .client()
            .user_id()
            .map(|user_id| user_id.as_str().to_owned());
        Self::new(
            Some(TypingNetworkContext::Matrix(session)),
            account_key,
            TYPING_LANE_TIMEOUT,
        )
    }

    #[cfg(test)]
    pub(super) fn synthetic(
        requests: mpsc::UnboundedSender<SyntheticTypingRequest>,
        account_key: String,
    ) -> Self {
        Self::new(
            Some(TypingNetworkContext::Synthetic { requests }),
            Some(account_key),
            TYPING_LANE_TIMEOUT,
        )
    }

    fn new(
        network: Option<TypingNetworkContext>,
        account_key: Option<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            network,
            account_key,
            timeout,
            tasks: FuturesUnordered::new(),
            rooms: HashMap::new(),
            next_correlation: 0,
        }
    }

    /// Admit one room-scoped typing command. `Err` is an explicit, correlated
    /// rejection: the lane has no session, the key belongs to another account,
    /// or that room's bounded FIFO (or the session-wide room bound) is full.
    pub(super) fn admit(
        &mut self,
        request_id: RequestId,
        key: TimelineKey,
        is_typing: bool,
    ) -> Result<(), TimelineFailureKind> {
        let Some(network_account) = self.account_key.as_deref() else {
            return Err(TimelineFailureKind::Sdk);
        };
        if key.account_key.0 != network_account {
            return Err(TimelineFailureKind::NotSubscribed);
        }
        let room_id = key.room_id().to_owned();
        if !self.rooms.contains_key(&room_id) && self.rooms.len() >= TYPING_LANE_MAX_ROOMS {
            trace_typing_operation("admit", request_id, 0, 0, "rejected", 0, false);
            return Err(TimelineFailureKind::QueueOverflow);
        }
        let request = TypingRequest {
            correlation: self.allocate_correlation(),
            request_id,
            key,
            room_id: room_id.clone(),
            is_typing,
            admitted_at: Instant::now(),
        };
        let mut spawn = None;
        let mut rejected = false;
        let queue_depth;
        {
            let state = self.rooms.entry(room_id.clone()).or_default();
            if state.active.is_none() {
                state.active = Some(request.clone());
                spawn = Some(request);
                queue_depth = 0;
            } else if state.queue.len() < TYPING_LANE_QUEUE_CAPACITY {
                state.queue.push_back(request.clone());
                queue_depth = state.queue.len() as u64;
            } else {
                queue_depth = state.queue.len() as u64;
                rejected = true;
            }
        }
        if rejected {
            trace_typing_operation("admit", request_id, 0, 0, "rejected", queue_depth, false);
            return Err(TimelineFailureKind::QueueOverflow);
        }
        trace_typing_operation("admit", request_id, 0, 0, "accepted", queue_depth, true);
        if let Some(request) = spawn {
            self.spawn(request);
        }
        Ok(())
    }

    /// Apply one polled completion. Only the room's exact active request
    /// settles; a duplicate or retired completion is inert. The next queued
    /// request, if any, is submitted before returning.
    pub(super) fn settle(&mut self, completion: TypingCompletion) -> Vec<TypingLaneSettlement> {
        let TypingCompletion {
            correlation,
            request,
            submitted_at,
            outcome,
        } = completion;
        let mut settlements = Vec::new();
        let mut next = None;
        let matched = self.rooms.get_mut(&request.room_id).is_some_and(|state| {
            let is_active = state
                .active
                .as_ref()
                .is_some_and(|active| active.correlation == correlation);
            if is_active {
                next = state.queue.pop_front();
                state.active = next.clone();
            }
            is_active
        });
        if !matched {
            return settlements;
        }
        if self
            .rooms
            .get(&request.room_id)
            .is_some_and(|state| state.active.is_none() && state.queue.is_empty())
        {
            self.rooms.remove(&request.room_id);
        }
        let duration_ms = submitted_at.elapsed().as_millis();
        match outcome {
            TypingOutcome::Succeeded => {
                trace_typing_operation(
                    "complete",
                    request.request_id,
                    0,
                    duration_ms,
                    "succeeded",
                    0,
                    true,
                );
                settlements.push(TypingLaneSettlement::Succeeded(request));
            }
            TypingOutcome::Failed => {
                trace_typing_operation(
                    "complete",
                    request.request_id,
                    0,
                    duration_ms,
                    "failed",
                    0,
                    false,
                );
                settlements.push(TypingLaneSettlement::Failed {
                    request,
                    kind: TimelineFailureKind::Sdk,
                });
            }
        }
        if let Some(next) = next {
            self.spawn(next);
        }
        settlements
    }

    /// Cancel every in-flight task and settle every admitted request. Used by
    /// the owner's ordered shutdown and its fail-safe drop, so no admitted
    /// request is left without a terminal.
    pub(super) fn settle_all(&mut self) -> Vec<TypingLaneSettlement> {
        self.tasks.clear();
        let mut settlements = Vec::new();
        for (_, state) in self.rooms.drain() {
            let mut fail = |request: TypingRequest| {
                trace_typing_operation("complete", request.request_id, 0, 0, "cancelled", 0, false);
                settlements.push(TypingLaneSettlement::Failed {
                    request,
                    kind: TimelineFailureKind::Sdk,
                });
            };
            if let Some(active) = state.active {
                fail(active);
            }
            for request in state.queue {
                fail(request);
            }
        }
        settlements
    }

    fn allocate_correlation(&mut self) -> u64 {
        let next = self.next_correlation.wrapping_add(1).max(1);
        self.next_correlation = next;
        next
    }

    fn spawn(&mut self, request: TypingRequest) {
        let Some(network) = self.network.clone() else {
            return;
        };
        let timeout = self.timeout;
        let correlation = request.correlation;
        trace_typing_operation(
            "submit",
            request.request_id,
            request.admitted_at.elapsed().as_millis(),
            0,
            "submitted",
            0,
            true,
        );
        let room_id = request.room_id.clone();
        let is_typing = request.is_typing;
        self.tasks.push(Box::pin(async move {
            let submitted_at = Instant::now();
            let outcome =
                executor::timeout(timeout, perform_typing_notice(network, room_id, is_typing))
                    .await;
            TypingCompletion {
                correlation,
                request,
                submitted_at,
                outcome: match outcome {
                    Ok(Ok(())) => TypingOutcome::Succeeded,
                    Ok(Err(())) | Err(_) => TypingOutcome::Failed,
                },
            }
        }));
    }

    #[cfg(test)]
    pub(super) fn is_idle(&self) -> bool {
        self.rooms.is_empty() && self.tasks.is_empty()
    }
}

#[cfg(test)]
mod tests;

async fn perform_typing_notice(
    network: TypingNetworkContext,
    room_id: String,
    is_typing: bool,
) -> Result<(), ()> {
    match network {
        TypingNetworkContext::Matrix(session) => {
            let room_id = matrix_sdk::ruma::RoomId::parse(room_id.as_str()).map_err(|_| ())?;
            let room = session.client().get_room(&room_id).ok_or(())?;
            room.typing_notice(is_typing).await.map_err(|_| ())
        }
        #[cfg(test)]
        TypingNetworkContext::Synthetic { requests } => {
            let (response, outcome) = oneshot::channel();
            requests
                .send(SyntheticTypingRequest {
                    is_typing,
                    response,
                })
                .map_err(|_| ())?;
            match outcome.await {
                Ok(true) => Ok(()),
                _ => Err(()),
            }
        }
    }
}
