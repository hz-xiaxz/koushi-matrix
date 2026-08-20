//! StoreActor: credential store access, per-account store paths, store/search
//! key derivation, and debug/test credential injection policy.
//!
//! Security invariants:
//! - Store and search keys NEVER cross the command/event boundary.
//! - If credential store or encryption cannot be initialized for an account,
//!   `LocalEncryptionUnavailable` is returned (fail-closed).
//! - The file-based credential store override is behind a compile-time gate:
//!   `#[cfg(any(debug_assertions, test, feature = "qa-bin"))]` only.
//!
//! Architecture: overview.md Platform Portability rule 3 — platform
//! capabilities live here behind a port. StoreActor is the only actor allowed
//! platform-conditional code.

pub(crate) mod composer_drafts;
mod credential_backend;
mod navigation;
mod read_state;
mod room_preferences;
mod scheduled_sends;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;
use std::sync::Arc;
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::Mutex;

use koushi_key::{LocalUnlockSecret, SessionKeyId};
use koushi_sdk::{
    MatrixClientStoreConfig, MatrixClientStoreKey, MatrixSearchIndexKey,
    MatrixSearchIndexStoreConfig,
};
use koushi_state::LocalEncryptionHealth;

use crate::failure::CoreFailure;
pub use credential_backend::{CredentialStoreBackend, OsCredentialStore};
#[cfg(any(debug_assertions, test, feature = "qa-bin"))]
pub use credential_backend::{FileCredentialStore, resolved_credential_backend_is_file_dir};

use composer_drafts::{
    decode_payload_json as decode_composer_draft_payload_json,
    encode_payload_json as encode_composer_draft_payload_json,
};
use credential_backend::{local_secret_error_health, record_local_unlock_secret};

/// Service name used for OS keyring entries. This is user-visible in macOS
/// Keychain Access, so keep it aligned with the shipped product name.
const CREDENTIAL_STORE_SERVICE_NAME: &str = "koushi-desktop";
const COMPOSER_DRAFTS_FILE_MAGIC: &[u8] = b"KOUSHI-DRAFTS-V1\0";
const COMPOSER_DRAFTS_NONCE_LEN: usize = 12;

/// Derive a filesystem-safe directory name from a `SessionKeyId`.
/// Uses the same base64url encoding the key crate uses for credential store
/// account names, so both namespaces are consistent.
fn account_dir_name(key_id: &SessionKeyId) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    // Build a deterministic slug: encode homeserver + user_id + device_id
    // separated by underscores so the path stays readable in debug tooling.
    format!(
        "{}_{}_{}",
        URL_SAFE_NO_PAD.encode(key_id.homeserver.as_bytes()),
        URL_SAFE_NO_PAD.encode(key_id.user_id.as_bytes()),
        URL_SAFE_NO_PAD.encode(key_id.device_id.as_bytes()),
    )
}

/// Resolved store configuration for one account.
///
/// Keys never leave this module's calling chain — they are consumed by
/// `login_with_password_with_store` / `restore_session_with_store` and then
/// dropped. They never appear in events, snapshots, or logs.
pub struct AccountStoreConfig {
    pub store_config: MatrixClientStoreConfig,
    /// The session key id that identifies this account in the credential store.
    /// Retained so the account actor can persist / delete credentials.
    pub session_key_id: SessionKeyId,
}

/// Resolved search index configuration for one account.
///
/// Key never crosses the command/event boundary. Consumed by the client
/// builder and then dropped.
pub struct AccountSearchIndexConfig {
    pub search_index_config: MatrixSearchIndexStoreConfig,
}

/// StoreActor: resolves and manages per-account credential-backed store configs.
///
/// Owns the single `CredentialStoreBackend` — used for both unlock secrets
/// and session persistence. AccountActor delegates all credential operations
/// through `StoreActor`.
///
/// In Phase 2 this is a pure value type (no background task). Phase 6 may
/// promote it to an owned task when search index mutations require it.
#[derive(Clone)]
pub struct StoreActor {
    pub(crate) credential_store: CredentialStoreBackend,
    data_dir: PathBuf,
    #[cfg(any(test, feature = "test-hooks"))]
    composer_draft_io_probe: Arc<Mutex<Option<ComposerDraftIoProbe>>>,
    #[cfg(test)]
    composer_draft_replace_fault: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(any(test, feature = "test-hooks"))]
struct ComposerDraftIoProbe {
    save_started: Option<tokio::sync::oneshot::Sender<()>>,
    save_release: Option<std::sync::mpsc::Receiver<()>>,
    save_completed: Option<tokio::sync::oneshot::Sender<()>>,
    load_started: Option<tokio::sync::oneshot::Sender<()>>,
    load_completed: Option<tokio::sync::oneshot::Sender<()>>,
}

impl StoreActor {
    /// Create the actor. `data_dir` is the application data directory under
    /// which per-account sub-directories are created.
    ///
    /// Uses the **in-memory** credential store by default (keyring-free).
    /// Production builds must use `with_os_backend` to inject the OS adapter.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            credential_store: CredentialStoreBackend::resolve(),
            data_dir: data_dir.into(),
            #[cfg(any(test, feature = "test-hooks"))]
            composer_draft_io_probe: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            composer_draft_replace_fault: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Create the actor with an injected OS credential store backend.
    /// Used by production `CoreRuntime::start_with_data_dir_and_os_backend`.
    pub fn with_os_backend(
        data_dir: impl Into<PathBuf>,
        os_backend: Arc<dyn koushi_key::CredentialBackend>,
    ) -> Self {
        let data_dir = data_dir.into();
        Self {
            credential_store: CredentialStoreBackend::resolve_with_os_backend(
                data_dir.clone(),
                os_backend,
            ),
            data_dir,
            #[cfg(any(test, feature = "test-hooks"))]
            composer_draft_io_probe: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            composer_draft_replace_fault: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Access the credential store backend (for session persistence in AccountActor).
    pub fn credential_backend(&self) -> &CredentialStoreBackend {
        &self.credential_store
    }

    /// Test-only constructor with an explicit backend (avoids the env-global
    /// `KOUSHI_QA_FILE_CREDENTIAL_STORE_DIR` race between unit tests).
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn with_backend(
        credential_store: CredentialStoreBackend,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            credential_store,
            data_dir: data_dir.into(),
            composer_draft_io_probe: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            composer_draft_replace_fault: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Resolve (and if necessary create) a store configuration for the given
    /// account identity. On first use a fresh `LocalUnlockSecret` is generated
    /// and persisted; on subsequent uses the existing secret is loaded.
    ///
    /// Returns `LocalEncryptionUnavailable` if the credential store or key
    /// derivation fails — login/restore must not proceed in that case.
    pub fn account_store_config(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<AccountStoreConfig, CoreFailure> {
        let secret = self.load_or_create_unlock_secret_for(key_id, "account_store")?;
        let sdk_store_key = secret.derive_sdk_store_key();
        let store_key = MatrixClientStoreKey::new(*sdk_store_key.as_bytes());

        let store_dir = self.account_store_dir(key_id);
        let cache_dir = self.account_cache_dir(key_id);

        let store_config =
            MatrixClientStoreConfig::new(&store_dir, store_key).with_cache_path(&cache_dir);

        Ok(AccountStoreConfig {
            store_config,
            session_key_id: key_id.clone(),
        })
    }

    /// Derive the encrypted ngram search index configuration for the given
    /// account. Called by `AccountActor` when building the store-backed client
    /// so the SDK search index is initialized with the correct key.
    ///
    /// Returns `LocalEncryptionUnavailable` if the credential store is
    /// unreachable — the same fail-closed behavior as `account_store_config`.
    pub fn account_search_index_config(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<AccountSearchIndexConfig, CoreFailure> {
        let secret = self.load_or_create_unlock_secret_for(key_id, "search_index")?;
        let search_key = secret.derive_search_index_key();
        let search_dir = self.account_search_index_dir(key_id);
        let config = MatrixSearchIndexStoreConfig::new(
            &search_dir,
            MatrixSearchIndexKey::new(search_key.as_str()),
        );
        Ok(AccountSearchIndexConfig {
            search_index_config: config,
        })
    }

    /// Delete the stored unlock secret and the per-account store/cache
    /// directories for an account (shutdown step 7: "clear credentials and
    /// stores"). Called during logout / account removal.
    ///
    /// Errors do not propagate — a logout that partially cleans up is better
    /// than a logout that fails. Matrix session JSON / pointers stored via the
    /// credential backend are cleaned up by AccountActor through the same
    /// backend.
    pub fn delete_account_credentials(&self, key_id: &SessionKeyId) -> Result<(), ()> {
        let credential_deleted = self.credential_store.delete(key_id).is_ok();
        let directory_deleted = match std::fs::remove_dir_all(self.account_root_dir(key_id)) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        if credential_deleted && directory_deleted {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Probe the stored local unlock secret without creating a new one.
    ///
    /// This is the Rust-owned source for Settings/Security credential-store
    /// health. It is intentionally kind-only; raw backend errors never leave
    /// the store layer.
    pub fn probe_local_encryption_health(&self, key_id: &SessionKeyId) -> LocalEncryptionHealth {
        match self.credential_store.load(key_id) {
            Ok(_) => LocalEncryptionHealth::Healthy,
            Err(error) => local_secret_error_health(&error),
        }
    }

    /// The OS or file-based credential store backend.
    pub fn credential_store_backend(&self) -> &CredentialStoreBackend {
        &self.credential_store
    }

    /// Application data directory under which per-account sub-directories are
    /// created.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    // --- private helpers ---

    fn load_or_create_unlock_secret(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<LocalUnlockSecret, CoreFailure> {
        self.load_or_create_unlock_secret_with_diagnostic(key_id, None)
    }

    fn load_or_create_unlock_secret_for(
        &self,
        key_id: &SessionKeyId,
        purpose: &'static str,
    ) -> Result<LocalUnlockSecret, CoreFailure> {
        self.load_or_create_unlock_secret_with_diagnostic(key_id, Some(purpose))
    }

    fn load_or_create_unlock_secret_with_diagnostic(
        &self,
        key_id: &SessionKeyId,
        purpose: Option<&'static str>,
    ) -> Result<LocalUnlockSecret, CoreFailure> {
        match self.credential_store.load(key_id) {
            Ok(secret) => {
                record_local_unlock_secret(purpose, "loaded");
                Ok(secret)
            }
            Err(err) if koushi_key::is_missing_credential_error(&err) => {
                // First use: generate and persist a new unlock secret.
                let secret = LocalUnlockSecret::generate();
                if self.credential_store.save(key_id, &secret).is_err() {
                    record_local_unlock_secret(purpose, "save_failed");
                    return Err(CoreFailure::LocalEncryptionUnavailable);
                }
                record_local_unlock_secret(purpose, "created");
                Ok(secret)
            }
            Err(_) => {
                record_local_unlock_secret(purpose, "load_failed");
                Err(CoreFailure::LocalEncryptionUnavailable)
            }
        }
    }

    fn load_unlock_secret(&self, key_id: &SessionKeyId) -> Result<LocalUnlockSecret, CoreFailure> {
        self.credential_store
            .load(key_id)
            .map_err(|_| CoreFailure::LocalEncryptionUnavailable)
    }

    fn account_root_dir(&self, key_id: &SessionKeyId) -> PathBuf {
        self.data_dir
            .join("accounts")
            .join(account_dir_name(key_id))
    }

    fn account_store_dir(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id).join("store")
    }

    fn account_cache_dir(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id).join("cache")
    }

    fn account_search_index_dir(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id).join("search-index")
    }
}

fn atomic_replace_file(
    path: &std::path::Path,
    payload: &[u8],
    fail_before_persist: bool,
) -> Result<(), CoreFailure> {
    use std::io::Write as _;

    let parent = path.parent().ok_or(CoreFailure::StoreUnavailable)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
    temporary
        .write_all(payload)
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    if fail_before_persist {
        return Err(CoreFailure::StoreUnavailable);
    }
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|_| CoreFailure::StoreUnavailable)
}

/// Derive a filesystem-safe directory name from a `SessionKeyId`.
/// Convert a `SessionInfo` (from koushi-state) into a `SessionKeyId`
/// (from koushi-key). This is the canonical mapping used everywhere
/// in the codebase.
pub fn session_key_id_from_info(info: &koushi_state::SessionInfo) -> SessionKeyId {
    SessionKeyId {
        homeserver: info.homeserver.clone(),
        user_id: info.user_id.clone(),
        device_id: info.device_id.clone(),
    }
}

/// Derive a canonical `AccountKey` string for a session. The account key is
/// the user's Matrix ID — e.g. `@alice:example.com`.
pub fn account_key_from_info(info: &koushi_state::SessionInfo) -> crate::ids::AccountKey {
    crate::ids::AccountKey(info.user_id.clone())
}
