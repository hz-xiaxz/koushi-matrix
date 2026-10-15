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
            .notification_sink_secret_configured(SINK)
            .expect("configured probe")
    );

    actor
        .save_notification_sink_secret(SINK, SECRET)
        .expect("save");

    assert!(
        actor
            .notification_sink_secret_configured(SINK)
            .expect("configured probe")
    );
    let loaded = actor
        .load_notification_sink_secret(SINK)
        .expect("load")
        .expect("a stored secret");
    assert_eq!(loaded.as_str(), SECRET);
    // The one shape that must never appear in a log line.
    assert_eq!(format!("{loaded:?}"), "NotificationSinkSecret(<redacted>)");
    assert!(!format!("{loaded:?}").contains(SECRET));

    actor.delete_notification_sink_secret(SINK).expect("delete");
    assert!(
        !actor
            .notification_sink_secret_configured(SINK)
            .expect("configured probe")
    );
    assert!(
        actor
            .load_notification_sink_secret(SINK)
            .expect("load")
            .is_none()
    );
}

#[test]
fn forgetting_a_secret_that_is_not_stored_succeeds() {
    let (actor, _data, _cred) = actor();
    actor
        .delete_notification_sink_secret(SINK)
        .expect("deleting an absent secret is not a failure");
}

#[test]
fn an_unusable_id_or_secret_is_typed_not_a_panic() {
    let (actor, _data, _cred) = actor();
    for sink_id in ["", "QQ-Mail", "sink with spaces", "sink/../escape"] {
        assert_eq!(
            actor.save_notification_sink_secret(sink_id, SECRET),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
        assert_eq!(
            actor.notification_sink_secret_configured(sink_id),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
        assert_eq!(
            actor.delete_notification_sink_secret(sink_id),
            Err(NotificationDeliverySecretError::InvalidSinkId),
            "sink id {sink_id:?}"
        );
    }
    assert_eq!(
        actor.save_notification_sink_secret(SINK, ""),
        Err(NotificationDeliverySecretError::InvalidSecret)
    );
    assert_eq!(
        actor.save_notification_sink_secret(SINK, "line\nbreak"),
        Err(NotificationDeliverySecretError::InvalidSecret)
    );
    // A refused write leaves nothing behind.
    assert!(
        !actor
            .notification_sink_secret_configured(SINK)
            .expect("configured probe")
    );
}
