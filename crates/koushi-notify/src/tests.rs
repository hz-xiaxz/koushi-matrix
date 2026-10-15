use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::resend::ResendSink;
use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
};

fn request() -> NotificationDeliveryRequest {
    NotificationDeliveryRequest {
        subject: "Koushi".to_owned(),
        body: "2 unread".to_owned(),
        destination: "me@example.com".to_owned(),
        from_address: "onboarding@resend.dev".to_owned(),
        sender_name: "Koushi".to_owned(),
        idempotency_key: "delivery-1".to_owned(),
    }
}

fn secret() -> koushi_key::NotificationSinkSecret {
    koushi_key::NotificationSinkSecret::new("re_test_key_value").expect("secret")
}

async fn sink_against(server: &MockServer) -> ResendSink {
    ResendSink::with_endpoint(format!("{}/emails", server.uri()))
}

#[tokio::test]
async fn resend_sends_the_decided_message_once_with_the_key_in_the_authorization_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .and(header("authorization", "Bearer re_test_key_value"))
        .and(header("idempotency-key", "delivery-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "ok"})))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = sink_against(&server)
        .await
        .send(&secret(), &request())
        .await;
    assert_eq!(outcome, NotificationDeliveryOutcome::Accepted);
    assert!(outcome.is_accepted());
    assert_eq!(outcome.failure(), None);

    let received = server
        .received_requests()
        .await
        .expect("the stub keeps requests");
    let body: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("the request body is JSON");
    assert_eq!(body["to"], serde_json::json!(["me@example.com"]));
    assert_eq!(body["from"], "Koushi <onboarding@resend.dev>");
    assert_eq!(body["subject"], "Koushi");
    // The prefix policy lives in the core: the sink carries the body it was given.
    assert_eq!(body["text"], "2 unread");
}

#[tokio::test]
async fn resend_keeps_the_bare_address_when_no_usable_sender_name_is_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let mut request = request();
    request.sender_name = "Koushi <spoofed>".to_owned();
    let outcome = sink_against(&server).await.send(&secret(), &request).await;
    assert!(outcome.is_accepted());

    let received = server
        .received_requests()
        .await
        .expect("the stub keeps requests");
    let body: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("the request body is JSON");
    assert_eq!(body["from"], "onboarding@resend.dev");
}

#[tokio::test]
async fn resend_maps_each_provider_refusal_to_its_own_typed_failure() {
    let cases = [
        (401, None, NotificationDeliveryFailure::InvalidSecret),
        (403, None, NotificationDeliveryFailure::NotPermitted),
        (422, None, NotificationDeliveryFailure::Rejected),
        (
            429,
            Some(("retry-after", "30")),
            NotificationDeliveryFailure::RateLimited {
                retry_after_secs: Some(30),
            },
        ),
        (
            429,
            None,
            NotificationDeliveryFailure::RateLimited {
                retry_after_secs: None,
            },
        ),
        (500, None, NotificationDeliveryFailure::Transport),
        (503, None, NotificationDeliveryFailure::Transport),
    ];

    for (status, extra_header, expected) in cases {
        let server = MockServer::start().await;
        let mut response = ResponseTemplate::new(status)
            .set_body_string("provider detail that must not surface to the product");
        if let Some((name, value)) = extra_header {
            response = response.insert_header(name, value);
        }
        Mock::given(method("POST"))
            .respond_with(response)
            .mount(&server)
            .await;

        let outcome = sink_against(&server)
            .await
            .send(&secret(), &request())
            .await;
        assert_eq!(
            outcome,
            NotificationDeliveryOutcome::Failed(expected),
            "status {status} should map to {expected:?}"
        );
    }
}

#[tokio::test]
async fn resend_reports_a_transport_failure_without_the_underlying_error() {
    // Bind and drop a listener so the port is closed: connecting must fail, and the
    // outcome must be a plain transport failure rather than a raw error string.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);

    let sink = ResendSink::with_endpoint(format!("http://{address}/emails"));
    let outcome = sink.send(&secret(), &request()).await;

    assert_eq!(
        outcome,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport)
    );
    assert!(!format!("{outcome:?}").contains(&address.to_string()));
}

#[test]
fn an_unusable_key_never_becomes_a_delivery() {
    // The secret type refuses a control character before a sink ever sees it, so this
    // guard is the last line of defence: it must still map to an invalid key rather
    // than to an attempt.
    assert!(koushi_key::NotificationSinkSecret::new("line\nbreak").is_err());
    assert_eq!(
        crate::resend::authorization_value("line\nbreak"),
        Err(NotificationDeliveryFailure::InvalidSecret)
    );
    assert!(crate::resend::authorization_value("re_live_key").is_ok());
}

#[test]
fn a_request_keeps_its_message_text_out_of_a_struct_dump() {
    // The body may carry message text when the user turned the plaintext prefix on, and
    // the addresses are personal identifiers, so the one type that reaches a sink is the
    // one type that must not print them. The key stays, because a retry is correlated by
    // it and it names no person and no message.
    let request = NotificationDeliveryRequest {
        subject: "Subject with words".to_owned(),
        body: "Rina: the roof is leaking".to_owned(),
        destination: "me@example.com".to_owned(),
        from_address: "sender@example.com".to_owned(),
        sender_name: "Sender Name".to_owned(),
        idempotency_key: "delivery-1".to_owned(),
    };

    let printed = format!("{request:?}");

    for private in [
        "Subject with words",
        "the roof is leaking",
        "me@example.com",
        "sender@example.com",
        "Sender Name",
    ] {
        assert!(
            !printed.contains(private),
            "{private} must not be printed: {printed}"
        );
    }
    assert!(printed.contains("delivery-1"), "{printed}");
}
