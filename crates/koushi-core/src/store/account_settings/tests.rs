use super::super::test_support::{file_store_actor, make_key_id};
use koushi_state::AccountSettingsValues;
use tempfile::tempdir;

#[test]
fn account_settings_are_encrypted_and_legacy_seed_is_retry_safe() {
    let data_dir = tempdir().expect("data dir");
    let credential_dir = tempdir().expect("credential dir");
    let key_id = make_key_id();
    let store = file_store_actor(&data_dir, &credential_dir);

    let mut legacy = AccountSettingsValues::default();
    legacy.notifications.desktop_notifications = false;
    legacy.notifications.message_previews = true;
    legacy.display.encrypted_url_previews_enabled = true;
    legacy.sidebar.scope_preferences.insert(
        "!private-space:test.example.com".to_owned(),
        Default::default(),
    );
    store
        .seed_account_settings_from_legacy(&key_id, &legacy)
        .expect("seed legacy settings");

    let path = store.account_settings_file(&key_id);
    assert!(path.ends_with("settings/account-settings.v1.enc"));
    let encrypted = std::fs::read(&path).expect("read encrypted settings");
    assert!(
        !encrypted
            .windows("!private-space:test.example.com".len())
            .any(|window| window == b"!private-space:test.example.com")
    );
    assert!(
        store
            .load_account_settings(&key_id)
            .expect("load account settings")
            == legacy
    );

    let mut changed = AccountSettingsValues::default();
    changed.notifications.desktop_notifications = true;
    store
        .save_account_settings(&key_id, &changed)
        .expect("save updated settings");
    store
        .seed_account_settings_from_legacy(&key_id, &legacy)
        .expect("retry migration");
    assert!(
        store
            .load_account_settings(&key_id)
            .expect("load migrated settings")
            == changed
    );

    let mut corrupted = encrypted;
    let last = corrupted.last_mut().expect("encrypted file bytes");
    *last ^= 1;
    std::fs::write(path, corrupted).expect("corrupt account settings");
    assert!(store.load_account_settings(&key_id).is_err());
}

#[test]
fn notification_migration_seeds_only_sound_and_badges_once() {
    use super::{ACCOUNT_SETTINGS_FILE_MAGIC, AccountSettingsFile};
    let data = tempdir().expect("data");
    let credentials = tempdir().expect("credentials");
    let key_id = make_key_id();
    let store = file_store_actor(&data, &credentials);
    let mut values = AccountSettingsValues::default();
    values.notifications.message_previews = true;
    values.notifications.desktop_notifications = false;
    let old_file = AccountSettingsFile {
        version: 1,
        legacy_migration_complete: true,
        values,
    };
    let secret = store
        .load_or_create_unlock_secret(&key_id)
        .expect("unlock secret");
    let payload = koushi_store::encrypt_envelope(
        ACCOUNT_SETTINGS_FILE_MAGIC,
        secret.derive_account_settings_key().as_bytes(),
        &serde_json::to_vec(&old_file).expect("old account json"),
        usize::MAX,
    )
    .expect("old encrypted account");
    let path = store.account_settings_file(&key_id);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create settings");
    std::fs::write(&path, payload).expect("write old account");
    store
        .migrate_account_notifications(&key_id, Some((false, false)))
        .expect("migrate");
    let mut migrated = store.load_account_settings(&key_id).expect("load migrated");
    assert!(!migrated.notifications.sound && !migrated.notifications.badges);
    assert!(migrated.notifications.message_previews);
    assert!(!migrated.notifications.desktop_notifications);
    assert!(migrated.delivery.sinks.is_empty());
    migrated.notifications.sound = true;
    store
        .save_account_settings(&key_id, &migrated)
        .expect("account edit");
    store
        .migrate_account_notifications(&key_id, Some((false, false)))
        .expect("retry");
    assert!(
        store
            .load_account_settings(&key_id)
            .expect("retry load")
            .notifications
            .sound
    );
}
