//! Private one-shot access to the active SDK session token (#1519).

use std::fmt;

use koushi_sdk::MatrixClientSession;
use koushi_state::{AuthSecret, SessionInfo};

/// A secret reply for one requesting connection, never snapshot/event data.
/// Dropping the reply zeroizes its token through `AuthSecret`.
pub struct SessionAccessToken {
    token: AuthSecret,
    session: SessionInfo,
    session_revision: u64,
}

impl SessionAccessToken {
    /// Expose only while fulfilling the explicitly requested private reply.
    pub fn token(&self) -> &str {
        self.token.expose_secret()
    }

    /// The exact account/device identity whose SDK token was read.
    pub fn session(&self) -> &SessionInfo {
        &self.session
    }

    /// The caller's authoritative session revision, for final delivery fencing.
    pub fn session_revision(&self) -> u64 {
        self.session_revision
    }
}

impl fmt::Debug for SessionAccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionAccessToken")
            .field("token", &"[redacted]")
            .field("session", &"[redacted]")
            .field("session_revision", &self.session_revision)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionAccessTokenError {
    #[error("account is not active")]
    AccountInactive,
    #[error("current session access token is unavailable")]
    Unavailable,
    #[error("session has changed")]
    StaleSession,
}

/// Read the SDK's current token without refreshing or serializing a session.
///
/// The runtime caller owns Ready/unlocked admission and validates the revision
/// again before delivering this private reply. This synchronous read validates
/// both the session wrapper and the SDK's authenticated identity.
pub fn read_current_session_access_token(
    session: Option<&MatrixClientSession>,
    expected: &SessionInfo,
    session_revision: u64,
) -> Result<SessionAccessToken, SessionAccessTokenError> {
    let session = session.ok_or(SessionAccessTokenError::AccountInactive)?;
    if &session.info != expected {
        return Err(SessionAccessTokenError::StaleSession);
    }
    let client = session.client();
    let (Some(user_id), Some(device_id)) = (client.user_id(), client.device_id()) else {
        return Err(SessionAccessTokenError::Unavailable);
    };
    let homeserver =
        url::Url::parse(&expected.homeserver).map_err(|_| SessionAccessTokenError::StaleSession)?;
    if user_id.as_str() != expected.user_id
        || device_id.as_str() != expected.device_id
        || client.homeserver() != homeserver
    {
        return Err(SessionAccessTokenError::StaleSession);
    }
    let token = client
        .access_token()
        .filter(|token| !token.is_empty())
        .ok_or(SessionAccessTokenError::Unavailable)?;
    Ok(SessionAccessToken {
        token: AuthSecret::new(token),
        session: expected.clone(),
        session_revision,
    })
}

#[cfg(test)]
mod tests;

/// Non-secret, JS-safe process-unique disclosure lifetime. Zero fails closed.
pub(crate) fn next_disclosure_epoch() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |value| (value < (1u64 << 53) - 1).then_some(value + 1),
    )
    .unwrap_or(0)
}
