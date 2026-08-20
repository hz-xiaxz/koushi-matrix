use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use koushi_sdk::{
    MatrixClientSession, MatrixOutboundGroupSessionToken, MatrixRoomKeyReshareOutcome,
    MatrixRoomKeyReshareTarget,
};

use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk_ui::timeline::{
    EncryptedMessage, TimelineItem as SdkTimelineItem, TimelineItemKind,
};
use tokio::sync::mpsc;
#[cfg(test)]
use tokio::sync::oneshot;

use crate::account_work::{AccountWorkKind, AccountWorkScheduler};
use crate::event::{CoreEvent, TimelineEvent};
use crate::executor;
use crate::failure::TimelineFailureKind;
use crate::ids::{RequestId, TimelineKey, TimelineKind};

// BEGIN GENERATED SIBLING IMPORTS
use super::actor::{TimelineActor, TimelineActorMessage};
use super::diagnostics::{
    decrypt_retry_backup_result_for_error, decrypt_retry_failure_for_room_operation,
    record_decrypt_retry_backup_lookup, record_decrypt_retry_device_request,
    record_decrypt_retry_request, record_decrypt_retry_settled, record_room_key_requester_stage,
    record_room_key_reshare,
};
use super::item_projection::{
    decrypt_retry_reason_from_content, key_request_stage_token, key_request_withheld_code_token,
    unable_to_decrypt_from_content,
};
use super::manager::{TimelineManagerActor, TimelineMessage};
// END GENERATED SIBLING IMPORTS

pub(super) const DECRYPT_RETRY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
struct RoomKeyReshareAttempt {
    delay: Duration,
    number: u8,
    target: MatrixRoomKeyReshareTarget,
}

const ROOM_KEY_RESHARE_ATTEMPTS: [RoomKeyReshareAttempt; 3] = [
    RoomKeyReshareAttempt {
        delay: Duration::from_secs(3),
        number: 1,
        target: MatrixRoomKeyReshareTarget::OwnOtherDevices,
    },
    RoomKeyReshareAttempt {
        delay: Duration::from_secs(5),
        number: 2,
        target: MatrixRoomKeyReshareTarget::PeerDevices,
    },
    RoomKeyReshareAttempt {
        delay: Duration::from_secs(15),
        number: 3,
        target: MatrixRoomKeyReshareTarget::OwnOtherDevices,
    },
];

fn spawn_delayed_timeline_message<T: Send + 'static>(
    tx: mpsc::Sender<T>,
    delay: Duration,
    message: T,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        executor::sleep(delay).await;
        let _ = tx.send(message).await;
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RoomKeyReshareCompletion {
    Sent {
        request_count: usize,
        recipient_count: usize,
        failed_recipient_count: usize,
    },
    NoSession,
    NoRecipients,
    StaleSession,
    NetworkError,
    SdkError,
}

struct RoomKeyReshareTaskSlot {
    attempt: u8,
    delayed: executor::JoinHandle<()>,
    started: bool,
    worker: Option<executor::JoinHandle<()>>,
}

impl RoomKeyReshareTaskSlot {
    fn abort(&self) {
        self.delayed.abort();
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

pub(super) struct RoomKeyReshareSchedule {
    session: MatrixOutboundGroupSessionToken,
    tasks: Vec<RoomKeyReshareTaskSlot>,
}

impl Drop for RoomKeyReshareSchedule {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

type RoomKeyReshareOperation =
    Pin<Box<dyn Future<Output = RoomKeyReshareCompletion> + Send + 'static>>;

#[cfg(test)]
struct RoomKeyReshareTestSignals {
    acquire_started: oneshot::Sender<()>,
    permit_acquired: oneshot::Sender<()>,
}

fn map_room_key_reshare_completion(
    outcome: Result<MatrixRoomKeyReshareOutcome, koushi_sdk::MatrixRoomOperationError>,
) -> RoomKeyReshareCompletion {
    match outcome {
        Ok(MatrixRoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        }) => RoomKeyReshareCompletion::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        },
        Ok(MatrixRoomKeyReshareOutcome::NoSession) => RoomKeyReshareCompletion::NoSession,
        Ok(MatrixRoomKeyReshareOutcome::NoRecipients) => RoomKeyReshareCompletion::NoRecipients,
        Ok(MatrixRoomKeyReshareOutcome::StaleSession) => RoomKeyReshareCompletion::StaleSession,
        Err(error)
            if error.failure_kind() == Some(koushi_sdk::MatrixRoomOperationFailureKind::Http) =>
        {
            RoomKeyReshareCompletion::NetworkError
        }
        Err(_) => RoomKeyReshareCompletion::SdkError,
    }
}

fn spawn_room_key_reshare_task_with_operation(
    account_work: AccountWorkScheduler,
    manager_tx: mpsc::Sender<TimelineMessage>,
    key: TimelineKey,
    actor_generation: u64,
    expected_session: MatrixOutboundGroupSessionToken,
    target: MatrixRoomKeyReshareTarget,
    attempt: u8,
    operation: RoomKeyReshareOperation,
    #[cfg(test)] test_signals: Option<RoomKeyReshareTestSignals>,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        #[cfg(test)]
        let (acquire_started, permit_acquired) = test_signals
            .map(|signals| (signals.acquire_started, signals.permit_acquired))
            .unzip();
        #[cfg(test)]
        if let Some(signal) = acquire_started {
            let _ = signal.send(());
        }
        let _permit = account_work.acquire(AccountWorkKind::RoomKeyReshare).await;
        #[cfg(test)]
        if let Some(signal) = permit_acquired {
            let _ = signal.send(());
        }
        let outcome = operation.await;
        let _ = manager_tx
            .send(TimelineMessage::RoomKeyReshareCompleted {
                key,
                actor_generation,
                expected_session,
                target,
                attempt,
                outcome,
            })
            .await;
    })
}

fn spawn_room_key_reshare_task(
    account_work: AccountWorkScheduler,
    session: Arc<MatrixClientSession>,
    manager_tx: mpsc::Sender<TimelineMessage>,
    key: TimelineKey,
    actor_generation: u64,
    expected_session: MatrixOutboundGroupSessionToken,
    target: MatrixRoomKeyReshareTarget,
    attempt: u8,
) -> executor::JoinHandle<()> {
    let expected_for_sdk = expected_session.clone();
    let room_id = key.room_id().to_owned();
    let operation: RoomKeyReshareOperation = Box::pin(async move {
        map_room_key_reshare_completion(
            koushi_sdk::force_reshare_room_key(&session, &room_id, Some(&expected_for_sdk), target)
                .await,
        )
    });
    spawn_room_key_reshare_task_with_operation(
        account_work,
        manager_tx,
        key,
        actor_generation,
        expected_session,
        target,
        attempt,
        operation,
        #[cfg(test)]
        None,
    )
}

impl TimelineManagerActor {
    pub(super) async fn schedule_room_key_reshares(&mut self, key: &TimelineKey) {
        if !matches!(key.kind, TimelineKind::Room { .. }) {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(actor_generation) = self.timeline_actor_generations.current_generation(key) else {
            return;
        };
        let client = session.client();
        let Ok(room_id) = matrix_sdk::ruma::RoomId::parse(key.room_id()) else {
            return;
        };
        let Some(room) = client.get_room(&room_id) else {
            return;
        };
        if !room.encryption_state().is_encrypted() {
            return;
        }
        let Ok(Some(outbound_session)) =
            koushi_sdk::current_outbound_group_session_token(session, key.room_id()).await
        else {
            return;
        };
        if self
            .send_enqueue_workers
            .room_key_reshares
            .get(key)
            .is_some_and(|schedule| schedule.session == outbound_session)
        {
            return;
        }

        let tasks = ROOM_KEY_RESHARE_ATTEMPTS
            .iter()
            .map(|attempt| RoomKeyReshareTaskSlot {
                attempt: attempt.number,
                delayed: spawn_delayed_timeline_message(
                    self.msg_tx.clone(),
                    attempt.delay,
                    TimelineMessage::RunRoomKeyReshare {
                        key: key.clone(),
                        actor_generation,
                        expected_session: outbound_session.clone(),
                        target: attempt.target,
                        attempt: attempt.number,
                    },
                ),
                started: false,
                worker: None,
            })
            .collect();
        self.send_enqueue_workers.room_key_reshares.insert(
            key.clone(),
            RoomKeyReshareSchedule {
                session: outbound_session,
                tasks,
            },
        );
        for attempt in ROOM_KEY_RESHARE_ATTEMPTS {
            record_room_key_reshare(
                "new_outbound_session",
                "scheduled",
                attempt.number,
                attempt.target,
                attempt.delay.as_secs(),
                0,
                0,
                0,
            );
        }
    }
    fn room_key_reshare_is_current(
        &self,
        key: &TimelineKey,
        actor_generation: u64,
        expected_session: &MatrixOutboundGroupSessionToken,
    ) -> bool {
        self.send_enqueue_workers
            .room_key_reshares
            .get(key)
            .is_some_and(|schedule| schedule.session == *expected_session)
            && self.timelines.contains_key(key)
            && self.timeline_actor_generations.current_generation(key) == Some(actor_generation)
    }
    pub(super) fn handle_room_key_reshare(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        expected_session: MatrixOutboundGroupSessionToken,
        target: MatrixRoomKeyReshareTarget,
        attempt: u8,
    ) {
        if !self.room_key_reshare_is_current(&key, actor_generation, &expected_session) {
            return;
        }
        let Some(session) = self.session.as_ref().cloned() else {
            return;
        };

        let task = spawn_room_key_reshare_task(
            self.account_work.clone(),
            session,
            self.msg_tx.clone(),
            key.clone(),
            actor_generation,
            expected_session.clone(),
            target,
            attempt,
        );

        // The manager is single-owner and has no await between validation and
        // insertion, but revalidate the exact schedule and slot anyway. If a
        // replacement/cleanup closed the slot before insertion, the new task
        // must be aborted rather than left unowned.
        if !self.room_key_reshare_is_current(&key, actor_generation, &expected_session) {
            task.abort();
            return;
        }
        let Some(schedule) = self.send_enqueue_workers.room_key_reshares.get_mut(&key) else {
            task.abort();
            return;
        };
        if schedule.session != expected_session {
            task.abort();
            return;
        }
        let Some(slot) = schedule
            .tasks
            .iter_mut()
            .find(|slot| slot.attempt == attempt)
        else {
            task.abort();
            return;
        };
        if slot.started {
            task.abort();
            return;
        }
        slot.started = true;
        slot.worker = Some(task);
    }
    fn take_room_key_reshare_worker(
        &mut self,
        key: &TimelineKey,
        expected_session: &MatrixOutboundGroupSessionToken,
        attempt: u8,
    ) -> Option<executor::JoinHandle<()>> {
        let schedule = self.send_enqueue_workers.room_key_reshares.get_mut(key)?;
        if schedule.session != *expected_session {
            return None;
        }
        schedule
            .tasks
            .iter_mut()
            .find(|slot| slot.attempt == attempt)
            .and_then(|slot| slot.worker.take())
    }
    pub(super) async fn handle_room_key_reshare_completed(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        expected_session: MatrixOutboundGroupSessionToken,
        target: MatrixRoomKeyReshareTarget,
        attempt: u8,
        outcome: RoomKeyReshareCompletion,
    ) {
        if !self.room_key_reshare_is_current(&key, actor_generation, &expected_session) {
            return;
        }
        let Some(worker) = self.take_room_key_reshare_worker(&key, &expected_session, attempt)
        else {
            // The first completion takes the per-attempt handle. Duplicate
            // completions therefore have no diagnostic or schedule effect.
            return;
        };
        worker.abort();
        let _ = worker.await;

        let trigger = match attempt {
            1 => "own_device_retry_1",
            2 => "peer_device_retry",
            _ => "own_device_retry_2",
        };
        let delay_seconds = ROOM_KEY_RESHARE_ATTEMPTS
            .iter()
            .find(|candidate| candidate.number == attempt)
            .map_or(0, |candidate| candidate.delay.as_secs());
        match outcome {
            RoomKeyReshareCompletion::Sent {
                request_count,
                recipient_count,
                failed_recipient_count,
            } => record_room_key_reshare(
                trigger,
                "sent",
                attempt,
                target,
                delay_seconds,
                request_count,
                recipient_count,
                failed_recipient_count,
            ),
            RoomKeyReshareCompletion::NoSession => {
                record_room_key_reshare(
                    trigger,
                    "no_session",
                    attempt,
                    target,
                    delay_seconds,
                    0,
                    0,
                    0,
                );
                self.send_enqueue_workers.room_key_reshares.remove(&key);
            }
            RoomKeyReshareCompletion::NoRecipients => record_room_key_reshare(
                trigger,
                "no_recipients",
                attempt,
                target,
                delay_seconds,
                0,
                0,
                0,
            ),
            RoomKeyReshareCompletion::StaleSession => {
                record_room_key_reshare(
                    trigger,
                    "cancelled",
                    attempt,
                    target,
                    delay_seconds,
                    0,
                    0,
                    0,
                );
                self.send_enqueue_workers.room_key_reshares.remove(&key);
            }
            RoomKeyReshareCompletion::NetworkError => record_room_key_reshare(
                trigger,
                "network_error",
                attempt,
                target,
                delay_seconds,
                0,
                0,
                0,
            ),
            RoomKeyReshareCompletion::SdkError => record_room_key_reshare(
                trigger,
                "sdk_error",
                attempt,
                target,
                delay_seconds,
                0,
                0,
                0,
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DecryptRetryReason {
    MissingRoomKey,
    Withheld,
    Malformed,
    Unknown,
}

impl DecryptRetryReason {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::MissingRoomKey => "missing_room_key",
            Self::Withheld => "withheld",
            Self::Malformed => "malformed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DecryptRetryBackupState {
    Available,
    Unknown,
}

impl DecryptRetryBackupState {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unknown => "unknown",
        }
    }
}

fn decrypt_retry_backup_state_for(
    backup: koushi_sdk::MatrixSecureBackupLocalState,
    recovery: koushi_sdk::MatrixSecureBackupRecoveryState,
) -> DecryptRetryBackupState {
    if backup == koushi_sdk::MatrixSecureBackupLocalState::Enabled
        && recovery == koushi_sdk::MatrixSecureBackupRecoveryState::Enabled
    {
        DecryptRetryBackupState::Available
    } else {
        DecryptRetryBackupState::Unknown
    }
}

#[derive(Clone, Copy)]
pub(super) enum DecryptRetryBackupResult {
    Found,
    NotFound,
    Network,
    Forbidden,
    InvalidBackup,
    Timeout,
    Sdk,
}

impl DecryptRetryBackupResult {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::NotFound => "not_found",
            Self::Network => "network",
            Self::Forbidden => "forbidden",
            Self::InvalidBackup => "invalid_backup",
            Self::Timeout => "timeout",
            Self::Sdk => "sdk",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DecryptRetryDeviceResult {
    Sent,
    Failed,
}

impl DecryptRetryDeviceResult {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DecryptRetryFailure {
    Network,
    Forbidden,
    Timeout,
    Sdk,
}

impl DecryptRetryFailure {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Forbidden => "forbidden",
            Self::Timeout => "timeout",
            Self::Sdk => "sdk",
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(super) enum DecryptRetrySettledResult {
    Decrypted,
    StillMissing,
    Withheld,
    Malformed,
    Timeout,
    Superseded,
}

impl DecryptRetrySettledResult {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Decrypted => "decrypted",
            Self::StillMissing => "still_missing",
            Self::Withheld => "withheld",
            Self::Malformed => "malformed",
            Self::Timeout => "timeout",
            Self::Superseded => "superseded",
        }
    }
}

pub(super) fn decrypt_retry_diff_settlement(
    diff: &eyeball_im::VectorDiff<Arc<SdkTimelineItem>>,
    event_id: &str,
) -> Option<DecryptRetrySettledResult> {
    let settlement = |item: &Arc<SdkTimelineItem>| {
        let TimelineItemKind::Event(event_item) = item.kind() else {
            return None;
        };
        if !event_item
            .event_id()
            .is_some_and(|candidate| candidate.as_str() == event_id)
        {
            return None;
        }
        if !event_item.content().is_unable_to_decrypt() {
            Some(DecryptRetrySettledResult::Decrypted)
        } else if matches!(
            decrypt_retry_reason_from_content(event_item.content()),
            DecryptRetryReason::Withheld
        ) {
            Some(DecryptRetrySettledResult::Withheld)
        } else {
            None
        }
    };
    match diff {
        eyeball_im::VectorDiff::PushFront { value }
        | eyeball_im::VectorDiff::PushBack { value }
        | eyeball_im::VectorDiff::Insert { value, .. }
        | eyeball_im::VectorDiff::Set { value, .. } => settlement(value),
        eyeball_im::VectorDiff::Reset { values } => values.iter().find_map(settlement),
        eyeball_im::VectorDiff::Append { values } => values.iter().find_map(settlement),
        eyeball_im::VectorDiff::Clear
        | eyeball_im::VectorDiff::PopFront
        | eyeball_im::VectorDiff::PopBack
        | eyeball_im::VectorDiff::Remove { .. }
        | eyeball_im::VectorDiff::Truncate { .. } => None,
    }
}

static NEXT_DECRYPT_RETRY_OPERATION: AtomicU64 = AtomicU64::new(1);

fn next_decrypt_retry_operation() -> u64 {
    NEXT_DECRYPT_RETRY_OPERATION.fetch_add(1, Ordering::Relaxed)
}

/// Presentation state of a user/automatic room-key request (issue #460).
/// Rust-owned; React renders the closed tokens only.
#[derive(Clone, Debug)]
pub(super) struct KeyRequestUiState {
    pub(super) stage: &'static str,
    pub(super) withheld_code: Option<&'static str>,
    pub(super) session_id: Option<String>,
    /// Command correlation for externally issued requests (both origins;
    /// issue #460); None only for actor-internal automatic work.
    request_id: Option<RequestId>,
}

#[derive(Clone)]
pub(super) struct PendingDecryptRetry {
    pub(super) event_id: String,
    operation: u64,
    attempt: u8,
    actor_generation: u64,
    started_at: executor::Instant,
    deadline: executor::Instant,
}

struct DecryptRetrySettlement {
    pending: PendingDecryptRetry,
    result: DecryptRetrySettledResult,
}

#[derive(Default)]
pub(super) struct DecryptRetryController {
    pub(super) pending: Option<PendingDecryptRetry>,
}

impl DecryptRetryController {
    fn admit(
        &mut self,
        event_id: &str,
        actor_generation: u64,
        started_at: executor::Instant,
    ) -> (PendingDecryptRetry, Option<PendingDecryptRetry>, bool) {
        if let Some(current) = self.pending.as_ref().filter(|pending| {
            pending.event_id == event_id && pending.actor_generation == actor_generation
        }) {
            return (current.clone(), None, true);
        }
        let previous = self.pending.take();
        let attempt = previous
            .as_ref()
            .filter(|pending| pending.event_id == event_id)
            .map_or(1, |pending| pending.attempt.saturating_add(1));
        let pending = PendingDecryptRetry {
            event_id: event_id.to_owned(),
            operation: next_decrypt_retry_operation(),
            attempt,
            actor_generation,
            started_at,
            deadline: started_at + DECRYPT_RETRY_TIMEOUT,
        };
        self.pending = Some(pending.clone());
        (pending, previous, false)
    }

    pub(super) fn is_current(&self, operation: u64, actor_generation: u64) -> bool {
        self.pending.as_ref().is_some_and(|pending| {
            pending.operation == operation && pending.actor_generation == actor_generation
        })
    }

    fn settle_if_current(
        &mut self,
        operation: u64,
        actor_generation: u64,
        result: DecryptRetrySettledResult,
    ) -> Option<DecryptRetrySettlement> {
        if !self.is_current(operation, actor_generation) {
            return None;
        }
        Some(DecryptRetrySettlement {
            pending: self.pending.take().expect("current retry is retained"),
            result,
        })
    }

    fn settle_timeout_if_current(
        &mut self,
        operation: u64,
        actor_generation: u64,
    ) -> Option<DecryptRetrySettlement> {
        self.settle_if_current(
            operation,
            actor_generation,
            DecryptRetrySettledResult::Timeout,
        )
    }
}

pub(super) fn decrypt_retry_settlement_operation(
    controller: &DecryptRetryController,
    actor_generation: u64,
    event_id: &str,
) -> Option<u64> {
    controller.pending.as_ref().and_then(|pending| {
        (pending.actor_generation == actor_generation && pending.event_id == event_id)
            .then_some(pending.operation)
    })
}

impl TimelineActor {
    fn begin_decrypt_retry(
        &mut self,
        event_id: &str,
        reason: DecryptRetryReason,
        backup_state: DecryptRetryBackupState,
    ) -> Option<PendingDecryptRetry> {
        let (pending, previous, coalesced) =
            self.decrypt_retry
                .admit(event_id, self.actor_generation, executor::Instant::now());
        if coalesced {
            record_room_key_requester_stage(
                pending.operation,
                "awaiting",
                "none",
                pending.started_at.elapsed(),
            );
            return None;
        }
        if let Some(previous) = previous {
            record_decrypt_retry_settled(
                previous.operation,
                DecryptRetrySettledResult::Superseded,
                previous.started_at.elapsed(),
            );
            if let Some(task) = self.decrypt_retry_timeout_task.take() {
                task.abort();
            }
            // Issue #460: the superseded request's operational window ends and
            // its timeout task is gone — move its presentation to
            // still_waiting (the Matrix request is still outstanding; a late
            // key or withheld observation settles it further).
            if let Some(state) = self.key_request_states.get_mut(&previous.event_id) {
                if !matches!(
                    state.stage,
                    "withheld" | "decryption_recovered" | "send_failed"
                ) {
                    state.stage = "still_waiting";
                }
            }
            if let Some(state) = self.key_request_states.get(&previous.event_id) {
                self.publish_key_request_state(&previous.event_id, state);
            }
        }
        record_decrypt_retry_request(
            pending.operation,
            pending.attempt,
            reason,
            backup_state,
            Duration::ZERO,
        );
        Some(pending)
    }
    fn schedule_decrypt_retry_timeout(&mut self, pending: &PendingDecryptRetry) {
        if let Some(task) = self.decrypt_retry_timeout_task.take() {
            task.abort();
        }
        self.decrypt_retry_timeout_task = Some(spawn_delayed_timeline_message(
            self.msg_tx.clone(),
            pending
                .deadline
                .saturating_duration_since(executor::Instant::now()),
            TimelineActorMessage::DecryptRetryTimeout {
                operation: pending.operation,
                actor_generation: pending.actor_generation,
            },
        ));
    }
    pub(super) fn settle_decrypt_retry(
        &mut self,
        operation: u64,
        result: DecryptRetrySettledResult,
    ) {
        let Some(settlement) = (match result {
            DecryptRetrySettledResult::Timeout => self
                .decrypt_retry
                .settle_timeout_if_current(operation, self.actor_generation),
            result => {
                self.decrypt_retry
                    .settle_if_current(operation, self.actor_generation, result)
            }
        }) else {
            return;
        };
        if let Some(task) = self.decrypt_retry_timeout_task.take() {
            task.abort();
        }
        record_decrypt_retry_settled(
            settlement.pending.operation,
            settlement.result,
            settlement.pending.started_at.elapsed(),
        );
        // Issue #460: update the presentation state for the affected event.
        let event_id = settlement.pending.event_id.clone();
        let stage = match settlement.result {
            DecryptRetrySettledResult::Decrypted => Some("decryption_recovered"),
            DecryptRetrySettledResult::Withheld => Some("withheld"),
            DecryptRetrySettledResult::Timeout => Some("still_waiting"),
            // Request enqueue/SDK failures settle as still-missing; surface
            // them as a terminal send failure so the UI is not stuck waiting.
            DecryptRetrySettledResult::StillMissing => Some("send_failed"),
            _ => None,
        };
        if let Some(stage) = stage {
            // Resolve a closed withheld code from the observed to-device
            // `m.room_key.withheld` for this event's session, if known.
            let existing_session = self
                .key_request_states
                .get(&event_id)
                .and_then(|state| state.session_id.clone());
            let withheld_code = existing_session.as_deref().and_then(|session| {
                self.withheld_codes
                    .get(&(self.key.room_id().to_owned(), session.to_owned()))
                    .copied()
            });
            self.key_request_states
                .entry(event_id.clone())
                .and_modify(|state| {
                    state.stage = stage;
                    state.withheld_code = withheld_code;
                })
                .or_insert(KeyRequestUiState {
                    stage,
                    withheld_code,
                    session_id: None,
                    request_id: None,
                });
            // Issue #460: every settle transition is published so the UI can
            // reflect withheld / still-waiting / send-failure even when no
            // timeline diff follows (static timeline, to-device withheld).
            if let Some(state) = self.key_request_states.get(&event_id) {
                self.publish_key_request_state(&event_id, state);
            }
        }
    }
    pub(super) fn publish_key_request_state(&self, event_id: &str, state: &KeyRequestUiState) {
        // Generation fence: a replaced actor must not publish outcomes for a
        // batch the UI has already discarded (same gate as timeline events).
        let Some(_lease) = self
            .timeline_actor_generations
            .try_acquire(&self.key, self.actor_generation)
        else {
            return;
        };
        let _ = self.event_tx.send(CoreEvent::Room(
            crate::event::RoomEvent::RoomKeyRequestStateChanged {
                key: self.key.clone(),
                event_id: event_id.to_owned(),
                request_id: state.request_id.clone(),
                stage: key_request_stage_token(state.stage),
                withheld_code: state
                    .withheld_code
                    .and_then(key_request_withheld_code_token),
            },
        ));
    }
    /// Issue #460: queue automatic key-request messages for events, retaining
    /// candidates that hit a full mailbox for a later retry (never blocks the
    /// projection on the actor's own bounded mailbox).
    pub(super) fn dispatch_auto_key_requests(&mut self, event_ids: Vec<String>) {
        for event_id in event_ids {
            if self.key_request_states.contains_key(&event_id) {
                continue;
            }
            match self.msg_tx.try_send(TimelineActorMessage::RequestRoomKey {
                request_id: None,
                event_id,
                origin: crate::command::KeyRequestOrigin::Automatic,
            }) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(message)) => {
                    match message {
                        TimelineActorMessage::RequestRoomKey { event_id, .. }
                            if !self.pending_auto_key_requests.contains(&event_id) =>
                        {
                            // Dedup: repeated Reset batches re-scan the same
                            // events; the pending set stays bounded by the
                            // number of distinct requestable events.
                            self.pending_auto_key_requests.push(event_id);
                        }
                        _ => {}
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {}
            }
        }
    }
    pub(super) async fn handle_request_room_key(
        &mut self,
        request_id: Option<RequestId>,
        event_id: String,
        origin: crate::command::KeyRequestOrigin,
    ) {
        let requested_event_id = event_id.clone();
        let event_id = match matrix_sdk::ruma::EventId::parse(&event_id) {
            Ok(event_id) => event_id,
            Err(_) => {
                if let Some(request_id) = request_id {
                    self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
                }
                return;
            }
        };
        let Some(event_item) = self.timeline.item_by_event_id(&event_id).await else {
            if let Some(request_id) = request_id {
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
            }
            return;
        };
        if !event_item.content().is_unable_to_decrypt() {
            if let Some(request_id) = request_id {
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendState);
            }
            return;
        }
        let retry_reason = decrypt_retry_reason_from_content(event_item.content());
        let backup_observation = self.session.observe_secure_backup_state();
        // Issue #460: automatic (thread auto-recovery) requests are one-shot
        // per event — once a request state exists, repeats are refused so a
        // settled still-waiting/withheld event is not re-spammed on every
        // render. Externally issued commands still retain correlation: when a
        // concrete request id is present, republish the in-flight state with
        // it (silent return only for actor-internal None).
        if origin == crate::command::KeyRequestOrigin::Automatic
            && self.key_request_states.contains_key(&requested_event_id)
        {
            if let Some(request_id) = request_id
                && let Some(state) = self.key_request_states.get(&requested_event_id)
            {
                let correlated = KeyRequestUiState {
                    stage: state.stage,
                    withheld_code: state.withheld_code,
                    session_id: state.session_id.clone(),
                    request_id: Some(request_id),
                };
                self.publish_key_request_state(&requested_event_id, &correlated);
            }
            return;
        }
        let Some(pending) = self.begin_decrypt_retry(
            &requested_event_id,
            retry_reason,
            decrypt_retry_backup_state_for(
                backup_observation.current.backup,
                backup_observation.current.recovery,
            ),
        ) else {
            // Issue #460: coalesced duplicate — the request for this event is
            // already in flight. Republish the current state correlated to
            // this accepted command so every accepted command retains its
            // request_id (the frontend apply is idempotent).
            if let Some(state) = self.key_request_states.get(&requested_event_id) {
                let correlated = KeyRequestUiState {
                    stage: state.stage,
                    withheld_code: state.withheld_code,
                    session_id: state.session_id.clone(),
                    request_id: request_id.clone(),
                };
                self.publish_key_request_state(&requested_event_id, &correlated);
            }
            return;
        };
        let Some(original_json) = event_item.original_json().cloned() else {
            self.settle_decrypt_retry(pending.operation, DecryptRetrySettledResult::Malformed);
            if let Some(request_id) = request_id {
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
            }
            return;
        };
        let room_id = match matrix_sdk::ruma::RoomId::parse(self.key.room_id()) {
            Ok(room_id) => room_id,
            Err(_) => {
                self.settle_decrypt_retry(pending.operation, DecryptRetrySettledResult::Malformed);
                if let Some(request_id) = request_id {
                    self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
                }
                return;
            }
        };
        let session_id =
            unable_to_decrypt_from_content(event_item.content()).and_then(|utd| utd.session_id);
        // Issue #460: publish the request state so the UI can show progress.
        self.key_request_states.insert(
            requested_event_id.clone(),
            KeyRequestUiState {
                stage: if origin == crate::command::KeyRequestOrigin::User {
                    "sent"
                } else {
                    "automatic"
                },
                withheld_code: None,
                session_id: session_id.clone(),
                request_id: request_id.clone(),
            },
        );
        // Issue #460: publish the accepted initial state so the UI can move
        // off its local optimistic marker onto Rust-owned waiting copy.
        if let Some(state) = self.key_request_states.get(&requested_event_id) {
            self.publish_key_request_state(&requested_event_id, state);
        }
        let Some(session_id) = session_id else {
            let result = executor::timeout_at(
                pending.deadline,
                koushi_sdk::request_room_key_for_event(
                    &self.session,
                    room_id.as_str(),
                    &original_json,
                ),
            )
            .await;
            match result {
                Ok(Ok(())) => record_decrypt_retry_device_request(
                    pending.operation,
                    DecryptRetryDeviceResult::Sent,
                    None,
                    pending.started_at.elapsed(),
                ),
                Ok(Err(error)) => {
                    record_decrypt_retry_device_request(
                        pending.operation,
                        DecryptRetryDeviceResult::Failed,
                        Some(decrypt_retry_failure_for_room_operation(&error)),
                        pending.started_at.elapsed(),
                    );
                    self.settle_decrypt_retry(
                        pending.operation,
                        DecryptRetrySettledResult::StillMissing,
                    );
                    if let Some(request_id) = request_id {
                        self.emit_timeline_failure(request_id, TimelineFailureKind::Sdk);
                    }
                    return;
                }
                Err(_) => {
                    record_decrypt_retry_device_request(
                        pending.operation,
                        DecryptRetryDeviceResult::Failed,
                        Some(DecryptRetryFailure::Timeout),
                        pending.started_at.elapsed(),
                    );
                    self.settle_decrypt_retry(
                        pending.operation,
                        DecryptRetrySettledResult::Timeout,
                    );
                    return;
                }
            }
            self.schedule_decrypt_retry_timeout(&pending);
            return;
        };
        match executor::timeout_at(
            pending.deadline,
            koushi_sdk::download_room_key_from_backup(&self.session, room_id.as_str(), &session_id),
        )
        .await
        {
            Ok(Ok(true)) => {
                record_decrypt_retry_backup_lookup(
                    pending.operation,
                    DecryptRetryBackupResult::Found,
                    pending.started_at.elapsed(),
                );
                if executor::timeout_at(
                    pending.deadline,
                    self.timeline.retry_decryption([session_id]),
                )
                .await
                .is_err()
                {
                    self.settle_decrypt_retry(
                        pending.operation,
                        DecryptRetrySettledResult::Timeout,
                    );
                    return;
                }
            }
            Ok(Ok(false)) => {
                record_decrypt_retry_backup_lookup(
                    pending.operation,
                    DecryptRetryBackupResult::NotFound,
                    pending.started_at.elapsed(),
                );
                let result = executor::timeout_at(
                    pending.deadline,
                    koushi_sdk::request_room_key_for_event(
                        &self.session,
                        room_id.as_str(),
                        &original_json,
                    ),
                )
                .await;
                match result {
                    Ok(Ok(())) => record_decrypt_retry_device_request(
                        pending.operation,
                        DecryptRetryDeviceResult::Sent,
                        None,
                        pending.started_at.elapsed(),
                    ),
                    Ok(Err(error)) => {
                        record_decrypt_retry_device_request(
                            pending.operation,
                            DecryptRetryDeviceResult::Failed,
                            Some(decrypt_retry_failure_for_room_operation(&error)),
                            pending.started_at.elapsed(),
                        );
                        self.settle_decrypt_retry(
                            pending.operation,
                            DecryptRetrySettledResult::StillMissing,
                        );
                        if let Some(request_id) = request_id {
                            self.emit_timeline_failure(request_id, TimelineFailureKind::Sdk);
                        }
                        return;
                    }
                    Err(_) => {
                        record_decrypt_retry_device_request(
                            pending.operation,
                            DecryptRetryDeviceResult::Failed,
                            Some(DecryptRetryFailure::Timeout),
                            pending.started_at.elapsed(),
                        );
                        self.settle_decrypt_retry(
                            pending.operation,
                            DecryptRetrySettledResult::Timeout,
                        );
                        return;
                    }
                }
            }
            Ok(Err(error)) => {
                record_decrypt_retry_backup_lookup(
                    pending.operation,
                    decrypt_retry_backup_result_for_error(&error),
                    pending.started_at.elapsed(),
                );
                let result = executor::timeout_at(
                    pending.deadline,
                    koushi_sdk::request_room_key_for_event(
                        &self.session,
                        room_id.as_str(),
                        &original_json,
                    ),
                )
                .await;
                match result {
                    Ok(Ok(())) => record_decrypt_retry_device_request(
                        pending.operation,
                        DecryptRetryDeviceResult::Sent,
                        None,
                        pending.started_at.elapsed(),
                    ),
                    Ok(Err(error)) => {
                        record_decrypt_retry_device_request(
                            pending.operation,
                            DecryptRetryDeviceResult::Failed,
                            Some(decrypt_retry_failure_for_room_operation(&error)),
                            pending.started_at.elapsed(),
                        );
                        self.settle_decrypt_retry(
                            pending.operation,
                            DecryptRetrySettledResult::StillMissing,
                        );
                        if let Some(request_id) = request_id {
                            self.emit_timeline_failure(request_id, TimelineFailureKind::Sdk);
                        }
                        return;
                    }
                    Err(_) => {
                        record_decrypt_retry_device_request(
                            pending.operation,
                            DecryptRetryDeviceResult::Failed,
                            Some(DecryptRetryFailure::Timeout),
                            pending.started_at.elapsed(),
                        );
                        self.settle_decrypt_retry(
                            pending.operation,
                            DecryptRetrySettledResult::Timeout,
                        );
                        return;
                    }
                }
            }
            Err(_) => {
                record_decrypt_retry_backup_lookup(
                    pending.operation,
                    DecryptRetryBackupResult::Timeout,
                    pending.started_at.elapsed(),
                );
                self.settle_decrypt_retry(pending.operation, DecryptRetrySettledResult::Timeout);
                return;
            }
        }
        self.schedule_decrypt_retry_timeout(&pending);
    }
    pub(super) async fn handle_request_late_decryption(
        &mut self,
        request_id: Option<RequestId>,
        trigger: &'static str,
    ) {
        // Consolidated receive-side summary: transport/Olm, merge, and
        // late-decryption groups plus event-cache health (#476).
        let diagnostics = koushi_sdk::room_key_receive_diagnostics(&self.session).await;
        crate::room_key_receive::record_room_key_receive_summary(&diagnostics, trigger);

        let items: Vec<_> = self.timeline.items().await.iter().cloned().collect();
        let session_ids = crate::room_key_receive::collect_visible_utd_sessions(&items);
        let requested = !session_ids.is_empty();
        if requested {
            koushi_sdk::request_late_decryption(
                &self.session,
                self.key.room_id(),
                session_ids.iter().cloned(),
            );
        }
        crate::room_key_receive::record_late_decryption_retry(session_ids.len(), requested);
        if let Some(request_id) = request_id {
            if !requested {
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendState);
            }
        }
    }
    /// Start or join the standard-only recovery operation for a missing-session
    /// UTD (issue #478). Only `MissingMegolmSession` UTDs are eligible.
    pub(super) fn ensure_room_key_recovery(&mut self, session_id: &str) {
        use crate::room_key_recovery::{RecoveryOperation, RecoveryStage};

        let resume = self.load_recovery_resume(session_id);
        let should_begin = {
            let op = self
                .room_key_recovery
                .entry(session_id.to_owned())
                .or_insert_with(|| {
                    self.next_session_alias += 1;
                    let mut op = RecoveryOperation::new(self.next_session_alias);
                    // Resume the bounded sequence from a persisted record so a
                    // restart does not duplicate requests or reset the backoff.
                    if let Some(record) = resume {
                        op.resume(record);
                    }
                    op
                });
            op.stage() == RecoveryStage::Detected && op.attempts() == 0 && op.begin_attempt()
        };
        if should_begin {
            let attempts = self
                .room_key_recovery
                .get(session_id)
                .map(|op| op.attempts())
                .unwrap_or(0);
            self.schedule_recovery_tick(session_id.to_owned(), attempts);
        }
        self.persist_recovery_state();
    }
    /// Path of the per-account recovery resume file.
    fn recovery_resume_path(&self) -> Option<std::path::PathBuf> {
        let data_dir = self.data_dir.as_ref()?;
        Some(data_dir.join(format!("recovery-{}.json", self.key.account_key.0)))
    }
    /// Persist the minimal safe resume records for the current operations.
    fn persist_recovery_state(&mut self) {
        let Some(path) = self.recovery_resume_path() else {
            return;
        };
        let records: std::collections::BTreeMap<
            String,
            crate::room_key_recovery::RecoveryResumeRecord,
        > = self
            .room_key_recovery
            .iter()
            .filter(|(_, op)| !op.is_terminal())
            .map(|(session, op)| (session.clone(), op.resume_record()))
            .collect();
        let Ok(payload) = serde_json::to_vec(&records) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, payload);
    }
    /// Load the resume record for a session, if any.
    fn load_recovery_resume(
        &self,
        session_id: &str,
    ) -> Option<crate::room_key_recovery::RecoveryResumeRecord> {
        let path = self.recovery_resume_path()?;
        let bytes = std::fs::read(&path).ok()?;
        let records: std::collections::BTreeMap<
            String,
            crate::room_key_recovery::RecoveryResumeRecord,
        > = serde_json::from_slice(&bytes).ok()?;
        records.get(session_id).copied()
    }
    fn schedule_recovery_tick(&mut self, session_id: String, attempt: u32) {
        use crate::room_key_recovery::RECOVERY_BACKOFF;
        if let Some(task) = self.recovery_tick_tasks.remove(&session_id) {
            task.abort();
        }
        let task = spawn_delayed_timeline_message(
            self.msg_tx.clone(),
            RECOVERY_BACKOFF,
            TimelineActorMessage::RoomKeyRecoveryTick {
                session_id: session_id.clone(),
                attempt,
                actor_generation: self.actor_generation,
            },
        );
        self.recovery_tick_tasks.insert(session_id, task);
    }
    /// Drive one automatic recovery step for a missing Megolm session
    /// (issue #478): local store, then trusted backup, then own verified
    /// devices, then a bounded wait for sender re-sharing; finally local
    /// redecryption. Never requests keys from peers and never re-broadcasts.
    pub(super) async fn handle_room_key_recovery_tick(
        &mut self,
        session_id: String,
        attempt: u32,
        actor_generation: u64,
    ) {
        use crate::room_key_recovery::{RecoveryStage, RecoveryStepOutcome as Outcome};

        if self.actor_generation != actor_generation {
            return;
        }
        let stage = match self.room_key_recovery.get(&session_id) {
            Some(op) if attempt == op.attempts() && !op.is_terminal() => op.stage(),
            _ => return,
        };
        let room_id = self.key.room_id().to_owned();
        let outcome = match stage {
            RecoveryStage::CheckingLocal => {
                match koushi_sdk::has_inbound_group_session(&self.session, &room_id, &session_id)
                    .await
                {
                    Ok(true) => Outcome::LocalFound,
                    Ok(false) | Err(_) => Outcome::LocalAbsent,
                }
            }
            RecoveryStage::CheckingBackup => {
                match koushi_sdk::download_room_key_from_backup(
                    &self.session,
                    &room_id,
                    &session_id,
                )
                .await
                {
                    Ok(true) => Outcome::BackupImported,
                    Ok(false) => Outcome::BackupAbsent,
                    Err(_) => Outcome::BackupUnavailable,
                }
            }
            RecoveryStage::RequestingOwnDevices => {
                // Request from own verified devices via the standard
                // m.room_key_request path using a matching UTD event.
                let raw = self
                    .timeline
                    .items()
                    .await
                    .iter()
                    .find_map(|item| {
                        let event = item.as_event()?;
                        let content = event.content();
                        let utd = content.as_unable_to_decrypt()?;
                        let EncryptedMessage::MegolmV1AesSha2 {
                            session_id: sid, ..
                        } = utd
                        else {
                            return None;
                        };
                        (sid.as_str() == session_id).then(|| event.original_json().cloned())
                    })
                    .flatten();
                match raw {
                    Some(raw) => {
                        match koushi_sdk::request_room_key_for_event(&self.session, &room_id, &raw)
                            .await
                        {
                            Ok(()) => Outcome::OwnDeviceRequestQueued,
                            Err(_) => Outcome::OwnDeviceRequestFailed,
                        }
                    }
                    None => Outcome::OwnDeviceRequestFailed,
                }
            }
            RecoveryStage::RepairingOlm => {
                // The standard Olm unwedge work (one-time-key claim and
                // m.dummy) is flushed by the SDK's outgoing request pump on
                // the next sync; record the closed stage and transition to
                // waiting.
                Outcome::OlmRepairFlushed
            }
            RecoveryStage::WaitingForKey => {
                // The key may have arrived (sender re-sharing incl. #477).
                match koushi_sdk::has_inbound_group_session(&self.session, &room_id, &session_id)
                    .await
                {
                    Ok(true) => Outcome::KeyArrived,
                    _ => Outcome::OwnDeviceRequestFailed,
                }
            }
            RecoveryStage::KeyReceived | RecoveryStage::RetryingDecryption => {
                // Key is stored: bounded local redecryption only.
                koushi_sdk::request_late_decryption(&self.session, &room_id, [session_id.clone()]);
                Outcome::RedecryptionRequested
            }
            stage => {
                // Other stages are not driver steps.
                return;
            }
        };
        let next = {
            let Some(op) = self.room_key_recovery.get_mut(&session_id) else {
                return;
            };
            op.observe(outcome)
        };
        self.persist_recovery_state();
        match next {
            RecoveryStage::Recovered => {
                crate::room_key_recovery::record_recovery_settled(RecoveryStage::Recovered);
            }
            RecoveryStage::AutomaticPathsExhausted | RecoveryStage::UnrecoverableNoKnownHolder => {
                crate::room_key_recovery::record_recovery_settled(next);
            }
            RecoveryStage::TemporarilyFailed => {
                // Bounded retry: schedule the next attempt if allowed.
                let can_retry = {
                    let Some(op) = self.room_key_recovery.get_mut(&session_id) else {
                        return;
                    };
                    op.begin_attempt()
                };
                if can_retry {
                    let attempts = self
                        .room_key_recovery
                        .get(&session_id)
                        .map(|op| op.attempts())
                        .unwrap_or(0);
                    self.schedule_recovery_tick(session_id, attempts);
                } else {
                    crate::room_key_recovery::record_recovery_settled(
                        RecoveryStage::AutomaticPathsExhausted,
                    );
                }
            }
            _ => {
                let attempts = self
                    .room_key_recovery
                    .get(&session_id)
                    .map(|op| op.attempts())
                    .unwrap_or(0);
                self.schedule_recovery_tick(session_id, attempts);
            }
        }
    }
    pub(super) async fn handle_forward_message(
        &mut self,
        request_id: RequestId,
        source_event_id: String,
        destination_room_id: String,
        transaction_id: String,
    ) {
        let Some(source) = self
            .project_message_source_for_event(&source_event_id)
            .await
        else {
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
            return;
        };
        let Some(body) = source
            .body
            .as_deref()
            .filter(|body| !body.trim().is_empty())
        else {
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendState);
            return;
        };
        if source.is_redacted {
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendState);
            return;
        }

        let destination_room_id_parsed = match matrix_sdk::ruma::RoomId::parse(&destination_room_id)
        {
            Ok(room_id) => room_id,
            Err(_) => {
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
                return;
            }
        };
        let Some(destination_room) = self.session.client().get_room(&destination_room_id_parsed)
        else {
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
            return;
        };

        let txn_id = matrix_sdk::ruma::OwnedTransactionId::from(transaction_id.clone());
        let content = RoomMessageEventContent::text_plain(body);
        match destination_room
            .send(content)
            .with_transaction_id(txn_id)
            .await
        {
            Ok(result) => {
                self.emit(CoreEvent::Timeline(TimelineEvent::MessageForwarded {
                    request_id,
                    key: self.key.clone(),
                    destination_room_id,
                    transaction_id,
                    event_id: result.response.event_id.to_string(),
                }));
            }
            Err(_) => {
                self.emit_timeline_failure(request_id, TimelineFailureKind::Sdk);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashMap;

    use std::sync::Arc;

    use std::time::Duration;

    use koushi_sdk::{
        MatrixClientSession, MatrixOutboundGroupSessionToken, MatrixRoomKeyReshareTarget,
    };

    use tokio::sync::{mpsc, oneshot};

    use crate::account_work::{AccountWorkKind, AccountWorkScheduler};

    use crate::command::TimelineCommand;
    use crate::event::{
        CoreEvent, RoomKeyRequestStage, RoomKeyRequestStateDto, RoomKeyRequestWithheldCode,
        TimelineEvent,
    };
    use crate::executor;

    #[cfg(any(test, feature = "test-hooks"))]
    use crate::ids::AccountKey;
    use crate::ids::TimelineKey;

    use koushi_diagnostics::DiagnosticValue;

    use super::super::diagnostics::{
        decrypt_retry_backup_result_for_error, record_decrypt_retry_backup_lookup,
        record_decrypt_retry_device_request, record_decrypt_retry_request,
        record_decrypt_retry_settled, record_room_key_reshare,
    };
    use super::super::item_projection::{
        key_request_stage_token, key_request_withheld_code_token, withheld_update_should_publish,
    };
    use super::super::manager::{TimelineManagerActor, TimelineManagerControl, TimelineMessage};
    use super::super::outbound_send::{
        TimelineSendCompletionDelivery, TimelineSendTerminalAdmission, TimelineSendTerminalHandoff,
    };
    use super::super::test_support::{
        fake_rid, live_tail_test_manager, test_timeline_actor_handle,
    };
    use super::{
        DecryptRetryBackupResult, DecryptRetryBackupState, DecryptRetryController,
        DecryptRetryDeviceResult, DecryptRetryFailure, DecryptRetryReason,
        DecryptRetrySettledResult, ROOM_KEY_RESHARE_ATTEMPTS, RoomKeyReshareCompletion,
        RoomKeyReshareSchedule, RoomKeyReshareTaskSlot, RoomKeyReshareTestSignals,
        decrypt_retry_backup_state_for, decrypt_retry_settlement_operation,
        next_decrypt_retry_operation, spawn_delayed_timeline_message,
        spawn_room_key_reshare_task_with_operation,
    };

    #[tokio::test(start_paused = true)]
    async fn room_key_reshare_wakes_only_at_the_three_bounded_delays() {
        let (tx, mut rx) = mpsc::channel(3);
        let tasks = ROOM_KEY_RESHARE_ATTEMPTS
            .iter()
            .map(|attempt| {
                spawn_delayed_timeline_message(tx.clone(), attempt.delay, attempt.number)
            })
            .collect::<Vec<_>>();

        for (advance, expected) in [(3, 1), (2, 2), (10, 3)] {
            tokio::time::advance(Duration::from_secs(advance)).await;
            assert_eq!(rx.recv().await, Some(expected));
        }
        assert!(rx.try_recv().is_err());
        for task in tasks {
            task.await.expect("timer task completed");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn delayed_room_key_reshare_wake_is_cancellable() {
        let (tx, mut rx) = mpsc::channel(1);
        let task = spawn_delayed_timeline_message(tx, Duration::from_secs(3), ());
        task.abort();

        tokio::time::advance(Duration::from_secs(3)).await;

        assert!(rx.try_recv().is_err());
        assert!(task.await.expect_err("aborted timer").is_cancelled());
    }

    async fn assert_room_key_reshare_slot_released(scheduler: &AccountWorkScheduler) {
        let permit = tokio::time::timeout(
            Duration::from_secs(1),
            scheduler.acquire(AccountWorkKind::RoomKeyReshare),
        )
        .await
        .expect("room-key reshare work must release its scheduler slot");
        drop(permit);
    }

    #[tokio::test]
    async fn room_key_reshare_waiter_does_not_block_manager_terminal_progress() {
        let fixture = room_key_reshare_fixture().await;
        let key = fixture.key.clone();
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.session = Some(fixture.session.clone());
        let generation = manager
            .timeline_actor_generations
            .activate_after_quiescence(&key)
            .await
            .generation;
        install_room_key_reshare_schedule(&mut manager, &key, fixture.token.clone());
        let scheduler = manager.account_work.clone();
        let _interactive = scheduler.begin_interactive(AccountWorkKind::MessageSend);
        let terminal_ingress = manager.terminal_ingress.clone();
        let mut event_rx = manager.event_tx.subscribe();
        let (control_tx, control_rx) = mpsc::channel(1);
        manager.control_rx = Some(control_rx);
        let manager_tx = manager.msg_tx.clone();
        let manager_task = executor::spawn(manager.run());

        manager_tx
            .send(TimelineMessage::RunRoomKeyReshare {
                key: key.clone(),
                actor_generation: generation,
                expected_session: fixture.token.clone(),
                target: MatrixRoomKeyReshareTarget::OwnOtherDevices,
                attempt: 1,
            })
            .await
            .expect("reshare wake must enter the manager mailbox");
        let (processed_tx, processed_rx) = oneshot::channel();
        manager_tx
            .send(TimelineMessage::TestLiveTailDispatchState {
                key: key.clone(),
                epoch: 0,
                response: processed_tx,
            })
            .await
            .expect("manager probe must enter after the reshare wake");
        tokio::time::timeout(Duration::from_secs(1), processed_rx)
            .await
            .expect("manager must keep polling while reshare waits")
            .expect("manager probe response");

        let terminal_request = fake_rid(92_000);
        assert!(matches!(
            terminal_ingress.admit(TimelineSendTerminalHandoff {
                submission_id: None,
                action: None,
                completion: Some(TimelineSendCompletionDelivery {
                    request_id: terminal_request,
                    key: key.clone(),
                    transaction_id: "reshare-terminal-progress".to_owned(),
                    event_id: "$reshare-terminal-progress:test".to_owned(),
                    diagnostic_correlation: None,
                }),
                failure: None,
            }),
            TimelineSendTerminalAdmission::Accepted
        ));
        let delivered = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let CoreEvent::Timeline(TimelineEvent::SendCompleted {
                    request_id,
                    key: completed_key,
                    ..
                }) = event_rx.recv().await.expect("manager event stream")
                    && request_id == terminal_request
                    && completed_key == key
                {
                    break;
                }
            }
        })
        .await;
        assert!(
            delivered.is_ok(),
            "the manager must deliver a correlated send terminal while reshare waits"
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        control_tx
            .send(TimelineManagerControl::Shutdown {
                acknowledged: shutdown_tx,
            })
            .await
            .expect("manager shutdown control");
        tokio::time::timeout(Duration::from_secs(1), shutdown_rx)
            .await
            .expect("manager shutdown must not wait for the reshare permit")
            .expect("manager shutdown acknowledgement");
        manager_task.await.expect("manager task");
        drop(_interactive);
        assert_room_key_reshare_slot_released(&scheduler).await;
    }

    #[tokio::test]
    async fn room_key_reshare_completion_is_exactly_once_and_stale_inputs_are_inert() {
        let fixture = room_key_reshare_fixture().await;
        let diagnostic_lock = koushi_diagnostics::test_support::lock();
        let mut manager = live_tail_test_manager(HashMap::from([(
            fixture.key.clone(),
            test_timeline_actor_handle(),
        )]));
        let generation = manager
            .timeline_actor_generations
            .activate_after_quiescence(&fixture.key)
            .await
            .generation;
        install_pending_room_key_reshare_worker(
            &mut manager,
            &fixture.key,
            generation,
            fixture.token.clone(),
            1,
        );
        let detail_start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        manager
            .handle_room_key_reshare_completed(
                fixture.key.clone(),
                generation,
                fixture.token.clone(),
                MatrixRoomKeyReshareTarget::OwnOtherDevices,
                1,
                RoomKeyReshareCompletion::Sent {
                    request_count: 1,
                    recipient_count: 1,
                    failed_recipient_count: 0,
                },
            )
            .await;
        let detail_after_first = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .iter()
            .skip(detail_start)
            .filter(|record| record.event.source == "core.room_key_reshare")
            .count();
        manager
            .handle_room_key_reshare_completed(
                fixture.key.clone(),
                generation,
                fixture.token.clone(),
                MatrixRoomKeyReshareTarget::OwnOtherDevices,
                1,
                RoomKeyReshareCompletion::Sent {
                    request_count: 1,
                    recipient_count: 1,
                    failed_recipient_count: 0,
                },
            )
            .await;
        let detail_after_duplicate = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .iter()
            .skip(detail_start)
            .filter(|record| record.event.source == "core.room_key_reshare")
            .count();
        assert_eq!(detail_after_first, 1);
        assert_eq!(detail_after_duplicate, detail_after_first);

        for (label, key, stale_generation, stale_token) in [
            (
                "key",
                TimelineKey::room(AccountKey("@stale:test".to_owned()), "!stale:test"),
                generation,
                fixture.token.clone(),
            ),
            (
                "generation",
                fixture.key.clone(),
                generation + 1,
                fixture.token.clone(),
            ),
            (
                "token",
                fixture.key.clone(),
                generation,
                fixture.other_token.clone(),
            ),
        ] {
            let mut stale_manager = live_tail_test_manager(HashMap::from([(
                fixture.key.clone(),
                test_timeline_actor_handle(),
            )]));
            let current_generation = stale_manager
                .timeline_actor_generations
                .activate_after_quiescence(&fixture.key)
                .await
                .generation;
            install_pending_room_key_reshare_worker(
                &mut stale_manager,
                &fixture.key,
                current_generation,
                fixture.token.clone(),
                1,
            );
            let stale_manager_detail_start = koushi_diagnostics::test_support::detail_snapshot()
                .records
                .len();
            stale_manager
                .handle_room_key_reshare_completed(
                    key,
                    stale_generation,
                    stale_token,
                    MatrixRoomKeyReshareTarget::OwnOtherDevices,
                    1,
                    RoomKeyReshareCompletion::Sent {
                        request_count: 1,
                        recipient_count: 1,
                        failed_recipient_count: 0,
                    },
                )
                .await;
            assert!(
                stale_manager
                    .send_enqueue_workers
                    .room_key_reshares
                    .get(&fixture.key)
                    .and_then(|schedule| schedule.tasks[0].worker.as_ref())
                    .is_some(),
                "stale {label} completion must not consume the active task"
            );
            assert_eq!(
                koushi_diagnostics::test_support::detail_snapshot()
                    .records
                    .iter()
                    .skip(stale_manager_detail_start)
                    .filter(|record| record.event.source == "core.room_key_reshare")
                    .count(),
                0,
                "stale {label} completion must not record diagnostics"
            );
        }
        drop(diagnostic_lock);
    }

    #[tokio::test]
    async fn room_key_reshare_replacement_unsubscribe_and_shutdown_abort_owned_work() {
        let fixture = room_key_reshare_fixture().await;

        for admitted in [false, true] {
            for cancellation in ["replacement", "unsubscribe", "shutdown"] {
                let mut manager = live_tail_test_manager(if cancellation == "unsubscribe" {
                    HashMap::from([(fixture.key.clone(), test_timeline_actor_handle())])
                } else {
                    HashMap::new()
                });
                let scheduler = manager.account_work.clone();
                let interactive =
                    (!admitted).then(|| scheduler.begin_interactive(AccountWorkKind::MessageSend));
                let (mut completion_tx, acquire_started, permit_acquired) =
                    install_controlled_room_key_reshare_worker(
                        &mut manager,
                        &fixture.key,
                        1,
                        fixture.token.clone(),
                        1,
                    );

                acquire_started
                    .await
                    .expect("reshare worker must enter account-work acquire");
                if admitted {
                    permit_acquired
                        .await
                        .expect("reshare worker must acquire its permit");
                }

                match cancellation {
                    "replacement" => {
                        install_room_key_reshare_schedule(
                            &mut manager,
                            &fixture.key,
                            fixture.other_token.clone(),
                        );
                    }
                    "unsubscribe" => {
                        manager
                            .handle_command(TimelineCommand::Unsubscribe {
                                request_id: fake_rid(92_001),
                                key: fixture.key.clone(),
                            })
                            .await;
                    }
                    "shutdown" => {
                        let (control_tx, control_rx) = mpsc::channel(1);
                        manager.control_rx = Some(control_rx);
                        let task = executor::spawn(manager.run());
                        let (ack_tx, ack_rx) = oneshot::channel();
                        control_tx
                            .send(TimelineManagerControl::Shutdown {
                                acknowledged: ack_tx,
                            })
                            .await
                            .expect("shutdown control");
                        ack_rx
                            .await
                            .expect("shutdown must acknowledge after cancellation");
                        task.await.expect("shutdown manager task");
                    }
                    _ => unreachable!(),
                }

                completion_tx.closed().await;
                drop(interactive);
                assert_room_key_reshare_slot_released(&scheduler).await;
            }
        }
    }

    struct RoomKeyReshareFixture {
        _server: matrix_sdk::test_utils::mocks::MatrixMockServer,
        session: Arc<MatrixClientSession>,
        key: TimelineKey,
        token: MatrixOutboundGroupSessionToken,
        other_token: MatrixOutboundGroupSessionToken,
    }

    async fn room_key_reshare_fixture() -> RoomKeyReshareFixture {
        use matrix_sdk::ruma::{RoomVersionId, device_id, room_id, user_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::{JoinedRoomBuilder, event_factory::EventFactory};
        use wiremock::{
            Mock, ResponseTemplate,
            matchers::{method, path_regex},
        };

        let server = MatrixMockServer::new().await;
        server.mock_crypto_endpoints_preset().await;
        let alice_id = user_id!("@alice:example.org");
        let bob_id = user_id!("@bob:example.org");
        let alice_device = device_id!("ALICEDEVICE");
        let bob_device = device_id!("BOBDEVICE");
        let alice = server
            .client_builder_for_crypto_end_to_end(alice_id, alice_device)
            .build()
            .await;
        let bob = server
            .client_builder_for_crypto_end_to_end(bob_id, bob_device)
            .build()
            .await;
        server.exchange_e2ee_identities(&alice, &bob).await;

        let first_room = room_id!("!reshare-first:example.org");
        let second_room = room_id!("!reshare-second:example.org");
        server
            .mock_sync()
            .ok_and_run(&alice, |builder| {
                for room_id in [first_room, second_room] {
                    let factory = EventFactory::new().sender(alice_id).room(room_id);
                    builder.add_joined_room(
                        JoinedRoomBuilder::new(room_id)
                            .add_state_event(factory.create(alice_id, RoomVersionId::V1))
                            .add_state_event(factory.room_encryption())
                            .add_state_event(factory.member(alice_id).into_raw())
                            .add_state_event(factory.member(bob_id).into_raw()),
                    );
                }
            })
            .await;
        let factory = EventFactory::new().sender(alice_id).room(first_room);
        server
            .mock_get_members()
            .ok(vec![
                factory.member(alice_id).into_raw(),
                factory.member(bob_id).into_raw(),
            ])
            .mount()
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(
                r"^/_matrix/client/.*/sendToDevice/m.room.encrypted/.*",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(server.server())
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(
                r"^/_matrix/client/.*/rooms/.*/send/m.room.encrypted/.*",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "event_id": "$reshare-event:example.org" })),
            )
            .expect(2)
            .mount(server.server())
            .await;

        for room_id in [first_room, second_room] {
            alice
                .get_room(room_id)
                .expect("synthetic encrypted room")
                .send(
                    matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(
                        "synthetic reshare fixture",
                    ),
                )
                .await
                .expect("synthetic encrypted send");
        }

        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            alice.clone(),
            koushi_state::SessionInfo {
                homeserver: server.uri(),
                user_id: alice_id.to_string(),
                device_id: alice_device.to_string(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        ));
        let token = koushi_sdk::current_outbound_group_session_token(&session, first_room.as_str())
            .await
            .expect("first outbound session lookup")
            .expect("first outbound session");
        let other_token =
            koushi_sdk::current_outbound_group_session_token(&session, second_room.as_str())
                .await
                .expect("second outbound session lookup")
                .expect("second outbound session");
        RoomKeyReshareFixture {
            _server: server,
            session,
            key: TimelineKey::room(AccountKey(alice_id.to_string()), first_room.as_str()),
            token,
            other_token,
        }
    }

    fn install_room_key_reshare_schedule(
        manager: &mut TimelineManagerActor,
        key: &TimelineKey,
        token: MatrixOutboundGroupSessionToken,
    ) {
        let tasks = ROOM_KEY_RESHARE_ATTEMPTS
            .iter()
            .map(|attempt| RoomKeyReshareTaskSlot {
                attempt: attempt.number,
                delayed: executor::spawn(async {}),
                started: false,
                worker: None,
            })
            .collect();
        manager.send_enqueue_workers.room_key_reshares.insert(
            key.clone(),
            RoomKeyReshareSchedule {
                session: token,
                tasks,
            },
        );
    }

    fn install_pending_room_key_reshare_worker(
        manager: &mut TimelineManagerActor,
        key: &TimelineKey,
        generation: u64,
        token: MatrixOutboundGroupSessionToken,
        attempt: u8,
    ) {
        let worker = spawn_room_key_reshare_task_with_operation(
            manager.account_work.clone(),
            manager.msg_tx.clone(),
            key.clone(),
            generation,
            token.clone(),
            MatrixRoomKeyReshareTarget::OwnOtherDevices,
            attempt,
            Box::pin(std::future::pending()),
            None,
        );
        install_room_key_reshare_schedule(manager, key, token);
        let schedule = manager
            .send_enqueue_workers
            .room_key_reshares
            .get_mut(key)
            .expect("reshare schedule");
        let slot = schedule
            .tasks
            .iter_mut()
            .find(|slot| slot.attempt == attempt)
            .expect("reshare attempt slot");
        slot.started = true;
        slot.worker = Some(worker);
    }

    fn install_controlled_room_key_reshare_worker(
        manager: &mut TimelineManagerActor,
        key: &TimelineKey,
        generation: u64,
        token: MatrixOutboundGroupSessionToken,
        attempt: u8,
    ) -> (
        oneshot::Sender<RoomKeyReshareCompletion>,
        oneshot::Receiver<()>,
        oneshot::Receiver<()>,
    ) {
        let (completion_tx, completion_rx) = oneshot::channel();
        let (acquire_started_tx, acquire_started_rx) = oneshot::channel();
        let (permit_acquired_tx, permit_acquired_rx) = oneshot::channel();
        let worker = spawn_room_key_reshare_task_with_operation(
            manager.account_work.clone(),
            manager.msg_tx.clone(),
            key.clone(),
            generation,
            token.clone(),
            MatrixRoomKeyReshareTarget::OwnOtherDevices,
            attempt,
            Box::pin(async move {
                completion_rx
                    .await
                    .unwrap_or(RoomKeyReshareCompletion::SdkError)
            }),
            Some(RoomKeyReshareTestSignals {
                acquire_started: acquire_started_tx,
                permit_acquired: permit_acquired_tx,
            }),
        );
        install_room_key_reshare_schedule(manager, key, token);
        let schedule = manager
            .send_enqueue_workers
            .room_key_reshares
            .get_mut(key)
            .expect("reshare schedule");
        let slot = schedule
            .tasks
            .iter_mut()
            .find(|slot| slot.attempt == attempt)
            .expect("reshare attempt slot");
        slot.started = true;
        slot.worker = Some(worker);
        (completion_tx, acquire_started_rx, permit_acquired_rx)
    }

    #[test]
    fn room_key_reshare_handler_does_not_hold_the_manager_on_sdk_work() {
        let source = include_str!("room_key_recovery.rs");
        let handler = source
            .split("fn handle_room_key_reshare(\n")
            .nth(1)
            .expect("room-key reshare handler")
            .split("fn take_room_key_reshare_worker")
            .next()
            .expect("room-key reshare handler boundary");
        assert!(
            !handler.contains(".await") && !handler.contains("force_reshare_room_key"),
            "the stable manager handler must only validate and launch owned work"
        );
        assert!(
            source.contains("RoomKeyReshareCompleted"),
            "reshare SDK results must return through a private completion message"
        );
    }

    #[test]
    fn decrypt_retry_diagnostics_are_fixed_token_and_private_data_free() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let operation = 48_217;

        record_decrypt_retry_request(
            operation,
            1,
            DecryptRetryReason::MissingRoomKey,
            DecryptRetryBackupState::Available,
            Duration::ZERO,
        );
        record_decrypt_retry_backup_lookup(
            operation,
            DecryptRetryBackupResult::Found,
            Duration::ZERO,
        );
        record_decrypt_retry_device_request(
            operation,
            DecryptRetryDeviceResult::Failed,
            Some(DecryptRetryFailure::Forbidden),
            Duration::ZERO,
        );
        record_decrypt_retry_settled(
            operation,
            DecryptRetrySettledResult::StillMissing,
            Duration::ZERO,
        );

        let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
        let records = diagnostics
            .records
            .iter()
            .filter(|record| {
                record.event.source == "core.decrypt_retry"
                    && record.event.fields.iter().any(|field| {
                        field.key == "operation"
                            && field.value == DiagnosticValue::Correlation(operation)
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .map(|record| (record.event.stage, &record.event.fields))
                .collect::<Vec<_>>(),
            vec![
                ("request", &records[0].event.fields),
                ("backup_lookup", &records[1].event.fields),
                ("device_request", &records[2].event.fields),
                ("settled", &records[3].event.fields),
            ]
        );
        for record in &records {
            assert_eq!(record.event.source, "core.decrypt_retry");
            assert!(record.event.fields.iter().any(|field| {
                field.key == "operation" && field.value == DiagnosticValue::Correlation(operation)
            }));
        }
        assert!(records[0].event.fields.iter().any(|field| {
            field.key == "reason" && field.value == DiagnosticValue::Token("missing_room_key")
        }));
        assert!(records[1].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("found")
        }));
        assert!(records[2].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("failed")
        }));
        assert!(records[2].event.fields.iter().any(|field| {
            field.key == "failure" && field.value == DiagnosticValue::Token("forbidden")
        }));
        assert!(records[3].event.fields.iter().any(|field| {
            field.key == "result" && field.value == DiagnosticValue::Token("still_missing")
        }));

        let serialized = serde_json::to_string(&records).expect("serialize diagnostics");
        for forbidden in [
            "!synthetic-room:example.invalid",
            "$synthetic-event:example.invalid",
            "@synthetic-user:example.invalid",
            "SYNTHETICDEVICE",
            "synthetic-session-id",
            "synthetic message body",
            "https://private.example.invalid",
            "/Users/member/private/store",
            "private-token",
            "recovery-key",
            "backup-version",
            "raw SDK error",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "diagnostic leaked {forbidden}"
            );
        }
    }

    #[test]
    fn decrypt_retry_controller_fences_deadline_settlement_and_replacement() {
        let mut controller = DecryptRetryController::default();
        let admitted_at = executor::Instant::now();
        let (first, replaced, coalesced) = controller.admit("$event-a:test", 7, admitted_at);
        assert!(replaced.is_none());
        assert!(!coalesced);
        assert!(first.deadline > admitted_at);
        assert!(controller.is_current(first.operation, 7));
        let (same, replaced, coalesced) =
            controller.admit("$event-a:test", 7, executor::Instant::now());
        assert!(coalesced);
        assert!(replaced.is_none());
        assert_eq!(same.operation, first.operation);

        assert!(
            controller
                .settle_if_current(first.operation, 8, DecryptRetrySettledResult::Decrypted)
                .is_none()
        );
        assert!(
            controller
                .settle_if_current(
                    first.operation.wrapping_add(1),
                    7,
                    DecryptRetrySettledResult::Timeout
                )
                .is_none()
        );
        assert!(controller.is_current(first.operation, 7));

        let (second, replaced, coalesced) =
            controller.admit("$event-b:test", 7, executor::Instant::now());
        assert!(!coalesced);
        assert_eq!(
            replaced.map(|pending| pending.operation),
            Some(first.operation)
        );
        assert!(!controller.is_current(first.operation, 7));
        assert!(controller.is_current(second.operation, 7));

        assert!(
            controller
                .settle_if_current(second.operation, 8, DecryptRetrySettledResult::Decrypted)
                .is_none()
        );
        let settled = controller
            .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Decrypted)
            .expect("current operation settles exactly once");
        assert_eq!(settled.pending.operation, second.operation);
        assert!(matches!(
            settled.result,
            DecryptRetrySettledResult::Decrypted
        ));
        assert!(!controller.is_current(second.operation, 7));
        assert!(
            controller
                .settle_if_current(second.operation, 7, DecryptRetrySettledResult::Timeout)
                .is_none()
        );
    }

    #[test]
    fn room_key_request_state_tokens_are_closed_and_serde_stable() {
        // Every internal stage literal maps to a closed wire token, and the
        // DTO serializes with the exact tokens the TypeScript union declares.
        let stage_cases = [
            ("sent", "sent"),
            ("automatic", "automatic"),
            ("still_waiting", "still_waiting"),
            ("withheld", "withheld"),
            ("decryption_recovered", "decryption_recovered"),
            ("send_failed", "send_failed"),
        ];
        for (literal, wire) in stage_cases {
            let serialized = serde_json::to_string(&key_request_stage_token(literal)).unwrap();
            assert_eq!(serialized, format!("\"{wire}\""));
        }
        let code_cases = [
            ("blacklisted", "blacklisted"),
            ("unverified", "unverified"),
            ("unauthorised", "unauthorised"),
            ("unavailable", "unavailable"),
        ];
        for (literal, wire) in code_cases {
            let serialized =
                serde_json::to_string(&key_request_withheld_code_token(literal)).unwrap();
            assert_eq!(serialized, format!("\"{wire}\""));
        }
        // Unknown / custom codes carry no specific copy: they map to None.
        assert!(key_request_withheld_code_token("custom").is_none());
        let dto = RoomKeyRequestStateDto {
            stage: key_request_stage_token("withheld"),
            withheld_code: key_request_withheld_code_token("unavailable"),
        };
        assert_eq!(
            serde_json::to_string(&dto).unwrap(),
            "{\"stage\":\"withheld\",\"withheldCode\":\"unavailable\"}"
        );
    }

    #[test]
    fn withheld_update_guard_allows_typed_code_and_never_regresses_terminal_stages() {
        // Stage settled withheld by a diff without a code still gains it.
        assert!(withheld_update_should_publish(
            "withheld",
            None,
            "unavailable"
        ));
        // A different typed code replaces the previous one.
        assert!(withheld_update_should_publish(
            "withheld",
            Some("unverified"),
            "blacklisted"
        ));
        // Duplicate observation of the same code is idempotent.
        assert!(!withheld_update_should_publish(
            "withheld",
            Some("unavailable"),
            "unavailable"
        ));
        // Non-withheld pending stages accept the refusal.
        assert!(withheld_update_should_publish("sent", None, "unavailable"));
        assert!(withheld_update_should_publish(
            "still_waiting",
            None,
            "unavailable"
        ));
        // Terminal stages are never regressed by a late observation.
        assert!(!withheld_update_should_publish(
            "decryption_recovered",
            None,
            "unavailable"
        ));
        assert!(!withheld_update_should_publish(
            "send_failed",
            None,
            "unavailable"
        ));
    }

    #[test]
    fn room_key_request_state_changed_debug_redacts_identifiers() {
        let event = CoreEvent::Room(crate::event::RoomEvent::RoomKeyRequestStateChanged {
            key: TimelineKey::room(
                crate::ids::AccountKey("@secret-account:example.invalid".to_owned()),
                "!secret-room:example.invalid",
            ),
            event_id: "$secret-event:example.invalid".to_owned(),
            request_id: None,
            stage: RoomKeyRequestStage::Withheld,
            withheld_code: Some(RoomKeyRequestWithheldCode::Unverified),
        });
        let rendered = format!("{event:?}");
        assert!(!rendered.contains("secret-account"));
        assert!(!rendered.contains("secret-room"));
        assert!(!rendered.contains("secret-event"));
        assert!(rendered.contains("withheld"));
    }

    #[test]
    fn decrypt_retry_diff_settlement_requires_current_generation_and_matching_event() {
        let mut controller = DecryptRetryController::default();
        let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

        assert_eq!(
            decrypt_retry_settlement_operation(&controller, 8, "$event:test"),
            None
        );
        assert_eq!(
            decrypt_retry_settlement_operation(&controller, 7, "$other:test"),
            None
        );
        assert_eq!(
            decrypt_retry_settlement_operation(&controller, 7, "$event:test"),
            Some(pending.operation)
        );
    }

    #[test]
    fn decrypt_retry_timeout_message_settles_current_operation_once() {
        let mut controller = DecryptRetryController::default();
        let (pending, _, _) = controller.admit("$event:test", 7, executor::Instant::now());

        let settled = controller
            .settle_timeout_if_current(pending.operation, 7)
            .expect("current timeout settles");
        assert!(matches!(settled.result, DecryptRetrySettledResult::Timeout));
        assert!(
            controller
                .settle_timeout_if_current(pending.operation, 7)
                .is_none()
        );
    }

    #[test]
    fn decrypt_retry_backup_state_only_reports_available_for_ready_local_recovery() {
        assert_eq!(
            decrypt_retry_backup_state_for(
                koushi_sdk::MatrixSecureBackupLocalState::Enabled,
                koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
            )
            .token(),
            "available"
        );
        for state in [
            (
                koushi_sdk::MatrixSecureBackupLocalState::Unknown,
                koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
            ),
            (
                koushi_sdk::MatrixSecureBackupLocalState::Enabled,
                koushi_sdk::MatrixSecureBackupRecoveryState::Unknown,
            ),
            (
                koushi_sdk::MatrixSecureBackupLocalState::Downloading,
                koushi_sdk::MatrixSecureBackupRecoveryState::Enabled,
            ),
        ] {
            assert_eq!(
                decrypt_retry_backup_state_for(state.0, state.1).token(),
                "unknown"
            );
        }
    }

    #[test]
    fn decrypt_retry_operation_sequence_is_process_wide_and_monotonic() {
        let first = next_decrypt_retry_operation();
        let second = next_decrypt_retry_operation();
        assert!(second > first);
    }

    #[test]
    fn decrypt_retry_backup_failures_keep_typed_private_kinds() {
        for (kind, expected) in [
            (
                koushi_sdk::E2eeTrustFailureKind::Network,
                DecryptRetryBackupResult::Network,
            ),
            (
                koushi_sdk::E2eeTrustFailureKind::Forbidden,
                DecryptRetryBackupResult::Forbidden,
            ),
            (
                koushi_sdk::E2eeTrustFailureKind::InvalidBackup,
                DecryptRetryBackupResult::InvalidBackup,
            ),
            (
                koushi_sdk::E2eeTrustFailureKind::Timeout,
                DecryptRetryBackupResult::Timeout,
            ),
            (
                koushi_sdk::E2eeTrustFailureKind::Sdk,
                DecryptRetryBackupResult::Sdk,
            ),
        ] {
            assert!(matches!(
                decrypt_retry_backup_result_for_error(&koushi_sdk::E2eeTrustError::Classified(
                    kind
                )),
                result if result.token() == expected.token()
            ));
        }
    }

    #[test]
    fn decrypt_retry_diagnostics_use_only_the_planned_outcome_tokens() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let operation = 48_218;

        record_decrypt_retry_request(
            operation,
            2,
            DecryptRetryReason::MissingRoomKey,
            DecryptRetryBackupState::Available,
            Duration::ZERO,
        );
        for result in [
            DecryptRetryBackupResult::Found,
            DecryptRetryBackupResult::NotFound,
            DecryptRetryBackupResult::Network,
            DecryptRetryBackupResult::Forbidden,
            DecryptRetryBackupResult::InvalidBackup,
            DecryptRetryBackupResult::Timeout,
            DecryptRetryBackupResult::Sdk,
        ] {
            record_decrypt_retry_backup_lookup(operation, result, Duration::ZERO);
        }
        record_decrypt_retry_device_request(
            operation,
            DecryptRetryDeviceResult::Sent,
            None,
            Duration::ZERO,
        );
        for failure in [
            DecryptRetryFailure::Network,
            DecryptRetryFailure::Forbidden,
            DecryptRetryFailure::Timeout,
            DecryptRetryFailure::Sdk,
        ] {
            record_decrypt_retry_device_request(
                operation,
                DecryptRetryDeviceResult::Failed,
                Some(failure),
                Duration::ZERO,
            );
        }
        for result in [
            DecryptRetrySettledResult::Decrypted,
            DecryptRetrySettledResult::StillMissing,
            DecryptRetrySettledResult::Withheld,
            DecryptRetrySettledResult::Malformed,
            DecryptRetrySettledResult::Timeout,
            DecryptRetrySettledResult::Superseded,
        ] {
            record_decrypt_retry_settled(operation, result, Duration::ZERO);
        }

        let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
        let tokens = diagnostics
            .records
            .iter()
            .filter(|record| {
                record.event.source == "core.decrypt_retry"
                    && record.event.fields.iter().any(|field| {
                        field.key == "operation"
                            && field.value == DiagnosticValue::Correlation(operation)
                    })
            })
            .flat_map(|record| record.event.fields.iter())
            .filter_map(|field| match field.value {
                DiagnosticValue::Token(token) => Some((field.key, token)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for expected in [
            ("backup_state", "available"),
            ("result", "found"),
            ("result", "not_found"),
            ("result", "network"),
            ("result", "forbidden"),
            ("result", "invalid_backup"),
            ("result", "timeout"),
            ("result", "sdk"),
            ("failure", "network"),
            ("failure", "forbidden"),
            ("failure", "timeout"),
            ("failure", "sdk"),
            ("result", "decrypted"),
            ("result", "still_missing"),
            ("result", "withheld"),
            ("result", "malformed"),
            ("result", "superseded"),
        ] {
            assert!(
                tokens.contains(&expected),
                "missing fixed token {expected:?}"
            );
        }
    }

    #[test]
    fn room_key_reshare_diagnostics_include_attempt_target_and_result() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let diagnostic_start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();

        record_room_key_reshare(
            "own_device_retry_1",
            "sent",
            1,
            MatrixRoomKeyReshareTarget::OwnOtherDevices,
            3,
            2,
            5,
            1,
        );

        let diagnostics = koushi_diagnostics::test_support::detail_snapshot();
        let record = diagnostics.records[diagnostic_start..]
            .iter()
            .find(|record| {
                record.event.source == "core.room_key_reshare" && record.event.stage == "attempt"
            })
            .expect("room-key reshare diagnostic");
        for (key, value) in [
            ("attempt", DiagnosticValue::Count(1)),
            ("target", DiagnosticValue::Token("own_other_devices")),
            ("delay_seconds", DiagnosticValue::Count(3)),
            ("request_count", DiagnosticValue::Count(2)),
            ("recipient_count", DiagnosticValue::Count(5)),
        ] {
            assert!(
                record
                    .event
                    .fields
                    .iter()
                    .any(|field| { field.key == key && field.value == value }),
                "missing {key}"
            );
        }
        assert!(record.event.fields.iter().all(|field| {
            !matches!(
                field.key,
                "room_id"
                    | "event_id"
                    | "user_id"
                    | "device_id"
                    | "session_id"
                    | "transaction_id"
                    | "request_id"
                    | "message"
                    | "key"
                    | "key_material"
            )
        }));
    }
}
