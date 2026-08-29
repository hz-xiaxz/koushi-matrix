use super::{COMPOSER_DRAFTS_NONCE_LEN, CoreFailure, StoreActor};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use koushi_key::LocalUnlockSecret;
use koushi_key::SessionKeyId;
use koushi_state::NavigationState;
use std::io::Write;
use std::path::PathBuf;

const NAVIGATION_FILE_MAGIC: &[u8] = b"KOUSHI-NAVIGATION-V1\0";

fn atomic_replace(path: &std::path::Path, payload: &[u8]) -> Result<(), CoreFailure> {
    let temporary_path = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    file.write_all(payload)
        .and_then(|_| file.sync_all())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    std::fs::rename(&temporary_path, path).map_err(|_| CoreFailure::StoreUnavailable)
}

impl StoreActor {
    pub fn load_navigation(&self, key_id: &SessionKeyId) -> Result<NavigationState, CoreFailure> {
        let path = self.account_navigation_file(key_id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return self.load_legacy_navigation(key_id);
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        decrypt_navigation_payload(&self.load_unlock_secret(key_id)?, &bytes)
    }

    pub fn save_navigation(
        &self,
        key_id: &SessionKeyId,
        navigation: &NavigationState,
    ) -> Result<(), CoreFailure> {
        let path = self.account_navigation_file(key_id);
        let legacy_path = self.account_navigation_legacy_file(key_id);
        if navigation == &NavigationState::default() {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
            match std::fs::remove_file(&legacy_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload =
            encrypt_navigation_payload(&self.load_or_create_unlock_secret(key_id)?, navigation)?;
        atomic_replace(&path, &payload)?;
        match std::fs::remove_file(&legacy_path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(CoreFailure::StoreUnavailable),
        }
    }

    fn load_legacy_navigation(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<NavigationState, CoreFailure> {
        let path = self.account_navigation_legacy_file(key_id);
        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(NavigationState::default());
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        serde_json::from_str(&json).map_err(|_| CoreFailure::StoreUnavailable)
    }

    fn account_navigation_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("navigation")
            .join("navigation.v1.enc")
    }

    fn account_navigation_legacy_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("navigation")
            .join("navigation.v1.json")
    }
}

fn encrypt_navigation_payload(
    secret: &LocalUnlockSecret,
    navigation: &NavigationState,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(navigation).map_err(|_| CoreFailure::StoreUnavailable)?;
    let key = secret.derive_navigation_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0_u8; COMPOSER_DRAFTS_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    let mut payload = Vec::with_capacity(
        NAVIGATION_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len(),
    );
    payload.extend_from_slice(NAVIGATION_FILE_MAGIC);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_navigation_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<NavigationState, CoreFailure> {
    let header_len = NAVIGATION_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len || !payload.starts_with(NAVIGATION_FILE_MAGIC) {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = NAVIGATION_FILE_MAGIC.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let nonce = Nonce::from_slice(&payload[nonce_start..nonce_end]);
    let key = secret.derive_navigation_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let plaintext = cipher
        .decrypt(nonce, &payload[nonce_end..])
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)
}

#[cfg(test)]
mod tests;
