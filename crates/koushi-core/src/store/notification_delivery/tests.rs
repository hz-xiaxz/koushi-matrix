//! #1427: the delivery credential seam. Every test below runs on the file credential
//! store in a temp directory, so no test can reach the OS keychain.

use super::*;
use crate::store::test_support::file_store_actor;

const SINK: &str = "qq-mail";
const SECRET: &str = "fixture-authorization-code";

fn actor() -> (StoreActor, tempfile::TempDir, tempfile::TempDir) {
    let data_dir = tempfile::tempdir().expect("data dir");
    let cred_dir = tempfile::tempdir().expect("credential dir");
    let actor = file_store_actor(&data_dir, &cred_dir);
    (actor, data_dir, cred_dir)
}

#[test]
fn secret_round_trips_through_the_file_store() {
    let (actor, _data, _cred) = actor();
    assert!(
        !actor
            .notification_sink_secret_configured(&notification_key(), SINK)
            .expect("configured probe")
    );

    actor
        .save_notification_sink_secret(&notification_key(), SINK, SECRET)
        .expect("save");

    assert!(
        actor
            .notification_sink_secret_configured(&notification_key(), SINK)
            .expect("configured probe")
    );
    let loaded = actor
        .load_notification_sink_secret(&notification_key(), SINK)
        .expect("load")
        .expect("a stored secret");
    assert_eq!(loaded.as_str(), SECRET);
    // The one shape that must never appear in a log line.
    assert_eq!(format!("{loaded:?}"), "NotificationSinkSecret(<redacted>)");
    assert!(!format!("{loaded:?}").contains(SECRET));

    actor
        .delete_notification_sink_secret(&notification_key(), SINK)
        .expect("delete");
    assert!(
        !actor
            .notification_sink_secret_configured(&notification_key(), SINK)
            .expect("configured probe")
    );
    assert!(
        actor
            .load_notification_sink_secret(&notification_key(), SINK)
            .expect("load")
            .is_none()
    );
}

#[test]
fn forgetting_a_secret_that_is_not_stored_succeeds() {
    let (actor, _data, _cred) = actor();
    actor
        .delete_notification_sink_secret(&notification_key(), SINK)
        .expect("deleting an absent secret is not a failure");
}

#[test]
fn an_unusable_id_or_secret_is_typed_not_a_panic() {
    let (actor, _data, _cred) = actor();
    for sink_id in ["", "QQ-Mail", "sink with spaces", "sink/../escape"] {
        assert_eq!(
            actor.save_notification_sink_secret(&notification_key(), sink_id, SECRET),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
        assert_eq!(
            actor.notification_sink_secret_configured(&notification_key(), sink_id),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
        assert_eq!(
            actor.delete_notification_sink_secret(&notification_key(), sink_id),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
    }
    assert_eq!(
        actor.save_notification_sink_secret(&notification_key(), SINK, ""),
        Err(NotificationDeliverySecretError::InvalidSecret)
    );
    assert_eq!(
        actor.save_notification_sink_secret(&notification_key(), SINK, "line\nbreak"),
        Err(NotificationDeliverySecretError::InvalidSecret)
    );
    // A refused write leaves nothing behind.
    assert!(
        !actor
            .notification_sink_secret_configured(&notification_key(), SINK)
            .expect("configured probe")
    );
}

fn notification_key() -> koushi_protocol::SessionKeyId {
    koushi_protocol::SessionKeyId {
        homeserver: "https://example.invalid".into(),
        user_id: "@me:example.invalid".into(),
        device_id: "DEVICE".into(),
    }
}

#[test]
fn same_sink_id_cannot_read_replace_or_delete_another_accounts_secret() {
    let (actor, _data, _cred) = actor();
    let owner = notification_key();
    actor
        .save_notification_sink_secret(&owner, SINK, SECRET)
        .expect("owner save");
    for other in [
        koushi_protocol::SessionKeyId {
            user_id: "@other:example.invalid".into(),
            ..owner.clone()
        },
        koushi_protocol::SessionKeyId {
            homeserver: "https://other.invalid".into(),
            ..owner.clone()
        },
        koushi_protocol::SessionKeyId {
            device_id: "OTHER".into(),
            ..owner.clone()
        },
    ] {
        assert!(
            actor
                .load_notification_sink_secret(&other, SINK)
                .expect("read other")
                .is_none()
        );
        actor
            .save_notification_sink_secret(&other, SINK, "other-fixture-secret")
            .expect("other save");
        actor
            .delete_notification_sink_secret(&other, SINK)
            .expect("other delete");
        assert_eq!(
            actor
                .load_notification_sink_secret(&owner, SINK)
                .expect("owner read")
                .expect("owner secret")
                .as_str(),
            SECRET
        );
    }
}

#[test]
fn deleting_an_account_removes_only_its_external_credentials() {
    let (actor, _data, _cred) = actor();
    let owner = notification_key();
    let other = SessionKeyId {
        user_id: "@other:example.invalid".into(),
        ..owner.clone()
    };
    let sink = koushi_state::NotificationSinkSettings {
        id: SINK.into(),
        destination: "fixture@example.invalid".into(),
        from_address: "sender@example.invalid".into(),
        ..Default::default()
    };
    let mut values = koushi_state::AccountSettingsValues::default();
    values.delivery.sinks.push(sink);
    actor
        .save_account_settings(&owner, &values)
        .expect("save account settings");
    actor
        .save_notification_sink_secret(&owner, SINK, SECRET)
        .expect("owner credential");
    actor
        .save_notification_sink_secret(&other, SINK, SECRET)
        .expect("other credential");
    actor
        .delete_account_credentials(&owner)
        .expect("delete account");
    assert!(
        actor
            .load_notification_sink_secret(&owner, SINK)
            .expect("deleted credential")
            .is_none()
    );
    assert!(
        actor
            .load_notification_sink_secret(&other, SINK)
            .expect("other credential")
            .is_some()
    );
}
