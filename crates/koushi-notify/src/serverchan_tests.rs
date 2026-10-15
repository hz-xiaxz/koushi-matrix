use koushi_key::NotificationSinkSecret;
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    ServerChanSendKeyError, ServerChanSink, classify, endpoint_is_permitted, form_encode,
    validate_serverchan_sendkey,
};
use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
};

const KEY: &str = "SCT123456TestKeyValue";

fn request() -> NotificationDeliveryRequest {
    NotificationDeliveryRequest {
        subject: "Mention in Room\nsecond line".to_owned(),
        body: "> hello & bye".to_owned(),
        destination: String::new(),
        from_address: String::new(),
        sender_name: String::new(),
        idempotency_key: "delivery-1".to_owned(),
    }
}

fn secret(value: &str) -> NotificationSinkSecret {
    NotificationSinkSecret::new(value).expect("secret")
}

#[test]
fn only_turbo_sendkeys_are_accepted() {
    assert_eq!(validate_serverchan_sendkey(KEY), Ok(()));
    assert_eq!(
        validate_serverchan_sendkey("sctp1234tABCDEF"),
        Err(ServerChanSendKeyError::ServerChan3Key)
    );
    assert_eq!(
        validate_serverchan_sendkey("SCTP1234"),
        Err(ServerChanSendKeyError::ServerChan3Key)
    );
    for malformed in [
        "",
        "SCT",
        "sct123",
        "SCT12/34",
        "SCT12 34",
        "re_123",
        "SCT12.send?x",
    ] {
        assert_eq!(
            validate_serverchan_sendkey(malformed),
            Err(ServerChanSendKeyError::Malformed),
            "{malformed:?}"
        );
    }
    let too_long = format!("SCT{}", "a".repeat(200));
    assert_eq!(
        validate_serverchan_sendkey(&too_long),
        Err(ServerChanSendKeyError::Malformed)
    );
}

#[test]
fn only_https_origins_or_loopback_stubs_may_carry_the_key() {
    assert!(endpoint_is_permitted("https://sctapi.ftqq.com"));
    assert!(endpoint_is_permitted("http://127.0.0.1:8080"));
    assert!(endpoint_is_permitted("http://localhost:8080"));
    for refused in [
        "http://sctapi.ftqq.com",
        "https://user:pass@sctapi.ftqq.com",
        "https://sctapi.ftqq.com?x=1",
        "https://sctapi.ftqq.com#x",
        "ftp://sctapi.ftqq.com",
        "not a url",
    ] {
        assert!(!endpoint_is_permitted(refused), "{refused}");
    }
}

#[test]
fn form_encoding_escapes_every_reserved_byte() {
    assert_eq!(form_encode("a b&c=d+e/é"), "a+b%26c%3Dd%2Be%2F%C3%A9");
}

#[test]
fn only_success_with_code_zero_is_accepted() {
    use reqwest::StatusCode;
    let failed = NotificationDeliveryOutcome::Failed;
    assert_eq!(
        classify(StatusCode::OK, None, Some(0)),
        NotificationDeliveryOutcome::Accepted
    );
    assert_eq!(
        classify(StatusCode::OK, None, Some(40001)),
        failed(NotificationDeliveryFailure::InvalidSecret)
    );
    assert_eq!(
        classify(StatusCode::UNAUTHORIZED, None, None),
        failed(NotificationDeliveryFailure::InvalidSecret)
    );
    assert_eq!(
        classify(StatusCode::TOO_MANY_REQUESTS, Some(60), None),
        failed(NotificationDeliveryFailure::RateLimited {
            retry_after_secs: Some(60)
        })
    );
    assert_eq!(
        classify(StatusCode::OK, None, Some(20001)),
        failed(NotificationDeliveryFailure::Rejected)
    );
    assert_eq!(
        classify(StatusCode::BAD_REQUEST, None, None),
        failed(NotificationDeliveryFailure::Rejected)
    );
    assert_eq!(
        classify(StatusCode::OK, None, None),
        failed(NotificationDeliveryFailure::Transport)
    );
    assert_eq!(
        classify(StatusCode::BAD_GATEWAY, None, Some(0)),
        failed(NotificationDeliveryFailure::Transport)
    );
}

#[tokio::test]
async fn serverchan_posts_one_form_with_a_single_line_title() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{KEY}.send")))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string(
            "title=Mention+in+Room+second+line&desp=%3E+hello+%26+bye",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"code": 0, "message": "", "data": {"pushid": "1"}}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let outcome = ServerChanSink::with_endpoint(server.uri())
        .send(&secret(KEY), &request())
        .await;
    assert_eq!(outcome, NotificationDeliveryOutcome::Accepted);
}

#[tokio::test]
async fn serverchan_maps_refusals_and_never_follows_a_redirect() {
    let cases = [
        (
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 40001})),
            NotificationDeliveryFailure::InvalidSecret,
        ),
        (
            ResponseTemplate::new(429).insert_header("retry-after", "120"),
            NotificationDeliveryFailure::RateLimited {
                retry_after_secs: Some(120),
            },
        ),
        (
            ResponseTemplate::new(503),
            NotificationDeliveryFailure::Transport,
        ),
        (
            ResponseTemplate::new(200).set_body_string("not json"),
            NotificationDeliveryFailure::Transport,
        ),
        (
            ResponseTemplate::new(200).set_body_string(format!(
                "{{\"code\":0,\"pad\":\"{}\"}}",
                "x".repeat(super::MAX_RESPONSE_BYTES)
            )),
            NotificationDeliveryFailure::Transport,
        ),
        (
            ResponseTemplate::new(302).insert_header("location", "https://example.invalid/"),
            NotificationDeliveryFailure::Rejected,
        ),
    ];
    for (response, expected) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let outcome = ServerChanSink::with_endpoint(server.uri())
            .send(&secret(KEY), &request())
            .await;
        assert_eq!(outcome, NotificationDeliveryOutcome::Failed(expected));
    }
}

#[tokio::test]
async fn serverchan_sends_nothing_for_a_bad_key_or_a_plain_http_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let sink = ServerChanSink::with_endpoint(server.uri());
    assert_eq!(
        sink.send(&secret("sctp1234tKEY"), &request()).await,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::InvalidSecret)
    );
    let remote = ServerChanSink::with_endpoint("http://sctapi.ftqq.com");
    assert_eq!(
        remote.send(&secret(KEY), &request()).await,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::NotPermitted)
    );
}

#[test]
fn the_sink_never_prints_its_endpoint_or_key() {
    let sink = ServerChanSink::with_endpoint("https://sctapi.ftqq.com");
    assert_eq!(format!("{sink:?}"), "ServerChanSink(..)");
    assert!(!format!("{:?}", secret(KEY)).contains(KEY));
}
