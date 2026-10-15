//! #1364: persisted per-room conversation-activity cache.
//!
//! Canon: `docs/architecture/state-machine.md`, "Sidebar Sections And Low
//! Priority". The record is a local ordering cache: it carries the newest
//! conversation timestamp this client observed per room so Recent-first
//! ordering survives a restart with no in-memory timeline events. It is
//! account-scoped, envelope-encrypted with its own derived key, and never sent
//! to a homeserver.

use super::{CoreFailure, StoreActor};
use koushi_key::LocalUnlockSecret;
use koushi_protocol::SessionKeyId;
use koushi_state::RoomActivityState;
use std::path::PathBuf;

const ROOM_ACTIVITY_FILE_MAGIC: &[u8] = b"KOUSHI-ROOM-ACTIVITY-V1\0";

impl StoreActor {
    pub fn load_room_activity(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<RoomActivityState, CoreFailure> {
        let path = self.account_room_activity_file(key_id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RoomActivityState::default());
            }
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };
        decrypt_room_activity_payload(&self.load_unlock_secret(key_id)?, &bytes)
    }

    pub fn save_room_activity(
        &self,
        key_id: &SessionKeyId,
        activity: &RoomActivityState,
    ) -> Result<(), CoreFailure> {
        let path = self.account_room_activity_file(key_id);
        if activity.is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload =
            encrypt_room_activity_payload(&self.load_or_create_unlock_secret(key_id)?, activity)?;
        koushi_store::atomic_replace_file(&path, &payload, false)
            .map_err(|_| CoreFailure::StoreUnavailable)
    }

    fn account_room_activity_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("room-activity")
            .join("activity.v1.enc")
    }
}

fn encrypt_room_activity_payload(
    secret: &LocalUnlockSecret,
    activity: &RoomActivityState,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(activity).map_err(|_| CoreFailure::StoreUnavailable)?;
    let key = secret.derive_room_activity_key();
    koushi_store::encrypt_envelope(
        ROOM_ACTIVITY_FILE_MAGIC,
        key.as_bytes(),
        &plaintext,
        usize::MAX,
    )
    .map_err(|_| CoreFailure::StoreUnavailable)
}

fn decrypt_room_activity_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<RoomActivityState, CoreFailure> {
    let key = secret.derive_room_activity_key();
    let plaintext = koushi_store::decrypt_envelope(
        ROOM_ACTIVITY_FILE_MAGIC,
        key.as_bytes(),
        payload,
        usize::MAX,
    )
    .map_err(|_| CoreFailure::StoreUnavailable)?;
    serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{file_store_actor, make_key_id};
    use super::*;
    use koushi_state::{ConversationActivitySource, RoomActivityRecord};
    use tempfile::tempdir;

    #[test]
    fn room_activity_is_encrypted_and_rejects_corruption() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let room_id = "!room:test.example.com";
        let activity = RoomActivityState {
            rooms: std::collections::BTreeMap::from([(
                room_id.to_owned(),
                RoomActivityRecord {
                    timestamp_ms: 1_700_000_000_123,
                    source: ConversationActivitySource::Message,
                },
            )]),
        };

        actor
            .save_room_activity(&key_id, &activity)
            .expect("save encrypted room activity");

        let path = actor.account_room_activity_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted room activity");
        assert!(
            !bytes
                .windows(room_id.len())
                .any(|window| window == room_id.as_bytes())
        );
        let timestamp = 1_700_000_000_123u64.to_string();
        assert!(
            !bytes
                .windows(timestamp.len())
                .any(|window| window == timestamp.as_bytes()),
            "the timestamp must not be readable in the stored payload"
        );

        let loaded = actor
            .load_room_activity(&key_id)
            .expect("load encrypted room activity");
        assert_eq!(loaded, activity);

        let mut corrupted = bytes;
        let last = corrupted.last_mut().expect("non-empty encrypted payload");
        *last ^= 0x01;
        std::fs::write(&path, corrupted).expect("write corrupted room activity");
        assert!(matches!(
            actor.load_room_activity(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }

    #[test]
    fn saved_room_activity_is_removed_when_it_becomes_empty() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let mut activity = RoomActivityState::default();
        activity.observe(
            "!room:test.example.com",
            koushi_state::ConversationActivity {
                timestamp_ms: 42,
                source: ConversationActivitySource::Message,
            },
        );
        actor
            .save_room_activity(&key_id, &activity)
            .expect("save room activity");
        let path = actor.account_room_activity_file(&key_id);
        assert!(path.exists());

        actor
            .save_room_activity(&key_id, &RoomActivityState::default())
            .expect("clearing room activity removes the file");
        assert!(!path.exists());
        assert_eq!(
            actor.load_room_activity(&key_id).expect("load"),
            RoomActivityState::default()
        );
    }
}
