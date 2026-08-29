use matrix_sdk::ruma::api::error::{ErrorKind, UnknownTokenErrorData};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{body_json, method, path_regex},
};

use super::{
    DeviceCleanupAuthMode, DeviceCleanupFailureKind, DeviceCleanupRemoteOutcome,
    MatrixClientSession, MatrixDeviceCleanupOutcome, SessionInfo,
    classify_device_cleanup_http_fact, cleanup_current_device, cleanup_oauth_session,
};

async fn session_for(server: &MatrixMockServer) -> MatrixClientSession {
    let client = server.client_builder().build().await;
    MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: server.server().uri(),
            user_id: client
                .user_id()
                .expect("mock client has a user id")
                .to_string(),
            device_id: client
                .device_id()
                .expect("mock client has a device id")
                .to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
    )
}

#[tokio::test]
async fn device_cleanup_auth_mode_is_legacy_without_an_oauth_full_session() {
    let server = MatrixMockServer::new().await;
    let session = session_for(&server).await;

    assert_eq!(
        session.device_cleanup_auth_mode(),
        DeviceCleanupAuthMode::Legacy
    );
}

#[tokio::test]
async fn device_cleanup_auth_mode_is_oauth_for_an_oauth_full_session() {
    let server = MatrixMockServer::new().await;
    let client = matrix_sdk::Client::builder()
        .homeserver_url(server.server().uri())
        .build()
        .await
        .expect("OAuth test client");
    client
        .oauth()
        .restore_session(
            matrix_sdk::test_utils::client::oauth::mock_session(
                matrix_sdk::test_utils::client::mock_session_tokens(),
            ),
            matrix_sdk_base::store::RoomLoadSettings::default(),
        )
        .await
        .expect("synthetic OAuth session");
    let session = MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: client.homeserver().to_string(),
            user_id: client.user_id().expect("OAuth user").to_string(),
            device_id: client.device_id().expect("OAuth device").to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::OAuth,
        },
    );
    assert_eq!(
        session.device_cleanup_auth_mode(),
        DeviceCleanupAuthMode::OAuth
    );
}

#[tokio::test]
async fn oauth_device_cleanup_revokes_tokens_without_matrix_uiaa() {
    let server = MatrixMockServer::new().await;
    let oauth_server = server.oauth();
    oauth_server
        .mock_server_metadata()
        .ok_https()
        .expect(1..)
        .named("server_metadata")
        .mount()
        .await;
    oauth_server
        .mock_revocation()
        .ok()
        .expect(1)
        .named("revocation")
        .mount()
        .await;
    let client = server.client_builder().unlogged().build().await;
    client
        .oauth()
        .restore_session(
            matrix_sdk::test_utils::client::oauth::mock_session(
                matrix_sdk::test_utils::client::mock_session_tokens_with_refresh(),
            ),
            matrix_sdk_base::store::RoomLoadSettings::default(),
        )
        .await
        .expect("synthetic OAuth session");
    let session = MatrixClientSession::from_client_for_testing(
        client.clone(),
        SessionInfo {
            homeserver: client.homeserver().to_string(),
            user_id: client.user_id().expect("OAuth user").to_string(),
            device_id: client.device_id().expect("OAuth device").to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::OAuth,
        },
    );
    client
        .oauth()
        .server_metadata()
        .await
        .expect("OAuth server metadata");
    assert_eq!(
        session.device_cleanup_auth_mode(),
        DeviceCleanupAuthMode::OAuth
    );

    assert_eq!(
        cleanup_oauth_session(client.oauth().insecure_rewrite_https_to_http()).await,
        Ok(MatrixDeviceCleanupOutcome::Settled(
            DeviceCleanupRemoteOutcome::Success
        ))
    );
}

#[tokio::test]
async fn oauth_device_cleanup_maps_an_absent_session_without_uiaa() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().unlogged().build().await;

    assert_eq!(
        cleanup_oauth_session(client.oauth()).await,
        Ok(MatrixDeviceCleanupOutcome::Settled(
            DeviceCleanupRemoteOutcome::AlreadyAbsent
        ))
    );
}

#[tokio::test]
async fn legacy_device_cleanup_deletes_the_authoritative_current_device_and_returns_uiaa() {
    let server = MatrixMockServer::new().await;
    let session = session_for(&server).await;
    let expected_device_id = session.info.device_id.clone();
    Mock::given(method("POST"))
        .and(path_regex(r"^/_matrix/client/(?:v3|r0)/delete_devices$"))
        .and(body_json(json!({ "devices": [expected_device_id] })))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "session": "opaque-uiaa-session",
                "flows": [{ "stages": ["m.login.password"] }],
                "params": {},
                "completed": []
        })))
        .expect(1)
        .mount(server.server())
        .await;

    let outcome = cleanup_current_device(&session, None, None)
        .await
        .expect("UIAA is an expected continuation, not a failure");
    assert_eq!(
        outcome,
        MatrixDeviceCleanupOutcome::UiaaRequired {
            session: Some("opaque-uiaa-session".to_owned()),
        }
    );
}

#[tokio::test]
async fn legacy_device_cleanup_keeps_unknown_token_retryable() {
    let server = MatrixMockServer::new().await;
    let session = session_for(&server).await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/_matrix/client/(?:v3|r0)/delete_devices$"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "errcode": "M_UNKNOWN_TOKEN",
                "error": "expired",
                "soft_logout": false
        })))
        .expect(1)
        .mount(server.server())
        .await;

    assert_eq!(
        cleanup_current_device(&session, None, None).await,
        Err(DeviceCleanupFailureKind::Forbidden)
    );
}

#[test]
fn device_cleanup_uiaa_debug_redacts_the_opaque_session() {
    let outcome = MatrixDeviceCleanupOutcome::UiaaRequired {
        session: Some("opaque-uiaa-session".to_owned()),
    };

    let debug = format!("{outcome:?}");
    assert!(debug.contains("SessionId(..)"));
    assert!(!debug.contains("opaque-uiaa-session"));
}

#[test]
fn device_cleanup_http_classification_requires_authoritative_absence() {
    assert_eq!(
        classify_device_cleanup_http_fact(
            Some(&ErrorKind::UnknownToken(UnknownTokenErrorData::new())),
            false,
        ),
        Err(DeviceCleanupFailureKind::Forbidden)
    );
    assert_eq!(
        classify_device_cleanup_http_fact(Some(&ErrorKind::NotFound), false),
        Err(DeviceCleanupFailureKind::Sdk)
    );
    assert_eq!(
        classify_device_cleanup_http_fact(Some(&ErrorKind::Forbidden), false),
        Err(DeviceCleanupFailureKind::Forbidden)
    );
    assert_eq!(
        classify_device_cleanup_http_fact(None, true),
        Err(DeviceCleanupFailureKind::Network)
    );
    assert_eq!(
        classify_device_cleanup_http_fact(None, false),
        Err(DeviceCleanupFailureKind::Sdk)
    );
}
