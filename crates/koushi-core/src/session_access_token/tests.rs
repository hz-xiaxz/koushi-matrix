use super::{SessionAccessTokenError, read_current_session_access_token};
use koushi_sdk::MatrixClientSession;
use koushi_state::{SessionAuthenticationMethod, SessionInfo};
use matrix_sdk::{
    SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    ruma::{owned_device_id, owned_user_id},
    store::RoomLoadSettings,
    test_utils::mocks::MatrixMockServer,
};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

async fn session(server: &MatrixMockServer, with_token: bool) -> MatrixClientSession {
    let client = server.client_builder().unlogged().build().await;
    let info = SessionInfo {
        homeserver: server.server().uri(),
        user_id: "@token-test:example.invalid".to_owned(),
        device_id: "TOKENDEVICE".to_owned(),
        authentication_method: SessionAuthenticationMethod::Unknown,
    };
    if with_token {
        client
            .matrix_auth()
            .restore_session(
                MatrixSession {
                    meta: SessionMeta {
                        user_id: owned_user_id!("@token-test:example.invalid"),
                        device_id: owned_device_id!("TOKENDEVICE"),
                    },
                    tokens: SessionTokens {
                        access_token: "synthetic-access-before-rotation".to_owned(), // secret-scan: allow (synthetic test credential)
                        refresh_token: Some("synthetic-refresh".to_owned()),
                    },
                },
                RoomLoadSettings::default(),
            )
            .await
            .expect("restore synthetic SDK session");
    }
    MatrixClientSession::from_client_for_testing(client, info)
}

#[tokio::test]
async fn current_sdk_token_is_read_again_after_sdk_rotation() {
    let server = MatrixMockServer::new().await;
    let session = session(&server, true).await;
    let first = read_current_session_access_token(Some(&session), &session.info, 7)
        .expect("current session token read succeeds");
    assert!(
        first.token() == "synthetic-access-before-rotation",
        "first SDK token differs"
    );
    assert!(
        first.session() == &session.info,
        "disclosure session differs"
    );
    assert_eq!(first.session_revision(), 7);

    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "synthetic-access-after-rotation",
            "refresh_token": "synthetic-refresh-next"
        })))
        .expect(1)
        .mount(server.server())
        .await;
    // Only this test stimulates SDK rotation. The disclosure helper does no
    // refresh, login, persistence or network operation.
    session
        .client()
        .matrix_auth()
        .refresh_access_token()
        .await
        .expect("synthetic SDK token rotation succeeds");
    let second = read_current_session_access_token(Some(&session), &session.info, 7)
        .expect("rotated current session token read succeeds");
    assert!(
        second.token() == "synthetic-access-after-rotation",
        "rotated SDK token differs"
    );
    assert!(
        first.token() != second.token(),
        "disclosure cached the previous SDK token"
    );
    let debug = format!("{second:?}");
    for private in [
        second.token(),
        &session.info.user_id,
        &session.info.device_id,
        &session.info.homeserver,
    ] {
        assert!(
            !debug.contains(private),
            "Debug exposed private session data"
        );
    }
}

#[tokio::test]
async fn inactive_and_missing_sdk_authentication_do_not_disclose() {
    let server = MatrixMockServer::new().await;
    let session = session(&server, false).await;
    assert!(matches!(
        read_current_session_access_token(None, &session.info, 1),
        Err(SessionAccessTokenError::AccountInactive)
    ));
    assert!(matches!(
        read_current_session_access_token(Some(&session), &session.info, 1),
        Err(SessionAccessTokenError::Unavailable)
    ));
}

#[tokio::test]
async fn mismatched_expected_session_never_discloses_the_active_token() {
    let server = MatrixMockServer::new().await;
    let session = session(&server, true).await;
    for field in ["user", "device", "homeserver"] {
        let mut expected = session.info.clone();
        match field {
            "user" => expected.user_id = "@other:example.invalid".to_owned(),
            "device" => expected.device_id = "OTHERDEVICE".to_owned(),
            _ => expected.homeserver = "https://other.example.invalid".to_owned(),
        }
        assert!(
            matches!(
                read_current_session_access_token(Some(&session), &expected, 1),
                Err(SessionAccessTokenError::StaleSession)
            ),
            "mismatched session was disclosed"
        );
        // Matching caller/wrapper metadata must still agree with the actual
        // authenticated SDK user/device/homeserver.
        let mut misleading = session.clone();
        misleading.info = expected;
        assert!(matches!(
            read_current_session_access_token(Some(&misleading), &misleading.info, 1),
            Err(SessionAccessTokenError::StaleSession)
        ));
    }
}

#[test]
fn failures_have_only_closed_static_messages() {
    for error in [
        SessionAccessTokenError::AccountInactive,
        SessionAccessTokenError::Unavailable,
        SessionAccessTokenError::StaleSession,
    ] {
        assert!(!error.to_string().contains('@'));
        assert!(!format!("{error:?}").contains("synthetic"));
    }
}
