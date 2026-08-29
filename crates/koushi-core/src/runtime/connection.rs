use super::{CoreCommandEnvelope, CoreRuntime};
use crate::command::{CoreCommand, RoomCommand};
use crate::composer_draft_lifecycle::{
    ComposerDraftCommandPermit, ComposerDraftLeaseFailure, ComposerDraftLeaseId,
    ComposerDraftLeaseRegistry, ComposerDraftScope, ComposerRendererGeneration,
};
use crate::event::{
    AppStateSnapshot, CoreEvent, IntentNoOpReason, IntentOutcome, VersionedAppStateSnapshot,
    project_room_event_display_labels, project_timeline_event_display_labels,
};
use crate::ids::{RequestId, RuntimeConnectionId};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CommandSubmitError {
    #[error("core runtime is closed")]
    RuntimeClosed,
    #[error("request id does not belong to this connection")]
    InvalidRequestId,
    #[error("composer draft command requires lease admission")]
    ComposerLeaseRequired,
    #[error("command does not carry a composer draft revision")]
    ComposerLeaseNotRequired,
    #[error("composer draft lease admission failed")]
    ComposerLease(ComposerDraftLeaseFailure),
}

/// Typed terminal failures returned by [`CoreConnection::select_room_and_wait`].
///
/// A matching `Committed` or benign no-op lifecycle event is only progress;
/// selection succeeds once the requested room is visible in the latest versioned
/// watch snapshot. Other requests and lagged broadcast events are ignored or
/// recovered from that snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SelectRoomError {
    #[error("room selection command could not be submitted: {0}")]
    CommandSubmit(#[source] CommandSubmitError),
    #[error("room selection requires a ready session")]
    SessionNotReady,
    #[error("room is not present in the current state")]
    RoomNotInState,
    #[error("room selection failed without a state change: {0:?}")]
    FailedNoOp(IntentNoOpReason),
    #[error("room selection operation failed: {0:?}")]
    OperationFailed(crate::failure::CoreFailure),
    #[error("core event stream closed")]
    EventStreamClosed,
    #[error("room selection timed out")]
    Timeout,
}

/// Surfaced when a consumer fell behind the bounded event queue. The
/// consumer must resync from the latest snapshot and (in later phases) the
/// per-timeline resync events; intermediate discrete events were dropped
/// for this consumer only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventStreamLag {
    pub skipped: u64,
}

/// One attached consumer: allocates request ids, submits commands, and
/// observes the shared event stream plus the latest snapshot.
pub struct CoreConnection {
    connection_id: RuntimeConnectionId,
    command_tx: mpsc::Sender<CoreCommandEnvelope>,
    composer_draft_leases: Arc<ComposerDraftLeaseRegistry>,
    event_rx: broadcast::Receiver<CoreEvent>,
    snapshot_rx: watch::Receiver<VersionedAppStateSnapshot>,
    next_sequence: AtomicU64,
}

/// Lightweight command submitter that can be cloned without cloning event or
/// snapshot receivers.
#[derive(Clone)]
pub struct CoreCommandHandle {
    connection_id: RuntimeConnectionId,
    command_tx: mpsc::Sender<CoreCommandEnvelope>,
    composer_draft_leases: Arc<ComposerDraftLeaseRegistry>,
}

impl CoreRuntime {
    /// Attach a consumer. Returns its connection handle; the handle's
    /// `RuntimeConnectionId` is the only id its commands may carry.
    pub fn attach(&self) -> CoreConnection {
        CoreConnection {
            connection_id: RuntimeConnectionId(
                self.next_connection_id.fetch_add(1, Ordering::Relaxed),
            ),
            command_tx: self.command_tx.clone(),
            composer_draft_leases: Arc::clone(&self.composer_draft_leases),
            event_rx: self.event_tx.subscribe(),
            snapshot_rx: self.snapshot_rx.clone(),
            next_sequence: AtomicU64::new(1),
        }
    }
}

impl CoreCommandHandle {
    /// Submit a command without a composer lease. Fails locally — before
    /// routing and before any `CoreEvent` is published — if the request id
    /// belongs to another connection or the command carries a composer
    /// revision and therefore requires [`Self::command_with_composer_lease`].
    pub async fn command(&self, command: CoreCommand) -> Result<(), CommandSubmitError> {
        self.validate_request_id(&command)?;
        if command.composer_draft_scope().is_some() {
            return Err(CommandSubmitError::ComposerLeaseRequired);
        }
        self.command_tx
            .send(CoreCommandEnvelope {
                command,
                composer_permit: None,
            })
            .await
            .map_err(|_| CommandSubmitError::RuntimeClosed)
    }

    pub fn begin_composer_draft_renderer_generation(
        &self,
    ) -> Result<ComposerRendererGeneration, ComposerDraftLeaseFailure> {
        self.composer_draft_leases.begin_renderer_generation()
    }

    pub fn acquire_composer_draft_lease(
        &self,
        generation: ComposerRendererGeneration,
        scope: ComposerDraftScope,
    ) -> Result<ComposerDraftLeaseId, ComposerDraftLeaseFailure> {
        self.composer_draft_leases.acquire(generation, scope)
    }

    pub fn release_composer_draft_lease(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
    ) -> Result<(), ComposerDraftLeaseFailure> {
        self.composer_draft_leases.release(generation, lease_id)
    }

    pub fn acquire_composer_draft_command_permit(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        scope: &ComposerDraftScope,
    ) -> Result<ComposerDraftCommandPermit, ComposerDraftLeaseFailure> {
        self.composer_draft_leases
            .try_command_permit(generation, lease_id, scope)
    }

    pub async fn command_with_composer_lease(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        command: CoreCommand,
    ) -> Result<(), CommandSubmitError> {
        let envelope = self.admit_composer_command(generation, lease_id, command)?;
        self.command_tx
            .send(envelope)
            .await
            .map_err(|_| CommandSubmitError::RuntimeClosed)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn command_with_composer_lease_after_admission(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        command: CoreCommand,
        admitted: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
    ) -> Result<(), CommandSubmitError> {
        let envelope = self.admit_composer_command(generation, lease_id, command)?;
        let _ = admitted.send(());
        let _ = release.await;
        self.command_tx
            .send(envelope)
            .await
            .map_err(|_| CommandSubmitError::RuntimeClosed)
    }

    fn validate_request_id(&self, command: &CoreCommand) -> Result<(), CommandSubmitError> {
        if command.request_id().connection_id != self.connection_id {
            return Err(CommandSubmitError::InvalidRequestId);
        }
        Ok(())
    }

    fn admit_composer_command(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        command: CoreCommand,
    ) -> Result<CoreCommandEnvelope, CommandSubmitError> {
        self.validate_request_id(&command)?;
        let scope = command
            .composer_draft_scope()
            .ok_or(CommandSubmitError::ComposerLeaseNotRequired)?;
        let composer_permit = self
            .composer_draft_leases
            .try_command_permit(generation, lease_id, &scope)
            .map_err(CommandSubmitError::ComposerLease)?;
        Ok(CoreCommandEnvelope {
            command,
            composer_permit: Some(composer_permit),
        })
    }
}

impl CoreConnection {
    pub fn connection_id(&self) -> RuntimeConnectionId {
        self.connection_id
    }

    /// Clone a lightweight command submitter for callers that must not hold
    /// the full connection guard while awaiting bounded channel capacity.
    pub fn command_handle(&self) -> CoreCommandHandle {
        CoreCommandHandle {
            connection_id: self.connection_id,
            command_tx: self.command_tx.clone(),
            composer_draft_leases: Arc::clone(&self.composer_draft_leases),
        }
    }

    /// Allocate the next request id for this connection. Request ids are
    /// allocated here, never hand-built by callers.
    pub fn next_request_id(&self) -> RequestId {
        RequestId {
            connection_id: self.connection_id,
            sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Submit a command without a composer lease. Revision-bearing composer
    /// commands fail closed and must use [`Self::command_with_composer_lease`].
    pub async fn command(&self, command: CoreCommand) -> Result<(), CommandSubmitError> {
        self.command_handle().command(command).await
    }

    pub fn begin_composer_draft_renderer_generation(
        &self,
    ) -> Result<ComposerRendererGeneration, ComposerDraftLeaseFailure> {
        self.command_handle()
            .begin_composer_draft_renderer_generation()
    }

    pub fn acquire_composer_draft_lease(
        &self,
        generation: ComposerRendererGeneration,
        scope: ComposerDraftScope,
    ) -> Result<ComposerDraftLeaseId, ComposerDraftLeaseFailure> {
        self.command_handle()
            .acquire_composer_draft_lease(generation, scope)
    }

    pub fn release_composer_draft_lease(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
    ) -> Result<(), ComposerDraftLeaseFailure> {
        self.command_handle()
            .release_composer_draft_lease(generation, lease_id)
    }

    pub fn acquire_composer_draft_command_permit(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        scope: &ComposerDraftScope,
    ) -> Result<ComposerDraftCommandPermit, ComposerDraftLeaseFailure> {
        self.command_handle()
            .acquire_composer_draft_command_permit(generation, lease_id, scope)
    }

    pub async fn command_with_composer_lease(
        &self,
        generation: ComposerRendererGeneration,
        lease_id: ComposerDraftLeaseId,
        command: CoreCommand,
    ) -> Result<(), CommandSubmitError> {
        self.command_handle()
            .command_with_composer_lease(generation, lease_id, command)
            .await
    }

    /// Receive the next event. On lag, intermediate events were dropped for
    /// this consumer; resync from [`Self::snapshot`].
    pub async fn recv_event(&mut self) -> Result<CoreEvent, EventStreamLag> {
        loop {
            match self.event_rx.recv().await {
                Ok(event) => return Ok(self.project_event_for_consumer(event)),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    return Err(EventStreamLag { skipped });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // Runtime shut down; surface as lag so callers resync and
                    // observe the final snapshot.
                    return Err(EventStreamLag { skipped: 0 });
                }
            }
        }
    }

    fn project_event_for_consumer(&self, mut event: CoreEvent) -> CoreEvent {
        match &mut event {
            CoreEvent::Timeline(timeline_event) => {
                let snapshot = self.snapshot_rx.borrow().state.clone();
                project_timeline_event_display_labels(timeline_event, &snapshot);
            }
            CoreEvent::Room(room_event) => {
                let snapshot = self.snapshot_rx.borrow().state.clone();
                project_room_event_display_labels(room_event, &snapshot);
            }
            CoreEvent::StateDelta(_)
            | CoreEvent::StateChanged(_)
            | CoreEvent::Account(_)
            | CoreEvent::Sync(_)
            | CoreEvent::LiveSignals(_)
            | CoreEvent::Search(_)
            | CoreEvent::E2eeTrust(_)
            | CoreEvent::Activity(_)
            | CoreEvent::LocalEncryption(_)
            | CoreEvent::NativeAttention(_)
            | CoreEvent::CjkTextPolicy(_)
            | CoreEvent::ThreadsList(_)
            | CoreEvent::OperationFailed { .. }
            | CoreEvent::IntentLifecycle { .. } => {}
        }
        event
    }

    /// Latest state snapshot (latest-wins watch semantics).
    pub fn snapshot(&self) -> AppStateSnapshot {
        self.snapshot_rx.borrow().state.clone()
    }

    /// Latest state snapshot with the generation used by `StateDelta`.
    pub fn versioned_snapshot(&self) -> VersionedAppStateSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    /// Causal snapshot-change barrier for deterministic runtime tests.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub async fn next_versioned_snapshot_for_testing(
        &mut self,
    ) -> Option<VersionedAppStateSnapshot> {
        self.snapshot_rx.changed().await.ok()?;
        Some(self.snapshot_rx.borrow_and_update().clone())
    }

    /// Select `room_id` and wait until the latest versioned watch snapshot names
    /// it as the active room.
    ///
    /// Lifecycle events only classify progress or a matching failure. The
    /// returned snapshot is the exact watch value that satisfied the predicate,
    /// including its publication generation. Broadcast lag is recovered by
    /// rechecking that latest value; `timeout` is one absolute deadline for the
    /// wait, not a per-event allowance.
    pub async fn select_room_and_wait(
        &mut self,
        room_id: String,
        timeout: Duration,
    ) -> Result<VersionedAppStateSnapshot, SelectRoomError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let request_id = self.next_request_id();
        tokio::time::timeout_at(
            deadline,
            self.command(CoreCommand::Room(RoomCommand::SelectRoom {
                request_id,
                room_id: room_id.clone(),
            })),
        )
        .await
        .map_err(|_| SelectRoomError::Timeout)?
        .map_err(SelectRoomError::CommandSubmit)?;
        loop {
            let current = self.versioned_snapshot();
            if current.state.navigation.active_room_id.as_deref() == Some(room_id.as_str()) {
                return Ok(current);
            }

            let event = match tokio::time::timeout_at(deadline, self.event_rx.recv()).await {
                Ok(Ok(event)) => self.project_event_for_consumer(event),
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    let current = self.versioned_snapshot();
                    return if current.state.navigation.active_room_id.as_deref()
                        == Some(room_id.as_str())
                    {
                        Ok(current)
                    } else {
                        Err(SelectRoomError::EventStreamClosed)
                    };
                }
                Err(_) => {
                    let current = self.versioned_snapshot();
                    return if current.state.navigation.active_room_id.as_deref()
                        == Some(room_id.as_str())
                    {
                        Ok(current)
                    } else {
                        Err(SelectRoomError::Timeout)
                    };
                }
            };

            let current = self.versioned_snapshot();
            if current.state.navigation.active_room_id.as_deref() == Some(room_id.as_str()) {
                return Ok(current);
            }

            match event {
                CoreEvent::OperationFailed {
                    request_id: failed_request_id,
                    failure,
                } if failed_request_id == request_id => {
                    return Err(SelectRoomError::OperationFailed(failure));
                }
                CoreEvent::IntentLifecycle {
                    request_id: lifecycle_request_id,
                    outcome: IntentOutcome::FailedNoOp(reason),
                    ..
                } if lifecycle_request_id == request_id => {
                    return Err(match reason {
                        IntentNoOpReason::SessionNotReady => SelectRoomError::SessionNotReady,
                        IntentNoOpReason::RoomNotInState => SelectRoomError::RoomNotInState,
                        reason => SelectRoomError::FailedNoOp(reason),
                    });
                }
                // Committed and benign no-op outcomes are progress only. The
                // watch snapshot remains the authoritative success transport.
                CoreEvent::IntentLifecycle {
                    request_id: lifecycle_request_id,
                    outcome: IntentOutcome::Committed | IntentOutcome::BenignNoOp(_),
                    ..
                } if lifecycle_request_id == request_id => {}
                _ => {}
            }
        }
    }
}
#[cfg(test)]
mod tests;
