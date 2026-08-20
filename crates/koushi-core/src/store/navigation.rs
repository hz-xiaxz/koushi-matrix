use super::{
    atomic_replace, CoreFailure, StoreActor, COMPOSER_DRAFTS_NONCE_LEN, NAVIGATION_FILE_MAGIC,
};
use chacha20poly1305::{
    aead::{rand_core::RngCore, Aead, OsRng},
    ChaCha20Poly1305, Key, KeyInit, Nonce,
};
use koushi_key::LocalUnlockSecret;
use koushi_state::NavigationState;

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
mod tests {
    use super::test_support::{file_store_actor, make_key_id};
    use super::{CoreFailure, StoreActor};
    use tempfile::tempdir;

    #[test]
    fn navigation_state_is_encrypted_and_rejects_corruption() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let navigation = NavigationState {
            active_space_id: Some("!space:test.example.com".to_owned()),
            active_room_id: Some("!room:test.example.com".to_owned()),
            space_order: vec!["!space:test.example.com".to_owned()],
            last_room_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                "!room:test.example.com".to_owned(),
            )]),
            last_selection_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                koushi_state::SpaceNavigationSelection {
                    surface: koushi_state::SpaceConversationSurface::Dms,
                    room_id: Some("!dm:test.example.com".to_owned()),
                },
            )]),
            room_scroll_anchors: std::collections::BTreeMap::new(),
            main_timeline_anchor: None,
        };

        actor
            .save_navigation(&key_id, &navigation)
            .expect("save encrypted navigation");

        let path = actor.account_navigation_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted navigation");
        assert!(!path.with_extension("tmp").exists());
        for plaintext in ["!space:test.example.com", "!room:test.example.com"] {
            assert!(!bytes
                .windows(plaintext.len())
                .any(|window| window == plaintext.as_bytes()));
        }

        let loaded = actor
            .load_navigation(&key_id)
            .expect("load encrypted navigation");
        assert_eq!(loaded, navigation);

        let mut corrupted = bytes;
        let last = corrupted
            .last_mut()
            .expect("non-empty encrypted navigation");
        *last ^= 0x01;
        std::fs::write(&path, corrupted).expect("write corrupted navigation");
        assert!(matches!(
            actor.load_navigation(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }

    #[test]
    fn legacy_navigation_json_loads_and_next_save_migrates_to_encrypted_file() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let navigation = NavigationState {
            active_space_id: Some("!space:test.example.com".to_owned()),
            active_room_id: Some("!room:test.example.com".to_owned()),
            space_order: vec!["!space:test.example.com".to_owned()],
            last_room_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                "!room:test.example.com".to_owned(),
            )]),
            last_selection_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                koushi_state::SpaceNavigationSelection {
                    surface: koushi_state::SpaceConversationSurface::Dms,
                    room_id: Some("!dm:test.example.com".to_owned()),
                },
            )]),
            room_scroll_anchors: std::collections::BTreeMap::new(),
            main_timeline_anchor: None,
        };
        let legacy_path = actor.account_navigation_legacy_file(&key_id);
        std::fs::create_dir_all(legacy_path.parent().expect("navigation parent"))
            .expect("create navigation parent");
        std::fs::write(
            &legacy_path,
            serde_json::to_string(&navigation).expect("serialize legacy navigation"),
        )
        .expect("write legacy navigation");

        let loaded = actor
            .load_navigation(&key_id)
            .expect("load legacy navigation");
        assert_eq!(loaded, navigation);

        actor
            .save_navigation(&key_id, &navigation)
            .expect("migrate navigation");
        assert!(!legacy_path.exists());

        let encrypted_path = actor.account_navigation_file(&key_id);
        let bytes = std::fs::read(&encrypted_path).expect("read encrypted navigation");
        for plaintext in ["!space:test.example.com", "!room:test.example.com"] {
            assert!(!bytes
                .windows(plaintext.len())
                .any(|window| window == plaintext.as_bytes()));
        }
    }

    #[test]
    fn default_navigation_removes_encrypted_and_legacy_files() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let navigation = NavigationState {
            active_space_id: None,
            active_room_id: Some("!room:test.example.com".to_owned()),
            space_order: Vec::new(),
            last_room_by_space_id: std::collections::BTreeMap::new(),
            last_selection_by_space_id: std::collections::BTreeMap::new(),
            room_scroll_anchors: std::collections::BTreeMap::new(),
            main_timeline_anchor: None,
        };

        actor
            .save_navigation(&key_id, &navigation)
            .expect("save encrypted navigation");
        let encrypted_path = actor.account_navigation_file(&key_id);
        assert!(encrypted_path.exists());

        let legacy_path = actor.account_navigation_legacy_file(&key_id);
        std::fs::create_dir_all(legacy_path.parent().expect("navigation parent"))
            .expect("create navigation parent");
        std::fs::write(&legacy_path, "{}").expect("write legacy navigation");
        assert!(legacy_path.exists());

        actor
            .save_navigation(&key_id, &NavigationState::default())
            .expect("clear navigation");
        assert!(!encrypted_path.exists());
        assert!(!legacy_path.exists());
        assert_eq!(
            actor
                .load_navigation(&key_id)
                .expect("load cleared navigation"),
            NavigationState::default()
        );
    }

    #[test]
    fn encrypted_navigation_state_preserves_room_scroll_anchor() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let navigation = NavigationState {
            active_space_id: Some("!space:test.example.com".to_owned()),
            active_room_id: Some("!room:test.example.com".to_owned()),
            space_order: vec!["!space:test.example.com".to_owned()],
            last_room_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                "!room:test.example.com".to_owned(),
            )]),
            last_selection_by_space_id: std::collections::BTreeMap::from([(
                "!space:test.example.com".to_owned(),
                koushi_state::SpaceNavigationSelection {
                    surface: koushi_state::SpaceConversationSurface::Rooms,
                    room_id: Some("!room:test.example.com".to_owned()),
                },
            )]),
            room_scroll_anchors: std::collections::BTreeMap::from([(
                "!room:test.example.com".to_owned(),
                koushi_state::TimelineScrollAnchor {
                    event_id: "$anchor:event".to_owned(),
                    edge: koushi_state::TimelineScrollAnchorEdge::Top,
                    offset_px: -32,
                    updated_at_ms: 1_820_000_000_000,
                },
            )]),
            main_timeline_anchor: None,
        };

        actor
            .save_navigation(&key_id, &navigation)
            .expect("save encrypted navigation");
        let loaded = actor
            .load_navigation(&key_id)
            .expect("load encrypted navigation");

        assert_eq!(loaded, navigation);
    }

    #[test]
    fn legacy_navigation_json_without_scroll_anchors_loads_with_empty_map() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let legacy_path = actor.account_navigation_legacy_file(&key_id);
        std::fs::create_dir_all(legacy_path.parent().expect("navigation parent"))
            .expect("create navigation parent");
        std::fs::write(
            &legacy_path,
            r#"{
                    "active_space_id":"!space:test.example.com",
                    "active_room_id":"!room:test.example.com",
                    "space_order":["!space:test.example.com"],
                    "last_room_by_space_id":{"!space:test.example.com":"!room:test.example.com"}
                }"#,
        )
        .expect("write legacy navigation");

        let loaded = actor
            .load_navigation(&key_id)
            .expect("load legacy navigation");

        assert!(loaded.room_scroll_anchors.is_empty());
        assert_eq!(
            loaded.active_room_id.as_deref(),
            Some("!room:test.example.com")
        );
    }
}
