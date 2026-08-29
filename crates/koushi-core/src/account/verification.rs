//! `verification` ownership for AccountActor.

use std::{future::Future, sync::Arc, time::Duration};

use futures_util::StreamExt;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel};
use koushi_sdk::MatrixClientSession;
use koushi_state::{
    AppAction, SasEmoji, TrustOperationFailureKind, VerificationCancelReason,
    VerificationFlowState, VerificationTarget,
};
use tokio::sync::{mpsc, oneshot};

use crate::event::{CoreEvent, E2eeTrustEvent};
use crate::executor;
use crate::failure::{CoreFailure, RecoveryFailureKind};
use crate::ids::{AccountKey, RequestId, RuntimeConnectionId};

use super::actor::{AccountActor, AccountMessage};
use super::local_data_cleanup::record_device_cleanup_offer;
use super::recovery_backup::{
    classify_e2ee_trust_error, project_identity_reset_failed_event,
    project_reset_identity_completed, project_reset_identity_error,
};

const IDENTITY_RESET_AUTH_TIMEOUT: Duration = Duration::from_secs(300);

const INCOMING_VERIFICATION_OBSERVER_JOIN_TIMEOUT: Duration = Duration::from_millis(100);

pub(super) const INCOMING_VERIFICATION_FLOW_ID_BASE: u64 = 1 << 63;

pub(super) struct VerificationObservation {
    pub(super) stop_tx: oneshot::Sender<()>,
    pub(super) task: crate::executor::JoinHandle<()>,
}

pub(super) struct IncomingVerificationObservation {
    stop_tx: oneshot::Sender<()>,
    task: crate::executor::JoinHandle<()>,
    observer: koushi_sdk::MatrixIncomingVerificationRequestObserver,
}

/// Session-owned observers must race every blocking outbound delivery against
/// their stop signal. Normal operation still awaits reliable mailbox delivery;
/// shutdown drops only the not-yet-delivered observer output so its owner can
/// join the task without mailbox-backpressure deadlock.
pub(super) async fn send_observer_output_until_stopped<T>(
    sender: &mpsc::Sender<T>,
    message: T,
    stop_rx: &mut oneshot::Receiver<()>,
) -> bool {
    tokio::select! {
        biased;
        _ = stop_rx => false,
        result = sender.send(message) => result.is_ok(),
    }
}

async fn stop_incoming_verification_observation(observation: IncomingVerificationObservation) {
    stop_incoming_verification_observation_with_timeout(
        observation,
        INCOMING_VERIFICATION_OBSERVER_JOIN_TIMEOUT,
    )
    .await;
}

async fn stop_incoming_verification_observation_with_timeout(
    observation: IncomingVerificationObservation,
    timeout: Duration,
) {
    let IncomingVerificationObservation {
        stop_tx,
        mut task,
        mut observer,
    } = observation;
    let _ = stop_tx.send(());
    if executor::timeout(timeout, &mut task).await.is_err() {
        task.abort();
        let _ = task.await;
    }
    observer.shutdown().await;
}

pub(super) struct PendingVerificationRequest {
    request_id: RequestId,
    target: VerificationTarget,
    handle: koushi_sdk::MatrixVerificationRequestHandle,
}

pub(super) struct PendingSasVerification {
    request_id: RequestId,
    target: VerificationTarget,
    handle: koushi_sdk::MatrixSasVerificationHandle,
}

#[derive(Clone, Copy)]
pub(super) enum VerificationTerminal {
    Success,
    Cancelled(VerificationCancelReason),
    Failed(TrustOperationFailureKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SasVerificationWaitState {
    RecipientDevices,
    ToDeviceDelivery,
    RemoteAccept,
    SasStart,
    Mac,
    CrossSigningSettlement,
    NormalSyncResume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SasAdoptionDecision {
    Adopt,
    Replay,
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IncomingVerificationRequestDecision {
    Adopt,
    Replay,
    Conflict,
}

#[derive(Clone, Copy)]
struct IncomingVerificationActivity<'a> {
    active_request: Option<(&'a VerificationTarget, &'a str)>,
    sas_active: bool,
    own_user_active: bool,
}

fn classify_incoming_verification_request(
    activity: IncomingVerificationActivity<'_>,
    incoming_target: &VerificationTarget,
    incoming_flow_id: &str,
) -> IncomingVerificationRequestDecision {
    if activity.own_user_active {
        return IncomingVerificationRequestDecision::Conflict;
    }

    match activity.active_request {
        Some((active_target, active_flow_id))
            if active_target == incoming_target && active_flow_id == incoming_flow_id =>
        {
            IncomingVerificationRequestDecision::Replay
        }
        Some(_) => IncomingVerificationRequestDecision::Conflict,
        None if activity.sas_active => IncomingVerificationRequestDecision::Conflict,
        None => IncomingVerificationRequestDecision::Adopt,
    }
}

pub(super) fn incoming_verification_request_is_current(
    message_generation: u64,
    current_generation: u64,
    has_session: bool,
) -> bool {
    has_session && message_generation == current_generation
}

fn classify_sas_adoption(
    active_flow_id: Option<u64>,
    incoming_flow_id: u64,
) -> SasAdoptionDecision {
    match active_flow_id {
        None => SasAdoptionDecision::Adopt,
        Some(active_flow_id) if active_flow_id == incoming_flow_id => SasAdoptionDecision::Replay,
        Some(_) => SasAdoptionDecision::Conflict,
    }
}

async fn resolve_sas_adoption<F, Fut>(
    active_flow_id: Option<u64>,
    incoming_flow_id: u64,
    reject_conflict: F,
) -> (SasAdoptionDecision, Option<bool>)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = bool>,
{
    let decision = classify_sas_adoption(active_flow_id, incoming_flow_id);
    let rejection_succeeded = match decision {
        SasAdoptionDecision::Conflict => Some(reject_conflict().await),
        SasAdoptionDecision::Adopt | SasAdoptionDecision::Replay => None,
    };
    (decision, rejection_succeeded)
}

pub(super) fn sas_verification_event(stage: &'static str, flow_id: u64) -> DiagnosticEvent {
    DiagnosticEvent::new(DiagnosticLevel::Info, "core.sas_verification", stage)
        .field(DiagnosticField::count("flow_id", flow_id))
}

pub(super) fn record_sas_verification_event(event: DiagnosticEvent) {
    koushi_diagnostics::record(event);
}

fn verification_request_state_token(
    state: &koushi_sdk::MatrixVerificationRequestState,
) -> &'static str {
    match state {
        koushi_sdk::MatrixVerificationRequestState::Created => "created",
        koushi_sdk::MatrixVerificationRequestState::Requested => "requested",
        koushi_sdk::MatrixVerificationRequestState::Ready => "ready",
        koushi_sdk::MatrixVerificationRequestState::SasStarted(_) => "sas_started",
        koushi_sdk::MatrixVerificationRequestState::Done => "done",
        koushi_sdk::MatrixVerificationRequestState::Cancelled { .. } => "cancelled",
        koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod => "unsupported_method",
    }
}

fn verification_cancel_kind_token(kind: koushi_sdk::MatrixVerificationCancelKind) -> &'static str {
    match kind {
        koushi_sdk::MatrixVerificationCancelKind::UnknownMethod => "unknown_method",
        koushi_sdk::MatrixVerificationCancelKind::KeyMismatch => "key_mismatch",
        koushi_sdk::MatrixVerificationCancelKind::User => "user",
        koushi_sdk::MatrixVerificationCancelKind::Timeout => "timeout",
        koushi_sdk::MatrixVerificationCancelKind::AcceptedElsewhere => "accepted_elsewhere",
        koushi_sdk::MatrixVerificationCancelKind::Other => "other",
    }
}

fn sas_state_token(state: &koushi_sdk::MatrixSasState) -> &'static str {
    match state {
        koushi_sdk::MatrixSasState::Created => "created",
        koushi_sdk::MatrixSasState::Started => "started",
        koushi_sdk::MatrixSasState::Accepted => "accepted",
        koushi_sdk::MatrixSasState::SasPresented { .. } => "sas_presented",
        koushi_sdk::MatrixSasState::Confirmed => "confirmed",
        koushi_sdk::MatrixSasState::Done => "done",
        koushi_sdk::MatrixSasState::Cancelled { .. } => "cancelled",
        koushi_sdk::MatrixSasState::UnsupportedShortAuth => "unsupported_short_auth",
    }
}

fn sas_waiting_for_token(waiting_for: SasVerificationWaitState) -> &'static str {
    match waiting_for {
        SasVerificationWaitState::RecipientDevices => "recipient_devices",
        SasVerificationWaitState::ToDeviceDelivery => "to_device_delivery",
        SasVerificationWaitState::RemoteAccept => "remote_accept",
        SasVerificationWaitState::SasStart => "sas_start",
        SasVerificationWaitState::Mac => "mac",
        SasVerificationWaitState::CrossSigningSettlement => "cross_signing_settlement",
        SasVerificationWaitState::NormalSyncResume => "normal_sync_resume",
    }
}

fn add_sas_waiting_for_field(
    event: DiagnosticEvent,
    waiting_for: SasVerificationWaitState,
) -> DiagnosticEvent {
    event.field(DiagnosticField::token(
        "waiting_for",
        sas_waiting_for_token(waiting_for),
    ))
}

fn sas_waiting_event(flow_id: u64, waiting_for: SasVerificationWaitState) -> DiagnosticEvent {
    add_sas_waiting_for_field(sas_verification_event("waiting", flow_id), waiting_for)
}

fn verification_request_waiting_for(
    state: &koushi_sdk::MatrixVerificationRequestState,
) -> Option<SasVerificationWaitState> {
    match state {
        koushi_sdk::MatrixVerificationRequestState::Created
        | koushi_sdk::MatrixVerificationRequestState::Requested => {
            Some(SasVerificationWaitState::RemoteAccept)
        }
        koushi_sdk::MatrixVerificationRequestState::Ready
        | koushi_sdk::MatrixVerificationRequestState::SasStarted(_) => {
            Some(SasVerificationWaitState::SasStart)
        }
        koushi_sdk::MatrixVerificationRequestState::Done
        | koushi_sdk::MatrixVerificationRequestState::Cancelled { .. }
        | koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod => None,
    }
}

fn sas_state_waiting_for(state: &koushi_sdk::MatrixSasState) -> Option<SasVerificationWaitState> {
    match state {
        koushi_sdk::MatrixSasState::Created | koushi_sdk::MatrixSasState::Started => {
            Some(SasVerificationWaitState::RemoteAccept)
        }
        koushi_sdk::MatrixSasState::Accepted => Some(SasVerificationWaitState::SasStart),
        koushi_sdk::MatrixSasState::Confirmed => Some(SasVerificationWaitState::Mac),
        koushi_sdk::MatrixSasState::Done
        | koushi_sdk::MatrixSasState::Cancelled { .. }
        | koushi_sdk::MatrixSasState::UnsupportedShortAuth
        | koushi_sdk::MatrixSasState::SasPresented { .. } => None,
    }
}

fn sas_state_changed_event(flow_id: u64, state: &koushi_sdk::MatrixSasState) -> DiagnosticEvent {
    let mut event = sas_verification_event("sas_state_changed", flow_id)
        .field(DiagnosticField::token("state", sas_state_token(state)));
    if let Some(waiting_for) = sas_state_waiting_for(state) {
        event = add_sas_waiting_for_field(event, waiting_for);
    }
    if let koushi_sdk::MatrixSasState::Cancelled {
        kind,
        cancelled_by_us,
    } = state
    {
        event = event
            .field(DiagnosticField::token(
                "cancel_kind",
                verification_cancel_kind_token(*kind),
            ))
            .field(DiagnosticField::boolean(
                "cancelled_by_us",
                *cancelled_by_us,
            ));
    }
    event
}

fn trust_failure_token(kind: TrustOperationFailureKind) -> &'static str {
    match kind {
        TrustOperationFailureKind::Cancelled => "cancelled",
        TrustOperationFailureKind::Mismatch => "mismatch",
        TrustOperationFailureKind::InvalidPassphrase => "invalid_passphrase",
        TrustOperationFailureKind::Network => "network",
        TrustOperationFailureKind::Forbidden => "forbidden",
        TrustOperationFailureKind::Timeout => "timeout",
        TrustOperationFailureKind::Sdk => "sdk",
    }
}

pub(super) fn recovery_failure_token(kind: RecoveryFailureKind) -> &'static str {
    match kind {
        RecoveryFailureKind::InvalidRecoveryKey => "invalid_recovery_key",
        RecoveryFailureKind::Network => "network",
        RecoveryFailureKind::Server => "server",
        RecoveryFailureKind::Timeout => "timeout",
    }
}

fn verification_terminal_token(terminal: VerificationTerminal) -> &'static str {
    match terminal {
        VerificationTerminal::Success => "success",
        VerificationTerminal::Cancelled(_) => "cancelled",
        VerificationTerminal::Failed(_) => "failed",
    }
}

fn verification_cancel_reason_token(reason: VerificationCancelReason) -> &'static str {
    match reason {
        VerificationCancelReason::User => "user",
        VerificationCancelReason::Mismatch => "mismatch",
    }
}

fn sas_settled_event(
    flow_id: u64,
    terminal: VerificationTerminal,
    waiting_for: Option<SasVerificationWaitState>,
) -> DiagnosticEvent {
    let mut event = sas_verification_event("settled", flow_id).field(DiagnosticField::token(
        "terminal",
        verification_terminal_token(terminal),
    ));
    if let Some(waiting_for) = waiting_for {
        event = add_sas_waiting_for_field(event, waiting_for);
    }
    match terminal {
        VerificationTerminal::Success => {}
        VerificationTerminal::Cancelled(reason) => {
            event = event.field(DiagnosticField::token(
                "reason",
                verification_cancel_reason_token(reason),
            ));
        }
        VerificationTerminal::Failed(kind) => {
            event = event.field(DiagnosticField::token(
                "failure_kind",
                trust_failure_token(kind),
            ));
        }
    }
    event
}

fn sas_timeout_fired_event(
    flow_id: u64,
    waiting_for: Option<SasVerificationWaitState>,
) -> DiagnosticEvent {
    match waiting_for {
        Some(waiting_for) => add_sas_waiting_for_field(
            sas_verification_event("timeout_fired", flow_id),
            waiting_for,
        ),
        None => sas_verification_event("timeout_fired", flow_id),
    }
}

async fn run_own_user_sas_start<T, F>(
    flow_id: u64,
    source: &'static str,
    start: F,
) -> Result<Option<T>, koushi_sdk::E2eeTrustError>
where
    F: Future<Output = Result<Option<T>, koushi_sdk::E2eeTrustError>>,
{
    record_sas_verification_event(
        sas_verification_event("sas_start_attempted", flow_id)
            .field(DiagnosticField::token("source", source)),
    );
    let result = start.await;
    let mut event = sas_verification_event("sas_start_finished", flow_id)
        .field(DiagnosticField::token("source", source));
    event = match &result {
        Ok(Some(_)) => event.field(DiagnosticField::token("outcome", "started")),
        Ok(None) => event.field(DiagnosticField::token("outcome", "pending")),
        Err(error) => {
            let kind = classify_e2ee_trust_error(error);
            event
                .field(DiagnosticField::token("outcome", "failed"))
                .field(DiagnosticField::token(
                    "failure_kind",
                    trust_failure_token(kind),
                ))
        }
    };
    record_sas_verification_event(event);
    result
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub enum SyntheticVerificationTerminal {
    Success,
    Cancelled(VerificationCancelReason),
    Failed(TrustOperationFailureKind),
}

fn sas_projection_action(own_user_flow: bool, flow_id: u64, emojis: Vec<SasEmoji>) -> AppAction {
    if own_user_flow {
        AppAction::GateSasPresented { flow_id, emojis }
    } else {
        AppAction::VerificationSasPresented {
            request_id: flow_id,
            emojis,
        }
    }
}

pub(super) fn incoming_verification_request_id(sequence: u64) -> RequestId {
    RequestId {
        connection_id: RuntimeConnectionId(0),
        sequence,
    }
}

impl AccountActor {
    pub(super) async fn start_incoming_verification_observer(
        &mut self,
        session: Arc<MatrixClientSession>,
    ) {
        self.incoming_verification_session_generation = self
            .incoming_verification_session_generation
            .wrapping_add(1);
        let generation = self.incoming_verification_session_generation;
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut observer = koushi_sdk::observe_incoming_verification_requests(&session).await;
        let mut receiver = observer
            .take_receiver()
            .expect("incoming verification observer receiver is available once");
        let tx = self.self_tx.clone();
        let task = crate::executor::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    request = receiver.recv() => {
                        let Some(request) = request else { break };
                        let (target, handle) = request.into_parts();
                        if !send_observer_output_until_stopped(
                            &tx,
                            AccountMessage::IncomingVerificationRequest {
                                generation,
                                target,
                                handle,
                            },
                            &mut stop_rx,
                        )
                        .await
                        {
                            break;
                        }
                    }
                }
            }
        });
        self.incoming_verification_observer = Some(IncomingVerificationObservation {
            stop_tx,
            task,
            observer,
        });
    }

    pub(super) fn next_incoming_verification_request_id(&mut self) -> RequestId {
        let sequence = self.next_incoming_verification_sequence;
        self.next_incoming_verification_sequence = self
            .next_incoming_verification_sequence
            .checked_add(1)
            .unwrap_or(INCOMING_VERIFICATION_FLOW_ID_BASE);
        incoming_verification_request_id(sequence)
    }

    pub(super) async fn stop_incoming_verification_observer(&mut self) {
        self.incoming_verification_session_generation = self
            .incoming_verification_session_generation
            .wrapping_add(1);
        if let Some(observation) = self.incoming_verification_observer.take() {
            stop_incoming_verification_observation(observation).await;
        }
    }

    pub(super) async fn cancel_identity_reset_handle(&mut self) {
        self.identity_reset_flow_id = None;
        if let Some(task) = self.identity_reset_timeout_task.take() {
            task.abort();
        }
        if let Some(handle) = self.identity_reset_handle.take() {
            handle.cancel().await;
        }
    }

    pub(super) fn spawn_identity_reset_auth_timeout(&mut self, flow_id: u64) {
        if let Some(task) = self.identity_reset_timeout_task.take() {
            task.abort();
        }
        let tx = self.self_tx.clone();
        self.identity_reset_timeout_task = Some(executor::spawn(async move {
            executor::sleep(IDENTITY_RESET_AUTH_TIMEOUT).await;
            let _ = tx
                .send(AccountMessage::IdentityResetAuthTimedOut { flow_id })
                .await;
        }));
    }

    fn clear_identity_reset_handle_after_completion(&mut self) {
        self.identity_reset_flow_id = None;
        if let Some(task) = self.identity_reset_timeout_task.take() {
            task.abort();
        }
        self.identity_reset_handle = None;
    }

    async fn stop_verification_request_observer(&mut self) {
        if let Some(observation) = self.verification_request_observer.take() {
            let _ = observation.stop_tx.send(());
            let _ = observation.task.await;
        }
    }

    async fn stop_sas_verification_observer(&mut self) {
        if let Some(observation) = self.sas_verification_observer.take() {
            let _ = observation.stop_tx.send(());
            let _ = observation.task.await;
        }
    }

    fn record_sas_waiting_for(&mut self, flow_id: u64, waiting_for: SasVerificationWaitState) {
        if self.sas_waiting_for == Some((flow_id, waiting_for)) {
            return;
        }
        self.sas_waiting_for = Some((flow_id, waiting_for));
        record_sas_verification_event(sas_waiting_event(flow_id, waiting_for));
    }

    fn active_sas_waiting_for(&self, flow_id: u64) -> Option<SasVerificationWaitState> {
        self.sas_waiting_for
            .filter(|(active_flow_id, _)| *active_flow_id == flow_id)
            .map(|(_, waiting_for)| waiting_for)
    }

    fn clear_sas_waiting_for(&mut self, flow_id: u64) {
        if self
            .sas_waiting_for
            .is_some_and(|(active_flow_id, _)| active_flow_id == flow_id)
        {
            self.sas_waiting_for = None;
        }
    }

    pub(super) async fn cancel_verification_handles(&mut self) {
        self.stop_sas_timeout().await;
        self.stop_verification_request_observer().await;
        self.stop_sas_verification_observer().await;
        self.sas_waiting_for = None;
        if let Some(pending) = self.sas_verification.take() {
            let _ = koushi_sdk::cancel_sas_verification(&pending.handle).await;
        }
        if let Some(pending) = self.verification_request.take() {
            let _ = koushi_sdk::cancel_verification_request(&pending.handle).await;
        }
        if let Some((_, handle)) = self.own_user_verification.take() {
            let _ = koushi_sdk::cancel_own_user_sas_verification(&handle).await;
        }
    }

    pub(super) async fn handle_request_verification(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
    ) {
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.send_actions(vec![AppAction::VerificationFailed {
                    request_id: request_id.sequence,
                    kind: TrustOperationFailureKind::Sdk,
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::SessionRequired);
                return;
            }
        };

        self.cancel_verification_handles().await;
        match koushi_sdk::request_device_verification(&session, &target).await {
            Ok(handle) => {
                self.verification_request = Some(PendingVerificationRequest {
                    request_id,
                    target: target.clone(),
                    handle: handle.clone(),
                });
                self.observe_verification_request(request_id, target.clone(), handle.clone());
                self.send_actions(vec![AppAction::VerificationRequested {
                    request_id: request_id.sequence,
                    target: target.clone(),
                }])
                .await;
                self.emit_verification_progress(VerificationFlowState::Requested {
                    request_id: request_id.sequence,
                    target,
                });
                self.project_verification_request_state(request_id, handle.state())
                    .await;
            }
            Err(error) => {
                self.project_verification_failure(
                    request_id.sequence,
                    target,
                    classify_e2ee_trust_error(&error),
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_start_own_user_sas(&mut self, request_id: RequestId, flow_id: u64) {
        let Some(session) = self.session.clone() else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        self.cancel_verification_handles().await;
        self.record_sas_waiting_for(flow_id, SasVerificationWaitState::RecipientDevices);
        let own_handle =
            match koushi_sdk::request_own_user_sas_verification(&session, flow_id).await {
                Ok(handle) => handle,
                Err(error) => {
                    let kind = classify_e2ee_trust_error(&error);
                    record_sas_verification_event(sas_settled_event(
                        flow_id,
                        VerificationTerminal::Failed(kind),
                        self.active_sas_waiting_for(flow_id),
                    ));
                    self.clear_sas_waiting_for(flow_id);
                    self.send_actions(vec![AppAction::VerificationFailed {
                        request_id: flow_id,
                        kind,
                    }])
                    .await;
                    return;
                }
            };
        if let Some(waiting_for) = verification_request_waiting_for(&own_handle.state()) {
            self.record_sas_waiting_for(flow_id, waiting_for);
        } else {
            self.record_sas_waiting_for(flow_id, SasVerificationWaitState::ToDeviceDelivery);
        }
        let sas = match run_own_user_sas_start(
            flow_id,
            "initial",
            koushi_sdk::start_own_user_sas_verification(&own_handle),
        )
        .await
        {
            Ok(Some(sas)) => sas,
            Ok(None) => {
                self.own_user_verification = Some((flow_id, own_handle));
                self.start_sas_timeout(flow_id);
                self.observe_own_user_verification(request_id, flow_id);
                return;
            }
            Err(error) => {
                let kind = classify_e2ee_trust_error(&error);
                record_sas_verification_event(sas_settled_event(
                    flow_id,
                    VerificationTerminal::Failed(kind),
                    self.active_sas_waiting_for(flow_id),
                ));
                self.clear_sas_waiting_for(flow_id);
                self.send_actions(vec![AppAction::VerificationFailed {
                    request_id: flow_id,
                    kind,
                }])
                .await;
                return;
            }
        };
        self.own_user_verification = Some((flow_id, own_handle));
        self.store_sas_verification(
            RequestId {
                connection_id: request_id.connection_id,
                sequence: flow_id,
            },
            VerificationTarget {
                user_id: "current-user".to_owned(),
                device_id: "eligible-device".to_owned(),
            },
            sas,
        )
        .await;
    }

    pub(super) async fn recheck_own_user_sas_after_sync(&mut self) {
        if self.sas_verification.is_some() {
            return;
        }
        let Some((flow_id, handle)) = self.own_user_verification.as_ref() else {
            return;
        };
        let state = handle.state();
        if !matches!(state, koushi_sdk::MatrixVerificationRequestState::Ready) {
            return;
        }
        let flow_id = *flow_id;
        let handle = handle.clone();
        self.record_sas_waiting_for(flow_id, SasVerificationWaitState::SasStart);
        match run_own_user_sas_start(
            flow_id,
            "provisional_encryption_sync",
            koushi_sdk::start_own_user_sas_verification(&handle),
        )
        .await
        {
            Ok(Some(sas)) => {
                self.store_sas_verification(
                    RequestId {
                        connection_id: RuntimeConnectionId(0),
                        sequence: flow_id,
                    },
                    VerificationTarget {
                        user_id: "current-user".to_owned(),
                        device_id: "eligible-device".to_owned(),
                    },
                    sas,
                )
                .await;
            }
            Ok(None) => {}
            Err(error) => {
                let kind = classify_e2ee_trust_error(&error);
                record_sas_verification_event(sas_settled_event(
                    flow_id,
                    VerificationTerminal::Failed(kind),
                    self.active_sas_waiting_for(flow_id),
                ));
                self.clear_sas_waiting_for(flow_id);
                self.send_actions(vec![AppAction::VerificationFailed {
                    request_id: flow_id,
                    kind,
                }])
                .await;
            }
        }
    }

    fn start_sas_timeout(&mut self, flow_id: u64) {
        if let Some(task) = self.sas_timeout_task.take() {
            task.abort();
        }
        let tx = self.self_tx.clone();
        self.sas_timeout_task = Some(executor::spawn(async move {
            executor::sleep(Duration::from_secs(120)).await;
            let _ = tx
                .send(AccountMessage::SasVerificationTimedOut { flow_id })
                .await;
        }));
    }

    async fn stop_sas_timeout(&mut self) {
        if let Some(task) = self.sas_timeout_task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    pub(super) async fn handle_sas_verification_timeout(&mut self, flow_id: u64) {
        let active = self
            .sas_verification
            .as_ref()
            .is_some_and(|pending| pending.request_id.sequence == flow_id)
            || self
                .own_user_verification
                .as_ref()
                .is_some_and(|(active_flow_id, _)| *active_flow_id == flow_id);
        if !active {
            return;
        }
        record_sas_verification_event(sas_timeout_fired_event(
            flow_id,
            self.active_sas_waiting_for(flow_id),
        ));
        self.settle_verification(
            flow_id,
            VerificationTerminal::Failed(TrustOperationFailureKind::Timeout),
        )
        .await;
    }

    fn observe_own_user_verification(&mut self, request_id: RequestId, flow_id: u64) {
        let Some((_, handle)) = self.own_user_verification.as_ref() else {
            return;
        };
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut states = handle.changes();
        let tx = self.self_tx.clone();
        let request_id = RequestId {
            connection_id: request_id.connection_id,
            sequence: flow_id,
        };
        let target = VerificationTarget {
            user_id: "current-user".to_owned(),
            device_id: "eligible-device".to_owned(),
        };
        let task = executor::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    state = states.next() => {
                        let Some(state) = state else {
                            let _ = tx.send(AccountMessage::VerificationRequestObserverEnded {
                                flow_id: request_id.sequence,
                            }).await;
                            break;
                        };
                        let terminal = matches!(state,
                            koushi_sdk::MatrixVerificationRequestState::Done
                            | koushi_sdk::MatrixVerificationRequestState::Cancelled { .. }
                            | koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod);
                        if tx.send(AccountMessage::VerificationRequestProgress {
                            request_id,
                            target: target.clone(),
                            state,
                        }).await.is_err() { break; }
                        if terminal { break; }
                    }
                }
            }
        });
        self.verification_request_observer = Some(VerificationObservation { stop_tx, task });
    }

    pub(super) async fn handle_incoming_verification_request(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        handle: koushi_sdk::MatrixVerificationRequestHandle,
    ) {
        let active_request = self
            .verification_request
            .as_ref()
            .map(|pending| (&pending.target, pending.handle.flow_id()));
        let decision = classify_incoming_verification_request(
            IncomingVerificationActivity {
                active_request,
                sas_active: self.sas_verification.is_some(),
                own_user_active: self.own_user_verification.is_some(),
            },
            &target,
            handle.flow_id(),
        );
        match decision {
            IncomingVerificationRequestDecision::Adopt => {}
            IncomingVerificationRequestDecision::Replay => return,
            IncomingVerificationRequestDecision::Conflict => {
                let _ = koushi_sdk::cancel_verification_request(&handle).await;
                return;
            }
        }

        self.verification_request = Some(PendingVerificationRequest {
            request_id,
            target: target.clone(),
            handle: handle.clone(),
        });
        self.observe_verification_request(request_id, target.clone(), handle.clone());
        self.send_actions(vec![AppAction::VerificationRequested {
            request_id: request_id.sequence,
            target: target.clone(),
        }])
        .await;
        self.emit_verification_progress(VerificationFlowState::Requested {
            request_id: request_id.sequence,
            target,
        });
        self.project_verification_request_state(request_id, handle.state())
            .await;
    }

    pub(super) async fn handle_accept_verification(&mut self, request_id: RequestId, flow_id: u64) {
        let Some(pending) = self
            .verification_request
            .as_ref()
            .filter(|pending| pending.request_id.sequence == flow_id)
        else {
            self.project_active_or_missing_verification_failure(request_id, flow_id)
                .await;
            return;
        };
        let pending_request_id = pending.request_id;
        let target = pending.target.clone();
        let handle = pending.handle.clone();

        match handle.state() {
            koushi_sdk::MatrixVerificationRequestState::Requested => {
                if let Err(error) = koushi_sdk::accept_verification_request(&handle).await {
                    self.project_verification_failure(
                        flow_id,
                        target,
                        classify_e2ee_trust_error(&error),
                    )
                    .await;
                    return;
                }
                self.project_verification_request_state(pending_request_id, handle.state())
                    .await;
            }
            koushi_sdk::MatrixVerificationRequestState::Ready => {
                match koushi_sdk::start_sas_verification(&handle).await {
                    Ok(Some(sas)) => {
                        self.store_sas_verification(pending_request_id, target, sas)
                            .await;
                    }
                    Ok(None) => {
                        self.project_verification_failure(
                            flow_id,
                            target,
                            TrustOperationFailureKind::Sdk,
                        )
                        .await;
                    }
                    Err(error) => {
                        self.project_verification_failure(
                            flow_id,
                            target,
                            classify_e2ee_trust_error(&error),
                        )
                        .await;
                    }
                }
            }
            koushi_sdk::MatrixVerificationRequestState::SasStarted(sas) => {
                self.store_sas_verification(pending_request_id, target, sas)
                    .await;
            }
            koushi_sdk::MatrixVerificationRequestState::Done => {
                self.project_verification_completed(pending_request_id)
                    .await;
            }
            koushi_sdk::MatrixVerificationRequestState::Created
            | koushi_sdk::MatrixVerificationRequestState::Cancelled { .. }
            | koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod => {
                self.project_verification_failure(flow_id, target, TrustOperationFailureKind::Sdk)
                    .await;
            }
        }
    }

    pub(super) async fn handle_confirm_sas_verification(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
    ) {
        let Some(pending) = self
            .sas_verification
            .as_ref()
            .filter(|pending| pending.request_id.sequence == flow_id)
        else {
            self.project_active_or_missing_verification_failure(request_id, flow_id)
                .await;
            return;
        };
        let pending_request_id = pending.request_id;
        let target = pending.target.clone();
        let handle = pending.handle.clone();

        match koushi_sdk::confirm_sas_verification(&handle).await {
            Ok(()) => {
                self.project_sas_state(pending_request_id, target, handle.state())
                    .await;
            }
            Err(error) => {
                self.project_verification_failure(
                    flow_id,
                    target,
                    classify_e2ee_trust_error(&error),
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_cancel_verification(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
        reason: VerificationCancelReason,
    ) {
        if self
            .recovery_task
            .as_ref()
            .is_some_and(|pending| pending.flow_id == flow_id)
        {
            self.stop_recovery_task().await;
            record_device_cleanup_offer("recovery_failed");
            self.send_actions(vec![AppAction::VerificationGateAttemptFailed {
                flow_id,
                kind: koushi_state::VerificationGateFailureKind::Cancelled,
            }])
            .await;
            return;
        }
        if self.active_verification_target(flow_id).is_some() {
            self.settle_verification(flow_id, VerificationTerminal::Cancelled(reason))
                .await;
            return;
        }
        enum CancelTarget {
            Sas {
                target: VerificationTarget,
                handle: koushi_sdk::MatrixSasVerificationHandle,
            },
            Request {
                target: VerificationTarget,
                handle: koushi_sdk::MatrixVerificationRequestHandle,
            },
            Own {
                handle: koushi_sdk::MatrixOwnUserVerificationHandle,
            },
        }

        let sas_target = self
            .sas_verification
            .as_ref()
            .filter(|pending| pending.request_id.sequence == flow_id)
            .map(|pending| CancelTarget::Sas {
                target: pending.target.clone(),
                handle: pending.handle.clone(),
            });
        let cancel_target = match reason {
            VerificationCancelReason::Mismatch => sas_target,
            VerificationCancelReason::User => sas_target
                .or_else(|| {
                    self.verification_request
                        .as_ref()
                        .filter(|pending| pending.request_id.sequence == flow_id)
                        .map(|pending| CancelTarget::Request {
                            target: pending.target.clone(),
                            handle: pending.handle.clone(),
                        })
                })
                .or_else(|| {
                    self.own_user_verification
                        .as_ref()
                        .filter(|(active_flow_id, _)| *active_flow_id == flow_id)
                        .map(|(_, handle)| CancelTarget::Own {
                            handle: handle.clone(),
                        })
                }),
        };

        let Some(cancel_target) = cancel_target else {
            if reason == VerificationCancelReason::Mismatch {
                self.emit_failure(request_id, CoreFailure::LocalEncryptionUnavailable);
            } else {
                self.project_active_or_missing_verification_failure(request_id, flow_id)
                    .await;
            }
            return;
        };

        let target = match &cancel_target {
            CancelTarget::Sas { target, .. } | CancelTarget::Request { target, .. } => {
                target.clone()
            }
            CancelTarget::Own { .. } => VerificationTarget {
                user_id: "current-user".to_owned(),
                device_id: "eligible-device".to_owned(),
            },
        };
        let result = match cancel_target {
            CancelTarget::Sas { handle, .. } => match reason {
                VerificationCancelReason::User => {
                    koushi_sdk::cancel_sas_verification(&handle).await
                }
                VerificationCancelReason::Mismatch => {
                    koushi_sdk::mismatch_sas_verification(&handle).await
                }
            },
            CancelTarget::Request { handle, .. } => {
                koushi_sdk::cancel_verification_request(&handle).await
            }
            CancelTarget::Own { handle } => {
                koushi_sdk::cancel_own_user_sas_verification(&handle).await
            }
        };

        self.stop_verification_request_observer().await;
        self.stop_sas_verification_observer().await;
        self.verification_request = None;
        self.sas_verification = None;
        self.own_user_verification = None;

        if let Err(error) = result {
            self.project_verification_failure(flow_id, target, classify_e2ee_trust_error(&error))
                .await;
        }
    }

    fn observe_verification_request(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        handle: koushi_sdk::MatrixVerificationRequestHandle,
    ) {
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut states = handle.changes();
        let tx = self.self_tx.clone();
        let task = crate::executor::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    state = states.next() => {
                        let Some(state) = state else {
                            let _ = tx.send(AccountMessage::VerificationRequestObserverEnded {
                                flow_id: request_id.sequence,
                            }).await;
                            break;
                        };
                        let terminal = matches!(
                            state,
                            koushi_sdk::MatrixVerificationRequestState::Done
                                | koushi_sdk::MatrixVerificationRequestState::Cancelled { .. }
                                | koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod
                        );
                        if tx
                            .send(AccountMessage::VerificationRequestProgress {
                                request_id,
                                target: target.clone(),
                                state,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if terminal {
                            break;
                        }
                    }
                }
            }
        });
        self.verification_request_observer = Some(VerificationObservation { stop_tx, task });
    }

    fn observe_sas_verification(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        handle: koushi_sdk::MatrixSasVerificationHandle,
    ) {
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut states = handle.changes();
        let tx = self.self_tx.clone();
        let task = crate::executor::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    state = states.next() => {
                        let Some(state) = state else {
                            let _ = tx.send(AccountMessage::SasVerificationObserverEnded {
                                flow_id: request_id.sequence,
                            }).await;
                            break;
                        };
                        let terminal = matches!(
                            state,
                            koushi_sdk::MatrixSasState::Done
                                | koushi_sdk::MatrixSasState::Cancelled { .. }
                                | koushi_sdk::MatrixSasState::UnsupportedShortAuth
                        );
                        if tx
                            .send(AccountMessage::SasVerificationProgress {
                                request_id,
                                target: target.clone(),
                                state,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if terminal {
                            break;
                        }
                    }
                }
            }
        });
        self.sas_verification_observer = Some(VerificationObservation { stop_tx, task });
    }

    pub(super) async fn handle_verification_request_progress(
        &mut self,
        request_id: RequestId,
        _target: VerificationTarget,
        state: koushi_sdk::MatrixVerificationRequestState,
    ) {
        if !self
            .verification_request
            .as_ref()
            .is_some_and(|pending| pending.request_id.sequence == request_id.sequence)
            && !self
                .own_user_verification
                .as_ref()
                .is_some_and(|(flow_id, _)| *flow_id == request_id.sequence)
        {
            return;
        }
        let waiting_for = verification_request_waiting_for(&state);
        if let Some(waiting_for) = waiting_for {
            self.record_sas_waiting_for(request_id.sequence, waiting_for);
        }
        let mut event = sas_verification_event("request_state_changed", request_id.sequence).field(
            DiagnosticField::token("state", verification_request_state_token(&state)),
        );
        if let Some(waiting_for) = waiting_for {
            event = add_sas_waiting_for_field(event, waiting_for);
        }
        if let koushi_sdk::MatrixVerificationRequestState::Cancelled {
            kind,
            cancelled_by_us,
        } = &state
        {
            event = event
                .field(DiagnosticField::token(
                    "cancel_kind",
                    verification_cancel_kind_token(*kind),
                ))
                .field(DiagnosticField::boolean(
                    "cancelled_by_us",
                    *cancelled_by_us,
                ));
        }
        record_sas_verification_event(event);
        self.project_verification_request_state(request_id, state)
            .await;
    }

    pub(super) async fn handle_sas_verification_progress(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        state: koushi_sdk::MatrixSasState,
    ) {
        if !self
            .sas_verification
            .as_ref()
            .is_some_and(|pending| pending.request_id.sequence == request_id.sequence)
        {
            return;
        }
        record_sas_verification_event(sas_state_changed_event(request_id.sequence, &state));
        self.project_sas_state(request_id, target, state).await;
    }

    async fn project_verification_request_state(
        &mut self,
        request_id: RequestId,
        state: koushi_sdk::MatrixVerificationRequestState,
    ) {
        match state {
            koushi_sdk::MatrixVerificationRequestState::Created
            | koushi_sdk::MatrixVerificationRequestState::Requested => {}
            koushi_sdk::MatrixVerificationRequestState::Ready => {
                self.send_actions(vec![AppAction::VerificationAccepted {
                    request_id: request_id.sequence,
                }])
                .await;
                if let Some((flow_id, handle)) = self.own_user_verification.as_ref()
                    && *flow_id == request_id.sequence
                    && self.sas_verification.is_none()
                {
                    let handle = handle.clone();
                    self.record_sas_waiting_for(
                        request_id.sequence,
                        SasVerificationWaitState::SasStart,
                    );
                    match run_own_user_sas_start(
                        request_id.sequence,
                        "request_ready",
                        koushi_sdk::start_own_user_sas_verification(&handle),
                    )
                    .await
                    {
                        Ok(Some(sas)) => {
                            self.store_sas_verification(
                                request_id,
                                VerificationTarget {
                                    user_id: "current-user".to_owned(),
                                    device_id: "eligible-device".to_owned(),
                                },
                                sas,
                            )
                            .await;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let kind = classify_e2ee_trust_error(&error);
                            record_sas_verification_event(sas_settled_event(
                                request_id.sequence,
                                VerificationTerminal::Failed(kind),
                                self.active_sas_waiting_for(request_id.sequence),
                            ));
                            self.clear_sas_waiting_for(request_id.sequence);
                            self.send_actions(vec![AppAction::VerificationFailed {
                                request_id: request_id.sequence,
                                kind,
                            }])
                            .await;
                        }
                    }
                }
            }
            koushi_sdk::MatrixVerificationRequestState::SasStarted(sas) => {
                let target = self
                    .verification_request
                    .as_ref()
                    .filter(|pending| pending.request_id.sequence == request_id.sequence)
                    .map(|pending| pending.target.clone())
                    .or_else(|| {
                        self.own_user_verification
                            .as_ref()
                            .and_then(|(flow_id, _)| {
                                (*flow_id == request_id.sequence).then(|| VerificationTarget {
                                    user_id: "current-user".to_owned(),
                                    device_id: "eligible-device".to_owned(),
                                })
                            })
                    });
                let Some(target) = target else {
                    return;
                };
                self.store_sas_verification(request_id, target, sas).await;
            }
            koushi_sdk::MatrixVerificationRequestState::Done => {
                self.project_verification_completed(request_id).await;
            }
            koushi_sdk::MatrixVerificationRequestState::Cancelled {
                kind,
                cancelled_by_us,
            } => {
                let _ = (kind, cancelled_by_us);
                self.project_active_or_missing_verification_failure_with_kind(
                    request_id,
                    request_id.sequence,
                    TrustOperationFailureKind::Cancelled,
                )
                .await;
            }
            koushi_sdk::MatrixVerificationRequestState::UnsupportedMethod => {
                self.project_active_or_missing_verification_failure(
                    request_id,
                    request_id.sequence,
                )
                .await;
            }
        }
    }

    async fn store_sas_verification(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        handle: koushi_sdk::MatrixSasVerificationHandle,
    ) {
        let active_flow_id = self
            .sas_verification
            .as_ref()
            .map(|pending| pending.request_id.sequence);
        let (decision, conflict_rejection_succeeded) =
            resolve_sas_adoption(active_flow_id, request_id.sequence, || async {
                koushi_sdk::cancel_sas_verification(&handle).await.is_ok()
            })
            .await;
        match decision {
            SasAdoptionDecision::Adopt => {}
            SasAdoptionDecision::Replay => return,
            SasAdoptionDecision::Conflict => {
                record_sas_verification_event(
                    sas_verification_event("conflicting_sas_rejected", request_id.sequence).field(
                        DiagnosticField::token(
                            "outcome",
                            if conflict_rejection_succeeded == Some(true) {
                                "success"
                            } else {
                                "failed"
                            },
                        ),
                    ),
                );
                return;
            }
        }

        self.stop_sas_verification_observer().await;
        self.sas_verification = Some(PendingSasVerification {
            request_id,
            target: target.clone(),
            handle: handle.clone(),
        });
        self.start_sas_timeout(request_id.sequence);
        self.observe_sas_verification(request_id, target.clone(), handle.clone());
        let initial_state = handle.state();
        if let Some(waiting_for) = sas_state_waiting_for(&initial_state) {
            self.record_sas_waiting_for(request_id.sequence, waiting_for);
        }
        record_sas_verification_event(sas_state_changed_event(request_id.sequence, &initial_state));
        if matches!(initial_state, koushi_sdk::MatrixSasState::Started)
            && let Err(error) = koushi_sdk::accept_sas_verification(&handle).await
        {
            self.project_verification_failure(
                request_id.sequence,
                target,
                classify_e2ee_trust_error(&error),
            )
            .await;
            return;
        }
        self.project_sas_state(request_id, target, handle.state())
            .await;
    }

    async fn project_sas_state(
        &mut self,
        request_id: RequestId,
        target: VerificationTarget,
        state: koushi_sdk::MatrixSasState,
    ) {
        if let Some(waiting_for) = sas_state_waiting_for(&state) {
            self.record_sas_waiting_for(request_id.sequence, waiting_for);
        }
        match state {
            koushi_sdk::MatrixSasState::Created
            | koushi_sdk::MatrixSasState::Started
            | koushi_sdk::MatrixSasState::Accepted => {}
            koushi_sdk::MatrixSasState::SasPresented { emojis } => {
                if emojis.len() != 7 {
                    self.project_verification_failure(
                        request_id.sequence,
                        target,
                        TrustOperationFailureKind::Sdk,
                    )
                    .await;
                    return;
                }
                let own_user_flow = self
                    .own_user_verification
                    .as_ref()
                    .is_some_and(|(flow_id, _)| *flow_id == request_id.sequence);
                let action =
                    sas_projection_action(own_user_flow, request_id.sequence, emojis.clone());
                self.send_actions(vec![action]).await;
                self.emit_verification_progress(VerificationFlowState::SasPresented {
                    request_id: request_id.sequence,
                    target,
                    emojis,
                });
            }
            koushi_sdk::MatrixSasState::Confirmed => {}
            koushi_sdk::MatrixSasState::Done => {
                self.project_verification_completed(request_id).await;
            }
            koushi_sdk::MatrixSasState::Cancelled { .. } => {
                self.project_verification_failure(
                    request_id.sequence,
                    target,
                    TrustOperationFailureKind::Cancelled,
                )
                .await;
            }
            koushi_sdk::MatrixSasState::UnsupportedShortAuth => {
                self.project_verification_failure(
                    request_id.sequence,
                    target,
                    TrustOperationFailureKind::Sdk,
                )
                .await;
            }
        }
    }

    pub(super) fn active_verification_target(&self, flow_id: u64) -> Option<VerificationTarget> {
        self.sas_verification
            .as_ref()
            .filter(|pending| pending.request_id.sequence == flow_id)
            .map(|pending| pending.target.clone())
            .or_else(|| {
                self.verification_request
                    .as_ref()
                    .filter(|pending| pending.request_id.sequence == flow_id)
                    .map(|pending| pending.target.clone())
            })
            .or_else(|| {
                self.own_user_verification
                    .as_ref()
                    .and_then(|(active_flow_id, _)| {
                        (*active_flow_id == flow_id).then(|| VerificationTarget {
                            user_id: "current-user".to_owned(),
                            device_id: "eligible-device".to_owned(),
                        })
                    })
            })
            .or_else(|| {
                #[cfg(test)]
                {
                    return self.synthetic_verification.as_ref().and_then(
                        |(active_flow_id, target)| {
                            (*active_flow_id == flow_id).then(|| target.clone())
                        },
                    );
                }
                #[cfg(not(test))]
                None
            })
    }

    pub(super) async fn settle_verification(
        &mut self,
        flow_id: u64,
        terminal: VerificationTerminal,
    ) {
        let Some(target) = self.active_verification_target(flow_id) else {
            return;
        };
        let waiting_for = if matches!(terminal, VerificationTerminal::Success) {
            Some(SasVerificationWaitState::CrossSigningSettlement)
        } else {
            self.active_sas_waiting_for(flow_id)
        };
        record_sas_verification_event(sas_settled_event(flow_id, terminal, waiting_for));
        self.stop_sas_timeout().await;
        self.stop_verification_request_observer().await;
        self.stop_sas_verification_observer().await;
        self.clear_sas_waiting_for(flow_id);
        let sas = self.sas_verification.take();
        let request = self.verification_request.take();
        let own = self.own_user_verification.take();
        #[cfg(test)]
        {
            self.synthetic_verification = None;
        }

        if !matches!(terminal, VerificationTerminal::Success) {
            if let Some(pending) = sas.as_ref() {
                let _ = match terminal {
                    VerificationTerminal::Cancelled(VerificationCancelReason::Mismatch) => {
                        koushi_sdk::mismatch_sas_verification(&pending.handle).await
                    }
                    _ => koushi_sdk::cancel_sas_verification(&pending.handle).await,
                };
            } else if let Some(pending) = request.as_ref() {
                let _ = koushi_sdk::cancel_verification_request(&pending.handle).await;
            } else if let Some((_, handle)) = own.as_ref() {
                let _ = koushi_sdk::cancel_own_user_sas_verification(handle).await;
            }
        }
        drop(sas);
        drop(request);
        drop(own);

        match terminal {
            VerificationTerminal::Success => {
                self.send_actions(vec![AppAction::VerificationCompleted {
                    request_id: flow_id,
                }])
                .await;
                self.request_authoritative_trust_recheck();
                record_sas_verification_event(sas_waiting_event(
                    flow_id,
                    SasVerificationWaitState::NormalSyncResume,
                ));
                self.emit_verification_progress(VerificationFlowState::Done {
                    request_id: flow_id,
                    target,
                });
            }
            VerificationTerminal::Cancelled(reason) => {
                self.send_actions(vec![AppAction::VerificationCancelled {
                    request_id: flow_id,
                    reason,
                }])
                .await;
            }
            VerificationTerminal::Failed(kind) => {
                self.send_actions(vec![AppAction::VerificationFailed {
                    request_id: flow_id,
                    kind,
                }])
                .await;
                self.emit_verification_progress(VerificationFlowState::Failed {
                    request_id: flow_id,
                    target,
                    kind,
                });
            }
        }
    }

    async fn project_verification_completed(&mut self, request_id: RequestId) {
        if self
            .active_verification_target(request_id.sequence)
            .is_none()
        {
            return;
        }
        self.settle_verification(request_id.sequence, VerificationTerminal::Success)
            .await;
    }

    async fn project_active_or_missing_verification_failure(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
    ) {
        self.project_active_or_missing_verification_failure_with_kind(
            request_id,
            flow_id,
            TrustOperationFailureKind::Sdk,
        )
        .await;
    }

    async fn project_active_or_missing_verification_failure_with_kind(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
        kind: TrustOperationFailureKind,
    ) {
        if self.active_verification_target(flow_id).is_some() {
            self.settle_verification(flow_id, VerificationTerminal::Failed(kind))
                .await;
        } else {
            self.send_actions(vec![AppAction::VerificationFailed {
                request_id: flow_id,
                kind,
            }])
            .await;
            let failure = if self.session.is_some() {
                CoreFailure::LocalEncryptionUnavailable
            } else {
                CoreFailure::SessionRequired
            };
            self.emit_failure(request_id, failure);
        }
    }

    async fn project_verification_failure(
        &mut self,
        flow_id: u64,
        _target: VerificationTarget,
        kind: TrustOperationFailureKind,
    ) {
        self.settle_verification(flow_id, VerificationTerminal::Failed(kind))
            .await;
    }

    fn emit_verification_progress(&self, state: VerificationFlowState) {
        if let Some(account_key) = self.active_account_key() {
            self.emit(CoreEvent::E2eeTrust(E2eeTrustEvent::VerificationProgress {
                account_key,
                state,
            }));
        }
    }

    pub(super) async fn handle_cancel_identity_reset(
        &mut self,
        _request_id: RequestId,
        flow_id: u64,
    ) {
        if self.identity_reset_flow_id != Some(flow_id) {
            return;
        }
        let account_key = self.active_account_key();
        self.cancel_identity_reset_handle().await;
        self.send_actions(vec![AppAction::ResetIdentityCancelled {
            request_id: flow_id,
        }])
        .await;
        if let Some(account_key) = account_key {
            for event in project_identity_reset_failed_event(
                flow_id,
                account_key,
                TrustOperationFailureKind::Cancelled,
            ) {
                self.emit(event);
            }
        }
    }

    pub(super) async fn handle_identity_reset_auth_timeout(&mut self, flow_id: u64) {
        if self.identity_reset_flow_id != Some(flow_id) {
            return;
        }
        let account_key = self.active_account_key();
        self.cancel_identity_reset_handle().await;
        self.send_actions(vec![AppAction::ResetIdentityTimedOut {
            request_id: flow_id,
        }])
        .await;
        if let Some(account_key) = account_key {
            for event in project_identity_reset_failed_event(
                flow_id,
                account_key,
                TrustOperationFailureKind::Timeout,
            ) {
                self.emit(event);
            }
        }
    }

    pub(super) async fn handle_submit_identity_reset_auth(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
        request: koushi_state::IdentityResetAuthRequest,
    ) {
        let flow_request_id = RequestId {
            connection_id: request_id.connection_id,
            sequence: flow_id,
        };
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.cancel_identity_reset_handle().await;
                self.send_actions(vec![AppAction::ResetIdentityFailed {
                    request_id: flow_id,
                    kind: TrustOperationFailureKind::Sdk,
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::SessionRequired);
                return;
            }
        };
        let account_key = AccountKey(session.info.user_id.clone());
        if self.identity_reset_flow_id != Some(flow_id) {
            return;
        }
        let result = match self.identity_reset_handle.as_ref() {
            Some(handle) => koushi_sdk::complete_identity_reset(&session, handle, &request).await,
            None => Err(koushi_sdk::E2eeTrustError::Sdk(
                "identity reset auth continuation missing".to_owned(),
            )),
        };

        drop(request);

        match result {
            Ok(()) => {
                self.clear_identity_reset_handle_after_completion();
                let (actions, events) =
                    project_reset_identity_completed(flow_request_id, account_key);
                self.send_actions(actions).await;
                for event in events {
                    self.emit(event);
                }
            }
            Err(error) => {
                self.cancel_identity_reset_handle().await;
                let (actions, events) =
                    project_reset_identity_error(flow_request_id, account_key, error);
                self.send_actions(actions).await;
                for event in events {
                    self.emit(event);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use koushi_diagnostics::DiagnosticField;

    use koushi_sdk::PersistableMatrixSession;
    use koushi_state::{
        AppAction, SasEmoji, TrustOperationFailureKind, VerificationCancelReason,
        VerificationTarget,
    };

    use tokio::sync::{broadcast, mpsc, oneshot};

    use super::{
        INCOMING_VERIFICATION_FLOW_ID_BASE, IncomingVerificationActivity,
        IncomingVerificationObservation, IncomingVerificationRequestDecision, SasAdoptionDecision,
        SasVerificationWaitState, SyntheticVerificationTerminal, VerificationTerminal,
        classify_incoming_verification_request, classify_sas_adoption,
        incoming_verification_request_id, incoming_verification_request_is_current,
        record_sas_verification_event, recovery_failure_token, resolve_sas_adoption,
        run_own_user_sas_start, sas_projection_action, sas_settled_event, sas_state_changed_event,
        sas_state_token, sas_timeout_fired_event, sas_verification_event, sas_waiting_for_token,
        send_observer_output_until_stopped, stop_incoming_verification_observation_with_timeout,
        trust_failure_token, verification_cancel_kind_token, verification_request_state_token,
        verification_terminal_token,
    };
    use crate::account::actor::{AccountActor, AccountMessage};
    use crate::account::recovery_backup::recovery_verification_event;
    use crate::account::test_support::{
        acknowledge_next_verified_projection, consume_initial_unknown_trust_projection,
        inspect_session_runtime, login_gated_actor, recv_account_action_with_sliding_sync_effects,
        shutdown_and_ack, spawn_actor_with_dirs, test_request_id,
    };
    use crate::command::AccountCommand;
    use crate::composer_draft_lifecycle::ComposerDraftLeaseRegistry;
    use crate::event::CoreEvent;
    use crate::executor;

    use crate::failure::{CoreFailure, RecoveryFailureKind};
    use crate::ids::RuntimeConnectionId;
    use crate::link_preview::LinkPreviewContext;

    use crate::store::CredentialStoreBackend;
    use crate::store::StoreActor;

    use tempfile::tempdir;

    #[test]
    fn own_user_sas_projects_gate_action_while_peer_sas_keeps_peer_projection() {
        let emojis = vec![
            SasEmoji {
                symbol: "x".into(),
                description: "opaque".into()
            };
            7
        ];
        assert!(
            matches!(sas_projection_action(true, 41, emojis.clone()), AppAction::GateSasPresented { flow_id: 41, emojis: projected } if projected == emojis)
        );
        assert!(
            matches!(sas_projection_action(false, 42, emojis.clone()), AppAction::VerificationSasPresented { request_id: 42, emojis: projected } if projected == emojis)
        );
    }

    #[test]
    fn sas_adoption_decision_adopts_once_and_rejects_replay_or_conflict() {
        assert_eq!(classify_sas_adoption(None, 41), SasAdoptionDecision::Adopt);
        assert_eq!(
            classify_sas_adoption(Some(41), 41),
            SasAdoptionDecision::Replay
        );
        assert_eq!(
            classify_sas_adoption(Some(41), 42),
            SasAdoptionDecision::Conflict
        );
    }

    #[tokio::test]
    async fn sas_replay_is_noop_but_conflict_runs_explicit_rejection() {
        let replay_rejections = Arc::new(AtomicU64::new(0));
        let replay = resolve_sas_adoption(Some(41), 41, {
            let replay_rejections = Arc::clone(&replay_rejections);
            move || async move {
                replay_rejections.fetch_add(1, Ordering::SeqCst);
                true
            }
        })
        .await;
        assert_eq!(replay, (SasAdoptionDecision::Replay, None));
        assert_eq!(replay_rejections.load(Ordering::SeqCst), 0);

        let conflict_rejections = Arc::new(AtomicU64::new(0));
        let conflict = resolve_sas_adoption(Some(41), 42, {
            let conflict_rejections = Arc::clone(&conflict_rejections);
            move || async move {
                conflict_rejections.fetch_add(1, Ordering::SeqCst);
                false
            }
        })
        .await;
        assert_eq!(conflict, (SasAdoptionDecision::Conflict, Some(false)));
        assert_eq!(conflict_rejections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn at_least_once_incoming_transport_uses_target_and_flow_identity() {
        let active_target = VerificationTarget {
            user_id: "@alice:example.test".to_owned(),
            device_id: "ALICE".to_owned(),
        };
        let peer_collision = VerificationTarget {
            user_id: "@mallory:example.test".to_owned(),
            device_id: "MALLORY".to_owned(),
        };
        let device_collision = VerificationTarget {
            user_id: active_target.user_id.clone(),
            device_id: "ALICE-SECOND".to_owned(),
        };
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: Some((&active_target, "stable-flow")),
                    sas_active: false,
                    own_user_active: false,
                },
                &peer_collision,
                "stable-flow",
            ),
            IncomingVerificationRequestDecision::Conflict,
            "the same opaque flow ID from a different peer/device must be rejected",
        );
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: Some((&active_target, "stable-flow")),
                    sas_active: false,
                    own_user_active: false,
                },
                &device_collision,
                "stable-flow",
            ),
            IncomingVerificationRequestDecision::Conflict,
            "the same opaque flow ID from a different device must be rejected",
        );
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: Some((&active_target, "stable-flow")),
                    sas_active: false,
                    own_user_active: false,
                },
                &active_target,
                "stable-flow",
            ),
            IncomingVerificationRequestDecision::Replay,
        );
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: Some((&active_target, "stable-flow")),
                    sas_active: false,
                    own_user_active: false,
                },
                &active_target,
                "other-flow",
            ),
            IncomingVerificationRequestDecision::Conflict,
        );
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: None,
                    sas_active: false,
                    own_user_active: false,
                },
                &active_target,
                "new-flow",
            ),
            IncomingVerificationRequestDecision::Adopt,
        );
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: None,
                    sas_active: true,
                    own_user_active: false,
                },
                &active_target,
                "new-flow",
            ),
            IncomingVerificationRequestDecision::Conflict,
            "an active SAS continuation must continue to reject a new request",
        );
    }

    #[test]
    fn active_own_user_verification_conflicts_with_incoming_request() {
        let incoming_target = VerificationTarget {
            user_id: "@alice:example.test".to_owned(),
            device_id: "ALICE".to_owned(),
        };
        assert_eq!(
            classify_incoming_verification_request(
                IncomingVerificationActivity {
                    active_request: None,
                    sas_active: false,
                    own_user_active: true,
                },
                &incoming_target,
                "incoming-flow",
            ),
            IncomingVerificationRequestDecision::Conflict,
            "an own-user verification owns the shared continuation/observer slots",
        );
    }

    #[test]
    fn incoming_verification_transport_rejects_stale_or_sessionless_messages() {
        assert!(incoming_verification_request_is_current(7, 7, true));
        assert!(!incoming_verification_request_is_current(6, 7, true));
        assert!(!incoming_verification_request_is_current(7, 7, false));
    }

    #[tokio::test]
    async fn incoming_verification_mailbox_send_is_stop_aware_when_full() {
        let (sender, mut receiver) = mpsc::channel(1);
        let (_first_stop_tx, mut first_stop_rx) = oneshot::channel();
        assert!(
            send_observer_output_until_stopped(&sender, 1_u8, &mut first_stop_rx,).await,
            "the first ready delivery must fill the product mailbox"
        );
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let blocked_send = executor::spawn(async move {
            send_observer_output_until_stopped(&sender, 2, &mut stop_rx).await
        });
        tokio::task::yield_now().await;

        stop_tx.send(()).expect("request observer stop");
        let delivered = executor::timeout(Duration::from_millis(20), blocked_send)
            .await
            .expect("a stop request must interrupt the full-mailbox send")
            .expect("send task");
        assert!(
            !delivered,
            "a stopped observer must not report the blocked send as delivered"
        );
        assert_eq!(receiver.recv().await, Some(1));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn incoming_verification_observer_join_has_a_bounded_abort_fallback() {
        let persistable = PersistableMatrixSession::from_json(
            r#"{"homeserver":"https://matrix.example.invalid","user_id":"@alice:example.invalid","device_id":"ALICEDEVICE","access_token":"synthetic-access"}"#,
        )
        .expect("synthetic session should deserialize");
        let session = koushi_sdk::restore_session(&persistable)
            .await
            .expect("synthetic session should restore");
        let mut observer = koushi_sdk::observe_incoming_verification_requests(&session).await;
        let receiver = observer
            .take_receiver()
            .expect("observer receiver is available once");
        let (stop_tx, _stop_rx) = oneshot::channel();
        let child = executor::spawn(async move {
            let _receiver = receiver;
            std::future::pending::<()>().await
        });
        let child_abort = child.abort_handle();
        let observation = IncomingVerificationObservation {
            stop_tx,
            task: child,
            observer,
        };
        let mut stop = executor::spawn(stop_incoming_verification_observation_with_timeout(
            observation,
            Duration::from_millis(1),
        ));

        let result = executor::timeout(Duration::from_millis(20), &mut stop).await;
        if result.is_err() {
            stop.abort();
            child_abort.abort();
        }
        assert!(
            result.is_ok(),
            "a nonresponsive observer must be aborted after a bounded join"
        );
    }

    #[tokio::test]
    async fn actor_sas_settlement_emits_exactly_one_terminal_and_clears_runtime() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let diagnostic_start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        let cred_dir = tempdir().expect("credential tempdir");
        let data_dir = tempdir().expect("data tempdir");
        let store = StoreActor::with_backend(
            CredentialStoreBackend::FileDir(crate::store::FileCredentialStore::new(
                cred_dir.path(),
            )),
            data_dir.path(),
        );
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, _) = broadcast::channel(16);
        let handle = AccountActor::spawn(
            store,
            action_tx,
            event_tx,
            LinkPreviewContext::default(),
            Arc::new(ComposerDraftLeaseRegistry::new()),
        );

        let cases = [
            SyntheticVerificationTerminal::Success,
            SyntheticVerificationTerminal::Cancelled(VerificationCancelReason::User),
            SyntheticVerificationTerminal::Cancelled(VerificationCancelReason::Mismatch),
            SyntheticVerificationTerminal::Failed(TrustOperationFailureKind::Timeout),
            SyntheticVerificationTerminal::Failed(TrustOperationFailureKind::Sdk),
        ];
        for (index, terminal) in cases.into_iter().enumerate() {
            let flow_id = index as u64 + 100;
            assert!(
                handle
                    .send(AccountMessage::ConfigureSyntheticVerification { flow_id })
                    .await
            );
            assert!(
                handle
                    .send(AccountMessage::SettleSyntheticVerification { flow_id, terminal })
                    .await
            );
            let actions = action_rx.recv().await.expect("one terminal action");
            assert_eq!(
                actions.len(),
                1,
                "flow {flow_id} must emit one terminal action"
            );
            let terminal_request_id = match (&terminal, actions.as_slice()) {
                (
                    SyntheticVerificationTerminal::Success,
                    [AppAction::VerificationCompleted { request_id }],
                )
                | (
                    SyntheticVerificationTerminal::Cancelled(_),
                    [AppAction::VerificationCancelled { request_id, .. }],
                )
                | (
                    SyntheticVerificationTerminal::Failed(_),
                    [AppAction::VerificationFailed { request_id, .. }],
                ) => *request_id,
                unexpected => panic!("unexpected terminal projection: {unexpected:?}"),
            };
            assert_eq!(terminal_request_id, flow_id);

            let (response, inspected) = oneshot::channel();
            assert!(
                handle
                    .send(AccountMessage::InspectVerificationRuntime { response })
                    .await
            );
            assert_eq!(
                inspected.await.expect("runtime inspection"),
                (false, false, false, false, false, false, false)
            );

            assert!(
                handle
                    .send(AccountMessage::SettleSyntheticVerification { flow_id, terminal })
                    .await
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(20), action_rx.recv())
                    .await
                    .is_err(),
                "stale terminal duplicated flow {flow_id}"
            );
        }
        let settled_flow_ids = koushi_diagnostics::test_support::detail_snapshot().records
            [diagnostic_start..]
            .iter()
            .filter(|record| {
                record.event.source == "core.sas_verification" && record.event.stage == "settled"
            })
            .filter_map(|record| {
                record
                    .event
                    .fields
                    .iter()
                    .find_map(|field| (field.key == "flow_id").then_some(&field.value))
            })
            .filter_map(|value| match value {
                koushi_diagnostics::DiagnosticValue::Count(flow_id) => Some(*flow_id),
                _ => None,
            })
            .filter(|flow_id| (100..=104).contains(flow_id))
            .collect::<Vec<_>>();
        assert_eq!(settled_flow_ids, vec![100, 101, 102, 103, 104]);
        shutdown_and_ack(&handle).await;
    }

    #[test]
    fn sas_verification_tokens_are_closed_and_private_safe() {
        use koushi_sdk::MatrixSasState as SasState;
        use koushi_sdk::MatrixVerificationCancelKind as CancelKind;
        use koushi_sdk::MatrixVerificationRequestState as RequestState;

        assert_eq!(
            verification_request_state_token(&RequestState::Created),
            "created"
        );
        assert_eq!(
            verification_request_state_token(&RequestState::Requested),
            "requested"
        );
        assert_eq!(
            verification_request_state_token(&RequestState::Ready),
            "ready"
        );
        assert_eq!(
            verification_request_state_token(&RequestState::Done),
            "done"
        );
        assert_eq!(
            verification_request_state_token(&RequestState::Cancelled {
                kind: CancelKind::Timeout,
                cancelled_by_us: false,
            }),
            "cancelled"
        );
        assert_eq!(
            verification_request_state_token(&RequestState::UnsupportedMethod),
            "unsupported_method"
        );

        let cancel_kinds = [
            (CancelKind::UnknownMethod, "unknown_method"),
            (CancelKind::KeyMismatch, "key_mismatch"),
            (CancelKind::User, "user"),
            (CancelKind::Timeout, "timeout"),
            (CancelKind::AcceptedElsewhere, "accepted_elsewhere"),
            (CancelKind::Other, "other"),
        ];
        for (kind, token) in cancel_kinds {
            assert_eq!(verification_cancel_kind_token(kind), token);
        }

        let sas_states = [
            (SasState::Created, "created"),
            (SasState::Started, "started"),
            (SasState::Accepted, "accepted"),
            (
                SasState::SasPresented { emojis: Vec::new() },
                "sas_presented",
            ),
            (SasState::Confirmed, "confirmed"),
            (SasState::Done, "done"),
            (
                SasState::Cancelled {
                    kind: CancelKind::Timeout,
                    cancelled_by_us: false,
                },
                "cancelled",
            ),
            (SasState::UnsupportedShortAuth, "unsupported_short_auth"),
        ];
        for (state, token) in sas_states {
            assert_eq!(sas_state_token(&state), token);
        }

        let failure_kinds = [
            (TrustOperationFailureKind::Cancelled, "cancelled"),
            (TrustOperationFailureKind::Mismatch, "mismatch"),
            (
                TrustOperationFailureKind::InvalidPassphrase,
                "invalid_passphrase",
            ),
            (TrustOperationFailureKind::Network, "network"),
            (TrustOperationFailureKind::Forbidden, "forbidden"),
            (TrustOperationFailureKind::Timeout, "timeout"),
            (TrustOperationFailureKind::Sdk, "sdk"),
        ];
        for (kind, token) in failure_kinds {
            assert_eq!(trust_failure_token(kind), token);
        }
        let recovery_failure_kinds = [
            (
                RecoveryFailureKind::InvalidRecoveryKey,
                "invalid_recovery_key",
            ),
            (RecoveryFailureKind::Network, "network"),
            (RecoveryFailureKind::Server, "server"),
            (RecoveryFailureKind::Timeout, "timeout"),
        ];
        for (kind, token) in recovery_failure_kinds {
            assert_eq!(recovery_failure_token(kind), token);
        }

        let wait_states = [
            (
                SasVerificationWaitState::RecipientDevices,
                "recipient_devices",
            ),
            (
                SasVerificationWaitState::ToDeviceDelivery,
                "to_device_delivery",
            ),
            (SasVerificationWaitState::RemoteAccept, "remote_accept"),
            (SasVerificationWaitState::SasStart, "sas_start"),
            (SasVerificationWaitState::Mac, "mac"),
            (
                SasVerificationWaitState::CrossSigningSettlement,
                "cross_signing_settlement",
            ),
            (
                SasVerificationWaitState::NormalSyncResume,
                "normal_sync_resume",
            ),
        ];
        for (state, token) in wait_states {
            assert_eq!(sas_waiting_for_token(state), token);
        }

        assert_eq!(
            verification_terminal_token(VerificationTerminal::Success),
            "success"
        );
        assert_eq!(
            verification_terminal_token(VerificationTerminal::Cancelled(
                VerificationCancelReason::User,
            )),
            "cancelled"
        );
        assert_eq!(
            verification_terminal_token(VerificationTerminal::Failed(
                TrustOperationFailureKind::Timeout,
            )),
            "failed"
        );
    }

    #[test]
    fn sas_cancel_diagnostic_contains_only_closed_private_safe_fields() {
        use koushi_sdk::MatrixSasState as SasState;
        use koushi_sdk::MatrixVerificationCancelKind as CancelKind;

        let cancelled = sas_state_changed_event(
            41,
            &SasState::Cancelled {
                kind: CancelKind::Timeout,
                cancelled_by_us: false,
            },
        );
        assert_eq!(
            koushi_diagnostics::format_event(&cancelled),
            "stage=sas_state_changed flow_id=41 state=cancelled cancel_kind=timeout cancelled_by_us=false"
        );

        let accepted = sas_state_changed_event(42, &SasState::Accepted);
        assert_eq!(
            koushi_diagnostics::format_event(&accepted),
            "stage=sas_state_changed flow_id=42 state=accepted waiting_for=sas_start"
        );

        let settled = sas_settled_event(
            43,
            VerificationTerminal::Failed(TrustOperationFailureKind::Timeout),
            Some(SasVerificationWaitState::RemoteAccept),
        );
        assert_eq!(
            koushi_diagnostics::format_event(&settled),
            "stage=settled flow_id=43 terminal=failed waiting_for=remote_accept failure_kind=timeout"
        );

        let timeout = sas_timeout_fired_event(44, Some(SasVerificationWaitState::Mac));
        assert_eq!(
            koushi_diagnostics::format_event(&timeout),
            "stage=timeout_fired flow_id=44 waiting_for=mac"
        );

        let recovery = recovery_verification_event("settled", 45)
            .field(DiagnosticField::token("terminal", "failed"))
            .field(DiagnosticField::token(
                "failure_kind",
                recovery_failure_token(RecoveryFailureKind::InvalidRecoveryKey),
            ));
        assert_eq!(
            koushi_diagnostics::format_event(&recovery),
            "stage=settled flow_id=45 flow_type=recovery_key terminal=failed failure_kind=invalid_recovery_key"
        );
    }

    #[tokio::test]
    async fn own_user_sas_start_helper_traces_started_pending_and_failed_results() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let diagnostic_start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();

        assert_eq!(
            run_own_user_sas_start(211, "request_ready", async {
                Ok::<_, koushi_sdk::E2eeTrustError>(Some(7_u8))
            })
            .await
            .expect("started result"),
            Some(7)
        );
        assert_eq!(
            run_own_user_sas_start(212, "initial", async {
                Ok::<Option<u8>, koushi_sdk::E2eeTrustError>(None)
            })
            .await
            .expect("pending result"),
            None
        );
        assert!(
            run_own_user_sas_start(213, "provisional_encryption_sync", async {
                Err::<Option<u8>, _>(koushi_sdk::E2eeTrustError::Sdk(
                    "private SDK detail".to_owned(),
                ))
            })
            .await
            .is_err()
        );

        let records = koushi_diagnostics::test_support::detail_snapshot().records;
        let events = records[diagnostic_start..]
            .iter()
            .filter(|record| record.event.source == "core.sas_verification")
            .map(|record| koushi_diagnostics::format_event(&record.event))
            .collect::<Vec<_>>();
        assert_eq!(
            events,
            vec![
                "stage=sas_start_attempted flow_id=211 source=request_ready",
                "stage=sas_start_finished flow_id=211 source=request_ready outcome=started",
                "stage=sas_start_attempted flow_id=212 source=initial",
                "stage=sas_start_finished flow_id=212 source=initial outcome=pending",
                "stage=sas_start_attempted flow_id=213 source=provisional_encryption_sync",
                "stage=sas_start_finished flow_id=213 source=provisional_encryption_sync outcome=failed failure_kind=sdk",
            ]
        );
        assert!(!events.join(" ").contains("private SDK detail"));
    }

    #[test]
    fn sas_verification_diagnostic_records_without_stderr() {
        let output = std::process::Command::new(
            std::env::current_exe().expect("current test executable should be available"),
        )
        .args([
            "--exact",
            "account::verification::tests::sas_verification_diagnostic_child",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .expect("SAS verification diagnostic child should run");
        assert!(output.status.success(), "child failed: {output:?}");

        let stderr = String::from_utf8(output.stderr).expect("child stderr should be utf8");
        assert!(
            stderr.is_empty(),
            "private diagnostics stay in the buffer only"
        );

        let stdout = String::from_utf8(output.stdout).expect("child stdout should be utf8");
        let snapshot: serde_json::Value = serde_json::from_str(
            stdout
                .lines()
                .find(|line| line.starts_with('{'))
                .expect("child should print one JSON snapshot"),
        )
        .expect("child output should be a JSON snapshot");
        assert!(snapshot["records"].as_array().is_some_and(|records| {
            records.iter().any(|record| {
                record["event"]["source"] == "core.sas_verification"
                    && record["event"]["stage"] == "request_state_changed"
            })
        }));
    }

    #[test]
    #[ignore]
    fn sas_verification_diagnostic_child() {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        record_sas_verification_event(
            sas_verification_event("request_state_changed", 41)
                .field(DiagnosticField::token("state", "cancelled"))
                .field(DiagnosticField::token("cancel_kind", "timeout"))
                .field(DiagnosticField::boolean("cancelled_by_us", false)),
        );
        println!(
            "{}",
            serde_json::to_string(&koushi_diagnostics::test_support::detail_snapshot())
                .expect("diagnostic snapshot should serialize")
        );
    }

    #[test]
    fn incoming_verification_flow_ids_use_reserved_internal_namespace() {
        let request_id = incoming_verification_request_id(INCOMING_VERIFICATION_FLOW_ID_BASE);

        assert_eq!(request_id.connection_id, RuntimeConnectionId(0));
        assert_eq!(request_id.sequence, INCOMING_VERIFICATION_FLOW_ID_BASE);
    }

    #[tokio::test]
    async fn own_user_sas_proof_success_enters_shared_authoritative_promotion_path() {
        let (handle, mut action_rx) = login_gated_actor().await;
        consume_initial_unknown_trust_projection(&mut action_rx).await;
        let flow_id = 83;
        handle
            .send(AccountMessage::ConfigureSyntheticVerification { flow_id })
            .await;
        handle
            .send(AccountMessage::SettleSyntheticVerification {
                flow_id,
                terminal: SyntheticVerificationTerminal::Success,
            })
            .await;
        let mut verification_completed = false;
        let mut authoritative_recheck_settled = false;
        while !(verification_completed && authoritative_recheck_settled) {
            let actions =
                recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx).await;
            verification_completed |= matches!(
                actions.as_slice(),
                [AppAction::VerificationCompleted { request_id }] if *request_id == flow_id
            );
            authoritative_recheck_settled |= matches!(
                actions.as_slice(),
                [AppAction::AuthoritativeDeviceTrustChanged {
                    trust: koushi_state::CurrentDeviceTrustState::Unknown
                        | koushi_state::CurrentDeviceTrustState::Unverified,
                    ..
                }]
            );
        }
        handle
            .send(AccountMessage::CurrentDeviceTrustChanged {
                generation: 2,
                trust: koushi_state::CurrentDeviceTrustState::Verified,
            })
            .await;
        acknowledge_next_verified_projection(&handle, &mut action_rx).await;
        assert_eq!(
            inspect_session_runtime(&handle).await,
            (true, true, true, true)
        );
        let _ = handle.send(AccountMessage::Shutdown).await;
    }

    #[tokio::test]
    async fn identity_reset_auth_without_session_settles_pending_state() {
        let cred_dir = tempdir().expect("tempdir");
        let data_dir = tempdir().expect("tempdir");
        let (handle, mut action_rx, mut event_rx) =
            spawn_actor_with_dirs(cred_dir.path(), data_dir.path());

        let request_id = test_request_id();
        let flow_id = 99;
        assert!(
            handle
                .send(AccountMessage::Command(
                    AccountCommand::SubmitIdentityResetAuth {
                        request_id,
                        flow_id,
                        request: koushi_state::IdentityResetAuthRequest::OAuthApproved,
                    }
                ))
                .await
        );

        let actions = action_rx.recv().await.expect("trust failure action batch");
        assert_eq!(
            actions,
            vec![AppAction::ResetIdentityFailed {
                request_id: flow_id,
                kind: koushi_state::TrustOperationFailureKind::Sdk,
            }]
        );

        match event_rx.recv().await.expect("event") {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed(SessionRequired), got {other:?}"),
        }
    }
}
