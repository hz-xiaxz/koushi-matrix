use super::{COMPOSER_DRAFTS_NONCE_LEN, CoreFailure, StoreActor};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use koushi_key::LocalUnlockSecret;
use koushi_protocol::SessionKeyId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

const READ_STATE_OUTBOX_V2_MAGIC: &[u8] = b"KOUSHI-READ-STATE-OUTBOX-V2\0";
const READ_STATE_OUTBOX_V1_MAGIC: &[u8] = b"KOUSHI-READ-STATE-OUTBOX-V1\0";
const READ_STATE_OUTBOX_V2_VERSION: u8 = 2;
const READ_STATE_OUTBOX_V1_VERSION: u8 = 1;
const READ_STATE_OUTBOX_MAX_BYTES: usize = 256 * 1024;
static READ_STATE_OUTBOX_GENERATIONS: OnceLock<Mutex<HashMap<PathBuf, (u64, u64)>>> =
    OnceLock::new();
static READ_STATE_OUTBOX_WRITERS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    OnceLock::new();

impl StoreActor {
    pub(crate) fn load_read_state_outbox(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<crate::read_state::ReadPersistenceSnapshot, CoreFailure> {
        let v2_path = self.account_read_state_outbox_file(key_id);
        let v1_path = self.account_read_state_outbox_v1_file(key_id);
        let v2_bytes = match std::fs::read(&v2_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        let v1_bytes = match std::fs::read(&v1_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        if v2_bytes.is_none() && v1_bytes.is_none() {
            return Ok(crate::read_state::ReadPersistenceSnapshot::default());
        }
        let secret = self.load_unlock_secret(key_id)?;
        if let Some(bytes) = v2_bytes {
            let snapshot = decrypt_read_state_outbox_v2_payload(&secret, &bytes)?;
            if v1_bytes.is_some() {
                remove_read_state_file(&v1_path)?;
            }
            return Ok(snapshot);
        }
        let bytes = v1_bytes.expect("V1 bytes exist when V2 bytes do not");
        let legacy = decrypt_read_state_outbox_v1_payload(&secret, &bytes)?;
        let snapshot = crate::read_state::ReadPersistenceSnapshot::from_legacy_entries(
            legacy
                .entries
                .into_iter()
                .map(|entry| (entry.key, entry.event_ids))
                .collect(),
        )
        .ok_or(CoreFailure::StoreUnavailable)?;
        // The V1 vector has no reliable chronology. The engine has already
        // selected the documented conservative next-wake entry; write exactly
        // that one-ID V2 snapshot atomically before removing V1. A failed V2
        // write therefore leaves the old file available for a later retry.
        self.save_read_state_outbox(key_id, &snapshot)?;
        Ok(snapshot)
    }

    pub(crate) fn save_read_state_outbox(
        &self,
        key_id: &SessionKeyId,
        snapshot: &crate::read_state::ReadPersistenceSnapshot,
    ) -> Result<(), CoreFailure> {
        let path = self.account_read_state_outbox_file(key_id);
        let legacy_path = self.account_read_state_outbox_v1_file(key_id);
        if snapshot.is_empty() {
            remove_read_state_file(&path)?;
            remove_read_state_file(&legacy_path)?;
            return Ok(());
        }
        let payload = encrypt_read_state_outbox_v2_payload(
            &self.load_or_create_unlock_secret(key_id)?,
            snapshot,
        )?;
        if payload.len() > READ_STATE_OUTBOX_MAX_BYTES {
            return Err(CoreFailure::StoreUnavailable);
        }
        crate::file::atomic_replace_file(&path, &payload, false)
            .map_err(|_| CoreFailure::StoreUnavailable)?;
        remove_read_state_file(&legacy_path)
    }

    pub(crate) fn save_read_state_outbox_if_current(
        &self,
        key_id: &SessionKeyId,
        session_generation: u64,
        save_generation: u64,
        snapshot: &crate::read_state::ReadPersistenceSnapshot,
    ) -> Result<bool, CoreFailure> {
        let path = self.account_read_state_outbox_file(key_id);
        save_read_state_outbox_generation_fenced(&path, session_generation, save_generation, || {
            self.save_read_state_outbox(key_id, snapshot)
        })
    }

    pub(crate) fn invalidate_read_state_outbox_saves(
        &self,
        key_id: &SessionKeyId,
        session_generation: u64,
    ) {
        let path = self.account_read_state_outbox_file(key_id);
        if let Ok(mut generations) = READ_STATE_OUTBOX_GENERATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            let generation = (session_generation, u64::MAX);
            if generations
                .get(&path)
                .is_none_or(|current| *current < generation)
            {
                generations.insert(path, generation);
            }
        }
    }

    fn account_read_state_outbox_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("read-state")
            .join("outbox.v2.enc")
    }

    fn account_read_state_outbox_v1_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("read-state")
            .join("outbox.v1.enc")
    }
}

fn save_read_state_outbox_generation_fenced(
    path: &std::path::Path,
    session_generation: u64,
    save_generation: u64,
    write: impl FnOnce() -> Result<(), CoreFailure>,
) -> Result<bool, CoreFailure> {
    let writer = {
        let mut writers = READ_STATE_OUTBOX_WRITERS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| CoreFailure::StoreUnavailable)?;
        Arc::clone(
            writers
                .entry(path.to_path_buf())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    };
    let _writer = writer.lock().map_err(|_| CoreFailure::StoreUnavailable)?;
    let proposed = (session_generation, save_generation);
    {
        let mut generations = READ_STATE_OUTBOX_GENERATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| CoreFailure::StoreUnavailable)?;
        if generations
            .get(path)
            .is_some_and(|current| *current > proposed)
        {
            return Ok(false);
        }
        generations.insert(path.to_path_buf(), proposed);
    }

    // Credential/keychain access, encryption, fsync, and atomic replacement
    // happen outside the global generation mutex. Only this path's writer is
    // serialized, so timeout-driven session invalidation remains bounded.
    write()?;

    let still_current = READ_STATE_OUTBOX_GENERATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| CoreFailure::StoreUnavailable)?
        .get(path)
        .is_some_and(|current| *current == proposed);
    if !still_current {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        }
    }
    Ok(still_current)
}

fn remove_read_state_file(path: &std::path::Path) -> Result<(), CoreFailure> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CoreFailure::StoreUnavailable),
    }
}

fn encrypt_read_state_outbox_v2_payload(
    secret: &LocalUnlockSecret,
    snapshot: &crate::read_state::ReadPersistenceSnapshot,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(&(READ_STATE_OUTBOX_V2_VERSION, snapshot))
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    encrypt_read_state_outbox_payload(secret, READ_STATE_OUTBOX_V2_MAGIC, plaintext)
}

fn encrypt_read_state_outbox_payload(
    secret: &LocalUnlockSecret,
    magic: &[u8],
    plaintext: Vec<u8>,
) -> Result<Vec<u8>, CoreFailure> {
    if plaintext.len() > READ_STATE_OUTBOX_MAX_BYTES {
        return Err(CoreFailure::StoreUnavailable);
    }
    let key = secret.derive_read_state_outbox_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0_u8; COMPOSER_DRAFTS_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    let mut payload =
        Vec::with_capacity(magic.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(magic);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_read_state_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
    magic: &[u8],
) -> Result<Vec<u8>, CoreFailure> {
    let header_len = magic.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len
        || payload.len() > READ_STATE_OUTBOX_MAX_BYTES
        || !payload.starts_with(magic)
    {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = magic.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let key = secret.derive_read_state_outbox_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    cipher
        .decrypt(
            Nonce::from_slice(&payload[nonce_start..nonce_end]),
            &payload[nonce_end..],
        )
        .map_err(|_| CoreFailure::StoreUnavailable)
}

fn decrypt_read_state_outbox_v2_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<crate::read_state::ReadPersistenceSnapshot, CoreFailure> {
    let plaintext = decrypt_read_state_payload(secret, payload, READ_STATE_OUTBOX_V2_MAGIC)?;
    let (version, snapshot): (u8, crate::read_state::ReadPersistenceSnapshot) =
        serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)?;
    if version != READ_STATE_OUTBOX_V2_VERSION
        || crate::read_state::ReadStateEngine::restore(0, snapshot.clone()).is_none()
    {
        return Err(CoreFailure::StoreUnavailable);
    }
    Ok(snapshot)
}

#[derive(Deserialize, Serialize)]
struct ReadPersistenceV1Snapshot {
    entries: Vec<ReadPersistenceV1Entry>,
}

#[derive(Deserialize, Serialize)]
struct ReadPersistenceV1Entry {
    key: crate::read_state::ReadStateKey,
    event_ids: Vec<String>,
}

fn decrypt_read_state_outbox_v1_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<ReadPersistenceV1Snapshot, CoreFailure> {
    let plaintext = decrypt_read_state_payload(secret, payload, READ_STATE_OUTBOX_V1_MAGIC)?;
    let (version, snapshot): (u8, ReadPersistenceV1Snapshot) =
        serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)?;
    if version != READ_STATE_OUTBOX_V1_VERSION {
        return Err(CoreFailure::StoreUnavailable);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests;
