use super::{COMPOSER_DRAFTS_NONCE_LEN, CoreFailure, StoreActor};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use koushi_key::LocalUnlockSecret;
use koushi_protocol::SessionKeyId;
use koushi_state::ScheduledSendStore;
use std::path::PathBuf;

const SCHEDULED_SENDS_FILE_MAGIC: &[u8] = b"KOUSHI-SCHEDULED-SENDS-V1\0";

impl StoreActor {
    pub fn load_scheduled_sends(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<ScheduledSendStore, CoreFailure> {
        let path = self.account_scheduled_sends_file(key_id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ScheduledSendStore::default());
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        decrypt_scheduled_sends_payload(&self.load_unlock_secret(key_id)?, &bytes)
    }

    pub fn save_scheduled_sends(
        &self,
        key_id: &SessionKeyId,
        scheduled_sends: &ScheduledSendStore,
    ) -> Result<(), CoreFailure> {
        let path = self.account_scheduled_sends_file(key_id);
        if scheduled_sends.items.is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload = encrypt_scheduled_sends_payload(
            &self.load_or_create_unlock_secret(key_id)?,
            scheduled_sends,
        )?;
        std::fs::write(path, payload).map_err(|_| CoreFailure::StoreUnavailable)
    }

    fn account_scheduled_sends_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("scheduled-sends")
            .join("scheduled.v1.enc")
    }
}

fn encrypt_scheduled_sends_payload(
    secret: &LocalUnlockSecret,
    scheduled_sends: &ScheduledSendStore,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext =
        serde_json::to_vec(scheduled_sends).map_err(|_| CoreFailure::StoreUnavailable)?;
    let key = secret.derive_scheduled_sends_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0_u8; COMPOSER_DRAFTS_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    let mut payload = Vec::with_capacity(
        SCHEDULED_SENDS_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len(),
    );
    payload.extend_from_slice(SCHEDULED_SENDS_FILE_MAGIC);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_scheduled_sends_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<ScheduledSendStore, CoreFailure> {
    let header_len = SCHEDULED_SENDS_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len || !payload.starts_with(SCHEDULED_SENDS_FILE_MAGIC) {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = SCHEDULED_SENDS_FILE_MAGIC.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let nonce = Nonce::from_slice(&payload[nonce_start..nonce_end]);
    let key = secret.derive_scheduled_sends_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let plaintext = cipher
        .decrypt(nonce, &payload[nonce_end..])
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{file_store_actor, make_key_id};
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scheduled_sends_are_encrypted_and_reject_corruption() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let plaintext = "secret scheduled body";
        let mut scheduled_sends = ScheduledSendStore {
            capability: koushi_state::ScheduledSendCapability::LocalFallback,
            ..ScheduledSendStore::default()
        };
        scheduled_sends.insert(koushi_state::ScheduledSendItem {
            scheduled_id: "sched-1".to_owned(),
            room_id: "!room:test.example.com".to_owned(),
            thread_root_event_id: None,
            body: plaintext.to_owned(),
            send_at_ms: 1_900_000_000_000,
            handle: koushi_state::ScheduledSendHandle::Local,
            is_dispatching: false,
        });

        actor
            .save_scheduled_sends(&key_id, &scheduled_sends)
            .expect("save encrypted scheduled sends");

        let path = actor.account_scheduled_sends_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted scheduled sends");
        assert!(
            !bytes
                .windows(plaintext.len())
                .any(|window| window == plaintext.as_bytes())
        );

        let loaded = actor
            .load_scheduled_sends(&key_id)
            .expect("load encrypted scheduled sends");
        assert_eq!(
            loaded.items.get("sched-1").map(|item| item.body.as_str()),
            Some(plaintext)
        );
        assert_eq!(
            loaded.capability,
            koushi_state::ScheduledSendCapability::LocalFallback
        );

        let mut corrupted = bytes;
        let last = corrupted
            .last_mut()
            .expect("non-empty encrypted scheduled sends");
        *last ^= 0x01;
        std::fs::write(&path, corrupted).expect("write corrupted scheduled sends");
        assert!(matches!(
            actor.load_scheduled_sends(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }

    #[test]
    fn loading_scheduled_sends_does_not_create_missing_credentials() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let path = actor.account_scheduled_sends_file(&key_id);
        std::fs::create_dir_all(path.parent().expect("scheduled sends parent"))
            .expect("create parent");
        std::fs::write(&path, SCHEDULED_SENDS_FILE_MAGIC).expect("write scheduled placeholder");

        assert!(matches!(
            actor.load_scheduled_sends(&key_id),
            Err(CoreFailure::LocalEncryptionUnavailable)
        ));
        let missing = actor.credential_backend().load(&key_id).unwrap_err();
        assert!(koushi_key::is_missing_credential_error(&missing));
    }

    #[test]
    fn empty_scheduled_sends_remove_persisted_file() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let mut scheduled_sends = ScheduledSendStore {
            capability: koushi_state::ScheduledSendCapability::LocalFallback,
            ..ScheduledSendStore::default()
        };
        scheduled_sends.insert(koushi_state::ScheduledSendItem {
            scheduled_id: "sched-1".to_owned(),
            room_id: "!room:test.example.com".to_owned(),
            thread_root_event_id: None,
            body: "scheduled body".to_owned(),
            send_at_ms: 1_900_000_000_000,
            handle: koushi_state::ScheduledSendHandle::Local,
            is_dispatching: false,
        });

        actor
            .save_scheduled_sends(&key_id, &scheduled_sends)
            .expect("save non-empty scheduled sends");
        let path = actor.account_scheduled_sends_file(&key_id);
        assert!(path.exists());

        actor
            .save_scheduled_sends(&key_id, &ScheduledSendStore::default())
            .expect("save empty scheduled sends");
        assert!(!path.exists());
        assert!(
            actor
                .load_scheduled_sends(&key_id)
                .expect("load removed scheduled sends")
                .items
                .is_empty()
        );
    }
}
