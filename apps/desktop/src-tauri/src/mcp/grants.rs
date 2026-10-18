//! Per-connection grants.
//!
//! A connection may call a tool only if the user has granted it for that named
//! client and that account. Grants live in `mcp/connections.json` in the app
//! data directory and are reloaded on every call: a revoked grant takes effect
//! immediately, with no restart.
//!
//! The file is read-only for the app. Anything that would write it (a settings
//! surface) comes later; until then the bridge fails closed, so a missing or
//! malformed file grants nothing.

use std::path::{Path, PathBuf};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel};
use koushi_mcp::wire::{ErrorCode, WireError};
use serde::Deserialize;

pub(crate) const FILE_NAME: &str = "connections.json";

/// What a tool needs from its grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Need {
    Read,
    Send,
    /// Reserved for the attachment tool, which lands in a later slice.
    #[allow(dead_code)]
    Attachments,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Grant {
    pub(crate) client_name: String,
    #[serde(default)]
    pub(crate) accounts: Vec<String>,
    #[serde(default)]
    pub(crate) allow_read: bool,
    #[serde(default)]
    pub(crate) allow_send: bool,
    #[serde(default)]
    pub(crate) allow_attachments: bool,
}

#[derive(Debug, Deserialize)]
struct GrantFile {
    #[serde(default)]
    connections: Vec<Grant>,
}

/// Records a grant-file problem as a token event. Paths and parse errors stay
/// out of diagnostics: they name user data.
fn record(reason: &'static str) {
    koushi_diagnostics::record(
        DiagnosticEvent::new(DiagnosticLevel::Warn, "mcp.grants", "configuration")
            .field(DiagnosticField::token("reason", reason)),
    );
}

pub(crate) fn grants_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mcp").join(FILE_NAME)
}

/// Loads every grant. A missing file means no grants; a malformed file is
/// treated the same way and logged, never partially applied.
pub(crate) fn load(data_dir: &Path) -> Vec<Grant> {
    let path = grants_path(data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(_) => {
            record("grants_read_failed");
            return Vec::new();
        }
    };
    match serde_json::from_slice::<GrantFile>(&bytes) {
        Ok(file) => file
            .connections
            .into_iter()
            .filter(|grant| {
                let usable = !grant.client_name.trim().is_empty() && !grant.accounts.is_empty();
                if !usable {
                    record("grants_entry_ignored");
                }
                usable
            })
            .collect(),
        Err(_) => {
            record("grants_parse_failed");
            Vec::new()
        }
    }
}

/// Returns the grant that authorizes `client_name` for `account_id`, or the
/// error the caller should surface.
pub(crate) fn authorize<'a>(
    grants: &'a [Grant],
    client_name: &str,
    account_id: &str,
    need: Need,
) -> Result<&'a Grant, WireError> {
    let Some(grant) = grants.iter().find(|grant| grant.client_name == client_name) else {
        return Err(WireError::new(
            ErrorCode::ConsentRequired,
            format!("no Koushi grant exists for {client_name}"),
        ));
    };
    if !grant.accounts.iter().any(|id| id == account_id) {
        return Err(WireError::new(
            ErrorCode::NotPermitted,
            format!("{client_name} is not granted access to {account_id}"),
        ));
    }
    let allowed = match need {
        Need::Read => grant.allow_read,
        Need::Send => grant.allow_send,
        Need::Attachments => grant.allow_attachments,
    };
    if !allowed {
        return Err(WireError::new(
            ErrorCode::NotPermitted,
            format!(
                "{client_name} is not granted {} access to {account_id}",
                match need {
                    Need::Read => "read",
                    Need::Send => "send",
                    Need::Attachments => "attachment",
                }
            ),
        ));
    }
    Ok(grant)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> Grant {
        Grant {
            client_name: "claude-code".to_owned(),
            accounts: vec!["account:@alice:example.org".to_owned()],
            allow_read: true,
            allow_send: false,
            allow_attachments: false,
        }
    }

    #[test]
    fn a_missing_file_grants_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(load(dir.path()).is_empty());
        let error = authorize(&[], "claude-code", "account:@alice:example.org", Need::Read)
            .expect_err("no grant");
        assert_eq!(error.code, ErrorCode::ConsentRequired);
    }

    #[test]
    fn reads_are_allowed_and_sends_are_not() {
        let grants = [grant()];
        assert!(
            authorize(
                &grants,
                "claude-code",
                "account:@alice:example.org",
                Need::Read
            )
            .is_ok()
        );
        let error = authorize(
            &grants,
            "claude-code",
            "account:@alice:example.org",
            Need::Send,
        )
        .expect_err("send not granted");
        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    #[test]
    fn another_account_is_not_permitted() {
        let grants = [grant()];
        let error = authorize(
            &grants,
            "claude-code",
            "account:@bob:example.org",
            Need::Read,
        )
        .expect_err("other account");
        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    #[test]
    fn unknown_clients_need_consent() {
        let grants = [grant()];
        let error = authorize(&grants, "codex", "account:@alice:example.org", Need::Read)
            .expect_err("unknown client");
        assert_eq!(error.code, ErrorCode::ConsentRequired);
    }

    #[test]
    fn a_malformed_file_grants_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        ensure_dir_for_test(dir.path());
        std::fs::write(grants_path(dir.path()), b"{ not json").expect("write");
        assert!(load(dir.path()).is_empty());
    }

    fn ensure_dir_for_test(data_dir: &Path) {
        std::fs::create_dir_all(data_dir.join("mcp")).expect("mkdir");
    }

    #[test]
    fn parses_a_valid_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        ensure_dir_for_test(dir.path());
        std::fs::write(
            grants_path(dir.path()),
            br#"{"schema":1,"connections":[{"client_name":"claude-code",
                "accounts":["account:@alice:example.org"],
                "allow_read":true,"allow_send":true,"allow_attachments":true}]}"#,
        )
        .expect("write");
        let grants = load(dir.path());
        assert_eq!(grants.len(), 1);
        assert!(
            authorize(
                &grants,
                "claude-code",
                "account:@alice:example.org",
                Need::Send
            )
            .is_ok()
        );
    }
}
