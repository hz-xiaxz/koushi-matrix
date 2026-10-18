use matrix_sdk::test_utils::{client::oauth, mocks::MatrixMockServer};
use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

use super::{MatrixClientSession, resolve_active_session_account_management_url};
use koushi_state::{SessionAuthenticationMethod, SessionInfo};

async fn password_session(server: &MatrixMockServer) -> MatrixClientSession {
    let client = server.client_builder().unlogged().build().await;
    MatrixClientSession::from_client_for_testing(
        client,
        SessionInfo {
            homeserver: server.server().uri(),
            user_id: "@alice:example.test".to_owned(),
            device_id: "PASSWORDDEVICE".to_owned(),
            authentication_method: SessionAuthenticationMethod::Password,
        },
    )
}

#[tokio::test]
async fn password_and_sso_sessions_prefer_modern_account_metadata() {
    for authentication_method in [
        SessionAuthenticationMethod::Password,
        SessionAuthenticationMethod::Sso,
    ] {
        let server = MatrixMockServer::new().await;
        server
            .oauth()
            .mock_server_metadata()
            .ok_https()
            .expect(1)
            .mount()
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/matrix/client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "m.authentication": { "account": "https://legacy.example.test/account" }
            })))
            .expect(0)
            .mount(server.server())
            .await;
        let mut session = password_session(&server).await;
        session.info.authentication_method = authentication_method;
        let destination = resolve_active_session_account_management_url(&session)
            .await
            .expect("modern metadata must work without an OAuth session");
        assert!(destination.starts_with("https://"));
        assert!(destination.contains("org.matrix.sessions_list"));
        assert!(!destination.contains("legacy.example.test"));
    }
}

#[tokio::test]
async fn unusable_modern_destination_preserves_non_oauth_well_known_fallback() {
    for destination in [
        None,
        Some("javascript:alert(1)"),
        Some("https://user:secret@example.test/"),
    ] {
        let server = MatrixMockServer::new().await;
        let mut metadata: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&server.oauth().server_metadata())
                .unwrap()
                .replace("http://", "https://"),
        )
        .unwrap();
        if let Some(destination) = destination {
            metadata["account_management_uri"] = json!(destination);
        } else {
            metadata
                .as_object_mut()
                .unwrap()
                .remove("account_management_uri");
        }
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/unstable/org.matrix.msc2965/auth_metadata",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
            .expect(1)
            .mount(server.server())
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/matrix/client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "m.authentication": { "account": "https://legacy.example.test/account" }
            })))
            .expect(1)
            .mount(server.server())
            .await;
        assert_eq!(
            resolve_active_session_account_management_url(&password_session(&server).await)
                .await
                .as_deref(),
            Some("https://legacy.example.test/account")
        );
    }
}

#[tokio::test]
async fn slow_modern_metadata_is_bounded_and_uses_legacy_fallback() {
    let server = MatrixMockServer::new().await;
    server
        .oauth()
        .mock_server_metadata()
        .with_delay(std::time::Duration::from_secs(10))
        .ok_https()
        .expect(1)
        .mount()
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/matrix/client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "m.authentication": { "account": "https://legacy.example.test/account" }
        })))
        .expect(1)
        .mount(server.server())
        .await;
    let session = password_session(&server).await;
    let destination = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        resolve_active_session_account_management_url(&session),
    )
    .await
    .expect("metadata timeout must leave time for the legacy fallback");
    assert_eq!(
        destination.as_deref(),
        Some("https://legacy.example.test/account")
    );
}

#[tokio::test]
async fn password_session_resolves_finalized_and_matrix_org_legacy_well_known() {
    for key in ["m.authentication", "org.matrix.msc2965.authentication"] {
        let server = MatrixMockServer::new().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/matrix/client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                key: { "account": "https://account.example.test/devices" }
            })))
            .expect(1)
            .mount(server.server())
            .await;

        assert_eq!(
            resolve_active_session_account_management_url(&password_session(&server).await)
                .await
                .as_deref(),
            Some("https://account.example.test/devices")
        );
    }
}

#[tokio::test]
async fn password_session_treats_missing_malformed_and_unsafe_metadata_as_unavailable() {
    for body in [
        json!({}),
        json!({ "m.authentication": { "account": 7 } }),
        json!({ "m.authentication": { "account": "javascript:alert(1)" } }),
        json!({ "m.authentication": { "account": "https://user:secret@example.test" } }),
    ] {
        let server = MatrixMockServer::new().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/matrix/client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(server.server())
            .await;

        assert_eq!(
            resolve_active_session_account_management_url(&password_session(&server).await).await,
            None
        );
    }
}

#[tokio::test]
async fn password_session_treats_http_and_json_failures_as_unavailable() {
    for response in [
        ResponseTemplate::new(500),
        ResponseTemplate::new(200).set_body_string("not-json"),
    ] {
        let server = MatrixMockServer::new().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/matrix/client"))
            .respond_with(response)
            .expect(1)
            .mount(server.server())
            .await;

        assert_eq!(
            resolve_active_session_account_management_url(&password_session(&server).await).await,
            None
        );
    }
}

#[tokio::test]
async fn password_session_treats_network_failure_as_unavailable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let address = listener.local_addr().expect("reserved address");
    drop(listener);
    let server = MatrixMockServer::new().await;
    let mut session = password_session(&server).await;
    session.info.homeserver = format!("http://{address}");

    assert_eq!(
        resolve_active_session_account_management_url(&session).await,
        None
    );
}

#[tokio::test]
async fn oauth_session_treats_metadata_failure_as_unavailable() {
    let server = MatrixMockServer::new().await;
    server
        .oauth()
        .mock_server_metadata()
        .error500()
        .expect(1)
        .mount()
        .await;
    let client = server.client_builder().unlogged().build().await;
    client
        .oauth()
        .restore_session(
            oauth::mock_session(matrix_sdk::test_utils::client::mock_session_tokens()),
            matrix_sdk_base::store::RoomLoadSettings::default(),
        )
        .await
        .expect("restore synthetic OAuth session");
    let session = MatrixClientSession::from_client_for_testing(
        client,
        SessionInfo {
            homeserver: server.server().uri(),
            user_id: "@alice:example.test".to_owned(),
            device_id: "OAUTHDEVICE".to_owned(),
            authentication_method: SessionAuthenticationMethod::OAuth,
        },
    );

    assert_eq!(
        resolve_active_session_account_management_url(&session).await,
        None
    );
}

#[tokio::test]
async fn oauth_session_uses_public_server_metadata_devices_list_action() {
    let server = MatrixMockServer::new().await;
    server
        .oauth()
        .mock_server_metadata()
        .ok_https()
        .expect(1)
        .mount()
        .await;
    let client = server.client_builder().unlogged().build().await;
    client
        .oauth()
        .restore_session(
            oauth::mock_session(matrix_sdk::test_utils::client::mock_session_tokens()),
            matrix_sdk_base::store::RoomLoadSettings::default(),
        )
        .await
        .expect("restore synthetic OAuth session");
    let session = MatrixClientSession::from_client_for_testing(
        client,
        SessionInfo {
            homeserver: server.server().uri(),
            user_id: "@alice:example.test".to_owned(),
            device_id: "OAUTHDEVICE".to_owned(),
            authentication_method: SessionAuthenticationMethod::OAuth,
        },
    );

    let destination = resolve_active_session_account_management_url(&session)
        .await
        .expect("mock metadata advertises account management");
    assert!(destination.starts_with("https://"));
    assert!(
        destination.contains("org.matrix.sessions_list"),
        "{destination}"
    );
}
