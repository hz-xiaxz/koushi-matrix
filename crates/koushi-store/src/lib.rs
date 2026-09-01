//! Native persistence mechanics for credentials and encrypted files.

mod credential_backend;
mod credential_vault;

pub use credential_backend::{CredentialStoreBackend, OsCredentialStore};
#[cfg(any(debug_assertions, test, feature = "test-hooks"))]
pub use credential_backend::{FileCredentialStore, resolved_credential_backend_is_file_dir};
pub use credential_backend::{local_secret_error_health, record_local_unlock_secret};
#[cfg(any(test, feature = "test-hooks"))]
pub use credential_vault::{CredentialVaultData, CredentialVaultFile};
pub use credential_vault::{
    LocalStoreMigrationRecord, LocalStoreMigrationState, PendingLoginRecord, PendingLoginState,
};

pub const CREDENTIAL_STORE_SERVICE_NAME: &str = "koushi-desktop";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvelopeError {
    PayloadTooLarge,
    Invalid,
    Encryption,
    Decryption,
}

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Encrypt a bounded payload as `magic || nonce || ciphertext`.
pub fn encrypt_envelope(
    magic: &[u8],
    key: &[u8; 32],
    plaintext: &[u8],
    max_payload: usize,
) -> Result<Vec<u8>, EnvelopeError> {
    if plaintext.len() > max_payload {
        return Err(EnvelopeError::PayloadTooLarge);
    }
    use chacha20poly1305::{
        ChaCha20Poly1305, Key, Nonce,
        aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
    };
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| EnvelopeError::Encryption)?;
    let mut payload = Vec::with_capacity(magic.len() + NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(magic);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

/// Decrypt a bounded `magic || nonce || ciphertext` payload.
pub fn decrypt_envelope(
    magic: &[u8],
    key: &[u8; 32],
    payload: &[u8],
    max_payload: usize,
) -> Result<Vec<u8>, EnvelopeError> {
    let header_len = magic.len() + NONCE_LEN;
    if payload.len() < header_len
        || payload.len()
            > magic
                .len()
                .saturating_add(NONCE_LEN + TAG_LEN)
                .saturating_add(max_payload)
        || !payload.starts_with(magic)
    {
        return Err(EnvelopeError::Invalid);
    }
    use chacha20poly1305::{
        ChaCha20Poly1305, Key, Nonce,
        aead::{Aead, KeyInit},
    };
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(&payload[magic.len()..header_len]),
            &payload[header_len..],
        )
        .map_err(|_| EnvelopeError::Decryption)
}

/// Replace a file atomically, syncing both the temporary file and directory.
pub fn atomic_replace_file(
    path: &std::path::Path,
    payload: &[u8],
    fail_before_persist: bool,
) -> std::io::Result<()> {
    use std::{fs, io::Write};
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "replacement path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(payload)?;
    temporary.as_file().sync_all()?;
    if fail_before_persist {
        return Err(std::io::Error::other(
            "atomic replacement failed before persist",
        ));
    }
    temporary.persist(path).map_err(|error| error.error)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EnvelopeError, decrypt_envelope, encrypt_envelope};

    const MAGIC: &[u8] = b"KOUSHI-STORE-TEST-V1\0";
    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn unbounded_envelope_round_trips_without_size_overflow() {
        let payload = encrypt_envelope(MAGIC, &KEY, b"synthetic", usize::MAX)
            .expect("encrypt unbounded fixture");
        let plaintext =
            decrypt_envelope(MAGIC, &KEY, &payload, usize::MAX).expect("decrypt unbounded fixture");
        assert_eq!(plaintext, b"synthetic");
    }

    #[test]
    fn decrypt_envelope_accepts_the_existing_magic_nonce_ciphertext_layout() {
        use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce, aead::Aead};

        let nonce = [3_u8; 12];
        let ciphertext = ChaCha20Poly1305::new(Key::from_slice(&KEY))
            .encrypt(Nonce::from_slice(&nonce), b"legacy".as_ref())
            .expect("encrypt legacy-layout fixture");
        let mut payload = MAGIC.to_vec();
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);

        assert_eq!(
            decrypt_envelope(MAGIC, &KEY, &payload, usize::MAX)
                .expect("decrypt legacy-layout fixture"),
            b"legacy"
        );
    }

    #[test]
    fn envelope_bounds_and_magic_fail_closed() {
        assert_eq!(
            encrypt_envelope(MAGIC, &KEY, b"x", 0),
            Err(EnvelopeError::PayloadTooLarge)
        );
        let payload = encrypt_envelope(MAGIC, &KEY, b"x", 1).expect("encrypt bounded fixture");
        assert_eq!(
            decrypt_envelope(MAGIC, &KEY, &payload, 0),
            Err(EnvelopeError::Invalid)
        );
        assert_eq!(
            decrypt_envelope(b"WRONG", &KEY, &payload, 1),
            Err(EnvelopeError::Invalid)
        );
    }
}
