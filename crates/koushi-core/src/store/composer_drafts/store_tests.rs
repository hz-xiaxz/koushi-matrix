use super::super::test_support::{file_store_actor, make_key_id};
use super::super::*;
use super::{
    CoreFailure, PersistedComposerDraftStoreV3, encrypt_composer_drafts_fixture_payload,
    persisted_projection,
};
use koushi_state::ComposerDraftStore;
use tempfile::tempdir;

fn persisted_composer_drafts(drafts: &ComposerDraftStore) -> PersistedComposerDraftStoreV3 {
    persisted_projection(drafts, &koushi_state::ComposerDraftProtection::default())
}

#[test]
fn composer_drafts_are_encrypted_and_reject_corruption() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let plaintext = "secret draft body";
    let mut drafts = ComposerDraftStore::default();
    drafts.set_room_draft("!room:test.example.com".to_owned(), plaintext.to_owned());
    assert!(
        drafts
            .apply_room_draft(
                "!sent:test.example.com".to_owned(),
                "accepted body".to_owned(),
                7.into(),
            )
            .expect("room draft revision should apply")
    );
    assert_eq!(
        drafts
            .advance_room_revision("!sent:test.example.com", 7.into())
            .expect("room acceptance should advance"),
        8.into()
    );
    assert!(
        drafts
            .apply_thread_draft(
                "!room:test.example.com".to_owned(),
                "$root:test.example.com".to_owned(),
                "thread accepted body".to_owned(),
                11.into(),
            )
            .expect("thread draft revision should apply")
    );
    assert_eq!(
        drafts
            .advance_thread_revision(
                "!room:test.example.com",
                "$root:test.example.com",
                11.into(),
            )
            .expect("thread acceptance should advance"),
        12.into()
    );

    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
        .expect("save encrypted drafts");

    let path = actor.account_composer_drafts_file(&key_id);
    let bytes = std::fs::read(&path).expect("read encrypted drafts");
    assert!(
        !bytes
            .windows(plaintext.len())
            .any(|window| window == plaintext.as_bytes())
    );

    let loaded = actor
        .load_composer_drafts(&key_id)
        .expect("load encrypted drafts");
    assert_eq!(
        loaded
            .rooms
            .get("!room:test.example.com")
            .map(koushi_state::ComposerDocument::plain_body),
        Some(plaintext.to_owned())
    );
    assert_eq!(loaded.room_revision("!sent:test.example.com"), 8.into());
    assert!(!loaded.rooms.contains_key("!sent:test.example.com"));
    assert_eq!(
        loaded.thread_revision("!room:test.example.com", "$root:test.example.com"),
        12.into()
    );
    assert!(
        loaded
            .threads
            .get("!room:test.example.com")
            .and_then(|threads| threads.get("$root:test.example.com"))
            .is_none()
    );

    let mut corrupted = bytes;
    let last = corrupted.last_mut().expect("non-empty encrypted payload");
    *last ^= 0x01;
    std::fs::write(&path, corrupted).expect("write corrupted drafts");
    assert!(matches!(
        actor.load_composer_drafts(&key_id),
        Err(CoreFailure::StoreUnavailable)
    ));
}

#[test]
fn composer_draft_store_creates_missing_parent() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let mut drafts = ComposerDraftStore::default();
    drafts
        .apply_room_draft(
            "!room:test.example.com".to_owned(),
            "synthetic draft".to_owned(),
            1.into(),
        )
        .expect("seed draft");
    let path = actor.account_composer_drafts_file(&key_id);
    assert!(!path.parent().expect("draft parent").exists());

    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
        .expect("save encrypted drafts");

    assert!(path.is_file());
}

#[test]
fn failed_composer_draft_atomic_replace_preserves_previous_payload_exactly() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);

    let mut old = ComposerDraftStore::default();
    old.apply_room_draft(
        "!room:test.example.com".to_owned(),
        "old synthetic draft".to_owned(),
        1.into(),
    )
    .expect("seed old draft");
    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&old))
        .expect("save old encrypted payload");
    let path = actor.account_composer_drafts_file(&key_id);
    let old_payload = std::fs::read(&path).expect("read old encrypted payload");

    let mut new = old.clone();
    new.apply_room_draft(
        "!room:test.example.com".to_owned(),
        "new synthetic draft".to_owned(),
        2.into(),
    )
    .expect("seed new draft");
    actor.fail_next_composer_draft_replace_for_testing();
    assert!(matches!(
        actor.save_composer_drafts(&key_id, &persisted_composer_drafts(&new)),
        Err(CoreFailure::StoreUnavailable)
    ));
    assert!(
        std::fs::read(&path).is_ok_and(|payload| payload == old_payload),
        "a failed replacement must leave the previous encrypted payload byte-exact"
    );
    assert_eq!(
        actor
            .load_composer_drafts(&key_id)
            .expect("old encrypted payload remains readable"),
        old
    );
}

#[test]
fn two_accounts_with_same_targets_migrate_and_collect_independently() {
    fn legacy_payload(label: &str, revision_base: u64) -> Vec<u8> {
        let room_revisions = (0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT)
            .map(|index| (format!("shared-{index:03}"), revision_base + index as u64))
            .collect::<std::collections::BTreeMap<_, _>>();
        serde_json::to_vec(&serde_json::json!({
                "rooms": {"shared-content": format!("{label}-body")},
                "threads": {"shared-content": {"root-shared": format!("{label}-thread")}},
                "room_revisions": room_revisions,
                "thread_revisions": {
                    "shared-content": {"root-shared": revision_base}
            }
        }))
        .expect("serialize legacy fixture")
    }

    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_a = make_key_id();
    let mut key_b = make_key_id();
    key_b.user_id = "@bob:test.example.com".to_owned();
    key_b.device_id = "DEVICE2".to_owned();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let path_a = actor.account_composer_drafts_file(&key_a);
    let path_b = actor.account_composer_drafts_file(&key_b);
    assert_ne!(path_a, path_b);

    for (key_id, path, payload) in [
        (&key_a, &path_a, legacy_payload("account-a", 10)),
        (&key_b, &path_b, legacy_payload("account-b", 1_000)),
    ] {
        let secret = actor
            .load_or_create_unlock_secret(key_id)
            .expect("seed account-local unlock secret");
        let encrypted = encrypt_composer_drafts_fixture_payload(&secret, &payload)
            .expect("encrypt legacy fixture");
        std::fs::create_dir_all(path.parent().expect("composer draft parent"))
            .expect("create composer draft parent");
        std::fs::write(path, encrypted).expect("write legacy encrypted payload");
    }

    let mut account_a = actor
        .load_composer_drafts(&key_a)
        .expect("migrate account A");
    let account_b = actor
        .load_composer_drafts(&key_b)
        .expect("migrate account B");
    assert_eq!(
        account_a
            .composer_for_thread("shared-content", "root-shared")
            .draft,
        "account-a-thread"
    );
    assert_eq!(
        account_b
            .composer_for_thread("shared-content", "root-shared")
            .draft,
        "account-b-thread"
    );

    actor
        .save_composer_drafts(&key_a, &persisted_composer_drafts(&account_a))
        .expect("write account A v2");
    actor
        .save_composer_drafts(&key_b, &persisted_composer_drafts(&account_b))
        .expect("write account B v2");

    assert!(
        account_a
            .apply_room_draft("zz-new".to_owned(), String::new(), 1.into())
            .expect("collect account A independently")
    );
    actor
        .save_composer_drafts(&key_a, &persisted_composer_drafts(&account_a))
        .expect("write collected account A v2");

    let account_a = actor
        .load_composer_drafts(&key_a)
        .expect("reload collected account A");
    let account_b = actor
        .load_composer_drafts(&key_b)
        .expect("reload untouched account B");
    assert!(account_a.room_revision("shared-000").is_zero());
    assert_eq!(account_a.room_revision("zz-new"), 1.into());
    assert_eq!(account_b.room_revision("shared-000"), 1_000.into());
    assert!(account_b.room_revision("zz-new").is_zero());
    assert_eq!(
        account_b.composer_for_room("shared-content").draft,
        "account-b-body"
    );
}

#[test]
fn legacy_composer_draft_payload_defaults_causal_revisions() {
    let legacy = r#"{
            "rooms":{"!room:test.example.com":"legacy room draft"},
            "threads":{"!room:test.example.com":{"$root:test.example.com":"legacy thread draft"}}
        }"#;

    let loaded: ComposerDraftStore =
        serde_json::from_str(legacy).expect("deserialize legacy draft payload");

    assert_eq!(loaded.room_revision("!room:test.example.com"), 0.into());
    assert_eq!(
        loaded.thread_revision("!room:test.example.com", "$root:test.example.com"),
        0.into()
    );
    assert_eq!(
        loaded.composer_for_room("!room:test.example.com").draft,
        "legacy room draft"
    );
    assert_eq!(
        loaded
            .composer_for_thread("!room:test.example.com", "$root:test.example.com")
            .draft,
        "legacy thread draft"
    );
}

#[test]
fn loading_composer_drafts_does_not_create_missing_credentials() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let path = actor.account_composer_drafts_file(&key_id);
    std::fs::create_dir_all(path.parent().expect("draft parent")).expect("create parent");
    std::fs::write(&path, COMPOSER_DRAFTS_FILE_MAGIC).expect("write draft placeholder");

    assert!(matches!(
        actor.load_composer_drafts(&key_id),
        Err(CoreFailure::LocalEncryptionUnavailable)
    ));
    let missing = actor.credential_backend().load(&key_id).unwrap_err();
    assert!(koushi_key::is_missing_credential_error(&missing));
}

#[test]
fn empty_composer_drafts_remove_persisted_file() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let mut drafts = ComposerDraftStore::default();
    drafts.set_room_draft("!room:test.example.com".to_owned(), "draft".to_owned());

    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
        .expect("save non-empty drafts");
    let path = actor.account_composer_drafts_file(&key_id);
    assert!(path.exists());

    actor
        .save_composer_drafts(
            &key_id,
            &persisted_composer_drafts(&ComposerDraftStore::default()),
        )
        .expect("save empty drafts");
    assert!(!path.exists());
    assert!(
        actor
            .load_composer_drafts(&key_id)
            .expect("load removed drafts")
            .is_empty()
    );
}

#[test]
fn composer_draft_persistence_keeps_content_and_applies_size_bounds() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let mut drafts = ComposerDraftStore::default();
    let oversized = "x".repeat(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES + 64);

    for index in 0..(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT + 8) {
        drafts.set_room_draft(format!("!room-{index}:test.example.com"), oversized.clone());
    }
    for index in 0..(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT + 8) {
        drafts.set_thread_draft(
            "!thread-room:test.example.com".to_owned(),
            format!("$root-{index}"),
            oversized.clone(),
        );
    }

    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
        .expect("save bounded drafts");
    let loaded = actor
        .load_composer_drafts(&key_id)
        .expect("load bounded drafts");

    assert_eq!(
        loaded.rooms.len(),
        koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT + 8
    );
    assert!(
        loaded
            .rooms
            .values()
            .all(|draft| draft.len() <= koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES)
    );
    let thread_count = loaded
        .threads
        .values()
        .map(std::collections::BTreeMap::len)
        .sum::<usize>();
    assert_eq!(
        thread_count,
        koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT + 8
    );
    assert!(
        loaded
            .threads
            .values()
            .flat_map(|room_threads| room_threads.values())
            .all(|draft| draft.len() <= koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES)
    );
}

#[test]
fn composer_draft_persistence_prioritizes_content_over_revision_tombstones() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let mut drafts = ComposerDraftStore::default();

    for index in 0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT {
        assert!(
            drafts
                .apply_room_draft(
                    format!("!a-tombstone-{index:04}:test.example.com"),
                    String::new(),
                    1.into(),
                )
                .expect("room tombstone should apply")
        );
    }
    for index in 0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT {
        assert!(
            drafts
                .apply_thread_draft(
                    "!thread-room:test.example.com".to_owned(),
                    format!("$a-tombstone-{index:04}"),
                    String::new(),
                    1.into(),
                )
                .expect("thread tombstone should apply")
        );
    }
    drafts.set_room_draft(
        "!z-active:test.example.com".to_owned(),
        "active room draft".to_owned(),
    );
    drafts.set_thread_draft(
        "!thread-room:test.example.com".to_owned(),
        "$z-active".to_owned(),
        "active thread draft".to_owned(),
    );

    actor
        .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
        .expect("save bounded drafts");
    let loaded = actor
        .load_composer_drafts(&key_id)
        .expect("load bounded drafts");

    assert_eq!(
        loaded
            .rooms
            .get("!z-active:test.example.com")
            .map(koushi_state::ComposerDocument::plain_body),
        Some("active room draft".to_owned())
    );
    assert_eq!(
        loaded
            .threads
            .get("!thread-room:test.example.com")
            .and_then(|threads| threads.get("$z-active"))
            .map(koushi_state::ComposerDocument::plain_body),
        Some("active thread draft".to_owned())
    );
}
