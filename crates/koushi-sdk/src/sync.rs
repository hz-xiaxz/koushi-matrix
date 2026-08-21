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
mod tests {

    use crate::MatrixClientSession;
    use crate::auth::SYNC_INVITE_PROBE_TIMEOUT;

    use koushi_state::SessionInfo;
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };
    #[test]
    fn encryption_sync_lifecycle_diagnostic_is_closed_and_private() {
        let _guard = koushi_diagnostics::test_support::lock();
        let start = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .len();
        super::record_encryption_sync_lifecycle(
            super::EncryptionSyncLifecycleOwner::Steady,
            matrix_sdk::encryption::EncryptionSyncReadinessSnapshot {
                generation: 7,
                state: matrix_sdk::encryption::EncryptionSyncReadinessState::Received,
            },
            super::EncryptionSyncLifecycleStage::FirstResponse,
            Duration::from_millis(123),
        );
        let snapshot = koushi_diagnostics::test_support::detail_snapshot();
        let record = snapshot.records[start..]
            .iter()
            .find(|record| record.event.source == "core.encryption_sync_lifecycle")
            .expect("lifecycle diagnostic");
        let text = format!("{:?}", record.event);
        assert!(text.contains("steady"));
        assert!(text.contains("received"));
        for forbidden in ["@user", "DEVICE", "!room", "https://", "sync-position"] {
            assert!(!text.contains(forbidden), "privacy leak: {text}");
        }
    }

    #[test]
    fn sliding_sync_invite_probe_contract_is_typed_bounded_and_discards_cursor() {
        let source = include_str!("sync.rs");
        let implementation = source
            .split("pub async fn probe_sliding_sync_invite_list_support")
            .nth(1)
            .and_then(|rest| rest.split("pub fn sync_once_blocking").next())
            .expect("typed invite-list support probe should precede sync entry points");
        let body = implementation
            .split("async fn build_sliding_sync_invite_probe_client")
            .next()
            .expect("typed invite-list support probe should precede its helpers");

        let timeout = body
            .find("tokio::time::timeout(SYNC_INVITE_PROBE_TIMEOUT, async {")
            .expect("the public probe must start one outer end-to-end timeout");
        let build = body
            .find("build_sliding_sync_invite_probe_client(session).await")
            .expect("the public probe must build its disposable client");
        let send = body
            .find("send_sliding_sync_invite_list_probe(&probe).await")
            .expect("the public probe must send its disposable request");
        assert!(
            timeout < build && build < send,
            "the one outer timeout must enclose disposable-client setup and its request"
        );
        assert!(implementation.contains(".send(request)"));
        assert!(implementation.contains("with_request_config"));
        assert!(implementation.contains("SYNC_INVITE_PROBE_TIMEOUT"));
        assert!(implementation.contains("disable_retry()"));
        assert!(!implementation.contains(".sliding_sync("));
        assert!(!implementation.contains("RoomListService::"));
    }
    #[derive(Clone, Copy)]
    enum InviteProbeTestResponse {
        Json(&'static [u8]),
        HttpError,
        Stall,
    }
    fn read_test_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("request read timeout");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).expect("read invite probe request");
            assert!(read > 0, "request closed before its headers completed");
            request.extend_from_slice(&chunk[..read]);
            assert!(request.len() <= 16 * 1024, "synthetic request is bounded");
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let head = String::from_utf8_lossy(&request[..header_end]);
        let content_length = head
            .lines()
            .skip(1)
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let read = stream.read(&mut chunk).expect("read invite probe body");
            assert!(read > 0, "request closed before its body completed");
            request.extend_from_slice(&chunk[..read]);
        }
        request
    }
    fn spawn_invite_probe_server(
        response: InviteProbeTestResponse,
    ) -> (String, mpsc::Receiver<Vec<u8>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("synthetic listener");
        let address = listener.local_addr().expect("synthetic listener address");
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().expect("invite probe request");
                let request = read_test_http_request(&mut stream);
                let request_line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .expect("request line")
                    .to_owned();
                if request_line.contains("/_matrix/client/versions") {
                    let body = br#"{"versions":["v1.12"]}"#;
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .expect("write versions response head");
                    stream
                        .write_all(body)
                        .expect("write versions response body");
                    continue;
                }
                if !request_line
                    .contains("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync")
                {
                    stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                        .expect("write background response");
                    continue;
                }
                request_tx
                    .send(request)
                    .expect("capture invite probe request");
                match response {
                    InviteProbeTestResponse::Json(body) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        stream.write_all(head.as_bytes()).expect("write response head");
                        stream.write_all(body).expect("write response body");
                    }
                    InviteProbeTestResponse::HttpError => stream
                        .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                        .expect("write error response"),
                    InviteProbeTestResponse::Stall => {
                        thread::sleep(Duration::from_millis(2_250));
                    }
                }
                break;
            }
        });
        (format!("http://{address}"), request_rx, server)
    }
    async fn authenticated_probe_client(homeserver: String) -> matrix_sdk::Client {
        let client = matrix_sdk::Client::builder()
            .homeserver_url(homeserver)
            .server_versions([matrix_sdk::ruma::api::MatrixVersion::V1_12])
            .build()
            .await
            .expect("synthetic client");
        client
            .matrix_auth()
            .restore_session(
                matrix_sdk::authentication::matrix::MatrixSession {
                    meta: matrix_sdk_base::SessionMeta {
                        user_id: matrix_sdk::ruma::owned_user_id!("@probe:example.invalid"),
                        device_id: matrix_sdk::ruma::owned_device_id!("PROBEDEVICE"),
                    },
                    tokens: matrix_sdk::SessionTokens {
                        access_token: "synthetic-probe-token".to_owned(), // secret-scan: allow
                        refresh_token: None,
                    },
                },
                matrix_sdk_base::store::RoomLoadSettings::default(),
            )
            .await
            .expect("synthetic session restore");
        client
    }
    fn probe_session(client: matrix_sdk::Client) -> MatrixClientSession {
        MatrixClientSession::from_client_for_testing(
            client.clone(),
            SessionInfo {
                homeserver: client.homeserver().to_string(),
                user_id: "@probe:example.invalid".to_owned(),
                device_id: "PROBEDEVICE".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        )
    }
    fn spawn_unknown_token_invite_probe_server()
    -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("synthetic unknown-token listener");
        let address = listener
            .local_addr()
            .expect("synthetic unknown-token listener address");
        let (path_tx, path_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("bounded synthetic listener");
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("blocking unknown-token stream");
                        let request = read_test_http_request(&mut stream);
                        let path = String::from_utf8_lossy(&request)
                            .lines()
                            .next()
                            .and_then(|line| line.split_ascii_whitespace().nth(1))
                            .expect("unknown-token request path")
                            .to_owned();
                        path_tx.send(path.clone()).expect("capture request path");
                        if path == "/_matrix/client/versions" {
                            let body = br#"{"versions":["v1.12"]}"#;
                            stream
                                .write_all(
                                    format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        body.len()
                                    )
                                    .as_bytes(),
                                )
                                .expect("write versions response head");
                            stream
                                .write_all(body)
                                .expect("write versions response body");
                        } else if path.starts_with(
                            "/_matrix/client/unstable/org.matrix.simplified_msc3575/sync?",
                        ) {
                            let body = br#"{"errcode":"M_UNKNOWN_TOKEN","error":"expired","soft_logout":false}"#;
                            let head = format!(
                                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            stream
                                .write_all(head.as_bytes())
                                .expect("write unknown-token response head");
                            stream
                                .write_all(body)
                                .expect("write unknown-token response body");
                        } else {
                            stream
                                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                                .expect("write unexpected-path response");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept unknown-token request: {error}"),
                }
            }
        });
        (format!("http://{address}"), path_rx, server)
    }
    async fn refresh_capable_probe_client(homeserver: String) -> matrix_sdk::Client {
        let client = matrix_sdk::Client::builder()
            .homeserver_url(homeserver)
            .server_versions([matrix_sdk::ruma::api::MatrixVersion::V1_12])
            .handle_refresh_tokens()
            .build()
            .await
            .expect("synthetic refresh-capable client");
        client
            .matrix_auth()
            .restore_session(
                matrix_sdk::authentication::matrix::MatrixSession {
                    meta: matrix_sdk_base::SessionMeta {
                        user_id: matrix_sdk::ruma::owned_user_id!("@probe:example.invalid"),
                        device_id: matrix_sdk::ruma::owned_device_id!("PROBEDEVICE"),
                    },
                    tokens: matrix_sdk::SessionTokens {
                        access_token: "expired-probe-token".to_owned(), // secret-scan: allow
                        refresh_token: Some("synthetic-refresh-token".to_owned()),
                    },
                },
                matrix_sdk_base::store::RoomLoadSettings::default(),
            )
            .await
            .expect("synthetic refresh session restore");
        client
    }
    #[tokio::test]
    async fn sliding_sync_invite_probe_sends_exact_authenticated_request() {
        let (homeserver, requests, server) =
            spawn_invite_probe_server(InviteProbeTestResponse::Json(
                br#"{"pos":"discarded","lists":{"koushi_invites":{"count":0}}}"#,
            ));
        let client = authenticated_probe_client(homeserver).await;
        let session = probe_session(client);

        assert_eq!(
            super::probe_sliding_sync_invite_list_support(&session).await,
            super::MatrixSlidingSyncInviteListSupport::Supported
        );
        let request = requests
            .recv_timeout(Duration::from_secs(2))
            .expect("captured invite probe request");
        let request = String::from_utf8(request).expect("ASCII HTTP request");
        let (head, body) = request.split_once("\r\n\r\n").expect("HTTP request split");
        let request_line = head.lines().next().expect("request line");
        let target = request_line
            .split_ascii_whitespace()
            .nth(1)
            .expect("request target");
        let target = url::Url::parse(&format!("http://example.invalid{target}"))
            .expect("request target URL");
        let query = target.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            target.path(),
            "/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"
        );
        assert_eq!(query.get("timeout").map(|value| value.as_ref()), Some("0"));
        assert_eq!(
            query.get("set_presence").map(|value| value.as_ref()),
            Some("offline")
        );
        assert_eq!(
            query.len(),
            2,
            "probe query must contain no cursor or txn id"
        );
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer synthetic-probe-token") // secret-scan: allow
        );

        let body: serde_json::Value = serde_json::from_str(body).expect("typed request JSON");
        assert_eq!(
            body,
            serde_json::json!({
                "conn_id": "koushi-invite",
                "lists": {
                    "koushi_invites": {
                        "ranges": [[0, 0]],
                        "timeline_limit": 0,
                        "filters": {"is_invite": true}
                    }
                }
            })
        );
        assert!(
            super::SYNC_INVITE_PROBE_CONNECTION_ID.len() <= 16,
            "probe connection id must satisfy MSC4186's length bound"
        );
        server.join().expect("synthetic invite probe server");
    }
    #[tokio::test]
    async fn sliding_sync_invite_probe_distinguishes_missing_list_from_supported_empty_list() {
        for (body, expected) in [
            (
                br#"{"pos":"discarded","lists":{"koushi_invites":{"count":0}}}"#.as_slice(),
                super::MatrixSlidingSyncInviteListSupport::Supported,
            ),
            (
                br#"{"pos":"discarded","lists":{}}"#.as_slice(),
                super::MatrixSlidingSyncInviteListSupport::KnownIncomplete,
            ),
        ] {
            let (homeserver, _requests, server) =
                spawn_invite_probe_server(InviteProbeTestResponse::Json(body));
            let client = authenticated_probe_client(homeserver).await;
            let session = probe_session(client);
            assert_eq!(
                super::probe_sliding_sync_invite_list_support(&session).await,
                expected
            );
            server.join().expect("synthetic invite probe server");
        }
    }
    #[tokio::test]
    async fn sliding_sync_invite_probe_maps_http_malformed_and_timeout_to_unknown() {
        for response in [
            InviteProbeTestResponse::HttpError,
            InviteProbeTestResponse::Json(br#"malformed"#),
            InviteProbeTestResponse::Stall,
        ] {
            let (homeserver, _requests, server) = spawn_invite_probe_server(response);
            let client = authenticated_probe_client(homeserver).await;
            let session = probe_session(client);
            assert_eq!(
                super::probe_sliding_sync_invite_list_support(&session).await,
                super::MatrixSlidingSyncInviteListSupport::Unknown
            );
            server.join().expect("synthetic invite probe server");
        }
    }
    #[tokio::test]
    async fn sliding_sync_invite_probe_unknown_token_never_refreshes_disposable_client() {
        let (homeserver, paths, server) = spawn_unknown_token_invite_probe_server();
        let authoritative = refresh_capable_probe_client(homeserver).await;
        let before = authoritative
            .session_tokens()
            .expect("authoritative tokens");
        let mut changes = authoritative.subscribe_to_session_changes();
        let session = probe_session(authoritative.clone());
        let started_at = tokio::time::Instant::now();

        assert_eq!(
            super::probe_sliding_sync_invite_list_support(&session).await,
            super::MatrixSlidingSyncInviteListSupport::Unknown
        );
        let elapsed = started_at.elapsed();
        assert!(
            elapsed < SYNC_INVITE_PROBE_TIMEOUT + Duration::from_millis(500),
            "the disposable probe exceeded its bounded deadline: {elapsed:?}"
        );
        assert!(matches!(
            changes.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(authoritative.session_tokens().as_ref(), Some(&before));
        server.join().expect("synthetic unknown-token server");
        let observed_paths = paths.try_iter().collect::<Vec<_>>();
        assert!(observed_paths.iter().any(|path| {
            path.starts_with("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync?")
        }));
        assert!(
            !observed_paths
                .iter()
                .any(|path| path == "/_matrix/client/v3/refresh"),
            "the disposable probe must not refresh tokens: {observed_paths:?}"
        );
    }
    #[tokio::test]
    async fn sliding_sync_invite_probe_unknown_token_isolated_from_authoritative_session() {
        let (homeserver, paths, server) = spawn_unknown_token_invite_probe_server();
        let authoritative = authenticated_probe_client(homeserver).await;
        let before = authoritative
            .session_tokens()
            .expect("authoritative tokens");
        let mut changes = authoritative.subscribe_to_session_changes();
        let session = probe_session(authoritative.clone());

        assert_eq!(
            super::probe_sliding_sync_invite_list_support(&session).await,
            super::MatrixSlidingSyncInviteListSupport::Unknown,
        );
        assert!(matches!(
            changes.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(authoritative.session_tokens().as_ref(), Some(&before));

        server.join().expect("synthetic unknown-token server");
        let observed_paths = paths.try_iter().collect::<Vec<_>>();
        assert!(observed_paths.iter().any(|path| {
            path.starts_with("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync?")
        }));
        assert!(
            !observed_paths
                .iter()
                .any(|path| path == "/_matrix/client/v3/refresh"),
            "the isolated probe must not refresh tokens: {observed_paths:?}"
        );
    }
}
