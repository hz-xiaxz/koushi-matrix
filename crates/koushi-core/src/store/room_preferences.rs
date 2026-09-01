use super::{COMPOSER_DRAFTS_NONCE_LEN, CoreFailure, StoreActor};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use koushi_key::LocalUnlockSecret;
use koushi_protocol::SessionKeyId;
use koushi_state::RoomPreferencesState;
use std::path::PathBuf;

const ROOM_PREFERENCES_FILE_MAGIC: &[u8] = b"KOUSHI-ROOM-PREFERENCES-V1\0";

impl StoreActor {
    pub fn load_room_preferences(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<RoomPreferencesState, CoreFailure> {
        let path = self.account_room_preferences_file(key_id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RoomPreferencesState::default());
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        decrypt_room_preferences_payload(&self.load_unlock_secret(key_id)?, &bytes)
    }

    pub fn save_room_preferences(
        &self,
        key_id: &SessionKeyId,
        preferences: &RoomPreferencesState,
    ) -> Result<(), CoreFailure> {
        let path = self.account_room_preferences_file(key_id);
        if preferences == &RoomPreferencesState::default() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload = encrypt_room_preferences_payload(
            &self.load_or_create_unlock_secret(key_id)?,
            preferences,
        )?;
        std::fs::write(path, payload).map_err(|_| CoreFailure::StoreUnavailable)
    }

    fn account_room_preferences_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("room-preferences")
            .join("preferences.v1.enc")
    }
}

fn encrypt_room_preferences_payload(
    secret: &LocalUnlockSecret,
    preferences: &RoomPreferencesState,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(preferences).map_err(|_| CoreFailure::StoreUnavailable)?;
    let key = secret.derive_room_preferences_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0_u8; COMPOSER_DRAFTS_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    let mut payload = Vec::with_capacity(
        ROOM_PREFERENCES_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len(),
    );
    payload.extend_from_slice(ROOM_PREFERENCES_FILE_MAGIC);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_room_preferences_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<RoomPreferencesState, CoreFailure> {
    let header_len = ROOM_PREFERENCES_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len || !payload.starts_with(ROOM_PREFERENCES_FILE_MAGIC) {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = ROOM_PREFERENCES_FILE_MAGIC.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let nonce = Nonce::from_slice(&payload[nonce_start..nonce_end]);
    let key = secret.derive_room_preferences_key();
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
    use koushi_state::{RoomPreference, RoomPreferencesState};
    use tempfile::tempdir;

    #[test]
    fn room_preferences_are_encrypted_and_reject_corruption() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let room_id = "!room:test.example.com";
        let preferences = RoomPreferencesState {
            rooms: std::collections::BTreeMap::from([(
                room_id.to_owned(),
                RoomPreference {
                    url_previews_enabled_override: Some(false),
                    ..RoomPreference::default()
                },
            )]),
        };

        actor
            .save_room_preferences(&key_id, &preferences)
            .expect("save encrypted room preferences");

        let path = actor.account_room_preferences_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted room preferences");
        assert!(
            !bytes
                .windows(room_id.len())
                .any(|window| window == room_id.as_bytes())
        );

        let loaded = actor
            .load_room_preferences(&key_id)
            .expect("load encrypted room preferences");
        assert_eq!(loaded, preferences);

        let mut corrupted = bytes;
        let last = corrupted
            .last_mut()
            .expect("non-empty encrypted room preferences");
        *last ^= 0x01;
        std::fs::write(&path, corrupted).expect("write corrupted room preferences");
        assert!(matches!(
            actor.load_room_preferences(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }
}
