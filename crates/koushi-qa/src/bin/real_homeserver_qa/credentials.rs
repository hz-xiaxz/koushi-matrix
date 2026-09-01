use super::AuthSecret;
use super::config::{ENV_CREDENTIALS_PATH, ENV_FILE_CREDENTIAL_STORE_DIR};

// ---------------------------------------------------------------------------
// Credentials (only loaded in debug/test builds)
// ---------------------------------------------------------------------------

/// Loaded from the credentials JSON file at the path given by
/// `ENV_CREDENTIALS_PATH`. Values are zeroized on drop via `AuthSecret`.
/// The file is read ONCE; the values are never written to stdout/stderr/logs.
#[cfg(any(debug_assertions, test))]
pub(super) struct RealCredentials {
    pub(super) homeserver: String,
    pub(super) user_id: String,
    /// Username part of user_id (before the colon): "@alice:server" -> "alice"
    pub(super) username: String,
    pub(super) password: AuthSecret,
    pub(super) recovery_key: AuthSecret,
    pub(super) device_display_name: String,
}

#[cfg(any(debug_assertions, test))]
impl RealCredentials {
    pub(super) fn load() -> Result<Self, String> {
        let path = std::env::var(ENV_CREDENTIALS_PATH).map_err(|_| {
            format!("{ENV_CREDENTIALS_PATH} is required (path to the credentials JSON file)")
        })?;

        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read credentials file at {path}: {e}"))?;

        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("credentials file is not valid JSON: {e}"))?;

        let homeserver = value["homeserver"]
            .as_str()
            .ok_or("credentials JSON missing 'homeserver'")?
            .to_owned();
        let user_id = value["user_id"]
            .as_str()
            .ok_or("credentials JSON missing 'user_id'")?
            .to_owned();
        // Extract username: "@alice:server" -> "alice"
        let username = user_id
            .trim_start_matches('@')
            .split(':')
            .next()
            .unwrap_or(&user_id)
            .to_owned();
        let password_str = value["password"]
            .as_str()
            .ok_or("credentials JSON missing 'password'")?
            .to_owned();
        let recovery_key_str = value["recovery_key"]
            .as_str()
            .ok_or("credentials JSON missing 'recovery_key'")?
            .to_owned();
        let device_display_name = value["device_display_name"]
            .as_str()
            .unwrap_or("Koushi Real QA")
            .to_owned();

        Ok(Self {
            homeserver,
            user_id,
            username,
            password: AuthSecret::new(password_str),
            recovery_key: AuthSecret::new(recovery_key_str),
            device_display_name,
        })
    }
}
#[cfg(any(debug_assertions, test))]
pub(super) fn assert_file_credential_store_active() -> Result<(), String> {
    if std::env::var_os(ENV_FILE_CREDENTIAL_STORE_DIR).is_none() {
        return Err(format!(
            "real-homeserver-qa refuses to run against the OS keychain: \
             {ENV_FILE_CREDENTIAL_STORE_DIR} is not set"
        ));
    }
    if !koushi_core::store::resolved_credential_backend_is_file_dir() {
        return Err(
            "real-homeserver-qa refuses to run against the OS keychain: \
             resolved credential store backend is not the file-dir backend"
                .to_owned(),
        );
    }
    Ok(())
}
