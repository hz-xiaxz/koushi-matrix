//! WeChat through ServerChan Turbo (Server酱·Turbo, #1460).
//!
//! ServerChan relays a message to the WeChat account that owns the SendKey, by default
//! through its 方糖 service account. The request is
//! `POST {endpoint}/{SendKey}.send` with an `application/x-www-form-urlencoded` body of
//! `title` (one line) and `desp` (Markdown). The SendKey is in the URL path, so the URL is
//! built here, sent here, and never logged, returned, or rendered into an error; a
//! transport error is dropped whole because its text can carry the URL.
//!
//! A JSON `code` of 0 means the service accepted the request. That is the only success:
//! nothing ServerChan answers proves the message reached WeChat, and the free plan keeps
//! a handful of messages per day, so a later message can be accepted and still not shown.

use std::fmt;
use std::time::Duration;

use koushi_key::NotificationSinkSecret;
use reqwest::header::{HeaderMap, RETRY_AFTER};

use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
};

/// ServerChan Turbo's API host (<https://sct.ftqq.com>).
pub const SERVERCHAN_ENDPOINT: &str = "https://sctapi.ftqq.com";

/// The prefix every ServerChan Turbo SendKey starts with.
const SENDKEY_PREFIX: &str = "SCT";

/// The longest SendKey this device stores. Real keys are far shorter; the cap only keeps
/// a pasted page from becoming a URL path.
const MAX_SENDKEY_CHARS: usize = 128;

/// How long one request may take, connection to last byte.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The most of an answer this sink reads. ServerChan's JSON is a few hundred bytes; a
/// longer answer is not one, and is a transport failure.
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

/// The `code` ServerChan answers for a SendKey it does not recognize.
const SERVERCHAN_BAD_KEY_CODE: i64 = 40001;

/// Why a value cannot be stored as a ServerChan Turbo SendKey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerChanSendKeyError {
    /// A ServerChan³ key (`sctp…`): a different service with a different API.
    ServerChan3Key,
    /// Not `SCT` followed by letters and digits.
    Malformed,
}

/// Check a SendKey before it is stored. The value is trimmed by the caller; this only
/// decides whether it has the shape of a Turbo key.
pub fn validate_serverchan_sendkey(value: &str) -> Result<(), ServerChanSendKeyError> {
    if value
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sctp"))
    {
        return Err(ServerChanSendKeyError::ServerChan3Key);
    }
    let Some(rest) = value.strip_prefix(SENDKEY_PREFIX) else {
        return Err(ServerChanSendKeyError::Malformed);
    };
    let well_formed = !rest.is_empty()
        && value.len() <= MAX_SENDKEY_CHARS
        && rest
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
    if well_formed {
        Ok(())
    } else {
        Err(ServerChanSendKeyError::Malformed)
    }
}

/// ServerChan Turbo over its HTTPS API.
#[derive(Clone)]
pub struct ServerChanSink {
    endpoint: String,
    client: reqwest::Client,
}

impl ServerChanSink {
    pub fn new() -> Self {
        Self::with_endpoint(SERVERCHAN_ENDPOINT)
    }

    /// Point the sink at another endpoint. The only production caller passes the
    /// default; a test passes a local stub.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            // rustls defaults, as for the Resend sink: this sink invents no TLS, proxy, or
            // CA policy of its own. Redirects are refused because the SendKey is in the
            // path and a redirect would carry it to a host this sink never chose.
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("a default reqwest client always builds"),
        }
    }

    pub async fn send(
        &self,
        secret: &NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> NotificationDeliveryOutcome {
        if validate_serverchan_sendkey(secret.as_str()).is_err() {
            // A stored value that is not a Turbo key is an invalid key; it must never be
            // put into a URL path and never reported as accepted.
            return NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::InvalidSecret);
        }
        if !endpoint_is_permitted(&self.endpoint) {
            // The key would travel in the clear or to an address that is not a plain
            // origin. Nothing is sent.
            return NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::NotPermitted);
        }
        let url = format!("{}/{}.send", self.endpoint, secret.as_str());
        let title = one_line(&request.subject);
        let body = form_body(&[("title", title.as_str()), ("desp", request.body.as_str())]);
        let response = self
            .client
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await;
        let response = match response {
            // The error text can carry the URL, and the URL carries the SendKey.
            Err(_) => {
                return NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport);
            }
            Ok(response) => response,
        };
        let status = response.status();
        let retry_after = retry_after_secs(response.headers());
        let code = bounded_body(response)
            .await
            .and_then(|bytes| response_code(&bytes));
        classify(status, retry_after, code)
    }
}

impl Default for ServerChanSink {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ServerChanSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerChanSink(..)")
    }
}

/// Whether the sink may put a SendKey into a request to this endpoint: an `https` origin,
/// or `http` to a loopback stub. Userinfo, a query, or a fragment is refused, since none
/// belongs in an origin and each could move the key somewhere unexpected.
fn endpoint_is_permitted(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some_and(|host| !host.is_empty()),
        "http" => matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
        _ => false,
    }
}

/// The JSON `code` of a ServerChan answer, or `None` when the body is not one.
fn response_code(bytes: &[u8]) -> Option<i64> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()?
        .get("code")?
        .as_i64()
}

/// The answer's body, read only up to [`MAX_RESPONSE_BYTES`]; `None` when it is longer or
/// the read fails.
async fn bounded_body(mut response: reqwest::Response) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// Turn one answer into a typed outcome.
///
/// Only HTTP success with `code == 0` is accepted. A 401/403 or the bad-key code is an
/// invalid key, a 429 is the service's per-IP rate limit, a 5xx or an answer with no
/// readable code is a transport failure, and any other refusal is a rejection.
pub(crate) fn classify(
    status: reqwest::StatusCode,
    retry_after_secs: Option<u64>,
    code: Option<i64>,
) -> NotificationDeliveryOutcome {
    let failure = match (status.as_u16(), code) {
        (200..=299, Some(0)) => return NotificationDeliveryOutcome::Accepted,
        (401 | 403, _) | (_, Some(SERVERCHAN_BAD_KEY_CODE)) => {
            NotificationDeliveryFailure::InvalidSecret
        }
        (429, _) => NotificationDeliveryFailure::RateLimited { retry_after_secs },
        (500..=599, _) | (200..=299, None) => NotificationDeliveryFailure::Transport,
        _ => NotificationDeliveryFailure::Rejected,
    };
    NotificationDeliveryOutcome::Failed(failure)
}

fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers.get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()
}

/// The title as one line: every control character and line separator becomes a space.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// `application/x-www-form-urlencoded` for a fixed set of fields.
fn form_body(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", form_encode(name), form_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode one form component: unreserved bytes stay, a space is `+`, every other
/// byte is `%XX`.
fn form_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
#[path = "serverchan_tests.rs"]
mod tests;
