use super::actor::{ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT, RoomActor};
use super::operations::classify_room_error;
use crate::account_work::AccountWorkKind;
use crate::executor;
use futures_util::FutureExt;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_protocol::event::{
    CoreEvent, EncryptionDebugOperationOutcome as CoreEncryptionDebugOutcome, RoomEvent,
    RoomKeyReshareOutcome,
};
use koushi_protocol::failure::CoreFailure;
use koushi_protocol::ids::RequestId;
use koushi_state::{AppAction, EncryptionDebugOperationKind};
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::Mutex;
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::{broadcast, oneshot};

fn room_key_reshare_outcome_from_sdk(
    outcome: koushi_sdk::MatrixRoomKeyReshareOutcome,
) -> RoomKeyReshareOutcome {
    match outcome {
        koushi_sdk::MatrixRoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        } => RoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        },
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoSession => RoomKeyReshareOutcome::NoSession,
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoRecipients => {
            RoomKeyReshareOutcome::NoRecipients
        }
        koushi_sdk::MatrixRoomKeyReshareOutcome::StaleSession => {
            RoomKeyReshareOutcome::StaleSession
        }
    }
}

fn record_manual_room_key_reshare(outcome: &koushi_sdk::MatrixRoomKeyReshareOutcome) {
    let (token, request_count, recipient_count, failed_recipient_count) = match outcome {
        koushi_sdk::MatrixRoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        } => (
            "sent",
            *request_count,
            *recipient_count,
            *failed_recipient_count,
        ),
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoSession => ("no_session", 0, 0, 0),
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoRecipients => ("no_recipients", 0, 0, 0),
        koushi_sdk::MatrixRoomKeyReshareOutcome::StaleSession => ("cancelled", 0, 0, 0),
    };
    record(
        DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_reshare", "attempt")
            .field(DiagnosticField::token("trigger", "manual"))
            .field(DiagnosticField::token("outcome", token))
            .field(DiagnosticField::count(
                "request_count",
                request_count.try_into().unwrap_or(u64::MAX),
            ))
            .field(DiagnosticField::count(
                "recipient_count",
                recipient_count.try_into().unwrap_or(u64::MAX),
            ))
            .field(DiagnosticField::count(
                "failed_recipient_count",
                failed_recipient_count.try_into().unwrap_or(u64::MAX),
            )),
    );
}

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) struct EncryptionDebugTestControl {
    pub(crate) kind: EncryptionDebugOperationKind,
    pub(crate) reached: oneshot::Sender<()>,
    pub(crate) completion: oneshot::Receiver<CoreEncryptionDebugOutcome>,
}

#[cfg(any(test, feature = "test-hooks"))]
pub(super) type EncryptionDebugTestControlSlot = Arc<Mutex<Option<EncryptionDebugTestControl>>>;

#[cfg(any(test, feature = "test-hooks"))]
fn take_encryption_debug_test_control(
    control: &mut Option<EncryptionDebugTestControl>,
    kind: EncryptionDebugOperationKind,
) -> Option<EncryptionDebugTestControl> {
    if control.as_ref().is_some_and(|control| control.kind == kind) {
        control.take()
    } else {
        None
    }
}

/// Fence for the in-flight temporary dangerous encryption-debug operation
/// (issue #538). Holds the cancellation sender (so logout/leave can stop
/// the SDK executor's wire effects), the actor session snapshot (post-check
/// fails closed if the session changed), and the spawned task handle for
/// bounded join on teardown.
pub(super) struct EncryptionDebugFence {
    request_id: RequestId,
    room_id: String,
    kind: EncryptionDebugOperationKind,
    session: Arc<koushi_sdk::MatrixClientSession>,
    cancel: broadcast::Sender<()>,
    /// Actor-owned lifecycle flag: set on logout/leave so the spawned task's
    /// validator fails closed before further wire effects.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    join: executor::JoinHandle<()>,
}

/// Reliable completion result of the encryption-debug operation task.
pub(super) struct EncryptionDebugCompletion {
    room_id: String,
    request_id: RequestId,
    kind: EncryptionDebugOperationKind,
    outcome: CoreEncryptionDebugOutcome,
}

impl RoomActor {
    /// Fence-verified settlement of a queued encryption-debug completion:
    /// inspect the fence first and take it only after request/room/kind
    /// match, so a stale completion cannot consume an unrelated replacement
    /// fence. Re-checks joined membership and the session pointer before
    /// settling.
    pub(super) async fn handle_encryption_debug_completion(
        &mut self,
        completion: EncryptionDebugCompletion,
    ) {
        let EncryptionDebugCompletion {
            room_id,
            request_id,
            kind,
            outcome,
        } = completion;
        let Some(fence) = self.encryption_debug_fences.get(&room_id) else {
            return;
        };
        if fence.request_id != request_id || fence.room_id != room_id || fence.kind != kind {
            return;
        }
        let fence = self
            .encryption_debug_fences
            .remove(&room_id)
            .expect("matched fence");
        let joined = match koushi_sdk::room_is_joined(&fence.session, &room_id).await {
            Ok(joined) => joined,
            Err(_) => false,
        };
        let outcome = if joined
            && self
                .session
                .as_ref()
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, &fence.session))
        {
            outcome
        } else {
            // The session changed or the user left the room while the
            // operation ran; fail closed rather than apply the result.
            CoreEncryptionDebugOutcome::CancelledStale
        };
        self.emit_encryption_debug_outcome(request_id, room_id, kind, outcome)
            .await;
    }

    /// Cancel and settle encryption-debug operations invalidated by room
    /// removal or account lifecycle changes.
    pub(super) async fn cancel_encryption_debug_for_rooms(&mut self, room_ids: &BTreeSet<String>) {
        let fences = room_ids
            .iter()
            .filter_map(|room_id| {
                self.encryption_debug_fences
                    .remove(room_id)
                    .map(|fence| (room_id.clone(), fence))
            })
            .collect::<Vec<_>>();
        for (room_id, mut fence) in fences {
            fence
                .cancelled
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = fence.cancel.send(());
            if tokio::time::timeout(ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT, &mut fence.join)
                .await
                .is_err()
            {
                fence.join.abort();
                let _ = fence.join.await;
            }
            self.emit_encryption_debug_outcome(
                fence.request_id,
                room_id.clone(),
                fence.kind,
                CoreEncryptionDebugOutcome::CancelledStale,
            )
            .await;
            self.reduce_reliable(vec![AppAction::EncryptionDebugOperationReset { room_id }])
                .await;
        }
    }

    pub(super) async fn handle_reshare_room_key(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let _interactive = self
            .account_work
            .begin_interactive(AccountWorkKind::UserRoomOperation);
        match koushi_sdk::reshare_room_key(session, &room_id).await {
            Ok(outcome) => {
                record_manual_room_key_reshare(&outcome);
                self.emit(CoreEvent::Room(RoomEvent::RoomKeyReshared {
                    request_id,
                    room_id,
                    outcome: room_key_reshare_outcome_from_sdk(outcome),
                }));
            }
            Err(error) => {
                let outcome = if error.failure_kind()
                    == Some(koushi_sdk::MatrixRoomOperationFailureKind::Http)
                {
                    "network_error"
                } else {
                    "sdk_error"
                };
                record(
                    DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_reshare", "attempt")
                        .field(DiagnosticField::token("trigger", "manual"))
                        .field(DiagnosticField::token("outcome", outcome)),
                );
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    fn map_force_new_outcome(
        outcome: koushi_sdk::MatrixForceNewSessionOutcome,
    ) -> CoreEncryptionDebugOutcome {
        match outcome {
            koushi_sdk::MatrixForceNewSessionOutcome::Completed => {
                CoreEncryptionDebugOutcome::Completed
            }
            koushi_sdk::MatrixForceNewSessionOutcome::RefusedNotEncrypted => {
                CoreEncryptionDebugOutcome::RefusedNotEncrypted
            }
            koushi_sdk::MatrixForceNewSessionOutcome::CancelledStale => {
                CoreEncryptionDebugOutcome::CancelledStale
            }
            koushi_sdk::MatrixForceNewSessionOutcome::Failed => CoreEncryptionDebugOutcome::Failed,
            koushi_sdk::MatrixForceNewSessionOutcome::Deadline => {
                CoreEncryptionDebugOutcome::Deadline
            }
        }
    }

    fn map_index0_share_outcome(
        outcome: koushi_sdk::MatrixIndex0ShareOutcome,
    ) -> CoreEncryptionDebugOutcome {
        match outcome {
            koushi_sdk::MatrixIndex0ShareOutcome::Completed => {
                CoreEncryptionDebugOutcome::Completed
            }
            koushi_sdk::MatrixIndex0ShareOutcome::RefusedNotEncrypted => {
                CoreEncryptionDebugOutcome::RefusedNotEncrypted
            }
            koushi_sdk::MatrixIndex0ShareOutcome::RefusedIndexAdvanced => {
                CoreEncryptionDebugOutcome::RefusedIndexAdvanced
            }
            koushi_sdk::MatrixIndex0ShareOutcome::NoSession => CoreEncryptionDebugOutcome::Failed,
            koushi_sdk::MatrixIndex0ShareOutcome::NoRecipients => {
                CoreEncryptionDebugOutcome::Failed
            }
            koushi_sdk::MatrixIndex0ShareOutcome::PolicyBlocked => {
                CoreEncryptionDebugOutcome::PolicyBlocked
            }
            koushi_sdk::MatrixIndex0ShareOutcome::CancelledStale => {
                CoreEncryptionDebugOutcome::CancelledStale
            }
            koushi_sdk::MatrixIndex0ShareOutcome::Deadline => CoreEncryptionDebugOutcome::Deadline,
            koushi_sdk::MatrixIndex0ShareOutcome::Failed => CoreEncryptionDebugOutcome::Failed,
        }
    }

    fn map_index0_resend_outcome(
        outcome: koushi_sdk::MatrixIndex0ResendOutcome,
    ) -> CoreEncryptionDebugOutcome {
        match outcome {
            koushi_sdk::MatrixIndex0ResendOutcome::Completed => {
                CoreEncryptionDebugOutcome::Completed
            }
            koushi_sdk::MatrixIndex0ResendOutcome::RefusedNotEncrypted => {
                CoreEncryptionDebugOutcome::RefusedNotEncrypted
            }
            koushi_sdk::MatrixIndex0ResendOutcome::NoSession => {
                CoreEncryptionDebugOutcome::NoSession
            }
            koushi_sdk::MatrixIndex0ResendOutcome::InboundSessionMissing => {
                CoreEncryptionDebugOutcome::InboundSessionMissing
            }
            koushi_sdk::MatrixIndex0ResendOutcome::InboundIndexAdvanced => {
                CoreEncryptionDebugOutcome::InboundIndexAdvanced
            }
            koushi_sdk::MatrixIndex0ResendOutcome::OriginalLedgerMissing => {
                CoreEncryptionDebugOutcome::OriginalLedgerMissing
            }
            koushi_sdk::MatrixIndex0ResendOutcome::NoRecipients => {
                CoreEncryptionDebugOutcome::NoRecipients
            }
            koushi_sdk::MatrixIndex0ResendOutcome::PolicyBlocked => {
                CoreEncryptionDebugOutcome::PolicyBlocked
            }
            koushi_sdk::MatrixIndex0ResendOutcome::StaleIdentityRefused => {
                CoreEncryptionDebugOutcome::StaleIdentityRefused
            }
            koushi_sdk::MatrixIndex0ResendOutcome::CancelledStale => {
                CoreEncryptionDebugOutcome::CancelledStale
            }
            koushi_sdk::MatrixIndex0ResendOutcome::Deadline => CoreEncryptionDebugOutcome::Deadline,
            koushi_sdk::MatrixIndex0ResendOutcome::Failed => CoreEncryptionDebugOutcome::Failed,
        }
    }

    fn index0_resend_outcome_token(outcome: koushi_sdk::MatrixIndex0ResendOutcome) -> &'static str {
        match outcome {
            koushi_sdk::MatrixIndex0ResendOutcome::Completed => "completed",
            koushi_sdk::MatrixIndex0ResendOutcome::RefusedNotEncrypted => "refused_not_encrypted",
            koushi_sdk::MatrixIndex0ResendOutcome::NoSession => "no_session",
            koushi_sdk::MatrixIndex0ResendOutcome::InboundSessionMissing => {
                "inbound_session_missing"
            }
            koushi_sdk::MatrixIndex0ResendOutcome::InboundIndexAdvanced => "inbound_index_advanced",
            koushi_sdk::MatrixIndex0ResendOutcome::OriginalLedgerMissing => {
                "original_ledger_missing"
            }
            koushi_sdk::MatrixIndex0ResendOutcome::NoRecipients => "no_recipients",
            koushi_sdk::MatrixIndex0ResendOutcome::PolicyBlocked => "policy_blocked",
            koushi_sdk::MatrixIndex0ResendOutcome::StaleIdentityRefused => "stale_identity_refused",
            koushi_sdk::MatrixIndex0ResendOutcome::CancelledStale => "cancelled_stale",
            koushi_sdk::MatrixIndex0ResendOutcome::Deadline => "deadline",
            koushi_sdk::MatrixIndex0ResendOutcome::Failed => "failed",
        }
    }

    fn record_index0_resend_failed() {
        Self::record_index0_resend_diagnostic(&koushi_sdk::MatrixIndex0ResendSummary {
            outcome: koushi_sdk::MatrixIndex0ResendOutcome::Failed,
            message_index_before: None,
            message_index_after: None,
            peer_ledger: 0,
            peer_sender_key_changed: 0,
            peer_eligible: 0,
            peer_accepted: 0,
            peer_missing: 0,
            policy_blocked: 0,
            inbound_first_known_index: None,
            claim: koushi_sdk::MatrixIndex0ClaimOutcome::NotNeeded,
            elapsed_ms: 0,
            room_event_sent: false,
            index0_consumed: false,
        });
    }

    fn record_index0_resend_diagnostic(summary: &koushi_sdk::MatrixIndex0ResendSummary) {
        record(
            DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_debug", "operation")
                .field(DiagnosticField::token("operation", "resend_index0"))
                .field(DiagnosticField::token(
                    "outcome",
                    Self::index0_resend_outcome_token(summary.outcome),
                ))
                .field(DiagnosticField::optional_count(
                    "index_before",
                    summary.message_index_before,
                ))
                .field(DiagnosticField::optional_count(
                    "index_after",
                    summary.message_index_after,
                ))
                .field(DiagnosticField::count(
                    "peer_ledger",
                    summary.peer_ledger.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_sender_key_changed",
                    summary
                        .peer_sender_key_changed
                        .try_into()
                        .unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_eligible",
                    summary.peer_eligible.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_accepted",
                    summary.peer_accepted.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_missing",
                    summary.peer_missing.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "policy_blocked",
                    summary.policy_blocked.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::optional_count(
                    "inbound_first_known_index",
                    summary.inbound_first_known_index,
                ))
                .field(DiagnosticField::token(
                    "claim",
                    Self::claim_outcome_token(summary.claim),
                ))
                .field(DiagnosticField::count("elapsed_ms", summary.elapsed_ms))
                .field(DiagnosticField::count("room_event_sent", 0))
                .field(DiagnosticField::count("index0_consumed", 0)),
        );
    }

    fn force_new_outcome_token(outcome: koushi_sdk::MatrixForceNewSessionOutcome) -> &'static str {
        match outcome {
            koushi_sdk::MatrixForceNewSessionOutcome::Completed => "completed",
            koushi_sdk::MatrixForceNewSessionOutcome::RefusedNotEncrypted => {
                "refused_not_encrypted"
            }
            koushi_sdk::MatrixForceNewSessionOutcome::CancelledStale => "cancelled_stale",
            koushi_sdk::MatrixForceNewSessionOutcome::Failed => "failed",
            koushi_sdk::MatrixForceNewSessionOutcome::Deadline => "deadline",
        }
    }

    fn index0_share_outcome_token(outcome: koushi_sdk::MatrixIndex0ShareOutcome) -> &'static str {
        match outcome {
            koushi_sdk::MatrixIndex0ShareOutcome::Completed => "completed",
            koushi_sdk::MatrixIndex0ShareOutcome::RefusedNotEncrypted => "refused_not_encrypted",
            koushi_sdk::MatrixIndex0ShareOutcome::RefusedIndexAdvanced => "refused_index_advanced",
            koushi_sdk::MatrixIndex0ShareOutcome::NoSession => "failed",
            koushi_sdk::MatrixIndex0ShareOutcome::NoRecipients => "failed",
            koushi_sdk::MatrixIndex0ShareOutcome::PolicyBlocked => "policy_blocked",
            koushi_sdk::MatrixIndex0ShareOutcome::CancelledStale => "cancelled_stale",
            koushi_sdk::MatrixIndex0ShareOutcome::Deadline => "deadline",
            koushi_sdk::MatrixIndex0ShareOutcome::Failed => "failed",
        }
    }

    fn claim_outcome_token(outcome: koushi_sdk::MatrixIndex0ClaimOutcome) -> &'static str {
        match outcome {
            koushi_sdk::MatrixIndex0ClaimOutcome::NotNeeded => "not_needed",
            koushi_sdk::MatrixIndex0ClaimOutcome::Succeeded => "succeeded",
            koushi_sdk::MatrixIndex0ClaimOutcome::Failed => "failed",
            koushi_sdk::MatrixIndex0ClaimOutcome::Deadline => "deadline",
        }
    }

    fn record_force_new_outbound_session_diagnostic(
        summary: &koushi_sdk::MatrixForceNewSessionSummary,
    ) {
        record(
            DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_debug", "operation")
                .field(DiagnosticField::token(
                    "operation",
                    "force_new_outbound_session",
                ))
                .field(DiagnosticField::token(
                    "outcome",
                    Self::force_new_outcome_token(summary.outcome),
                ))
                .field(DiagnosticField::count(
                    "fresh",
                    u64::from(summary.fresh_session_created),
                ))
                .field(DiagnosticField::boolean(
                    "index_after_set",
                    summary.message_index.is_some(),
                ))
                .field(DiagnosticField::count(
                    "index_after",
                    summary.message_index.map(u64::from).unwrap_or(0),
                ))
                .field(DiagnosticField::count("elapsed_ms", summary.elapsed_ms))
                .field(DiagnosticField::count("room_event_sent", 0))
                .field(DiagnosticField::count("index0_consumed", 0)),
        );
    }

    fn record_index0_share_diagnostic(summary: &koushi_sdk::MatrixIndex0ShareSummary) {
        record(
            DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_debug", "operation")
                .field(DiagnosticField::token("operation", "share_index0"))
                .field(DiagnosticField::token(
                    "outcome",
                    Self::index0_share_outcome_token(summary.outcome),
                ))
                .field(DiagnosticField::boolean(
                    "index_before_set",
                    summary.message_index_before.is_some(),
                ))
                .field(DiagnosticField::count(
                    "index_before",
                    summary.message_index_before.map(u64::from).unwrap_or(0),
                ))
                .field(DiagnosticField::boolean(
                    "index_after_set",
                    summary.message_index_after.is_some(),
                ))
                .field(DiagnosticField::count(
                    "index_after",
                    summary.message_index_after.map(u64::from).unwrap_or(0),
                ))
                .field(DiagnosticField::count(
                    "own_eligible",
                    summary.own_eligible.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "own_accepted",
                    summary.own_accepted.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "own_missing",
                    summary.own_missing.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_eligible",
                    summary.peer_eligible.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_accepted",
                    summary.peer_accepted.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_missing",
                    summary.peer_missing.try_into().unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::count(
                    "peer_users_zero_accepted",
                    summary
                        .peer_users_with_zero_accepted
                        .try_into()
                        .unwrap_or(u64::MAX),
                ))
                .field(DiagnosticField::token(
                    "claim",
                    Self::claim_outcome_token(summary.claim),
                ))
                .field(DiagnosticField::count("elapsed_ms", summary.elapsed_ms))
                .field(DiagnosticField::count("room_event_sent", 0))
                .field(DiagnosticField::count("index0_consumed", 0)),
        );
    }

    fn record_encryption_debug_failed(operation: &'static str) {
        record(
            DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_debug", "operation")
                .field(DiagnosticField::token("operation", operation))
                .field(DiagnosticField::token("outcome", "failed"))
                .field(DiagnosticField::count("room_event_sent", 0))
                .field(DiagnosticField::count("index0_consumed", 0)),
        );
    }

    pub(super) async fn handle_force_new_outbound_session(
        &mut self,
        request_id: RequestId,
        room_id: String,
    ) {
        self.handle_encryption_debug_operation(
            request_id,
            room_id,
            EncryptionDebugOperationKind::ForceNewOutboundSession,
        )
        .await;
    }

    pub(super) async fn handle_share_index0_room_key(
        &mut self,
        request_id: RequestId,
        room_id: String,
    ) {
        self.handle_encryption_debug_operation(
            request_id,
            room_id,
            EncryptionDebugOperationKind::ShareIndex0Key,
        )
        .await;
    }

    pub(super) async fn handle_resend_index0_room_key(
        &mut self,
        request_id: RequestId,
        room_id: String,
    ) {
        self.handle_encryption_debug_operation(
            request_id,
            room_id,
            EncryptionDebugOperationKind::ResendIndex0Key,
        )
        .await;
    }

    /// Shared body of the temporary dangerous encryption-debug controls
    /// (issue #538). Runs the SDK operation (bounded by the SDK's monotonic
    /// deadline), dispatches the Rust-owned state-machine actions (Started
    /// then Settled/Failed), and emits the typed RoomEvent. The session must
    /// still be the one the operation started with when it completes;
    /// otherwise the outcome is `CancelledStale`.
    async fn handle_encryption_debug_operation(
        &mut self,
        request_id: RequestId,
        room_id: String,
        kind: EncryptionDebugOperationKind,
    ) {
        // In-flight registry: at most one encryption-debug operation per
        // room; a concurrent start for the same room is rejected, while
        // other rooms remain usable (issue #538).
        if self.encryption_debug_fences.contains_key(&room_id) {
            self.emit_encryption_debug_rejection(
                request_id,
                room_id,
                kind,
                CoreEncryptionDebugOutcome::Failed,
            );
            return;
        }
        if !self
            .known_room_ids
            .read()
            .expect("known room ids lock")
            .contains(&room_id)
        {
            self.emit_encryption_debug_rejection(
                request_id,
                room_id,
                kind,
                CoreEncryptionDebugOutcome::Failed,
            );
            return;
        }
        let Some(session) = &self.session else {
            self.emit_encryption_debug_rejection(
                request_id,
                room_id,
                kind,
                CoreEncryptionDebugOutcome::CancelledStale,
            );
            return;
        };
        let task_session = std::sync::Arc::clone(session);
        self.reduce_reliable(vec![AppAction::EncryptionDebugOperationStarted {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            kind,
        }])
        .await;

        // Cancellable task: the actor loop stays responsive while the SDK
        // executor runs, so logout/leave can signal cancellation and join.
        let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(1);
        let cancelled_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_cancelled = std::sync::Arc::clone(&cancelled_flag);
        let completion_tx = self.encryption_debug_completion_tx.clone();
        let op_room_id = room_id.clone();
        let op_request_id = request_id;
        let session_for_fence = std::sync::Arc::clone(session);
        let known_room_ids = Arc::clone(&self.known_room_ids);
        #[cfg(any(test, feature = "test-hooks"))]
        let test_control = take_encryption_debug_test_control(
            &mut *self
                .encryption_debug_test_control
                .lock()
                .expect("encryption-debug test control lock"),
            kind,
        );
        #[cfg(any(test, feature = "test-hooks"))]
        if let Some(control) = test_control {
            let (cancel_tx, _cancel_rx) = broadcast::channel::<()>(1);
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let task_cancelled = Arc::clone(&cancelled);
            let completion_tx = completion_tx.clone();
            let op_room_id = op_room_id.clone();
            let join = executor::spawn(async move {
                let _ = control.reached.send(());
                let outcome = control
                    .completion
                    .await
                    .unwrap_or(CoreEncryptionDebugOutcome::CancelledStale);
                let _ = completion_tx.send(EncryptionDebugCompletion {
                    room_id: op_room_id,
                    request_id: op_request_id,
                    kind,
                    outcome,
                });
            });
            self.encryption_debug_fences.insert(
                room_id.clone(),
                EncryptionDebugFence {
                    request_id,
                    room_id,
                    kind,
                    session: session_for_fence,
                    cancel: cancel_tx,
                    cancelled: task_cancelled,
                    join,
                },
            );
            return;
        }
        let join = executor::spawn(async move {
            let outcome = std::panic::AssertUnwindSafe(async {
                match kind {
                    EncryptionDebugOperationKind::ForceNewOutboundSession => {
                        let task_known_room_ids = Arc::clone(&known_room_ids);
                        let task_room_id = op_room_id.clone();
                        let validate: Box<dyn Fn() -> bool + Send + Sync> = Box::new(move || {
                            !task_cancelled.load(std::sync::atomic::Ordering::SeqCst)
                                && task_known_room_ids
                                    .read()
                                    .is_ok_and(|room_ids| room_ids.contains(&task_room_id))
                        });
                        match koushi_sdk::force_new_outbound_session(
                            &task_session,
                            &op_room_id,
                            &mut cancel_rx,
                            validate,
                        )
                        .await
                        {
                            Ok(summary) => {
                                RoomActor::record_force_new_outbound_session_diagnostic(&summary);
                                RoomActor::map_force_new_outcome(summary.outcome)
                            }
                            Err(_) => {
                                RoomActor::record_encryption_debug_failed(
                                    "force_new_outbound_session",
                                );
                                CoreEncryptionDebugOutcome::Failed
                            }
                        }
                    }
                    EncryptionDebugOperationKind::ShareIndex0Key => {
                        let task_known_room_ids = Arc::clone(&known_room_ids);
                        let task_room_id = op_room_id.clone();
                        let validate: Box<dyn Fn() -> bool + Send + Sync> = Box::new(move || {
                            !task_cancelled.load(std::sync::atomic::Ordering::SeqCst)
                                && task_known_room_ids
                                    .read()
                                    .is_ok_and(|room_ids| room_ids.contains(&task_room_id))
                        });
                        match koushi_sdk::share_index0_room_key(
                            &task_session,
                            &op_room_id,
                            &mut cancel_rx,
                            validate,
                        )
                        .await
                        {
                            Ok(summary) => {
                                RoomActor::record_index0_share_diagnostic(&summary);
                                RoomActor::map_index0_share_outcome(summary.outcome)
                            }
                            Err(_) => {
                                RoomActor::record_encryption_debug_failed("share_index0");
                                CoreEncryptionDebugOutcome::Failed
                            }
                        }
                    }
                    EncryptionDebugOperationKind::ResendIndex0Key => {
                        let task_known_room_ids = Arc::clone(&known_room_ids);
                        let task_room_id = op_room_id.clone();
                        let validate: Box<dyn Fn() -> bool + Send + Sync> = Box::new(move || {
                            !task_cancelled.load(std::sync::atomic::Ordering::SeqCst)
                                && task_known_room_ids
                                    .read()
                                    .is_ok_and(|room_ids| room_ids.contains(&task_room_id))
                        });
                        match koushi_sdk::resend_index0_room_key(
                            &task_session,
                            &op_room_id,
                            &mut cancel_rx,
                            validate,
                        )
                        .await
                        {
                            Ok(summary) => {
                                RoomActor::record_index0_resend_diagnostic(&summary);
                                RoomActor::map_index0_resend_outcome(summary.outcome)
                            }
                            Err(_) => {
                                RoomActor::record_index0_resend_failed();
                                CoreEncryptionDebugOutcome::Failed
                            }
                        }
                    }
                }
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                if kind == EncryptionDebugOperationKind::ResendIndex0Key {
                    RoomActor::record_index0_resend_failed();
                } else {
                    RoomActor::record_encryption_debug_failed("encryption_debug_panic");
                }
                CoreEncryptionDebugOutcome::Failed
            });
            // Reliable nonblocking completion lane (unbounded): the actor
            // may be mid-teardown (SessionCleared joins this task); teardown
            // settles inline, and a queued completion is consumed by the
            // select loop and dropped as stale if the fence is gone.
            let _ = completion_tx.send(EncryptionDebugCompletion {
                room_id: op_room_id,
                request_id: op_request_id,
                kind,
                outcome,
            });
        });
        self.encryption_debug_fences.insert(
            room_id.clone(),
            EncryptionDebugFence {
                request_id,
                room_id,
                kind,
                session: session_for_fence,
                cancel: cancel_tx,
                cancelled: cancelled_flag,
                join,
            },
        );
    }

    fn emit_encryption_debug_rejection(
        &self,
        request_id: RequestId,
        room_id: String,
        kind: EncryptionDebugOperationKind,
        outcome: CoreEncryptionDebugOutcome,
    ) {
        match kind {
            EncryptionDebugOperationKind::ResendIndex0Key => Self::record_index0_resend_failed(),
            EncryptionDebugOperationKind::ForceNewOutboundSession => {
                Self::record_encryption_debug_failed("force_new_outbound_session");
            }
            EncryptionDebugOperationKind::ShareIndex0Key => {
                Self::record_encryption_debug_failed("share_index0");
            }
        }
        let event = match kind {
            EncryptionDebugOperationKind::ForceNewOutboundSession => {
                CoreEvent::Room(RoomEvent::OutboundSessionForced {
                    request_id,
                    room_id,
                    outcome,
                })
            }
            EncryptionDebugOperationKind::ShareIndex0Key => {
                CoreEvent::Room(RoomEvent::Index0RoomKeyShared {
                    request_id,
                    room_id,
                    outcome,
                })
            }
            EncryptionDebugOperationKind::ResendIndex0Key => {
                CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
                    request_id,
                    room_id,
                    outcome,
                })
            }
        };
        self.emit(event);
    }

    async fn emit_encryption_debug_outcome(
        &self,
        request_id: RequestId,
        room_id: String,
        kind: EncryptionDebugOperationKind,
        outcome: CoreEncryptionDebugOutcome,
    ) {
        let event = match kind {
            EncryptionDebugOperationKind::ForceNewOutboundSession => {
                CoreEvent::Room(RoomEvent::OutboundSessionForced {
                    request_id,
                    room_id: room_id.clone(),
                    outcome,
                })
            }
            EncryptionDebugOperationKind::ShareIndex0Key => {
                CoreEvent::Room(RoomEvent::Index0RoomKeyShared {
                    request_id,
                    room_id: room_id.clone(),
                    outcome,
                })
            }
            EncryptionDebugOperationKind::ResendIndex0Key => {
                CoreEvent::Room(RoomEvent::Index0RoomKeyResent {
                    request_id,
                    room_id: room_id.clone(),
                    outcome,
                })
            }
        };
        self.emit(event);
        let action = match outcome {
            CoreEncryptionDebugOutcome::Completed => AppAction::EncryptionDebugOperationSettled {
                request_id: request_id.sequence,
                room_id,
                kind,
                outcome,
            },
            _ => AppAction::EncryptionDebugOperationFailed {
                request_id: request_id.sequence,
                room_id,
                kind,
                outcome,
            },
        };
        self.reduce_reliable(vec![action]).await;
    }
}

#[cfg(test)]
mod tests;
