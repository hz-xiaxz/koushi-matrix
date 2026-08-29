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
mod tests {
    use super::*;
    use crate::event::{
        ThreadSummaryDto, TimelineDiff, TimelineEvent, TimelineItem, TimelineItemId,
    };
    use crate::ids::{AccountKey, TimelineKey, TimelineKind};
    use futures_util::FutureExt;
    use koushi_state::{
        AppAction, AppState, ComposerTarget, LocalUserAliasUpdateState, OwnProfile, ProfileState,
        SessionInfo, UserProfile, reduce,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn scripted_connection(
        event_capacity: usize,
    ) -> (
        CoreConnection,
        mpsc::Receiver<CoreCommandEnvelope>,
        broadcast::Sender<CoreEvent>,
        watch::Sender<VersionedAppStateSnapshot>,
    ) {
        let connection_id = RuntimeConnectionId(41);
        let (command_tx, command_rx) = mpsc::channel(4);
        let (event_tx, event_rx) = broadcast::channel(event_capacity);
        let (snapshot_tx, snapshot_rx) = watch::channel(VersionedAppStateSnapshot {
            generation: 0,
            state: AppState::default(),
        });
        (
            CoreConnection {
                connection_id,
                command_tx,
                composer_draft_leases: Arc::new(ComposerDraftLeaseRegistry::new()),
                event_rx,
                snapshot_rx,
                next_sequence: AtomicU64::new(1),
            },
            command_rx,
            event_tx,
            snapshot_tx,
        )
    }

    fn selected_snapshot(room_id: &str, generation: u64) -> VersionedAppStateSnapshot {
        let mut state = AppState::default();
        state.navigation.active_room_id = Some(room_id.to_owned());
        VersionedAppStateSnapshot { generation, state }
    }

    #[tokio::test]
    async fn committed_lifecycle_waits_for_the_matching_published_snapshot() {
        let room_id = "!committed-before-watch:example.test";
        let (mut connection, mut command_rx, event_tx, snapshot_tx) = scripted_connection(4);
        let mut waiter =
            Box::pin(connection.select_room_and_wait(room_id.to_owned(), Duration::from_secs(1)));
        assert!(waiter.as_mut().now_or_never().is_none());
        let request_id = command_rx
            .recv()
            .await
            .expect("select command")
            .command
            .request_id();

        event_tx
            .send(CoreEvent::IntentLifecycle {
                request_id,
                outcome: IntentOutcome::Committed,
                published_generation: 17,
            })
            .expect("committed lifecycle");
        assert!(
            waiter.as_mut().now_or_never().is_none(),
            "telemetry must not settle selection before watch publication"
        );

        let published = selected_snapshot(room_id, 17);
        snapshot_tx
            .send(published.clone())
            .expect("publish selected snapshot");
        event_tx
            .send(CoreEvent::StateChanged(published.state.clone()))
            .expect("publish state event");
        assert_eq!(waiter.await.expect("settled selection"), published);
    }

    #[tokio::test]
    async fn select_room_waiter_recovers_lag_from_latest_watch_snapshot() {
        let room_id = "!lagged-watch:example.test";
        let (mut connection, mut command_rx, event_tx, snapshot_tx) = scripted_connection(1);
        let mut waiter =
            Box::pin(connection.select_room_and_wait(room_id.to_owned(), Duration::from_secs(1)));
        assert!(waiter.as_mut().now_or_never().is_none());
        let _command = command_rx.recv().await.expect("select command");

        event_tx
            .send(CoreEvent::StateChanged(AppState::default()))
            .expect("first event");
        event_tx
            .send(CoreEvent::StateChanged(AppState::default()))
            .expect("overflowing event");
        let published = selected_snapshot(room_id, 23);
        snapshot_tx
            .send(published.clone())
            .expect("publish selected snapshot");

        assert_eq!(waiter.await.expect("lag recovery settlement"), published);
    }

    #[tokio::test]
    async fn matching_operation_failure_returns_the_typed_core_failure() {
        let (mut connection, mut command_rx, event_tx, _snapshot_tx) = scripted_connection(1);
        let mut waiter = Box::pin(connection.select_room_and_wait(
            "!matching-operation-failure:example.test".to_owned(),
            Duration::from_secs(1),
        ));
        assert!(waiter.as_mut().now_or_never().is_none());
        let request_id = command_rx
            .recv()
            .await
            .expect("select command")
            .command
            .request_id();

        event_tx
            .send(CoreEvent::OperationFailed {
                request_id,
                failure: crate::failure::CoreFailure::SessionRequired,
            })
            .expect("matching failure");
        assert_eq!(
            waiter.await,
            Err(SelectRoomError::OperationFailed(
                crate::failure::CoreFailure::SessionRequired
            ))
        );
    }

    #[tokio::test]
    async fn matching_superseded_lifecycle_returns_the_typed_noop() {
        let (mut connection, mut command_rx, event_tx, _snapshot_tx) = scripted_connection(1);
        let mut waiter = Box::pin(connection.select_room_and_wait(
            "!matching-superseded:example.test".to_owned(),
            Duration::from_secs(1),
        ));
        assert!(waiter.as_mut().now_or_never().is_none());
        let request_id = command_rx
            .recv()
            .await
            .expect("select command")
            .command
            .request_id();

        event_tx
            .send(CoreEvent::IntentLifecycle {
                request_id,
                outcome: IntentOutcome::FailedNoOp(IntentNoOpReason::Superseded),
                published_generation: 0,
            })
            .expect("matching superseded lifecycle");
        assert_eq!(
            waiter.await,
            Err(SelectRoomError::FailedNoOp(IntentNoOpReason::Superseded))
        );
    }

    #[tokio::test]
    async fn unrelated_request_failures_do_not_settle_room_selection() {
        let room_id = "!unrelated-request:example.test";
        let (mut connection, mut command_rx, event_tx, snapshot_tx) = scripted_connection(4);
        let mut waiter =
            Box::pin(connection.select_room_and_wait(room_id.to_owned(), Duration::from_secs(1)));
        assert!(waiter.as_mut().now_or_never().is_none());
        let request_id = command_rx
            .recv()
            .await
            .expect("select command")
            .command
            .request_id();
        let unrelated_request_id = RequestId {
            connection_id: request_id.connection_id,
            sequence: request_id.sequence + 1,
        };

        event_tx
            .send(CoreEvent::OperationFailed {
                request_id: unrelated_request_id,
                failure: crate::failure::CoreFailure::SessionRequired,
            })
            .expect("unrelated failure");
        event_tx
            .send(CoreEvent::IntentLifecycle {
                request_id: unrelated_request_id,
                outcome: IntentOutcome::FailedNoOp(IntentNoOpReason::RoomNotInState),
                published_generation: 0,
            })
            .expect("unrelated lifecycle");
        assert!(waiter.as_mut().now_or_never().is_none());

        let published = selected_snapshot(room_id, 29);
        snapshot_tx
            .send(published.clone())
            .expect("publish selected snapshot");
        event_tx
            .send(CoreEvent::StateChanged(published.state.clone()))
            .expect("publish state event");
        assert_eq!(waiter.await.expect("settled selection"), published);
    }

    #[tokio::test]
    async fn closed_event_stream_returns_a_final_matching_snapshot() {
        let room_id = "!closed-after-publish:example.test";
        let (mut connection, mut command_rx, event_tx, snapshot_tx) = scripted_connection(1);
        let mut waiter =
            Box::pin(connection.select_room_and_wait(room_id.to_owned(), Duration::from_secs(1)));
        assert!(waiter.as_mut().now_or_never().is_none());
        let _command = command_rx.recv().await.expect("select command");
        let published = selected_snapshot(room_id, 31);
        snapshot_tx
            .send(published.clone())
            .expect("publish final selected snapshot");

        drop(event_tx);
        assert_eq!(waiter.await.expect("final watch settlement"), published);
    }

    #[tokio::test]
    async fn closed_event_stream_returns_a_typed_selection_error() {
        let (mut connection, mut command_rx, event_tx, _snapshot_tx) = scripted_connection(1);
        let mut waiter = Box::pin(connection.select_room_and_wait(
            "!closed-stream:example.test".to_owned(),
            Duration::from_secs(1),
        ));
        assert!(waiter.as_mut().now_or_never().is_none());
        let _command = command_rx.recv().await.expect("select command");

        drop(event_tx);
        assert_eq!(waiter.await, Err(SelectRoomError::EventStreamClosed));
    }

    #[test]
    fn standalone_composer_command_permit_outlives_activation_lease() {
        let composer_draft_leases = Arc::new(ComposerDraftLeaseRegistry::new());
        let (command_tx, _command_rx) = mpsc::channel(1);
        let handle = CoreCommandHandle {
            connection_id: RuntimeConnectionId(1),
            command_tx,
            composer_draft_leases: Arc::clone(&composer_draft_leases),
        };
        let account = koushi_key::SessionKeyId {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@permit:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
        };
        let target = ComposerTarget::Main {
            room_id: "!room:example.invalid".to_owned(),
        };
        let scope = ComposerDraftScope {
            account: account.clone(),
            target: target.clone(),
        };
        let generation = handle
            .begin_composer_draft_renderer_generation()
            .expect("renderer generation");
        let lease_id = handle
            .acquire_composer_draft_lease(generation, scope.clone())
            .expect("activation lease");
        let permit = handle
            .acquire_composer_draft_command_permit(generation, lease_id, &scope)
            .expect("standalone terminal permit");

        handle
            .release_composer_draft_lease(generation, lease_id)
            .expect("release activation lease");
        assert_eq!(
            composer_draft_leases.protected_targets(&account),
            std::collections::BTreeSet::from([target.clone()])
        );

        drop(permit);
        assert!(composer_draft_leases.protected_targets(&account).is_empty());
    }

    #[test]
    fn core_connection_command_handle_clones_submit_path() {
        let source = include_str!("connection.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("runtime production source should precede tests");
        let handle_impl = production_source
            .split("impl CoreCommandHandle")
            .nth(1)
            .expect("CoreConnection should expose a lightweight command handle");
        let connection_impl = production_source
            .split("impl CoreConnection")
            .nth(1)
            .expect("CoreConnection impl should exist");
        let command_handle_fn = connection_impl
            .split("pub fn command_handle")
            .nth(1)
            .expect("CoreConnection should clone a command handle for submitters")
            .split("pub fn next_request_id")
            .next()
            .expect("command_handle should precede request-id allocation");
        let command_fn = connection_impl
            .split("pub async fn command")
            .nth(1)
            .expect("CoreConnection command helper should exist")
            .split("pub async fn recv_event")
            .next()
            .expect("command helper should precede event receiving");

        assert!(
            production_source.contains("#[derive(Clone)]\npub struct CoreCommandHandle"),
            "the command submit path must be cloneable without cloning event/snapshot receivers"
        );
        assert!(
            handle_impl.contains("self.command_tx")
                && handle_impl.contains(".send(CoreCommandEnvelope")
                && handle_impl.contains("command,")
                && handle_impl.contains("composer_permit")
                && handle_impl.contains(".await"),
            "the command handle must own the bounded send await"
        );
        assert!(
            command_handle_fn.contains("command_tx: self.command_tx.clone()"),
            "CoreConnection::command_handle must clone only the bounded sender"
        );
        assert!(
            command_fn.contains("self.command_handle().command(command).await"),
            "CoreConnection::command should delegate through the same submit handle"
        );
    }

    #[tokio::test]
    async fn timeline_sender_label_and_reaction_sender_preview_follow_people_facing_policy() {
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (event_tx, event_rx) = broadcast::channel(4);
        let mut state = AppState::default();
        reduce(&mut state, AppAction::AppStarted);
        reduce(
            &mut state,
            AppAction::RestoreSessionSucceeded(SessionInfo {
                homeserver: "https://example.invalid".to_owned(),
                user_id: "@me:example.invalid".to_owned(),
                device_id: "DEVICE".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            }),
        );
        reduce(
            &mut state,
            AppAction::CurrentDeviceTrustChanged(koushi_state::CurrentDeviceTrustState::Verified),
        );
        state.profile = ProfileState {
            own: OwnProfile {
                display_name: Some("Me Upstream".to_owned()),
                avatar: None,
            },
            room_users: BTreeMap::new(),
            ignored_user_ids: BTreeSet::new(),
            ignored_user_update: koushi_state::IgnoredUserUpdateState::Idle,
            users: BTreeMap::from([
                (
                    "@alice:example.invalid".to_owned(),
                    UserProfile {
                        user_id: "@alice:example.invalid".to_owned(),
                        display_name: Some("Alice Upstream".to_owned()),
                        display_label: "Alice Alias".to_owned(),
                        original_display_label: "Alice Upstream".to_owned(),
                        mention_search_terms: vec![],
                        avatar: None,
                    },
                ),
                (
                    "@bob:example.invalid".to_owned(),
                    UserProfile {
                        user_id: "@bob:example.invalid".to_owned(),
                        display_name: Some("Bob Upstream".to_owned()),
                        display_label: "Bob Alias".to_owned(),
                        original_display_label: "Bob Upstream".to_owned(),
                        mention_search_terms: vec![],
                        avatar: None,
                    },
                ),
                (
                    "@carol:example.invalid".to_owned(),
                    UserProfile {
                        user_id: "@carol:example.invalid".to_owned(),
                        display_name: Some("Carol Upstream".to_owned()),
                        display_label: "Carol Alias".to_owned(),
                        original_display_label: "Carol Upstream".to_owned(),
                        mention_search_terms: vec![],
                        avatar: None,
                    },
                ),
            ]),
            local_aliases: BTreeMap::from([
                (
                    "@alice:example.invalid".to_owned(),
                    "Alice Alias".to_owned(),
                ),
                ("@bob:example.invalid".to_owned(), "Bob Alias".to_owned()),
                (
                    "@carol:example.invalid".to_owned(),
                    "Carol Alias".to_owned(),
                ),
            ]),
            local_alias_update: LocalUserAliasUpdateState::Idle,
            update: Default::default(),
        };
        let (_snapshot_tx, snapshot_rx) = watch::channel(VersionedAppStateSnapshot {
            generation: 0,
            state,
        });
        let mut connection = CoreConnection {
            connection_id: RuntimeConnectionId(7),
            command_tx,
            composer_draft_leases: Arc::new(ComposerDraftLeaseRegistry::new()),
            event_rx,
            snapshot_rx,
            next_sequence: AtomicU64::new(1),
        };
        let key = TimelineKey {
            account_key: AccountKey("@me:example.invalid".to_owned()),
            kind: TimelineKind::Room {
                room_id: "!room:example.invalid".to_owned(),
            },
        };

        let _ = event_tx.send(CoreEvent::Timeline(TimelineEvent::InitialItems {
            request_id: None,
            cause_request_id: None,
            key,
            actor_generation: 0,
            generation: crate::ids::TimelineGeneration(0),
            items: vec![TimelineItem {
                request_state: None,
                id: TimelineItemId::Event {
                    event_id: "$event:example.invalid".to_owned(),
                },
                sender: Some("@alice:example.invalid".to_owned()),
                sender_label: Some("Alice Room Name".to_owned()),
                sender_avatar: None,
                body: Some("hello".to_owned()),
                notice_i18n: None,
                message_kind: Default::default(),
                spoiler_spans: Vec::new(),
                timestamp_ms: Some(1),
                in_reply_to_event_id: Some("$root:example.invalid".to_owned()),
                formatted: None,
                reply_quote: Some(koushi_state::ReplyQuote {
                    event_id: "$root:example.invalid".to_owned(),
                    sender: Some("@bob:example.invalid".to_owned()),
                    sender_label: None,
                    body_preview: Some("quoted".to_owned()),
                    formatted: None,
                    state: koushi_state::ReplyQuoteState::Ready,
                }),
                thread_root: None,
                thread_summary: Some(ThreadSummaryDto {
                    reply_count: 1,
                    latest_event_id: Some("$latest:example.invalid".to_owned()),
                    latest_sender: Some("@carol:example.invalid".to_owned()),
                    latest_sender_label: None,
                    latest_body_preview: Some("latest".to_owned()),
                    latest_timestamp_ms: Some(2),
                }),
                media: None,
                link_previews: None,
                link_ranges: Vec::new(),
                reactions: vec![crate::event::ReactionGroup {
                    key: "👍".to_owned(),
                    count: 1,
                    reacted_by_me: false,
                    my_reaction_event_id: None,
                    sender_preview: vec![crate::event::ReactionSender {
                        user_id: "@bob:example.invalid".to_owned(),
                        display_label: Some("Bob Room Name".to_owned()),
                    }],
                }],
                can_react: false,
                is_redacted: false,
                is_hidden: false,
                can_redact: false,
                is_edited: false,
                can_edit: false,
                actions: Default::default(),
                send_state: None,
                unable_to_decrypt: None,
                display_metadata: None,
            }],
        }));

        match connection.recv_event().await.expect("timeline event") {
            CoreEvent::Timeline(TimelineEvent::InitialItems { items, .. }) => {
                let item = items.first().expect("projected item");
                assert_eq!(item.sender.as_deref(), Some("@alice:example.invalid"));
                assert_eq!(item.sender_label.as_deref(), Some("Alice Alias"));
                assert_eq!(
                    item.reactions[0].sender_preview[0].display_label.as_deref(),
                    Some("Bob Alias")
                );
                let quote = item.reply_quote.as_ref().expect("reply quote");
                assert_eq!(quote.sender.as_deref(), Some("@bob:example.invalid"));
                assert_eq!(quote.sender_label.as_deref(), Some("Bob Alias"));
                let thread = item.thread_summary.as_ref().expect("thread summary");
                assert_eq!(
                    thread.latest_sender.as_deref(),
                    Some("@carol:example.invalid")
                );
                assert_eq!(thread.latest_sender_label.as_deref(), Some("Carol Alias"));
            }
            other => panic!("expected projected timeline event, got {other:?}"),
        }

        let key = TimelineKey {
            account_key: AccountKey("@me:example.invalid".to_owned()),
            kind: TimelineKind::Room {
                room_id: "!room:example.invalid".to_owned(),
            },
        };
        let _ = event_tx.send(CoreEvent::Timeline(TimelineEvent::ItemsUpdated {
            key,
            generation: crate::ids::TimelineGeneration(0),
            batch_id: crate::ids::TimelineBatchId(1),
            diffs: vec![TimelineDiff::PushBack {
                item: TimelineItem {
                    request_state: None,
                    id: TimelineItemId::Event {
                        event_id: "$later:example.invalid".to_owned(),
                    },
                    sender: Some("@room-only:example.invalid".to_owned()),
                    sender_label: Some("Room-only Person".to_owned()),
                    sender_avatar: None,
                    body: Some("later".to_owned()),
                    notice_i18n: None,
                    message_kind: Default::default(),
                    spoiler_spans: Vec::new(),
                    timestamp_ms: Some(3),
                    in_reply_to_event_id: None,
                    formatted: None,
                    reply_quote: None,
                    thread_root: None,
                    thread_summary: None,
                    media: None,
                    link_previews: None,
                    link_ranges: Vec::new(),
                    reactions: Vec::new(),
                    can_react: false,
                    is_redacted: false,
                    is_hidden: false,
                    can_redact: false,
                    is_edited: false,
                    can_edit: false,
                    actions: Default::default(),
                    send_state: None,
                    unable_to_decrypt: None,
                    display_metadata: None,
                },
            }],
        }));

        match connection.recv_event().await.expect("timeline diff event") {
            CoreEvent::Timeline(TimelineEvent::ItemsUpdated { diffs, .. }) => {
                let TimelineDiff::PushBack { item } = diffs.first().expect("projected diff item")
                else {
                    panic!("expected push-back diff");
                };
                assert_eq!(item.sender.as_deref(), Some("@room-only:example.invalid"));
                assert_eq!(item.sender_label.as_deref(), Some("Room-only Person"));
            }
            other => panic!("expected projected timeline diff event, got {other:?}"),
        }
    }
}
