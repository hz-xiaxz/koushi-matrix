use matrix_sdk::{
    HttpError,
    config::RequestConfig,
    ruma::{api::client::uiaa::UiaaResponse, api::error::FromHttpResponseError},
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, time::Duration};
use url::Url;

const SIMPLIFIED_SLIDING_SYNC_FEATURE: &str = "org.matrix.simplified_msc3575";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    Versions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpStatusClass {
    Success,
    ClientError,
    ServerError,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryTransportFailureKind {
    Timeout,
    Connection,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryResponseFailureKind {
    InvalidHomeserver,
    HttpStatus,
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlidingSyncDiscoveryResult {
    Supported {
        source: DiscoverySource,
        advertised: bool,
        http_status_class: Option<HttpStatusClass>,
    },
    Unsupported {
        advertised: bool,
        http_status_class: Option<HttpStatusClass>,
    },
    Unreachable {
        failure: DiscoveryTransportFailureKind,
    },
    InvalidResponse {
        failure: DiscoveryResponseFailureKind,
        http_status_class: Option<HttpStatusClass>,
    },
}

pub async fn discover_sliding_sync_support(homeserver: &str) -> SlidingSyncDiscoveryResult {
    let Some(homeserver) = normalized_homeserver_url(homeserver) else {
        return SlidingSyncDiscoveryResult::InvalidResponse {
            failure: DiscoveryResponseFailureKind::InvalidHomeserver,
            http_status_class: None,
        };
    };

    // Keep request construction, response decoding, and request policy in the Matrix SDK/Ruma
    // path. The injected native transport differs from the SDK default only by refusing redirects:
    // together with disable_retry(), that makes capability discovery exactly one HTTP request.
    let http_client = match reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .user_agent("matrix-rust-sdk")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            attempt.error("capability discovery does not follow redirects")
        }))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return SlidingSyncDiscoveryResult::Unreachable {
                failure: DiscoveryTransportFailureKind::Other,
            };
        }
    };
    let client = match matrix_sdk::Client::builder()
        .homeserver_url(homeserver)
        .http_client(http_client)
        .build()
        .await
    {
        Ok(client) => client,
        Err(_) => {
            return SlidingSyncDiscoveryResult::Unreachable {
                failure: DiscoveryTransportFailureKind::Other,
            };
        }
    };
    let request_config = RequestConfig::new()
        .timeout(DISCOVERY_TIMEOUT)
        .disable_retry()
        .skip_auth();
    let response = match client.fetch_server_versions(Some(request_config)).await {
        Ok(response) => response,
        Err(error) => {
            return classify_http_error(&error);
        }
    };

    match response
        .unstable_features
        .get(SIMPLIFIED_SLIDING_SYNC_FEATURE)
    {
        Some(true) => SlidingSyncDiscoveryResult::Supported {
            source: DiscoverySource::Versions,
            advertised: true,
            http_status_class: Some(HttpStatusClass::Success),
        },
        Some(false) => SlidingSyncDiscoveryResult::Unsupported {
            advertised: true,
            http_status_class: Some(HttpStatusClass::Success),
        },
        None => SlidingSyncDiscoveryResult::Unsupported {
            advertised: false,
            http_status_class: Some(HttpStatusClass::Success),
        },
    }
}

fn classify_http_error(error: &HttpError) -> SlidingSyncDiscoveryResult {
    match error {
        HttpError::Reqwest(error) if error.is_redirect() => {
            SlidingSyncDiscoveryResult::InvalidResponse {
                failure: DiscoveryResponseFailureKind::HttpStatus,
                http_status_class: Some(HttpStatusClass::Other),
            }
        }
        HttpError::Reqwest(error) => SlidingSyncDiscoveryResult::Unreachable {
            failure: classify_transport_failure(error),
        },
        HttpError::Api(error) => match error.as_ref() {
            FromHttpResponseError::Deserialization(_) => {
                SlidingSyncDiscoveryResult::InvalidResponse {
                    failure: DiscoveryResponseFailureKind::Malformed,
                    http_status_class: Some(HttpStatusClass::Success),
                }
            }
            FromHttpResponseError::Server(response) => {
                SlidingSyncDiscoveryResult::InvalidResponse {
                    failure: DiscoveryResponseFailureKind::HttpStatus,
                    http_status_class: Some(classify_status(match response {
                        UiaaResponse::AuthResponse(_) => StatusCode::UNAUTHORIZED,
                        UiaaResponse::MatrixError(error) => error.status_code,
                    })),
                }
            }
            _ => SlidingSyncDiscoveryResult::InvalidResponse {
                failure: DiscoveryResponseFailureKind::Malformed,
                http_status_class: None,
            },
        },
        HttpError::Cached(error) => classify_http_error(error),
        HttpError::IntoHttp(_) | HttpError::RefreshToken(_) => {
            SlidingSyncDiscoveryResult::InvalidResponse {
                failure: DiscoveryResponseFailureKind::Malformed,
                http_status_class: None,
            }
        }
        #[cfg(target_os = "android")]
        HttpError::VerifierBuilder(_) => SlidingSyncDiscoveryResult::Unreachable {
            failure: DiscoveryTransportFailureKind::Connection,
        },
    }
}

fn classify_transport_failure(error: &reqwest::Error) -> DiscoveryTransportFailureKind {
    if error.is_timeout() {
        DiscoveryTransportFailureKind::Timeout
    } else if error.is_connect() {
        DiscoveryTransportFailureKind::Connection
    } else {
        DiscoveryTransportFailureKind::Other
    }
}

fn classify_status(status: StatusCode) -> HttpStatusClass {
    if status.is_success() {
        HttpStatusClass::Success
    } else if status.is_client_error() {
        HttpStatusClass::ClientError
    } else if status.is_server_error() {
        HttpStatusClass::ServerError
    } else {
        HttpStatusClass::Other
    }
}

fn normalized_homeserver_url(input: &str) -> Option<Url> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let base = Url::parse(&candidate).ok()?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return None;
    }
    if base.scheme() == "http" && !is_loopback(&base) {
        return None;
    }
    Some(base)
}

fn is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}
