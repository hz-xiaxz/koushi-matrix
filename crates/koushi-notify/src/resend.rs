use std::fmt;

use koushi_key::NotificationSinkSecret;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER};

use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
};

/// Resend's transactional email endpoint
/// (<https://resend.com/docs/api-reference/emails/send-email>).
pub const RESEND_ENDPOINT: &str = "https://api.resend.com/emails";

/// Longest idempotency key Resend accepts.
const IDEMPOTENCY_KEY_MAX_BYTES: usize = 256;

/// Resend over its HTTPS API: the first mail sink, because it needs neither a domain
/// nor a card for a self-addressed notice on its free plan.
#[derive(Clone)]
pub struct ResendSink {
    endpoint: String,
    client: reqwest::Client,
}

impl ResendSink {
    pub fn new() -> Self {
        Self::with_endpoint(RESEND_ENDPOINT)
    }

    /// Point the sink at another endpoint. The only production caller passes the
    /// default; a test passes a local stub.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            // The tree has no shared outbound client, and this sink must not become the
            // first place that invents TLS, proxy, or CA policy: rustls defaults only.
            client: reqwest::Client::builder()
                .build()
                .expect("a default reqwest client always builds"),
        }
    }

    pub async fn send(
        &self,
        secret: &NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> NotificationDeliveryOutcome {
        let mut headers = HeaderMap::new();
        // `Bearer <key>`: the value is built here, sent here, and never printed,
        // cached, or returned.
        match authorization_value(secret.as_str()) {
            Ok(value) => {
                headers.insert(AUTHORIZATION, value);
            }
            Err(failure) => {
                // A key that cannot even form a header is an invalid key, not a
                // transport problem, and it must never be reported as delivered.
                return NotificationDeliveryOutcome::Failed(failure);
            }
        }
        if request.idempotency_key.len() <= IDEMPOTENCY_KEY_MAX_BYTES
            && let Ok(value) = HeaderValue::from_str(&request.idempotency_key)
        {
            headers.insert("Idempotency-Key", value);
        }

        let payload = serde_json::json!({
            "from": sender_field(&request.sender_name, &request.from_address),
            "to": [request.destination.as_str()],
            "subject": request.subject.as_str(),
            "text": request.body.as_str(),
        });

        match self
            .client
            .post(&self.endpoint)
            .headers(headers)
            .json(&payload)
            .send()
            .await
        {
            // The transport error text is deliberately dropped: it can carry the
            // endpoint and request detail, and nothing here is allowed to surface it.
            Err(_) => NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
            Ok(response) => classify(response.status(), response.headers()),
        }
    }
}

impl Default for ResendSink {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ResendSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResendSink(..)")
    }
}

/// `Bearer <key>`, or the invalid-key failure when the key cannot be a header value.
///
/// The secret type already refuses a control character, so this is the last line of
/// defence rather than the first; it exists so that no path can turn an unusable key
/// into a claimed delivery.
pub(crate) fn authorization_value(key: &str) -> Result<HeaderValue, NotificationDeliveryFailure> {
    HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| NotificationDeliveryFailure::InvalidSecret)
}

/// A display name that can appear in a sender field, or `None` when it cannot.
///
/// A name carrying `<`, `>`, `"`, or a line ending could restructure the field it is
/// placed in, so it is refused rather than escaped: the bare address is always a valid
/// sender, and dropping a name is the smaller surprise.
pub(crate) fn usable_sender_name(sender_name: &str) -> Option<&str> {
    let usable = !sender_name.is_empty()
        && !sender_name
            .chars()
            .any(|character| matches!(character, '<' | '>' | '"' | '\r' | '\n'));
    usable.then_some(sender_name)
}

/// `Name <address>` when a usable display name is given, the bare address otherwise.
///
/// Both mail sinks build the same sender field, so it lives here rather than twice; the
/// SMTP sink passes an already-reduced bare address.
pub(crate) fn sender_field(sender_name: &str, from_address: &str) -> String {
    match usable_sender_name(sender_name) {
        Some(sender_name) => format!("{sender_name} <{from_address}>"),
        None => from_address.to_owned(),
    }
}

fn classify(status: reqwest::StatusCode, headers: &HeaderMap) -> NotificationDeliveryOutcome {
    if status.is_success() {
        return NotificationDeliveryOutcome::Delivered;
    }
    let failure = match status.as_u16() {
        401 => NotificationDeliveryFailure::InvalidSecret,
        403 => NotificationDeliveryFailure::NotPermitted,
        429 => NotificationDeliveryFailure::RateLimited {
            retry_after_secs: retry_after_secs(headers),
        },
        500..=599 => NotificationDeliveryFailure::Transport,
        _ => NotificationDeliveryFailure::Rejected,
    };
    NotificationDeliveryOutcome::Failed(failure)
}

/// The provider's own `Retry-After` in seconds, when it sent a plain integer. An HTTP
/// date form is ignored rather than guessed at: the caller falls back to its own
/// backoff.
fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers.get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()
}
