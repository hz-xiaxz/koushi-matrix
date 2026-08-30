//! Core-owned request settlement over the event broadcast and snapshot watch.
//!
//! These types are intentionally not serde DTOs. They are an in-process API;
//! adapters may choose how to encode the returned result at their boundary.

use std::fmt;

use koushi_state::{
    AppState, ComposerDraftRevision, ComposerTarget, FocusedContextState, InviteOperationState,
    SearchScope, SessionState, SubmissionId,
};
use tokio::sync::broadcast;

use super::connection::CoreConnection;
use crate::event::{
    AccountEvent, CoreEvent, IntentNoOpReason, IntentOutcome, RoomEvent, TimelineEvent,
    VersionedAppStateSnapshot,
};
use crate::failure::CoreFailure;
use crate::ids::{AccountKey, RequestId, TimelineKey};

#[derive(Clone, Eq, PartialEq)]
pub enum OutcomeCorrelation {
    Request(RequestId),
    Submission {
        request_id: RequestId,
        submission_id: SubmissionId,
    },
}

#[derive(Clone, Eq, PartialEq)]
pub enum RoomOperationKind {
    SpaceChildSet {
        space_id: String,
        child_room_id: String,
    },
    UserInvited {
        user_id: String,
    },
    InviteAccepted,
    InviteDeclined,
    MarkedAsRead,
    MarkedAsUnread,
    RoomLeft,
    RoomForgotten,
    RoomTagSet,
    RoomTagRemoved,
    PinEvent,
    UnpinEvent,
    MemberModerated {
        target_user_id: String,
    },
    MemberRoleUpdated {
        target_user_id: String,
    },
}

#[derive(Clone, Eq, PartialEq)]
pub enum RequestOutcomeExpectation {
    OidcAuthorization {
        request_id: RequestId,
    },
    AuthDiscovery {
        request_id: RequestId,
        homeserver: String,
    },
    Authenticated {
        request_id: RequestId,
        account_key: Option<AccountKey>,
    },
    SignedOut {
        request_id: RequestId,
        account_key: AccountKey,
    },
    SavedSessions {
        request_id: RequestId,
    },
    RoomSelected {
        request_id: RequestId,
        room_id: String,
        account_key: Option<AccountKey>,
        allow_initial: bool,
    },
    FocusedContextClosed {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: Option<String>,
    },
    FocusedContextOpened {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: String,
        event_id: Option<String>,
    },
    MainTimelineAnchor {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        allow_live_fallback: bool,
    },
    RoomCreated {
        request_id: RequestId,
        account_key: AccountKey,
    },
    SpaceCreated {
        request_id: RequestId,
        account_key: AccountKey,
    },
    DirectMessageStarted {
        request_id: RequestId,
        account_key: AccountKey,
    },
    RoomJoined {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: String,
    },
    InviteWorkflow {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: String,
        query: String,
    },
    RoomOperation {
        request_id: RequestId,
        account_key: AccountKey,
        room_id: String,
        operation: RoomOperationKind,
    },
    SearchStarted {
        request_id: RequestId,
        account_key: Option<AccountKey>,
        query: String,
        scope: SearchScope,
    },
    SearchClosed {
        request_id: RequestId,
        account_key: Option<AccountKey>,
        allow_initial: bool,
    },
    UploadStaging {
        request_id: RequestId,
        account_key: AccountKey,
        target: ComposerTarget,
        staged_ids: Vec<String>,
    },
    ComposerAccepted {
        request_id: RequestId,
        account_key: AccountKey,
        target: ComposerTarget,
        expected_revision: ComposerDraftRevision,
    },
    Submission {
        request_id: RequestId,
        account_key: AccountKey,
        target: ComposerTarget,
        submission_id: SubmissionId,
    },
    PreparedMediaQueued {
        request_id: RequestId,
        key: TimelineKey,
        transaction_id: String,
    },
}

#[derive(Clone, Eq, PartialEq)]
pub enum RequestOutcome {
    OidcAuthorization {
        request_id: RequestId,
        authorization_url: String,
        state: String,
        generation: u64,
    },
    AuthDiscovery {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    Authenticated {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    SignedOut {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    SavedSessions {
        request_id: RequestId,
        sessions: Vec<koushi_state::SessionInfo>,
    },
    RoomSelected {
        snapshot: VersionedAppStateSnapshot,
    },
    FocusedContext {
        snapshot: VersionedAppStateSnapshot,
    },
    MainTimelineAnchor {
        snapshot: VersionedAppStateSnapshot,
    },
    RoomCreated {
        request_id: RequestId,
        room_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
    SpaceCreated {
        request_id: RequestId,
        space_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
    DirectMessageStarted {
        request_id: RequestId,
        room_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
    RoomJoined {
        request_id: RequestId,
        room_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
    InviteWorkflow {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    RoomOperation {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    Search {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    UploadStaging {
        request_id: RequestId,
        snapshot: VersionedAppStateSnapshot,
    },
    ComposerAccepted {
        request_id: RequestId,
        revision: ComposerDraftRevision,
        snapshot: VersionedAppStateSnapshot,
    },
    SubmissionAccepted {
        request_id: RequestId,
        submission_id: SubmissionId,
        transaction_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
    SubmissionRejected {
        request_id: RequestId,
        submission_id: SubmissionId,
        kind: crate::failure::TimelineFailureKind,
        snapshot: VersionedAppStateSnapshot,
    },
    PreparedMediaQueued {
        request_id: RequestId,
        transaction_id: String,
        snapshot: VersionedAppStateSnapshot,
    },
}

#[derive(Clone, Copy, Eq, PartialEq, thiserror::Error)]
pub enum RequestOutcomeError {
    #[error("request operation failed")]
    OperationFailed { failure: CoreFailure },
    #[error("request completed without applying a state change")]
    FailedNoOp { reason: IntentNoOpReason },
    #[error("request outcome event stream lagged")]
    Lagged,
    #[error("request outcome event stream disconnected")]
    Disconnected,
    #[error("request outcome timed out")]
    TimedOut,
    #[error("request outcome correlation or expectation is invalid")]
    InvalidOutcome,
}

impl fmt::Debug for OutcomeCorrelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(request_id) => formatter
                .debug_struct("Request")
                .field("request_id", request_id)
                .finish(),
            Self::Submission { request_id, .. } => formatter
                .debug_struct("Submission")
                .field("request_id", request_id)
                .field("submission_id", &"SubmissionId(..)")
                .finish(),
        }
    }
}

impl fmt::Debug for RoomOperationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SpaceChildSet { .. } => "SpaceChildSet",
            Self::UserInvited { .. } => "UserInvited",
            Self::InviteAccepted => "InviteAccepted",
            Self::InviteDeclined => "InviteDeclined",
            Self::MarkedAsRead => "MarkedAsRead",
            Self::MarkedAsUnread => "MarkedAsUnread",
            Self::RoomLeft => "RoomLeft",
            Self::RoomForgotten => "RoomForgotten",
            Self::RoomTagSet => "RoomTagSet",
            Self::RoomTagRemoved => "RoomTagRemoved",
            Self::PinEvent => "PinEvent",
            Self::UnpinEvent => "UnpinEvent",
            Self::MemberModerated { .. } => "MemberModerated",
            Self::MemberRoleUpdated { .. } => "MemberRoleUpdated",
        };
        formatter
            .debug_tuple("RoomOperationKind")
            .field(&kind)
            .finish()
    }
}

impl fmt::Debug for RequestOutcomeExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::OidcAuthorization { .. } => "OidcAuthorization",
            Self::AuthDiscovery { .. } => "AuthDiscovery",
            Self::Authenticated { .. } => "Authenticated",
            Self::SignedOut { .. } => "SignedOut",
            Self::SavedSessions { .. } => "SavedSessions",
            Self::RoomSelected { .. } => "RoomSelected",
            Self::FocusedContextClosed { .. } => "FocusedContextClosed",
            Self::FocusedContextOpened { .. } => "FocusedContextOpened",
            Self::MainTimelineAnchor { .. } => "MainTimelineAnchor",
            Self::RoomCreated { .. } => "RoomCreated",
            Self::SpaceCreated { .. } => "SpaceCreated",
            Self::DirectMessageStarted { .. } => "DirectMessageStarted",
            Self::RoomJoined { .. } => "RoomJoined",
            Self::InviteWorkflow { .. } => "InviteWorkflow",
            Self::RoomOperation { .. } => "RoomOperation",
            Self::SearchStarted { .. } => "SearchStarted",
            Self::SearchClosed { .. } => "SearchClosed",
            Self::UploadStaging { .. } => "UploadStaging",
            Self::ComposerAccepted { .. } => "ComposerAccepted",
            Self::Submission { .. } => "Submission",
            Self::PreparedMediaQueued { .. } => "PreparedMediaQueued",
        };
        formatter
            .debug_tuple("RequestOutcomeExpectation")
            .field(&kind)
            .finish()
    }
}

impl fmt::Debug for RequestOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::OidcAuthorization { .. } => "OidcAuthorization",
            Self::AuthDiscovery { .. } => "AuthDiscovery",
            Self::Authenticated { .. } => "Authenticated",
            Self::SignedOut { .. } => "SignedOut",
            Self::SavedSessions { .. } => "SavedSessions",
            Self::RoomSelected { .. } => "RoomSelected",
            Self::FocusedContext { .. } => "FocusedContext",
            Self::MainTimelineAnchor { .. } => "MainTimelineAnchor",
            Self::RoomCreated { .. } => "RoomCreated",
            Self::SpaceCreated { .. } => "SpaceCreated",
            Self::DirectMessageStarted { .. } => "DirectMessageStarted",
            Self::RoomJoined { .. } => "RoomJoined",
            Self::InviteWorkflow { .. } => "InviteWorkflow",
            Self::RoomOperation { .. } => "RoomOperation",
            Self::Search { .. } => "Search",
            Self::UploadStaging { .. } => "UploadStaging",
            Self::ComposerAccepted { .. } => "ComposerAccepted",
            Self::SubmissionAccepted { .. } => "SubmissionAccepted",
            Self::SubmissionRejected { .. } => "SubmissionRejected",
            Self::PreparedMediaQueued { .. } => "PreparedMediaQueued",
        };
        formatter
            .debug_tuple("RequestOutcome")
            .field(&kind)
            .finish()
    }
}

impl fmt::Debug for RequestOutcomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { failure } => formatter
                .debug_struct("OperationFailed")
                .field("failure", failure)
                .finish(),
            Self::FailedNoOp { reason } => formatter
                .debug_struct("FailedNoOp")
                .field("reason", reason)
                .finish(),
            Self::Lagged => formatter.write_str("Lagged"),
            Self::Disconnected => formatter.write_str("Disconnected"),
            Self::TimedOut => formatter.write_str("TimedOut"),
            Self::InvalidOutcome => formatter.write_str("InvalidOutcome"),
        }
    }
}

impl RequestOutcomeExpectation {
    fn request_id(&self) -> RequestId {
        match self {
            Self::OidcAuthorization { request_id }
            | Self::AuthDiscovery { request_id, .. }
            | Self::Authenticated { request_id, .. }
            | Self::SignedOut { request_id, .. }
            | Self::SavedSessions { request_id, .. }
            | Self::RoomSelected { request_id, .. }
            | Self::FocusedContextClosed { request_id, .. }
            | Self::FocusedContextOpened { request_id, .. }
            | Self::MainTimelineAnchor { request_id, .. }
            | Self::RoomCreated { request_id, .. }
            | Self::SpaceCreated { request_id, .. }
            | Self::DirectMessageStarted { request_id, .. }
            | Self::RoomJoined { request_id, .. }
            | Self::InviteWorkflow { request_id, .. }
            | Self::RoomOperation { request_id, .. }
            | Self::SearchStarted { request_id, .. }
            | Self::SearchClosed { request_id, .. }
            | Self::UploadStaging { request_id, .. }
            | Self::ComposerAccepted { request_id, .. }
            | Self::Submission { request_id, .. }
            | Self::PreparedMediaQueued { request_id, .. } => *request_id,
        }
    }

    fn lag_is_terminal(&self) -> bool {
        matches!(
            self,
            Self::OidcAuthorization { .. }
                | Self::AuthDiscovery { .. }
                | Self::SearchStarted { .. }
                | Self::SearchClosed { .. }
                | Self::RoomCreated { .. }
                | Self::SpaceCreated { .. }
                | Self::ComposerAccepted { .. }
                | Self::Submission { .. }
                | Self::PreparedMediaQueued { .. }
                | Self::SavedSessions { .. }
        )
    }
}

impl CoreConnection {
    /// Wait for a closed typed request outcome. The broadcast is a wake source;
    /// the watch snapshot is the authority for projection-backed success.
    pub async fn wait_for_request_outcome(
        &mut self,
        correlation: OutcomeCorrelation,
        expectation: RequestOutcomeExpectation,
        baseline_generation: u64,
        deadline: tokio::time::Instant,
    ) -> Result<RequestOutcome, RequestOutcomeError> {
        if !correlation_matches(&correlation, &expectation) {
            return Err(RequestOutcomeError::InvalidOutcome);
        }

        let mut progress: Option<EventProgress> = None;
        if let Some(outcome) = snapshot_outcome(
            &expectation,
            &self.versioned_snapshot(),
            baseline_generation,
            allows_initial_snapshot(&expectation),
        ) {
            return Ok(outcome);
        }

        loop {
            if let Some(outcome) = progress.as_ref().and_then(|progress| {
                progress.snapshot_outcome(
                    &expectation,
                    &self.versioned_snapshot(),
                    baseline_generation,
                )
            }) {
                return Ok(outcome);
            }
            if tokio::time::Instant::now() >= deadline {
                return final_result(
                    &expectation,
                    &self.versioned_snapshot(),
                    baseline_generation,
                    RequestOutcomeError::TimedOut,
                    progress,
                );
            }

            let received = tokio::time::timeout_at(deadline, async {
                tokio::select! {
                    biased;
                    changed = self.snapshot_rx.changed() => SnapshotWake::from_changed(changed),
                    event = self.event_rx.recv() => SnapshotWake::from_event(event, &self),
                }
            })
            .await;

            match received {
                Ok(SnapshotWake::SnapshotChanged) => {}
                Ok(SnapshotWake::Event(event)) => match event_progress(event, &expectation) {
                    Ok(Some(next)) => {
                        if let Some(outcome) =
                            next.event_outcome(&expectation, &self.versioned_snapshot())
                        {
                            return Ok(outcome);
                        }
                        progress = Some(next);
                    }
                    Ok(None) => {}
                    Err(error) => return Err(error),
                },
                Ok(SnapshotWake::Lagged) => {
                    if let Some(outcome) = progress.as_ref().and_then(|progress| {
                        progress.snapshot_outcome(
                            &expectation,
                            &self.versioned_snapshot(),
                            baseline_generation,
                        )
                    }) {
                        return Ok(outcome);
                    }
                    if expectation.lag_is_terminal() {
                        return final_result(
                            &expectation,
                            &self.versioned_snapshot(),
                            baseline_generation,
                            RequestOutcomeError::Lagged,
                            progress,
                        );
                    }
                }
                Ok(SnapshotWake::Disconnected) => {
                    return final_result(
                        &expectation,
                        &self.versioned_snapshot(),
                        baseline_generation,
                        RequestOutcomeError::Disconnected,
                        progress,
                    );
                }
                Err(_) => {
                    return final_result(
                        &expectation,
                        &self.versioned_snapshot(),
                        baseline_generation,
                        RequestOutcomeError::TimedOut,
                        progress,
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
enum EventProgress {
    Oidc {
        request_id: RequestId,
        authorization_url: String,
        state: String,
    },
    AuthDiscovery {
        request_id: RequestId,
        homeserver: String,
    },
    SavedSessions {
        request_id: RequestId,
        sessions: Vec<koushi_state::SessionInfo>,
    },
    RoomCreated {
        request_id: RequestId,
        room_id: String,
    },
    SpaceCreated {
        request_id: RequestId,
        space_id: String,
    },
    DirectMessageStarted {
        request_id: RequestId,
        room_id: String,
    },
    RoomJoined {
        request_id: RequestId,
        room_id: String,
    },
    Authenticated {
        request_id: RequestId,
        account_key: AccountKey,
    },
    SignedOut {
        request_id: RequestId,
        account_key: AccountKey,
    },
    Focused {
        request_id: RequestId,
        opened: bool,
    },
    Anchor {
        request_id: RequestId,
        live_fallback: bool,
    },
    RoomOperation {
        request_id: RequestId,
    },
    InviteWorkflow {
        request_id: RequestId,
    },
    Search {
        request_id: RequestId,
    },
    UploadStaging {
        request_id: RequestId,
    },
    ComposerAccepted {
        request_id: RequestId,
    },
    SubmissionAccepted {
        request_id: RequestId,
        submission_id: SubmissionId,
        transaction_id: String,
    },
    SubmissionRejected {
        request_id: RequestId,
        submission_id: SubmissionId,
        kind: crate::failure::TimelineFailureKind,
    },
    PreparedMediaQueued {
        request_id: RequestId,
        transaction_id: String,
    },
}

impl EventProgress {
    fn event_outcome(
        &self,
        expectation: &RequestOutcomeExpectation,
        snapshot: &VersionedAppStateSnapshot,
    ) -> Option<RequestOutcome> {
        match (self, expectation) {
            (
                Self::Oidc {
                    request_id,
                    authorization_url,
                    state,
                },
                RequestOutcomeExpectation::OidcAuthorization { .. },
            ) => Some(RequestOutcome::OidcAuthorization {
                request_id: *request_id,
                authorization_url: authorization_url.clone(),
                state: state.clone(),
                generation: snapshot.generation,
            }),
            (
                Self::SavedSessions {
                    request_id,
                    sessions,
                },
                RequestOutcomeExpectation::SavedSessions { .. },
            ) => Some(RequestOutcome::SavedSessions {
                request_id: *request_id,
                sessions: sessions.clone(),
            }),
            (
                Self::SubmissionRejected {
                    request_id,
                    submission_id,
                    kind,
                },
                RequestOutcomeExpectation::Submission {
                    submission_id: expected_submission_id,
                    ..
                },
            ) if submission_id == expected_submission_id => {
                Some(RequestOutcome::SubmissionRejected {
                    request_id: *request_id,
                    submission_id: submission_id.clone(),
                    kind: *kind,
                    snapshot: snapshot.clone(),
                })
            }
            (
                Self::PreparedMediaQueued {
                    request_id,
                    transaction_id,
                },
                RequestOutcomeExpectation::PreparedMediaQueued {
                    transaction_id: expected_transaction_id,
                    ..
                },
            ) if transaction_id == expected_transaction_id => {
                Some(RequestOutcome::PreparedMediaQueued {
                    request_id: *request_id,
                    transaction_id: transaction_id.clone(),
                    snapshot: snapshot.clone(),
                })
            }
            _ => None,
        }
    }

    fn request_id(&self) -> RequestId {
        match self {
            Self::Oidc { request_id, .. }
            | Self::AuthDiscovery { request_id, .. }
            | Self::SavedSessions { request_id, .. }
            | Self::RoomCreated { request_id, .. }
            | Self::SpaceCreated { request_id, .. }
            | Self::DirectMessageStarted { request_id, .. }
            | Self::RoomJoined { request_id, .. }
            | Self::Authenticated { request_id, .. }
            | Self::SignedOut { request_id, .. }
            | Self::Focused { request_id, .. }
            | Self::Anchor { request_id, .. }
            | Self::RoomOperation { request_id }
            | Self::InviteWorkflow { request_id }
            | Self::Search { request_id }
            | Self::UploadStaging { request_id }
            | Self::ComposerAccepted { request_id }
            | Self::PreparedMediaQueued { request_id, .. }
            | Self::SubmissionAccepted { request_id, .. }
            | Self::SubmissionRejected { request_id, .. } => *request_id,
        }
    }

    fn snapshot_outcome(
        &self,
        expectation: &RequestOutcomeExpectation,
        snapshot: &VersionedAppStateSnapshot,
        baseline_generation: u64,
    ) -> Option<RequestOutcome> {
        snapshot_outcome_for_progress(self, expectation, snapshot, baseline_generation)
    }
}

enum SnapshotWake {
    SnapshotChanged,
    Event(CoreEvent),
    Lagged,
    Disconnected,
}

impl SnapshotWake {
    fn from_changed(result: Result<(), tokio::sync::watch::error::RecvError>) -> Self {
        if result.is_ok() {
            Self::SnapshotChanged
        } else {
            Self::Disconnected
        }
    }

    fn from_event(
        result: Result<CoreEvent, broadcast::error::RecvError>,
        connection: &CoreConnection,
    ) -> Self {
        match result {
            Ok(event) => Self::Event(connection.project_event_for_consumer(event)),
            Err(broadcast::error::RecvError::Lagged(_)) => Self::Lagged,
            Err(broadcast::error::RecvError::Closed) => Self::Disconnected,
        }
    }
}

fn correlation_matches(
    correlation: &OutcomeCorrelation,
    expectation: &RequestOutcomeExpectation,
) -> bool {
    match correlation {
        OutcomeCorrelation::Request(request_id) => *request_id == expectation.request_id(),
        OutcomeCorrelation::Submission {
            request_id,
            submission_id,
        } => matches!(
            expectation,
            RequestOutcomeExpectation::Submission {
                request_id: expected_request_id,
                submission_id: expected_submission_id,
                ..
            } if request_id == expected_request_id && submission_id == expected_submission_id
        ),
    }
}

fn event_progress(
    event: CoreEvent,
    expectation: &RequestOutcomeExpectation,
) -> Result<Option<EventProgress>, RequestOutcomeError> {
    let request_id = expectation.request_id();
    match event {
        CoreEvent::OperationFailed {
            request_id: event_request_id,
            failure,
        } if event_request_id == request_id => {
            Err(RequestOutcomeError::OperationFailed { failure })
        }
        CoreEvent::IntentLifecycle {
            request_id: event_request_id,
            outcome: IntentOutcome::FailedNoOp(reason),
            ..
        } if event_request_id == request_id => Err(RequestOutcomeError::FailedNoOp { reason }),
        CoreEvent::Account(account_event) => match account_event {
            AccountEvent::OidcAuthorizationCreated {
                request_id: event_request_id,
                authorization_url,
                state,
            } if matches!(
                expectation,
                RequestOutcomeExpectation::OidcAuthorization { .. }
            ) && event_request_id == request_id =>
            {
                Ok(Some(EventProgress::Oidc {
                    request_id,
                    authorization_url,
                    state,
                }))
            }
            AccountEvent::AuthDiscoveryChanged {
                request_id: event_request_id,
                homeserver,
            } if matches!(expectation, RequestOutcomeExpectation::AuthDiscovery { .. })
                && event_request_id == request_id =>
            {
                Ok(Some(EventProgress::AuthDiscovery {
                    request_id,
                    homeserver,
                }))
            }
            AccountEvent::SavedSessionsListed {
                request_id: event_request_id,
                sessions,
            } if matches!(expectation, RequestOutcomeExpectation::SavedSessions { .. })
                && event_request_id == request_id =>
            {
                Ok(Some(EventProgress::SavedSessions {
                    request_id,
                    sessions,
                }))
            }
            AccountEvent::LoggedIn {
                request_id: event_request_id,
                account_key,
            }
            | AccountEvent::SessionRestored {
                request_id: event_request_id,
                account_key,
            } if matches!(expectation, RequestOutcomeExpectation::Authenticated { .. })
                && event_request_id == request_id
                && matches!(
                    expectation,
                    RequestOutcomeExpectation::Authenticated {
                        account_key: None,
                        ..
                    } | RequestOutcomeExpectation::Authenticated {
                        account_key: Some(_),
                        ..
                    }
                ) =>
            {
                Ok(Some(EventProgress::Authenticated {
                    request_id,
                    account_key,
                }))
            }
            AccountEvent::LoggedOut {
                request_id: event_request_id,
                account_key,
            } if matches!(expectation, RequestOutcomeExpectation::SignedOut { .. })
                && event_request_id == request_id =>
            {
                Ok(Some(EventProgress::SignedOut {
                    request_id,
                    account_key,
                }))
            }
            _ => Ok(None),
        },
        CoreEvent::Room(room_event) => room_event_progress(room_event, expectation, request_id),
        CoreEvent::Timeline(timeline_event) => {
            timeline_event_progress(timeline_event, expectation, request_id)
        }
        CoreEvent::Search(search_event) => match search_event {
            crate::event::SearchEvent::Results {
                request_id: event_request_id,
                ..
            } if matches!(expectation, RequestOutcomeExpectation::SearchStarted { .. })
                && event_request_id == request_id =>
            {
                Ok(Some(EventProgress::Search { request_id }))
            }
            _ => Ok(None),
        },
        CoreEvent::IntentLifecycle {
            request_id: event_request_id,
            outcome: IntentOutcome::BenignNoOp(reason),
            ..
        } if event_request_id == request_id
            && matches!(
                expectation,
                RequestOutcomeExpectation::MainTimelineAnchor { .. }
            ) =>
        {
            if matches!(reason, IntentNoOpReason::TimelineTargetMissing)
                && matches!(
                    expectation,
                    RequestOutcomeExpectation::MainTimelineAnchor {
                        allow_live_fallback: true,
                        ..
                    }
                )
            {
                Ok(Some(EventProgress::Anchor {
                    request_id,
                    live_fallback: true,
                }))
            } else {
                Err(RequestOutcomeError::FailedNoOp { reason })
            }
        }
        CoreEvent::IntentLifecycle {
            request_id: event_request_id,
            outcome: IntentOutcome::Committed,
            ..
        } if event_request_id == request_id => match expectation {
            RequestOutcomeExpectation::RoomSelected { .. } => Ok(Some(EventProgress::Focused {
                request_id,
                opened: false,
            })),
            RequestOutcomeExpectation::FocusedContextClosed { .. } => {
                Ok(Some(EventProgress::Focused {
                    request_id,
                    opened: false,
                }))
            }
            RequestOutcomeExpectation::FocusedContextOpened { .. } => {
                Ok(Some(EventProgress::Focused {
                    request_id,
                    opened: true,
                }))
            }
            RequestOutcomeExpectation::MainTimelineAnchor { .. } => {
                Ok(Some(EventProgress::Anchor {
                    request_id,
                    live_fallback: false,
                }))
            }
            RequestOutcomeExpectation::SearchStarted { .. } => {
                Ok(Some(EventProgress::Search { request_id }))
            }
            RequestOutcomeExpectation::SearchClosed { .. } => {
                Ok(Some(EventProgress::Search { request_id }))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn room_event_progress(
    event: RoomEvent,
    expectation: &RequestOutcomeExpectation,
    request_id: RequestId,
) -> Result<Option<EventProgress>, RequestOutcomeError> {
    match event {
        RoomEvent::RoomCreated {
            request_id: event_request_id,
            room_id,
        } if matches!(expectation, RequestOutcomeExpectation::RoomCreated { .. })
            && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomCreated {
                request_id,
                room_id,
            }))
        }
        RoomEvent::SpaceCreated {
            request_id: event_request_id,
            space_id,
        } if matches!(expectation, RequestOutcomeExpectation::SpaceCreated { .. })
            && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::SpaceCreated {
                request_id,
                space_id,
            }))
        }
        RoomEvent::DirectMessageStarted {
            request_id: event_request_id,
            room_id,
        } if matches!(
            expectation,
            RequestOutcomeExpectation::DirectMessageStarted { .. }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::DirectMessageStarted {
                request_id,
                room_id,
            }))
        }
        RoomEvent::RoomJoined {
            request_id: event_request_id,
            room_id,
        } if matches!(expectation, RequestOutcomeExpectation::RoomJoined { .. })
            && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomJoined {
                request_id,
                room_id,
            }))
        }
        RoomEvent::InviteBatchCompleted {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::InviteWorkflow { .. }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::InviteWorkflow { request_id }))
        }
        RoomEvent::ComposerSlashCommandRejected {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::ComposerAccepted { .. }
        ) && event_request_id == request_id =>
        {
            Err(RequestOutcomeError::FailedNoOp {
                reason: IntentNoOpReason::SessionNotReady,
            })
        }
        RoomEvent::MarkedAsRead {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::MarkedAsRead,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        RoomEvent::MarkedAsUnread {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::MarkedAsUnread,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        RoomEvent::InviteAccepted {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::InviteAccepted,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        RoomEvent::InviteDeclined {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::InviteDeclined,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        RoomEvent::RoomLeft {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::RoomLeft,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        RoomEvent::RoomForgotten {
            request_id: event_request_id,
            ..
        } if matches!(
            expectation,
            RequestOutcomeExpectation::RoomOperation {
                operation: RoomOperationKind::RoomForgotten,
                ..
            }
        ) && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::RoomOperation { request_id }))
        }
        _ => Ok(None),
    }
}

fn timeline_event_progress(
    event: TimelineEvent,
    expectation: &RequestOutcomeExpectation,
    request_id: RequestId,
) -> Result<Option<EventProgress>, RequestOutcomeError> {
    match event {
        TimelineEvent::SubmissionAccepted {
            request_id: event_request_id,
            submission_id,
            transaction_id,
            ..
        } if matches!(expectation, RequestOutcomeExpectation::Submission { .. })
            && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::SubmissionAccepted {
                request_id,
                submission_id,
                transaction_id,
            }))
        }
        TimelineEvent::SubmissionRejected {
            request_id: event_request_id,
            submission_id,
            kind,
            ..
        } if matches!(expectation, RequestOutcomeExpectation::Submission { .. })
            && event_request_id == request_id =>
        {
            Ok(Some(EventProgress::SubmissionRejected {
                request_id,
                submission_id,
                kind,
            }))
        }
        TimelineEvent::MediaSendQueued {
            request_id: event_request_id,
            transaction_id,
            key,
        } if matches!(
            expectation,
            RequestOutcomeExpectation::PreparedMediaQueued { .. }
        ) && event_request_id == request_id =>
        {
            if let RequestOutcomeExpectation::PreparedMediaQueued {
                key: expected_key,
                transaction_id: expected_transaction_id,
                ..
            } = expectation
            {
                if key == *expected_key && transaction_id == *expected_transaction_id {
                    Ok(Some(EventProgress::PreparedMediaQueued {
                        request_id,
                        transaction_id,
                    }))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn snapshot_outcome(
    expectation: &RequestOutcomeExpectation,
    snapshot: &VersionedAppStateSnapshot,
    baseline_generation: u64,
    allow_initial: bool,
) -> Option<RequestOutcome> {
    if !allow_initial && snapshot.generation <= baseline_generation {
        return None;
    }
    match expectation {
        RequestOutcomeExpectation::RoomSelected {
            room_id,
            account_key,
            ..
        } if snapshot.state.navigation.active_room_id.as_deref() == Some(room_id.as_str())
            && account_matches(&snapshot.state, account_key.as_ref()) =>
        {
            Some(RequestOutcome::RoomSelected {
                snapshot: snapshot.clone(),
            })
        }
        RequestOutcomeExpectation::SearchClosed {
            request_id,
            account_key,
            allow_initial: true,
        } if snapshot.state.search == koushi_state::SearchState::Closed
            && account_matches(&snapshot.state, account_key.as_ref()) =>
        {
            Some(RequestOutcome::Search {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        _ => None,
    }
}

fn snapshot_outcome_for_progress(
    progress: &EventProgress,
    expectation: &RequestOutcomeExpectation,
    snapshot: &VersionedAppStateSnapshot,
    baseline_generation: u64,
) -> Option<RequestOutcome> {
    if snapshot.generation <= baseline_generation {
        return None;
    }
    if progress.request_id() != expectation.request_id() {
        return None;
    }
    match (progress, expectation) {
        (
            EventProgress::Oidc {
                authorization_url,
                state,
                ..
            },
            RequestOutcomeExpectation::OidcAuthorization { request_id },
        ) => Some(RequestOutcome::OidcAuthorization {
            request_id: *request_id,
            authorization_url: authorization_url.clone(),
            state: state.clone(),
            generation: snapshot.generation,
        }),
        (
            EventProgress::AuthDiscovery { homeserver, .. },
            RequestOutcomeExpectation::AuthDiscovery {
                request_id,
                homeserver: expected_homeserver,
            },
        ) if homeserver == expected_homeserver
            && auth_discovery_matches(&snapshot.state, expected_homeserver) =>
        {
            Some(RequestOutcome::AuthDiscovery {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Authenticated { account_key, .. },
            RequestOutcomeExpectation::Authenticated {
                request_id,
                account_key: expected_account_key,
            },
        ) if expected_account_key
            .as_ref()
            .is_none_or(|expected| expected == account_key)
            && account_matches(&snapshot.state, Some(account_key))
            && session_is_login_transport_terminal(&snapshot.state.session) =>
        {
            Some(RequestOutcome::Authenticated {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::SignedOut { account_key, .. },
            RequestOutcomeExpectation::SignedOut {
                request_id,
                account_key: expected_account_key,
            },
        ) if account_key == expected_account_key
            && matches!(snapshot.state.session, SessionState::SignedOut) =>
        {
            Some(RequestOutcome::SignedOut {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::RoomCreated { room_id, .. },
            RequestOutcomeExpectation::RoomCreated {
                request_id,
                account_key,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .rooms
                .iter()
                .any(|room| room.room_id == *room_id) =>
        {
            Some(RequestOutcome::RoomCreated {
                request_id: *request_id,
                room_id: room_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::SpaceCreated { space_id, .. },
            RequestOutcomeExpectation::SpaceCreated {
                request_id,
                account_key,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .spaces
                .iter()
                .any(|space| space.space_id == *space_id) =>
        {
            Some(RequestOutcome::SpaceCreated {
                request_id: *request_id,
                space_id: space_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::DirectMessageStarted { room_id, .. },
            RequestOutcomeExpectation::DirectMessageStarted {
                request_id,
                account_key,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .rooms
                .iter()
                .any(|room| room.room_id == *room_id) =>
        {
            Some(RequestOutcome::DirectMessageStarted {
                request_id: *request_id,
                room_id: room_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::RoomJoined { room_id, .. },
            RequestOutcomeExpectation::RoomJoined {
                request_id,
                account_key,
                room_id: expected_room_id,
            },
        ) if room_id == expected_room_id
            && account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .rooms
                .iter()
                .any(|room| room.room_id == *room_id) =>
        {
            Some(RequestOutcome::RoomJoined {
                request_id: *request_id,
                room_id: room_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Focused { opened: false, .. },
            RequestOutcomeExpectation::FocusedContextClosed {
                request_id,
                account_key,
                room_id,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot.state.focused_context == FocusedContextState::Closed
            && snapshot.state.navigation.main_timeline_anchor.is_none()
            && room_target_matches(&snapshot.state, room_id.as_deref()) =>
        {
            Some(RequestOutcome::FocusedContext {
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Focused { opened: true, .. },
            RequestOutcomeExpectation::FocusedContextOpened {
                request_id,
                account_key,
                room_id,
                event_id,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && focused_context_matches(&snapshot.state, room_id, event_id.as_deref()) =>
        {
            Some(RequestOutcome::FocusedContext {
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Focused { opened: false, .. },
            RequestOutcomeExpectation::RoomSelected {
                room_id,
                account_key,
                ..
            },
        ) if snapshot.state.navigation.active_room_id.as_deref() == Some(room_id.as_str())
            && account_matches(&snapshot.state, account_key.as_ref()) =>
        {
            Some(RequestOutcome::RoomSelected {
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Anchor { live_fallback, .. },
            RequestOutcomeExpectation::MainTimelineAnchor {
                request_id,
                key,
                event_id,
                ..
            },
        ) if account_matches(&snapshot.state, Some(&key.account_key))
            && timeline_key_matches(key, event_id)
            && if *live_fallback {
                snapshot_has_live_main_timeline(&snapshot.state, key.room_id())
            } else {
                snapshot_has_main_timeline_anchor(&snapshot.state, key.room_id(), event_id)
            } =>
        {
            Some(RequestOutcome::MainTimelineAnchor {
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::RoomOperation { .. },
            RequestOutcomeExpectation::RoomOperation {
                request_id,
                account_key,
                room_id,
                ..
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .rooms
                .iter()
                .any(|room| room.room_id == *room_id) =>
        {
            Some(RequestOutcome::RoomOperation {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::InviteWorkflow { .. },
            RequestOutcomeExpectation::InviteWorkflow {
                request_id,
                account_key,
                room_id,
                query,
            },
        ) if account_matches(&snapshot.state, Some(account_key))
            && snapshot.state.invite_workflow.query.room_id.as_deref()
                == Some(room_id.as_str())
            && snapshot.state.invite_workflow.query.query == *query
            && !matches!(
                snapshot.state.invite_workflow.operation,
                InviteOperationState::Idle | InviteOperationState::Pending { .. }
            ) =>
        {
            Some(RequestOutcome::InviteWorkflow {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Search { .. },
            RequestOutcomeExpectation::SearchStarted {
                request_id,
                account_key,
                query,
                scope,
            },
        ) if account_matches(&snapshot.state, account_key.as_ref())
            && search_state_matches(&snapshot.state, request_id, query, scope) =>
        {
            Some(RequestOutcome::Search {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::Search { .. },
            RequestOutcomeExpectation::SearchClosed {
                request_id,
                account_key,
                ..
            },
        ) if account_matches(&snapshot.state, account_key.as_ref())
            && snapshot.state.search == koushi_state::SearchState::Closed =>
        {
            Some(RequestOutcome::Search {
                request_id: *request_id,
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::SubmissionAccepted {
                submission_id,
                transaction_id,
                ..
            },
            RequestOutcomeExpectation::Submission {
                request_id,
                account_key,
                target,
                submission_id: expected_submission_id,
            },
        ) if submission_id == expected_submission_id
            && account_matches(&snapshot.state, Some(account_key))
            && snapshot
                .state
                .timeline
                .submission_registry
                .active_submissions
                .iter()
                .any(|active| {
                    &active.submission_id == submission_id
                        && active.transaction_id == *transaction_id
                        && active.target == *target
                }) =>
        {
            Some(RequestOutcome::SubmissionAccepted {
                request_id: *request_id,
                submission_id: submission_id.clone(),
                transaction_id: transaction_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        (
            EventProgress::SubmissionRejected {
                submission_id,
                kind,
                ..
            },
            RequestOutcomeExpectation::Submission {
                request_id,
                submission_id: expected_submission_id,
                ..
            },
        ) if submission_id == expected_submission_id => Some(RequestOutcome::SubmissionRejected {
            request_id: *request_id,
            submission_id: submission_id.clone(),
            kind: *kind,
            snapshot: snapshot.clone(),
        }),
        (
            EventProgress::PreparedMediaQueued { transaction_id, .. },
            RequestOutcomeExpectation::PreparedMediaQueued {
                request_id,
                key,
                transaction_id: expected_transaction_id,
            },
        ) if transaction_id == expected_transaction_id
            && key.account_key.0 == snapshot_account_key(&snapshot.state).unwrap_or_default() =>
        {
            Some(RequestOutcome::PreparedMediaQueued {
                request_id: *request_id,
                transaction_id: transaction_id.clone(),
                snapshot: snapshot.clone(),
            })
        }
        _ => None,
    }
}

fn final_result(
    expectation: &RequestOutcomeExpectation,
    snapshot: &VersionedAppStateSnapshot,
    baseline_generation: u64,
    error: RequestOutcomeError,
    progress: Option<EventProgress>,
) -> Result<RequestOutcome, RequestOutcomeError> {
    if let Some(outcome) = snapshot_outcome(
        expectation,
        snapshot,
        baseline_generation,
        allows_initial_snapshot(expectation),
    ) {
        return Ok(outcome);
    }
    if let Some(progress) = progress
        .and_then(|progress| progress.snapshot_outcome(expectation, snapshot, baseline_generation))
    {
        return Ok(progress);
    }
    Err(error)
}

fn allows_initial_snapshot(expectation: &RequestOutcomeExpectation) -> bool {
    matches!(
        expectation,
        RequestOutcomeExpectation::RoomSelected {
            allow_initial: true,
            ..
        }
    )
}

fn snapshot_account_key(state: &AppState) -> Option<String> {
    match &state.session {
        SessionState::SwitchingAccount { info }
        | SessionState::Provisional { info, .. }
        | SessionState::AwaitingVerification { info, .. }
        | SessionState::Verifying { info, .. }
        | SessionState::AwaitingBootstrapConfirmation { info, .. }
        | SessionState::Rejecting { info, .. }
        | SessionState::Ready(info)
        | SessionState::Locked(info)
        | SessionState::CapabilityBlocked { info, .. } => Some(info.user_id.clone()),
        SessionState::SignedOut
        | SessionState::Restoring
        | SessionState::Authenticating { .. }
        | SessionState::LoggingOut => None,
    }
}

fn account_matches(state: &AppState, expected: Option<&AccountKey>) -> bool {
    expected
        .is_none_or(|expected| snapshot_account_key(state).as_deref() == Some(expected.0.as_str()))
}

fn session_is_login_transport_terminal(session: &SessionState) -> bool {
    matches!(
        session,
        SessionState::Provisional {
            phase: koushi_state::ProvisionalPhase::RecheckingTrust { failure: Some(_) },
            ..
        } | SessionState::AwaitingVerification { .. }
            | SessionState::Verifying { .. }
            | SessionState::AwaitingBootstrapConfirmation { .. }
            | SessionState::Rejecting { .. }
            | SessionState::Ready(_)
    )
}

fn auth_discovery_matches(state: &AppState, homeserver: &str) -> bool {
    matches!(
        &state.auth,
        koushi_state::AuthDiscoveryState::Ready { homeserver: current, .. }
            | koushi_state::AuthDiscoveryState::Failed { homeserver: current, .. }
            if current == homeserver
    )
}

fn room_target_matches(state: &AppState, expected_room_id: Option<&str>) -> bool {
    state.navigation.active_room_id.as_deref() == expected_room_id
}

fn focused_context_matches(state: &AppState, room_id: &str, event_id: Option<&str>) -> bool {
    match &state.focused_context {
        FocusedContextState::Opening {
            room_id: current_room_id,
            event_id: current_event_id,
        }
        | FocusedContextState::Open {
            room_id: current_room_id,
            event_id: current_event_id,
            ..
        } => {
            current_room_id == room_id
                && event_id.is_none_or(|expected| expected == current_event_id)
        }
        FocusedContextState::Closed => false,
    }
}

fn timeline_key_matches(key: &TimelineKey, event_id: &str) -> bool {
    matches!(
        &key.kind,
        crate::ids::TimelineKind::Focused {
            room_id,
            event_id: key_event_id,
        } if room_id == key.room_id() && key_event_id == event_id
    )
}

fn snapshot_has_main_timeline_anchor(state: &AppState, room_id: &str, event_id: &str) -> bool {
    state.navigation.active_room_id.as_deref() == Some(room_id)
        && state
            .navigation
            .main_timeline_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.event_id == event_id)
}

fn snapshot_has_live_main_timeline(state: &AppState, room_id: &str) -> bool {
    state.navigation.active_room_id.as_deref() == Some(room_id)
        && state.focused_context == FocusedContextState::Closed
        && state.navigation.main_timeline_anchor.is_none()
}

fn search_state_matches(
    state: &AppState,
    request_id: &RequestId,
    query: &str,
    scope: &SearchScope,
) -> bool {
    matches!(
        &state.search,
        koushi_state::SearchState::TooShort {
            request_id: state_request_id,
            query: state_query,
            scope: state_scope,
            ..
        }
        | koushi_state::SearchState::Searching {
            request_id: state_request_id,
            query: state_query,
            scope: state_scope,
        }
        | koushi_state::SearchState::Results {
            request_id: state_request_id,
            query: state_query,
            scope: state_scope,
            ..
        }
        | koushi_state::SearchState::Failed {
            request_id: state_request_id,
            query: state_query,
            scope: state_scope,
            ..
        } if *state_request_id == request_id.sequence
            && state_query == query
            && state_scope == scope
    )
}
