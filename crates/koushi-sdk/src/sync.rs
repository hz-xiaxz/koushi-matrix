use crate::auth::{
    SYNC_INVITE_PROBE_CONNECTION_ID, SYNC_INVITE_PROBE_LIST_KEY, SYNC_INVITE_PROBE_TIMEOUT,
};
use crate::{
    MatrixClientSession, MatrixSlidingSyncInviteListSupport, ProvisionalEncryptionSyncError,
};
use futures_util::StreamExt;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel};
#[cfg(test)]
use koushi_state::SessionInfo;
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

#[cfg(test)]
mod provisional_encryption_sync_tests {
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use serde_json::json;
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    use super::SessionInfo;

    #[tokio::test]
    async fn uses_simplified_sliding_sync_without_room_lists() {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let session = super::MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: server.uri(),
                user_id: "@provisional:example.invalid".to_owned(),
                device_id: "PROVISIONAL".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        );
        Mock::given(method("POST"))
            .and(path(
                "/_matrix/client/unstable/org.matrix.simplified_msc3575/sync",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "pos": "0" })))
            .expect(1)
            .mount(&server.server())
            .await;

        super::provisional_encryption_sync_loop(
            &session,
            super::new_encryption_sync_permit_owner(),
            || async { super::MatrixSyncLoopControl::Stop },
        )
        .await
        .expect("one provisional encryption response");

        let requests = server.received_requests().await.expect("captured requests");
        let request = requests
            .iter()
            .find(|request| {
                request.url.path() == "/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"
            })
            .expect("simplified sliding sync request");
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON body");
        assert_eq!(body["conn_id"], "encryption");
        assert_eq!(body["extensions"]["e2ee"]["enabled"], true);
        assert_eq!(body["extensions"]["to_device"]["enabled"], true);
        assert!(body.get("lists").is_none());
        assert!(
            requests
                .iter()
                .all(|request| request.url.path() != "/_matrix/client/v3/sync"),
            "provisional encryption must not issue classic /sync"
        );
    }
}

#[derive(Debug, Error)]
pub enum MatrixSyncError {
    #[error("Matrix sync failed")]
    Sdk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixSyncLoopControl {
    Continue,
    Stop,
}

/// Run one bounded, read-only MSC4186 invite-list preflight operation before
/// the authoritative sync owner starts. Any successful response cursor and
/// room payload are intentionally discarded.
pub async fn probe_sliding_sync_invite_list_support(
    session: &MatrixClientSession,
) -> MatrixSlidingSyncInviteListSupport {
    match tokio::time::timeout(SYNC_INVITE_PROBE_TIMEOUT, async {
        let Some(probe) = build_sliding_sync_invite_probe_client(session).await else {
            return MatrixSlidingSyncInviteListSupport::Unknown;
        };
        send_sliding_sync_invite_list_probe(&probe).await
    })
    .await
    {
        Ok(support) => support,
        Err(_) => MatrixSlidingSyncInviteListSupport::Unknown,
    }
}

async fn build_sliding_sync_invite_probe_client(
    session: &MatrixClientSession,
) -> Option<matrix_sdk::Client> {
    use matrix_sdk::authentication::matrix::MatrixSession;
    use matrix_sdk_base::store::RoomLoadSettings;

    let authoritative = session.client();
    let meta = authoritative.session_meta()?.clone();
    let access_token = authoritative.access_token()?;
    let probe = matrix_sdk::Client::builder()
        .homeserver_url(authoritative.homeserver())
        .build()
        .await
        .ok()?;

    probe
        .matrix_auth()
        .restore_session(
            MatrixSession {
                meta,
                tokens: matrix_sdk::SessionTokens {
                    access_token,
                    refresh_token: None,
                },
            },
            RoomLoadSettings::default(),
        )
        .await
        .ok()?;
    Some(probe)
}

async fn send_sliding_sync_invite_list_probe(
    client: &matrix_sdk::Client,
) -> MatrixSlidingSyncInviteListSupport {
    use matrix_sdk::{
        config::RequestConfig,
        ruma::{api::client::sync::sync_events::v5 as http, assign, presence::PresenceState, uint},
    };

    let list = assign!(http::request::List::default(), {
        ranges: vec![(uint!(0), uint!(0))],
        room_details: assign!(http::request::RoomDetails::default(), {
            timeline_limit: uint!(0),
        }),
        filters: Some(assign!(http::request::ListFilters::default(), {
            is_invite: Some(true),
        })),
    });
    let request = assign!(http::Request::new(), {
        conn_id: Some(SYNC_INVITE_PROBE_CONNECTION_ID.to_owned()),
        timeout: Some(Duration::ZERO),
        set_presence: PresenceState::Offline,
        lists: [(SYNC_INVITE_PROBE_LIST_KEY.to_owned(), list)].into_iter().collect(),
    });
    let request_config = RequestConfig::new()
        .timeout(SYNC_INVITE_PROBE_TIMEOUT)
        .disable_retry();

    match tokio::time::timeout(
        SYNC_INVITE_PROBE_TIMEOUT,
        client.send(request).with_request_config(request_config),
    )
    .await
    {
        Ok(Ok(response)) if response.lists.contains_key(SYNC_INVITE_PROBE_LIST_KEY) => {
            MatrixSlidingSyncInviteListSupport::Supported
        }
        Ok(Ok(_)) => MatrixSlidingSyncInviteListSupport::KnownIncomplete,
        Ok(Err(_)) | Err(_) => MatrixSlidingSyncInviteListSupport::Unknown,
    }
}

#[cfg(any(test, feature = "test-hooks", feature = "smoke"))]
pub fn sync_once_blocking(session: &MatrixClientSession) -> Result<(), MatrixSyncError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| MatrixSyncError::Sdk)?;

    runtime.block_on(sync_once(session))
}

#[cfg(any(test, feature = "test-hooks", feature = "smoke"))]
pub async fn sync_once(session: &MatrixClientSession) -> Result<(), MatrixSyncError> {
    session
        .client()
        .sync_once(matrix_sdk::config::SyncSettings::default())
        .await
        .map(|_| ())
        .map_err(|_| MatrixSyncError::Sdk)
}

/// Close every SDK store connection for a session before deleting its keyed
/// on-disk store. Completion is a barrier: all in-flight store operations and
/// SQLite pools have drained when this returns.
pub async fn close_session_stores(session: &MatrixClientSession) -> Result<(), MatrixSyncError> {
    session
        .client()
        .pause()
        .await
        .map_err(|_| MatrixSyncError::Sdk)
}

pub type EncryptionSyncPermitOwner =
    Arc<tokio::sync::Mutex<matrix_sdk_ui::encryption_sync_service::EncryptionSyncPermit>>;

pub fn new_encryption_sync_permit_owner() -> EncryptionSyncPermitOwner {
    Arc::new(tokio::sync::Mutex::new(
        matrix_sdk_ui::encryption_sync_service::EncryptionSyncPermit::new(),
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptionSyncLifecycleOwner {
    Provisional,
    Steady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptionSyncLifecycleStage {
    Created,
    FirstRequest,
    FirstResponse,
    Failed,
    Terminated,
    Handoff,
    Replaced,
}

pub fn record_encryption_sync_lifecycle(
    owner: EncryptionSyncLifecycleOwner,
    snapshot: matrix_sdk::encryption::EncryptionSyncReadinessSnapshot,
    stage: EncryptionSyncLifecycleStage,
    elapsed: Duration,
) {
    let owner = match owner {
        EncryptionSyncLifecycleOwner::Provisional => "provisional",
        EncryptionSyncLifecycleOwner::Steady => "steady",
    };
    let level = match stage {
        EncryptionSyncLifecycleStage::Failed | EncryptionSyncLifecycleStage::Terminated => {
            DiagnosticLevel::Warn
        }
        _ => DiagnosticLevel::Info,
    };
    let stage = match stage {
        EncryptionSyncLifecycleStage::Created => "created",
        EncryptionSyncLifecycleStage::FirstRequest => "first_request",
        EncryptionSyncLifecycleStage::FirstResponse => "first_response",
        EncryptionSyncLifecycleStage::Failed => "failed",
        EncryptionSyncLifecycleStage::Terminated => "terminated",
        EncryptionSyncLifecycleStage::Handoff => "handoff",
        EncryptionSyncLifecycleStage::Replaced => "replaced",
    };
    let readiness = match snapshot.state {
        matrix_sdk::encryption::EncryptionSyncReadinessState::NotStarted => "not_started",
        matrix_sdk::encryption::EncryptionSyncReadinessState::Pending => "pending",
        matrix_sdk::encryption::EncryptionSyncReadinessState::Received => "received",
        matrix_sdk::encryption::EncryptionSyncReadinessState::Failed => "failed",
        matrix_sdk::encryption::EncryptionSyncReadinessState::Cancelled => "cancelled",
    };
    koushi_diagnostics::record(
        DiagnosticEvent::new(level, "core.encryption_sync_lifecycle", stage)
            .field(DiagnosticField::token("owner", owner))
            .field(DiagnosticField::count("generation", snapshot.generation))
            .field(DiagnosticField::token("readiness", readiness))
            .field(DiagnosticField::milliseconds(
                "elapsed_ms",
                elapsed.as_millis(),
            )),
    );
}

/// Runs the encryption-only Simplified Sliding Sync owner used while a newly
/// authenticated session is waiting for trust admission.
///
/// The callback runs after every committed encryption response. Dropping this
/// future drops the SDK stream and its exclusive permit before normal sync is
/// allowed to start.
pub async fn provisional_encryption_sync_loop<F, C>(
    session: &MatrixClientSession,
    permit: EncryptionSyncPermitOwner,
    mut on_successful_sync: F,
) -> Result<(), ProvisionalEncryptionSyncError>
where
    F: FnMut() -> C,
    C: Future<Output = MatrixSyncLoopControl>,
{
    use matrix_sdk_ui::encryption_sync_service::EncryptionSyncService;

    let started = Instant::now();
    let permit = permit.lock_owned().await;
    let service = EncryptionSyncService::new(session.client().clone(), None)
        .await
        .map_err(|_| ProvisionalEncryptionSyncError::Sdk)?;
    let stream = service.sync(permit);
    record_encryption_sync_lifecycle(
        EncryptionSyncLifecycleOwner::Provisional,
        session.client().encryption_sync_readiness_snapshot(),
        EncryptionSyncLifecycleStage::Created,
        started.elapsed(),
    );
    record_encryption_sync_lifecycle(
        EncryptionSyncLifecycleOwner::Provisional,
        session.client().encryption_sync_readiness_snapshot(),
        EncryptionSyncLifecycleStage::FirstRequest,
        started.elapsed(),
    );
    futures_util::pin_mut!(stream);
    let mut first_response = true;

    while let Some(result) = stream.next().await {
        if result.is_err() {
            record_encryption_sync_lifecycle(
                EncryptionSyncLifecycleOwner::Provisional,
                session.client().encryption_sync_readiness_snapshot(),
                EncryptionSyncLifecycleStage::Failed,
                started.elapsed(),
            );
            return Err(ProvisionalEncryptionSyncError::Sdk);
        }
        if first_response {
            first_response = false;
            record_encryption_sync_lifecycle(
                EncryptionSyncLifecycleOwner::Provisional,
                session.client().encryption_sync_readiness_snapshot(),
                EncryptionSyncLifecycleStage::FirstResponse,
                started.elapsed(),
            );
        }
        if on_successful_sync().await == MatrixSyncLoopControl::Stop {
            record_encryption_sync_lifecycle(
                EncryptionSyncLifecycleOwner::Provisional,
                session.client().encryption_sync_readiness_snapshot(),
                EncryptionSyncLifecycleStage::Handoff,
                started.elapsed(),
            );
            return Ok(());
        }
    }

    record_encryption_sync_lifecycle(
        EncryptionSyncLifecycleOwner::Provisional,
        session.client().encryption_sync_readiness_snapshot(),
        EncryptionSyncLifecycleStage::Terminated,
        started.elapsed(),
    );
    Err(ProvisionalEncryptionSyncError::Sdk)
}

pub async fn sync_loop<F, C>(
    session: &MatrixClientSession,
    on_successful_sync: F,
) -> Result<(), MatrixSyncError>
where
    F: Fn() -> C,
    C: Future<Output = MatrixSyncLoopControl>,
{
    session
        .client()
        .sync_with_callback(matrix_sdk::config::SyncSettings::default(), move |_| {
            let callback = on_successful_sync();
            async move {
                match callback.await {
                    MatrixSyncLoopControl::Continue => matrix_sdk::LoopCtrl::Continue,
                    MatrixSyncLoopControl::Stop => matrix_sdk::LoopCtrl::Break,
                }
            }
        })
        .await
        .map_err(|_| MatrixSyncError::Sdk)
}

#[cfg(test)]
mod tests;
