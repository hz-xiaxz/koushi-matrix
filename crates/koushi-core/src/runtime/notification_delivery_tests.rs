//! #1427: the delivery channel as the runtime actually wires it.
//!
//! These tests drive the real `CoreRuntime` over an isolated data directory and a file
//! credential store: the command path, the credential seam, the actor's own observation, and
//! the send that leaves through a stubbed relay. No test touches the OS keychain, and the only
//! socket any of them opens is a wiremock stub on the loopback interface.

use std::time::Duration;

use koushi_protocol::command::{AppCommand, CoreCommand};
use koushi_state::{
    NotificationDeliveryOutcomeState, NotificationDeliverySettings, NotificationSinkConfig,
    NotificationSinkSettings, SettingsPatch,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::tests::wait_for_runtime_snapshot;
use super::{CoreRuntime, IsolatedStores};

/// The one sink every test configures.
const SINK: &str = "qq-mail";
/// The fixture credential. It never reaches a log, a snapshot, or a file this repo keeps.
const SECRET: &str = "fixture-relay-secret";
const ROOM: &str = "!room:example.invalid";

const TEST_LIVENESS: Duration = Duration::from_secs(60);

fn delivery_settings(endpoint: &str) -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        enabled: true,
        only_when_unfocused: false,
        dwell_secs: 60,
        min_interval_secs: 900,
        sinks: vec![NotificationSinkSettings {
            id: SINK.to_owned(),
            enabled: true,
            destination: "fixture@example.invalid".to_owned(),
            from_address: "koushi@example.invalid".to_owned(),
            sender_name: "Koushi".to_owned(),
            config: NotificationSinkConfig::Resend {
                endpoint: endpoint.to_owned(),
            },
            secret_binding: None,
        }],
        ..NotificationDeliverySettings::default()
    }
}

/// A runtime whose settings already carry one complete, enabled sink.
async fn runtime_with_sink(endpoint: &str) -> (CoreRuntime, IsolatedStores, super::CoreConnection) {
    runtime_with_delivery(delivery_settings(endpoint)).await
}

/// Put one delivery block into the app-scoped settings through the real command path, and wait
/// for the projection the actor's own observation publishes for it.
async fn put_delivery_settings(
    connection: &mut super::CoreConnection,
    delivery: NotificationDeliverySettings,
) {
    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(AppCommand::UpdateSettings {
            request_id,
            patch: SettingsPatch {
                delivery: Some(delivery),
                ..SettingsPatch::default()
            },
        }))
        .await
        .expect("the settings command is admitted");
}

async fn runtime_with_delivery(
    delivery: NotificationDeliverySettings,
) -> (CoreRuntime, IsolatedStores, super::CoreConnection) {
    let (runtime, stores) = CoreRuntime::start_isolated_with_event_capacity(64);
    let mut connection = runtime.attach();
    put_delivery_settings(&mut connection, delivery).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        !state.notification_delivery.sinks.is_empty()
    })
    .await;
    (runtime, stores, connection)
}

/// The binding the settings file records for the fixture sink, as a test can read it back.
fn settings_file_has_binding(stores: &IsolatedStores, binding: &str) -> bool {
    let path = stores.data_dir().join("settings").join("settings.json");
    std::fs::read_to_string(path)
        .map(|json| json.contains(binding))
        .unwrap_or(false)
}

/// Wait until the settings file names this binding, which the persist effect writes after the
/// state has already changed.
async fn wait_for_settings_binding(stores: &IsolatedStores, binding: &str) {
    tokio::time::timeout(TEST_LIVENESS, async {
        loop {
            if settings_file_has_binding(stores, binding) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("configuring a secret persists the binding it was entered for");
}

/// Wait for the operation failure that answers one request id.
///
/// The delivery commands settle through the ordinary operation-failure event, and their
/// expectation is not one of the curated feature outcomes the shared waiter knows.
async fn wait_for_failure(
    connection: &mut super::CoreConnection,
    request_id: koushi_protocol::RequestId,
) -> koushi_protocol::CoreFailure {
    tokio::time::timeout(TEST_LIVENESS, async {
        loop {
            match connection.recv_event().await {
                Ok(koushi_protocol::CoreEvent::OperationFailed {
                    request_id: failed,
                    failure,
                }) if failed == request_id => return failure,
                // Any other event, and a lag that dropped some, is not this answer.
                Ok(_) | Err(_) => continue,
            }
        }
    })
    .await
    .expect("the refusal settles")
}

/// Wait for the settings file the injected patch is persisted to, and return its bytes.
///
/// The patch is persisted by an effect, so the file can appear after the projection it caused.
async fn wait_for_settings_file(path: &std::path::Path) -> Vec<u8> {
    tokio::time::timeout(TEST_LIVENESS, async {
        loop {
            if let Ok(bytes) = std::fs::read(path) {
                return bytes;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the settings file is written")
}

fn sink_state(connection: &super::CoreConnection) -> koushi_state::NotificationDeliverySinkState {
    connection
        .snapshot()
        .notification_delivery
        .sinks
        .into_iter()
        .next()
        .expect("the fixture settings carry one sink")
}

async fn set_secret(connection: &super::CoreConnection) {
    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(
            AppCommand::SetNotificationDeliverySecret {
                request_id,
                sink_id: SINK.to_owned(),
                secret: SECRET.to_owned(),
            },
        ))
        .await
        .expect("the delivery command is admitted");
}

/// A store over the same isolated credential directory the runtime was given, so a test can
/// read back what the command wrote without going through the app.
fn credential_store(stores: &IsolatedStores) -> crate::store::StoreActor {
    crate::store::StoreActor::with_backend(
        koushi_store::CredentialStoreBackend::FileDir(koushi_store::FileCredentialStore::new(
            stores.credential_dir().to_path_buf(),
        )),
        stores.data_dir().to_path_buf(),
    )
}

#[tokio::test]
async fn the_secret_command_round_trips_and_flips_configured() {
    let (runtime, stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    assert!(!sink_state(&connection).secret_configured);
    assert_eq!(sink_state(&connection).last_outcome, None);
    assert!(
        !sink_state(&connection).secret_retired,
        "a sink that never had a credential has nothing to explain"
    );

    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;

    let store = credential_store(&stores);
    let stored = store
        .load_notification_sink_secret(SINK)
        .expect("the credential store is readable")
        .expect("the command stored the secret");
    assert_eq!(stored.as_str(), SECRET);

    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(
            AppCommand::ClearNotificationDeliverySecret {
                request_id,
                sink_id: SINK.to_owned(),
            },
        ))
        .await
        .expect("the clear command is admitted");
    wait_for_runtime_snapshot(&mut connection, |state| {
        !state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    assert!(
        store
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_none()
    );
    assert!(
        !sink_state(&connection).secret_retired,
        "clearing a password the user chose to clear is not a retirement to explain"
    );
    drop(runtime);
}

#[tokio::test]
async fn an_unusable_sink_id_is_refused_without_touching_the_store() {
    let (runtime, stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    // A sink the settings do not hold has no credential to write and no sink to configure.
    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(
            AppCommand::SetNotificationDeliverySecret {
                request_id,
                sink_id: "Not A Sink Id".to_owned(),
                secret: SECRET.to_owned(),
            },
        ))
        .await
        .expect("the command is admitted and fails as an operation failure");
    let failure = wait_for_failure(&mut connection, request_id).await;
    assert!(
        matches!(
            failure,
            koushi_protocol::CoreFailure::NotificationDeliveryRejected { .. }
        ),
        "{failure:?}"
    );
    assert!(
        credential_store(&stores)
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_none()
    );
    assert!(
        !connection
            .snapshot()
            .settings
            .values
            .delivery
            .sinks
            .is_empty()
    );
    drop(runtime);
}

#[tokio::test]
async fn the_secret_reaches_no_snapshot_no_debug_and_no_settings_file() {
    let (runtime, stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    let settings_path = stores.data_dir().join("settings").join("settings.json");
    let without_binding = wait_for_settings_file(&settings_path).await;

    let command = AppCommand::SetNotificationDeliverySecret {
        request_id: connection.next_request_id(),
        sink_id: SINK.to_owned(),
        secret: SECRET.to_owned(),
    };
    let debug = format!("{command:?}");
    assert!(
        !debug.contains(SECRET),
        "the command Debug prints redacted: {debug}"
    );
    assert!(debug.contains("NotificationSinkSecret(<redacted>)"));

    connection
        .command(CoreCommand::App(command))
        .await
        .expect("the delivery command is admitted");
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;

    let app_state = serde_json::to_string(&connection.snapshot()).expect("app state serializes");
    assert!(
        !app_state.contains(SECRET),
        "the app state carries no secret"
    );
    let delivery_projection =
        serde_json::to_string(&connection.snapshot().notification_delivery).expect("projection");
    assert!(!delivery_projection.contains(SECRET));

    // What the settings file gains is the binding - the server and account the credential was
    // entered for - and never the value. The frontend never supplies the binding, so a file that
    // carries one is a file the core wrote.
    let binding = connection.snapshot().settings.values.delivery.sinks[0].credential_identity();
    let configured_once = wait_for_settings_file(&settings_path).await;
    assert_eq!(binding, "resend:https://relay.example.invalid/emails");
    assert!(settings_file_has_binding(&stores, &binding));
    assert!(
        !configured_once
            .windows(SECRET.len())
            .any(|window| window == SECRET.as_bytes())
    );

    // Two different credentials for one configuration are indistinguishable in the settings, and
    // clearing the secret takes the settings back to exactly what they were before it.
    let cleared = {
        let request_id = connection.next_request_id();
        connection
            .command(CoreCommand::App(
                AppCommand::ClearNotificationDeliverySecret {
                    request_id,
                    sink_id: SINK.to_owned(),
                },
            ))
            .await
            .expect("the clear command is admitted");
        wait_for_runtime_snapshot(&mut connection, |state| {
            !state.notification_delivery.sinks[0].secret_configured
        })
        .await;
        std::fs::read(&settings_path).expect("the settings file is still there")
    };
    assert_eq!(
        without_binding, cleared,
        "clearing the secret restores the settings byte for byte"
    );

    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    assert_eq!(
        configured_once,
        std::fs::read(&settings_path).expect("the settings file is still there"),
        "the settings depend on the configuration, not on the credential value"
    );

    // The outcome state is a verdict with no value of its own, and it survives the projection.
    let outcome = NotificationDeliveryOutcomeState::InvalidSecret { at_ms: 5 };
    let outcome_debug = format!("{outcome:?}");
    assert!(!outcome_debug.contains(SECRET));
    assert!(
        !serde_json::to_string(&outcome)
            .expect("outcome")
            .contains(SECRET)
    );
    drop(runtime);
}

#[tokio::test]
async fn changing_the_recipient_or_the_switch_keeps_the_credential() {
    let (runtime, stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;

    // Who receives the mail, what the sender is called and whether the sink is switched on are
    // not what the credential authenticates, so none of them may throw the password away. The
    // block is the one the state holds, with the binding carried back untouched, which is what
    // the settings form sends.
    let mut cosmetic = *connection.snapshot().settings.values.delivery;
    assert!(cosmetic.sinks[0].secret_binding.is_some());
    cosmetic.sinks[0].destination = "other@example.invalid".to_owned();
    cosmetic.sinks[0].from_address = "other@example.invalid".to_owned();
    cosmetic.sinks[0].sender_name = "Someone Else".to_owned();
    cosmetic.sinks[0].enabled = false;
    put_delivery_settings(&mut connection, cosmetic).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        !state.settings.values.delivery.sinks[0].enabled
    })
    .await;
    assert!(sink_state(&connection).secret_configured);
    assert!(
        !sink_state(&connection).secret_retired,
        "a cosmetic edit retires nothing"
    );
    assert!(
        credential_store(&stores)
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_some(),
        "a cosmetic edit must not delete the credential"
    );

    // Switching it back on is the same kind of edit.
    let mut switched_on = *connection.snapshot().settings.values.delivery;
    switched_on.sinks[0].enabled = true;
    put_delivery_settings(&mut connection, switched_on).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.settings.values.delivery.sinks[0].enabled
    })
    .await;
    assert!(sink_state(&connection).secret_configured);
    drop(runtime);
}

#[tokio::test]
async fn changing_the_server_retires_the_credential_and_flips_configured() {
    // The form fills the preset, host, port and account together, so that is what a user picking
    // another provider does here.
    let mut smtp = delivery_settings("https://relay.example.invalid/emails");
    smtp.sinks[0].config = NotificationSinkConfig::Smtp {
        preset: Some("qq-mail".to_owned()),
        host: "smtp.qq.com".to_owned(),
        port: 465,
        tls: koushi_state::SmtpTls::Implicit,
        username: "fixture@qq.com".to_owned(),
    };
    let (runtime, stores, mut connection) = runtime_with_delivery(smtp.clone()).await;
    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    wait_for_settings_binding(&stores, "smtp:smtp.qq.com:465:fixture@qq.com").await;

    let mut moved = smtp.clone();
    moved.sinks[0].config = NotificationSinkConfig::Smtp {
        preset: Some("gmail".to_owned()),
        host: "smtp.gmail.com".to_owned(),
        port: 465,
        tls: koushi_state::SmtpTls::Implicit,
        username: "fixture@qq.com".to_owned(),
    };
    put_delivery_settings(&mut connection, moved).await;

    // The binding no longer names this configuration, so the credential is not this sink's, the
    // sink reads as unconfigured, and the record is gone rather than waiting to be offered to the
    // wrong server.
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        !state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    // The user never typed the identity, so the UI cannot work out that the password went away;
    // the projection has to say it.
    assert!(state.notification_delivery.sinks[0].secret_retired);
    assert!(
        credential_store(&stores)
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_none(),
        "a credential entered for another server is deleted, not kept"
    );
    assert!(!settings_file_has_binding(
        &stores,
        "smtp:smtp.qq.com:465:fixture@qq.com"
    ));

    // Saving a password for the configuration the sink names now is the user acting on that, so
    // the explanation stops being shown.
    set_secret(&connection).await;
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    assert!(!state.notification_delivery.sinks[0].secret_retired);
    drop(runtime);
}

#[tokio::test]
async fn a_stored_credential_with_no_binding_is_deleted_and_never_used() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (runtime, stores) = CoreRuntime::start_isolated_with_event_capacity(64);
    let mut connection = runtime.attach();
    // A credential that nothing in the settings names, as a hand-edited or half-restored profile
    // leaves behind.
    credential_store(&stores)
        .save_notification_sink_secret(SINK, SECRET)
        .expect("the fixture credential is stored");

    put_delivery_settings(
        &mut connection,
        delivery_settings(&format!("{}/emails", server.uri())),
    )
    .await;
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        !state.notification_delivery.sinks.is_empty()
            && !state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    assert_eq!(state.notification_delivery.sinks[0].last_outcome, None);
    assert!(
        state.notification_delivery.sinks[0].secret_retired,
        "the record was there and the core removed it, so the user is owed an explanation"
    );
    assert!(
        !state.notification_delivery.sinks[0].secret_configured,
        "and the sink is not configured"
    );
    assert!(
        credential_store(&stores)
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_none(),
        "a credential no binding names belongs to no destination this sink has"
    );

    // Nothing may be attempted with it either: a test send is refused by kind, and no request ever
    // reaches the relay.
    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(AppCommand::TestNotificationDelivery {
            request_id,
            sink_id: SINK.to_owned(),
        }))
        .await
        .expect("the test send is admitted");
    let failure = wait_for_failure(&mut connection, request_id).await;
    assert!(
        matches!(
            failure,
            koushi_protocol::CoreFailure::NotificationDeliveryRejected {
                kind: koushi_protocol::NotificationDeliveryFailureKind::SecretMissing
            }
        ),
        "{failure:?}"
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the stub keeps requests")
            .len(),
        0
    );
    drop(runtime);
}

#[tokio::test]
async fn a_settings_file_whose_binding_moved_comes_up_unconfigured() {
    let (runtime, stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    let binding = "resend:https://relay.example.invalid/emails";
    wait_for_settings_binding(&stores, binding).await;
    drop(runtime);

    // The settings file is editable while the app is closed, so a binding that no longer names
    // this configuration must fail closed on the next start rather than be trusted.
    let settings_path = stores.data_dir().join("settings").join("settings.json");
    let edited = std::fs::read_to_string(&settings_path)
        .expect("the settings file is there")
        .replace(binding, "resend:https://elsewhere.example.invalid/emails");
    std::fs::write(&settings_path, edited).expect("the edited settings file is written");

    let runtime = CoreRuntime::restart_isolated(&stores);
    let mut connection = runtime.attach();
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        !state.notification_delivery.sinks.is_empty()
            && !state.notification_delivery.sinks[0].secret_configured
    })
    .await;
    // No edit happened in this run - the file said something that could not be trusted - so the
    // flag is the only way the frontend can tell the user their password is gone.
    assert!(state.notification_delivery.sinks[0].secret_retired);
    assert!(!state.notification_delivery.sinks[0].secret_configured);
    assert!(
        credential_store(&stores)
            .load_notification_sink_secret(SINK)
            .expect("the credential store is readable")
            .is_none(),
        "the credential loaded beside a binding that moved belongs to another server"
    );
    drop(runtime);
}

#[tokio::test]
async fn a_test_send_delivers_through_the_sink_and_records_the_outcome() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "ok"})))
        .expect(1)
        .mount(&server)
        .await;
    let endpoint = format!("{}/emails", server.uri());
    let (runtime, _stores, mut connection) = runtime_with_sink(&endpoint).await;
    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;

    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(AppCommand::TestNotificationDelivery {
            request_id,
            sink_id: SINK.to_owned(),
        }))
        .await
        .expect("the test send is admitted");
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].last_outcome.is_some()
    })
    .await;
    assert!(matches!(
        state.notification_delivery.sinks[0].last_outcome,
        Some(NotificationDeliveryOutcomeState::Delivered { at_ms }) if at_ms > 0
    ));

    let requests = server
        .received_requests()
        .await
        .expect("the stub keeps requests");
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("the request body is JSON");
    let text = body["text"].as_str().expect("the body carries text");
    // The canned message says it is a test and carries no room, event, or message data.
    assert!(text.to_ascii_lowercase().contains("test"), "{text}");
    assert!(!text.contains(ROOM));
    // A test send consumes no coalescing window, so the real delivery is still to come.
    assert!(
        connection
            .snapshot()
            .notification_delivery
            .sinks
            .iter()
            .all(|sink| sink.secret_configured)
    );
    drop(runtime);
}

#[tokio::test]
async fn a_rejected_secret_is_typed_and_never_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let endpoint = format!("{}/emails", server.uri());
    let (runtime, _stores, mut connection) = runtime_with_sink(&endpoint).await;
    set_secret(&connection).await;
    wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].secret_configured
    })
    .await;

    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(AppCommand::TestNotificationDelivery {
            request_id,
            sink_id: SINK.to_owned(),
        }))
        .await
        .expect("the test send is admitted");
    let state = wait_for_runtime_snapshot(&mut connection, |state| {
        state.notification_delivery.sinks[0].last_outcome.is_some()
    })
    .await;
    assert!(matches!(
        state.notification_delivery.sinks[0].last_outcome,
        Some(NotificationDeliveryOutcomeState::InvalidSecret { .. })
    ));
    // The sink still reads as configured, so the product can ask for a new credential; what it
    // must not do is try the same rejected value again.
    assert!(state.notification_delivery.sinks[0].secret_configured);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the stub keeps requests")
            .len(),
        1,
        "an invalid credential is a terminal verdict"
    );
    drop(runtime);
}

#[tokio::test]
async fn a_test_send_for_an_unconfigured_sink_is_refused_by_kind() {
    let (runtime, _stores, mut connection) =
        runtime_with_sink("https://relay.example.invalid/emails").await;
    let request_id = connection.next_request_id();
    connection
        .command(CoreCommand::App(AppCommand::TestNotificationDelivery {
            request_id,
            sink_id: SINK.to_owned(),
        }))
        .await
        .expect("the test send is admitted");
    let failure = wait_for_failure(&mut connection, request_id).await;
    assert!(
        matches!(
            failure,
            koushi_protocol::CoreFailure::NotificationDeliveryRejected { .. }
        ),
        "{failure:?}"
    );
    assert_eq!(sink_state(&connection).last_outcome, None);
    drop(runtime);
}
