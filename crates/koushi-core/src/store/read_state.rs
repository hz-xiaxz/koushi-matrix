use super::{
    CoreFailure, StoreActor, COMPOSER_DRAFTS_NONCE_LEN, READ_STATE_OUTBOX_FILE_MAGIC,
    READ_STATE_OUTBOX_GENERATIONS, READ_STATE_OUTBOX_MAX_BYTES, READ_STATE_OUTBOX_VERSION,
    READ_STATE_OUTBOX_WRITERS,
};
use chacha20poly1305::{
    aead::{rand_core::RngCore, Aead, OsRng},
    ChaCha20Poly1305, Key, KeyInit, Nonce,
};
use koushi_key::LocalUnlockSecret;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const READ_STATE_OUTBOX_VERSION: u8 = 1;
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
        let path = self.account_read_state_outbox_file(key_id);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(crate::read_state::ReadPersistenceSnapshot::default());
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        if metadata.len() > READ_STATE_OUTBOX_MAX_BYTES as u64 {
            return Err(CoreFailure::StoreUnavailable);
        }
        let bytes = std::fs::read(&path).map_err(|_| CoreFailure::StoreUnavailable)?;
        decrypt_read_state_outbox_payload(&self.load_unlock_secret(key_id)?, &bytes)
    }

    pub(crate) fn save_read_state_outbox(
        &self,
        key_id: &SessionKeyId,
        snapshot: &crate::read_state::ReadPersistenceSnapshot,
    ) -> Result<(), CoreFailure> {
        let path = self.account_read_state_outbox_file(key_id);
        if snapshot.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(CoreFailure::StoreUnavailable),
            };
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload = encrypt_read_state_outbox_payload(
            &self.load_or_create_unlock_secret(key_id)?,
            snapshot,
        )?;
        if payload.len() > READ_STATE_OUTBOX_MAX_BYTES {
            return Err(CoreFailure::StoreUnavailable);
        }
        atomic_replace_file(&path, &payload, false)
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
            .join("outbox.v1.enc")
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
}

fn encrypt_read_state_outbox_payload(
    secret: &LocalUnlockSecret,
    snapshot: &crate::read_state::ReadPersistenceSnapshot,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(&(READ_STATE_OUTBOX_VERSION, snapshot))
        .map_err(|_| CoreFailure::StoreUnavailable)?;
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
    let mut payload = Vec::with_capacity(
        READ_STATE_OUTBOX_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len(),
    );
    payload.extend_from_slice(READ_STATE_OUTBOX_FILE_MAGIC);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_read_state_outbox_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<crate::read_state::ReadPersistenceSnapshot, CoreFailure> {
    let header_len = READ_STATE_OUTBOX_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len
        || payload.len() > READ_STATE_OUTBOX_MAX_BYTES
        || !payload.starts_with(READ_STATE_OUTBOX_FILE_MAGIC)
    {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = READ_STATE_OUTBOX_FILE_MAGIC.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let key = secret.derive_read_state_outbox_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&payload[nonce_start..nonce_end]),
            &payload[nonce_end..],
        )
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    if plaintext.len() > READ_STATE_OUTBOX_MAX_BYTES {
        return Err(CoreFailure::StoreUnavailable);
    }
    let (version, snapshot): (u8, crate::read_state::ReadPersistenceSnapshot) =
        serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)?;
    if version != READ_STATE_OUTBOX_VERSION
        || crate::read_state::ReadStateEngine::restore(0, snapshot.clone()).is_none()
    {
        return Err(CoreFailure::StoreUnavailable);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::test_support::{file_store_actor, make_key_id};
    use super::{CoreFailure, StoreActor};
    use tempfile::tempdir;

    #[test]
    fn atomic_read_state_outbox_replace_overwrites_an_existing_file() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("outbox.enc");

        atomic_replace_file(&path, b"first", false).expect("first atomic write");
        atomic_replace_file(&path, b"second", false).expect("replacement atomic write");

        assert_eq!(std::fs::read(path).expect("read replacement"), b"second");
    }

    #[test]
    fn read_state_outbox_round_trips_encrypted_without_plaintext_identifiers() {
        use crate::read_state::{ReadStateEngine, ReadStateKey, ReadTarget, ReadWaiterId};

        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let room_id = "!secret-room:test.example.com";
        let event_id = "$secret-event:test.example.com";
        let mut engine = ReadStateEngine::new(7);
        engine.admit(
            7,
            ReadStateKey::PublicUnthreaded {
                room_id: room_id.to_owned(),
            },
            ReadTarget::new(event_id.to_owned()),
            ReadWaiterId::new(1),
        );
        let snapshot = engine.persistence_snapshot();

        actor
            .save_read_state_outbox(&key_id, &snapshot)
            .expect("save encrypted read-state outbox");

        let path = actor.account_read_state_outbox_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted outbox");
        assert!(!bytes
            .windows(room_id.len())
            .any(|window| window == room_id.as_bytes()));
        assert!(!bytes
            .windows(event_id.len())
            .any(|window| window == event_id.as_bytes()));
        assert_eq!(
            actor
                .load_read_state_outbox(&key_id)
                .expect("load encrypted outbox"),
            snapshot
        );
        assert!(std::fs::read_dir(path.parent().expect("outbox parent"))
            .expect("read outbox parent")
            .all(|entry| !entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
    }

    #[test]
    fn read_state_outbox_fails_closed_for_wrong_key_and_corruption() {
        use crate::read_state::{ReadStateEngine, ReadStateKey, ReadTarget, ReadWaiterId};

        let data_dir = tempdir().expect("tempdir");
        let first_cred_dir = tempdir().expect("tempdir");
        let second_cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let first = file_store_actor(&data_dir, &first_cred_dir);
        let second = file_store_actor(&data_dir, &second_cred_dir);
        let mut engine = ReadStateEngine::new(9);
        engine.admit(
            9,
            ReadStateKey::FullyReadAndPrivateUnthreaded {
                room_id: "!secret-room:test.example.com".to_owned(),
            },
            ReadTarget::new("$secret-event:test.example.com".to_owned()),
            ReadWaiterId::new(1),
        );
        first
            .save_read_state_outbox(&key_id, &engine.persistence_snapshot())
            .expect("save encrypted read-state outbox");

        second
            .account_store_config(&key_id)
            .expect("create a different unlock secret");
        assert!(matches!(
            second.load_read_state_outbox(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));

        let path = first.account_read_state_outbox_file(&key_id);
        let mut bytes = std::fs::read(&path).expect("read encrypted outbox");
        *bytes.last_mut().expect("encrypted payload byte") ^= 1;
        std::fs::write(&path, bytes).expect("write corrupt outbox");
        assert!(matches!(
            first.load_read_state_outbox(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }

    #[test]
    fn empty_read_state_snapshot_removes_the_durable_outbox() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let path = actor.account_read_state_outbox_file(&key_id);
        let snapshot = crate::read_state::ReadPersistenceSnapshot::default();

        actor
            .save_read_state_outbox(&key_id, &snapshot)
            .expect("empty snapshot delete is idempotent");

        assert!(!path.exists());
        assert_eq!(
            actor
                .load_read_state_outbox(&key_id)
                .expect("missing outbox is empty"),
            snapshot
        );
    }

    #[test]
    fn stale_read_state_outbox_save_cannot_overwrite_newer_session_generation() {
        use crate::read_state::{ReadStateEngine, ReadStateKey, ReadTarget, ReadWaiterId};

        fn snapshot(event_id: &str) -> crate::read_state::ReadPersistenceSnapshot {
            let mut engine = ReadStateEngine::new(1);
            engine.admit(
                1,
                ReadStateKey::PublicUnthreaded {
                    room_id: "!synthetic-room:test.example.com".to_owned(),
                },
                ReadTarget::new(event_id.to_owned()),
                ReadWaiterId::new(1),
            );
            engine.persistence_snapshot()
        }

        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let newer = snapshot("$newer:test.example.com");
        let stale = snapshot("$stale:test.example.com");

        assert!(actor
            .save_read_state_outbox_if_current(&key_id, 2, 1, &newer)
            .expect("newer session save"));
        assert!(!actor
            .save_read_state_outbox_if_current(&key_id, 1, u64::MAX, &stale)
            .expect("stale session is rejected"));
        assert_eq!(
            actor
                .load_read_state_outbox(&key_id)
                .expect("load generation-fenced outbox"),
            newer
        );
    }

    #[test]
    fn blocked_outbox_io_does_not_block_invalidation_or_win_after_timeout() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let path = actor.account_read_state_outbox_file(&key_id);
        let writer_path = path.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            save_read_state_outbox_generation_fenced(&writer_path, 1, 1, || {
                entered_tx.send(()).expect("announce blocked write");
                let _ = release_rx.recv();
                std::fs::create_dir_all(writer_path.parent().expect("outbox parent directory"))
                    .map_err(|_| CoreFailure::StoreUnavailable)?;
                std::fs::write(&writer_path, b"stale").map_err(|_| CoreFailure::StoreUnavailable)
            })
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("writer reaches simulated blocking IO");

        let invalidator = actor.clone();
        let invalidation_key = key_id.clone();
        let (invalidated_tx, invalidated_rx) = std::sync::mpsc::channel();
        let invalidation = std::thread::spawn(move || {
            invalidator.invalidate_read_state_outbox_saves(&invalidation_key, 2);
            invalidated_tx.send(()).expect("report invalidation");
        });
        invalidated_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("invalidation must not wait for keychain or filesystem IO");

        release_tx.send(()).expect("release simulated IO");
        assert!(
            !writer
                .join()
                .expect("writer thread")
                .expect("generation-fenced write"),
            "the invalidated writer must report stale"
        );
        invalidation.join().expect("invalidation thread");
        assert!(
            !path.exists(),
            "a stale writer that finishes after invalidation must not win"
        );
    }
}
