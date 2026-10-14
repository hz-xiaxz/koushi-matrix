//! #1427: device-local notification delivery, proved end to end against a
//! disposable local homeserver and a loopback stand-in for the provider.
//!
//! User A is the device under test; user B is the peer that produces the
//! attention. A configures exactly one Resend sink whose endpoint is a stub
//! listener this lane owns on `127.0.0.1`, writes the sink credential through the
//! app's own typed command, and never enables the channel until the phases that
//! assert it. The stub records only what the lane must assert: whether the
//! request carried the expected bearer, the idempotency key, and the request body.
//! It compares the bearer against the expected value without ever keeping it, so
//! the lane cannot leak the credential into its own artifacts.
//!
//! Every assertion is about what left the device and when:
//!
//! 1. the channel off produces no request at all, however long the room stays
//!    unread;
//! 2. with the channel on and a one second dwell, a direct message produces no
//!    request before the dwell and exactly one after, addressed to this lane's own
//!    stub, authorized by the sink credential, naming the room by the label the
//!    badge shows, and carrying none of the room id, the event id, the sender, or
//!    the message text;
//! 3. a read that lands inside the dwell cancels the delivery, and the same room
//!    made unread again delivers after a fresh dwell;
//! 4. a second delivery inside `min_interval_secs` is held, and is released - not
//!    dropped - when the window reopens;
//! 5. a room on the watch list delivers while its category is off, and a room that
//!    joins a watched Space after the list was written delivers too;
//! 6. the sink secret reaches neither the settings snapshot, nor the diagnostics
//!    stream, nor this lane's own captured requests.
//!
//! Private data: the lane prints tokens and counts only. Room ids, event ids,
//! account ids, room labels, and message bodies stay inside the process, and the
//! leak assertions are evaluated here so only a boolean reaches the transcript.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use koushi_state::{
    AppState, NotificationDeliverySettings, NotificationDeliveryTarget, NotificationSinkConfig,
    NotificationSinkSettings, SettingsPatch, SettingsPatchScope, room_activity_unread_count,
};

use super::cleanup::cleanup_logged_in_runtime;
use super::event_wait::{
    send_main_timeline_read_receipt, subscribe_timeline_for_qa, wait_for_dm_room_in_room_list,
    wait_for_invite_in_snapshot, wait_for_room_in_room_list, wait_for_space_in_space_list,
};
use super::fixtures::{
    accept_invite_for_qa, create_room_for_qa, create_space_for_qa, invite_user_for_qa,
    set_space_child_for_qa, start_direct_message_for_qa,
};
use super::participants::{
    QaParticipantLoginGate, QaParticipantLoginOutcome, login_synced_participant_for_qa, qa_data_dir,
};
use super::registry::{EVENT_TIMEOUT, QaConfig, ROOM_LIST_EVENT_TIMEOUT};
use super::scenario_read_state::send_text_and_wait_event;
use super::{AppCommand, CoreCommand, CoreConnection, RoomCommand, TimelineKey, TimelineKind};

/// The one sink this lane configures. Sink ids are the credential store's own
/// alphabet, so the id can also be a file name component inside the QA store.
const QA_DELIVERY_SINK_ID: &str = "qa-stub-sink";

/// The path the stub answers on. The product posts to whatever endpoint the
/// sink holds, so the path only has to be recognizable in a capture.
const QA_STUB_PATH: &str = "/qa/notification-delivery";

/// How long the lane watches for the request that must never arrive.
const QA_DELIVERY_ABSENCE_WINDOW: Duration = Duration::from_millis(300);

/// The dwell used by the assertions that want a delivery, in seconds.
const QA_DELIVERY_DWELL_SECS: u64 = 1;

/// The dwell used by the assertion that a read inside the dwell cancels one.
///
/// Long enough that the read receipt's round trip through the homeserver cannot
/// outlast it on a loaded machine, so a passing assertion proves cancellation
/// rather than a race.
const QA_DELIVERY_LONG_DWELL_SECS: u64 = 5;

/// A coalescing window far longer than the lane's own runtime, so anything that
/// arrives after the first delivery is provably held rather than late.
const QA_DELIVERY_MIN_INTERVAL_SECS: u64 = 300;

/// How many deliveries the six phases between them must produce.
const QA_DELIVERY_EXPECTED_REQUESTS: usize = 5;

/// Fixture text, deliberately unmistakable so a leak check is meaningful.
const QA_DELIVERY_OFF_BODY: &str = "Koushi QA delivery channel off message";
const QA_DELIVERY_DWELL_BODY: &str = "Koushi QA delivery dwell message";
const QA_DELIVERY_CANCELLED_BODY: &str = "Koushi QA delivery cancelled message";
const QA_DELIVERY_REARMED_BODY: &str = "Koushi QA delivery rearmed message";
const QA_DELIVERY_HELD_BODY: &str = "Koushi QA delivery held message";
const QA_DELIVERY_WATCHED_ROOM_BODY: &str = "Koushi QA delivery watched room message";
const QA_DELIVERY_WATCHED_SPACE_BODY: &str = "Koushi QA delivery watched space message";
const QA_DELIVERY_BINDING_BODY: &str = "Koushi QA delivery changed endpoint message";

/// Run the delivery lane: two synthetic participants, one stub sink, and the six
/// phases whose tokens are the lane's evidence.
pub(super) async fn run_notification_delivery_scenario(config: &QaConfig) -> Result<(), String> {
    let secret = format!("qa-notification-delivery-{}", std::process::id());
    let stub = QaDeliveryStub::start(&secret)?;
    let user_a = format!("@{}:{}", config.user_a, config.server_name);
    let user_b = format!("@{}:{}", config.user_b, config.server_name);

    let participant_a = login_synced_participant_for_qa(
        &config.homeserver,
        qa_data_dir("notification-delivery-a"),
        &config.user_a,
        &config.password_a,
        "Koushi Notification Delivery QA A",
        "notification delivery login A",
        "notification delivery gate A",
        QaParticipantLoginGate::BootstrapNewIdentity,
    )
    .await?;
    let participant_b = login_synced_participant_for_qa(
        &config.homeserver,
        qa_data_dir("notification-delivery-b"),
        &config.user_b,
        &config.password_b,
        "Koushi Notification Delivery QA B",
        "notification delivery login B",
        "notification delivery gate B",
        QaParticipantLoginGate::BootstrapNewIdentity,
    )
    .await?;

    let QaParticipantLoginOutcome {
        runtime: runtime_a,
        conn: mut conn_a,
        account_key: account_key_a,
        ..
    } = participant_a;
    let QaParticipantLoginOutcome {
        runtime: runtime_b,
        conn: mut conn_b,
        account_key: account_key_b,
        ..
    } = participant_b;

    let flow = QaDeliveryFlow {
        stub: &stub,
        sink_id: QA_DELIVERY_SINK_ID,
        secret: &secret,
        user_a: &user_a,
        user_b: &user_b,
        conn_a: &mut conn_a,
        conn_b: &mut conn_b,
        account_key_b: &account_key_b,
        settings: delivery_settings(&stub, QA_DELIVERY_SINK_ID),
        delivered_event: None,
    };
    let result = flow.run().await;

    let cleanup_b = cleanup_logged_in_runtime(
        conn_b,
        runtime_b,
        account_key_b,
        "notification delivery cleanup B",
    )
    .await;
    let cleanup_a = cleanup_logged_in_runtime(
        conn_a,
        runtime_a,
        account_key_a,
        "notification delivery cleanup A",
    )
    .await;

    result?;
    cleanup_b?;
    cleanup_a?;
    Ok(())
}

/// One delivery phase: the settings under test, the attention the peer produces,
/// and the request the stub must (or must not) see.
struct QaDeliveryFlow<'a> {
    stub: &'a QaDeliveryStub,
    sink_id: &'a str,
    secret: &'a str,
    user_a: &'a str,
    user_b: &'a str,
    conn_a: &'a mut CoreConnection,
    conn_b: &'a mut CoreConnection,
    account_key_b: &'a super::AccountKey,
    settings: NotificationDeliverySettings,
    /// The last event user B sent that a phase proved was delivered, used as the
    /// read marker that ends that room's current qualifying episode.
    delivered_event: Option<String>,
}

impl QaDeliveryFlow<'_> {
    async fn run(mut self) -> Result<(), String> {
        if !koushi_store::resolved_credential_backend_is_file_dir() {
            return Err(
                "notification delivery: the credential store is not the QA file store".to_owned(),
            );
        }
        println!("notification_delivery_file_credential_store=ok");

        // A DM between the two participants is the room every "ordinary attention"
        // assertion watches. Its label is the projection's own, captured live
        // rather than assumed, so the body assertion can require exactly what the
        // badge shows.
        let dm_room_id =
            start_direct_message_for_qa(self.conn_a, self.user_b, "notification delivery DM")
                .await?;
        wait_for_dm_room_in_room_list(self.conn_a, &dm_room_id, "notification delivery DM list")
            .await?;
        wait_for_invite_in_snapshot(
            self.conn_b,
            &dm_room_id,
            Some(true),
            "notification delivery DM invite",
        )
        .await?;
        accept_invite_for_qa(self.conn_b, &dm_room_id, "notification delivery DM accept").await?;
        let dm_key_b = TimelineKey {
            account_key: self.account_key_b.clone(),
            kind: TimelineKind::Room {
                room_id: dm_room_id.clone(),
            },
        };
        subscribe_timeline_for_qa(self.conn_b, &dm_key_b, "notification delivery DM send").await?;

        // The channel is deliberately unfocused: the default suppression is
        // "only when the window is unfocused", and a lane that never reports focus
        // would otherwise be proving suppression rather than delivery.
        self.report_unfocused().await?;
        println!("notification_delivery_unfocused=ok");

        // Install the sink with the channel off, then write its credential through
        // the app's own command. Both land before any phase that asserts delivery,
        // so a missing request is never explained by an unconfigured sink.
        self.settings.dwell_secs = QA_DELIVERY_DWELL_SECS;
        self.settings.min_interval_secs = 0;
        self.apply_settings("notification delivery install sink")
            .await?;
        self.store_secret().await?;

        self.assert_channel_off(&dm_room_id, &dm_key_b).await?;
        let label = self.room_label(&dm_room_id)?;
        self.assert_dwell_and_request(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_read_cancels(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_min_interval_holds(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_watch_list().await?;
        self.assert_secret_hygiene().await?;
        self.assert_secret_binding(&dm_room_id, &dm_key_b).await?;

        println!("notification_delivery=ok");
        Ok(())
    }

    /// Report the window as unfocused, which is the state the delivery channel
    /// requires, and wait for the projection to agree.
    async fn report_unfocused(&mut self) -> Result<(), String> {
        let request_id = self.conn_a.next_request_id();
        self.conn_a
            .command(CoreCommand::App(AppCommand::ObserveNativeWindowFocus {
                request_id,
                focused: false,
                observation_generation: 1,
            }))
            .await
            .map_err(|error| format!("notification delivery: focus observation failed: {error}"))?;
        let conn = &mut *self.conn_a;
        wait_for_condition(
            conn,
            "notification delivery unfocused window",
            EVENT_TIMEOUT,
            |state| (!state.native_attention_context.window_focused).then_some(()),
        )
        .await
    }

    /// Apply the whole delivery block and wait until the snapshot holds it, so a
    /// phase never races the settings it is about to assert against.
    async fn apply_settings(&mut self, label: &str) -> Result<(), String> {
        // `UpdateSettings` replaces the whole delivery block, and `secret_binding` is
        // Rust's own field rather than one the lane may choose, so carry the current
        // value through instead of clearing it with the patch.
        let current = self.conn_a.snapshot().settings.values.delivery.clone();
        for sink in &mut self.settings.sinks {
            sink.secret_binding = current
                .sink(&sink.id)
                .and_then(|existing| existing.secret_binding.clone());
        }
        let request_id = self.conn_a.next_request_id();
        self.conn_a
            .command(CoreCommand::App(AppCommand::UpdateSettings {
                request_id,
                patch: SettingsPatch {
                    scope: Some(SettingsPatchScope::App),
                    delivery: Some(self.settings.clone()),
                    ..SettingsPatch::default()
                },
            }))
            .await
            .map_err(|error| format!("{label}: submit delivery settings failed: {error}"))?;
        let expected = lane_visible_delivery(&self.settings);
        let conn = &mut *self.conn_a;
        wait_for_condition(conn, label, EVENT_TIMEOUT, move |state| {
            (lane_visible_delivery(&state.settings.values.delivery) == expected).then_some(())
        })
        .await
    }

    /// Write the sink credential and wait for the projection to read the sink as
    /// configured, which is also the lane's evidence that the store took it.
    async fn store_secret(&mut self) -> Result<(), String> {
        let request_id = self.conn_a.next_request_id();
        self.conn_a
            .command(CoreCommand::App(
                AppCommand::SetNotificationDeliverySecret {
                    request_id,
                    sink_id: self.sink_id.to_owned(),
                    secret: self.secret_value(),
                },
            ))
            .await
            .map_err(|error| format!("notification delivery: store sink secret failed: {error}"))?;
        let sink_id = self.sink_id.to_owned();
        let conn = &mut *self.conn_a;
        wait_for_condition(
            conn,
            "notification delivery sink configured",
            EVENT_TIMEOUT,
            move |state| {
                state
                    .notification_delivery
                    .sinks
                    .iter()
                    .any(|sink| sink.sink_id == sink_id && sink.secret_configured)
                    .then_some(())
            },
        )
        .await
    }

    /// Assertion 1: with the channel off, an unread direct message produces no
    /// request at all, however long the lane waits.
    async fn assert_channel_off(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let event_id = self
            .send_from_b(room_key_b, "qa-delivery-off", QA_DELIVERY_OFF_BODY)
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery off unread")
            .await?;
        if unread == 0 {
            return Err(
                "notification delivery off: the direct message did not go unread".to_owned(),
            );
        }
        settle(self.conn_a, QA_DELIVERY_ABSENCE_WINDOW * 8).await;
        if self.stub.count() != 0 {
            return Err(format!(
                "notification delivery off: {} request(s) left the device while the channel was off",
                self.stub.count()
            ));
        }
        if self.room_unread(room_id)? == 0 {
            return Err(
                "notification delivery off: the room stopped qualifying during the window"
                    .to_owned(),
            );
        }
        // Leave the room read, so the dwell the next phase measures is the one its
        // own message arms rather than the one this phase's message left behind.
        self.mark_room_read(room_id, &event_id).await?;
        println!("notification_delivery_channel_off=ok");
        Ok(())
    }

    /// Assertion 2: the dwell, the request's destination and authorization, and the
    /// privacy of its body.
    async fn assert_dwell_and_request(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
        label: &str,
    ) -> Result<(), String> {
        self.settings.enabled = true;
        self.settings.dwell_secs = QA_DELIVERY_DWELL_SECS;
        self.settings.min_interval_secs = 0;
        self.apply_settings("notification delivery enable channel")
            .await?;

        // The room is read, so the channel has nothing to deliver and the message
        // below is the transition the dwell is measured from.
        settle(self.conn_a, QA_DELIVERY_ABSENCE_WINDOW).await;
        if self.stub.count() != 0 {
            return Err(format!(
                "notification delivery dwell: {} request(s) left the device with no unread room",
                self.stub.count()
            ));
        }

        let event_id = self
            .send_from_b(room_key_b, "qa-delivery-dwell", QA_DELIVERY_DWELL_BODY)
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery dwell unread")
            .await?;

        // Nothing may leave the device before the dwell elapses. The window is well
        // inside the one second the settings ask for.
        settle(self.conn_a, QA_DELIVERY_ABSENCE_WINDOW).await;
        if self.stub.count() != 0 {
            return Err(format!(
                "notification delivery dwell: {} request(s) left the device before the dwell elapsed",
                self.stub.count()
            ));
        }

        self.wait_requests(1, "notification delivery first request")
            .await?;
        settle(self.conn_a, Duration::from_millis(1500)).await;
        if self.stub.count() != 1 {
            return Err(format!(
                "notification delivery dwell: expected exactly one request after the dwell, saw {}",
                self.stub.count()
            ));
        }

        let request = self.request(0)?;
        let expectation = DeliveryRequestExpectation {
            label: "notification delivery request",
            headline: "Direct message in",
            room_label: label,
            unread,
            room_id,
            event_id: &event_id,
            message_body: QA_DELIVERY_DWELL_BODY,
            user_a: self.user_a,
            user_b: self.user_b,
        };
        if !request.authorization_present {
            return Err(
                "notification delivery request: the request carried no authorization header"
                    .to_owned(),
            );
        }
        if !request.authorization_matches {
            return Err(
                "notification delivery request: the request did not carry the sink credential"
                    .to_owned(),
            );
        }
        if request
            .idempotency_key
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            return Err(
                "notification delivery request: the request carried no idempotency key".to_owned(),
            );
        }
        if !self.stub.endpoint().starts_with("http://127.0.0.1:") {
            return Err(
                "notification delivery request: the sink endpoint is not loopback".to_owned(),
            );
        }
        assert_delivery_body(&request.body, &expectation)?;
        self.delivered_event = Some(event_id);
        println!("notification_delivery_dwell=ok");
        println!("notification_delivery_request=ok");
        println!("notification_delivery_body=ok");
        Ok(())
    }

    /// Assertion 3: a read inside the dwell cancels the delivery, and the next
    /// unread transition delivers after a fresh dwell.
    async fn assert_read_cancels(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
        label: &str,
    ) -> Result<(), String> {
        // Clear the room first so the dwell the next message arms is its own
        // episode, and lengthen the dwell only while the room is not qualifying.
        let delivered_event = self.delivered_event.clone().ok_or_else(|| {
            "notification delivery cancelled: no delivered event to read".to_owned()
        })?;
        self.mark_room_read(room_id, &delivered_event).await?;
        self.settings.dwell_secs = QA_DELIVERY_LONG_DWELL_SECS;
        self.apply_settings("notification delivery long dwell")
            .await?;
        let requests_before = self.stub.count();

        let event_id = self
            .send_from_b(
                room_key_b,
                "qa-delivery-cancelled",
                QA_DELIVERY_CANCELLED_BODY,
            )
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery cancelled unread")
            .await?;
        if unread == 0 {
            return Err(
                "notification delivery cancelled: the room never went unread, so no dwell was armed to cancel"
                    .to_owned(),
            );
        }
        let anchor = Instant::now();
        self.mark_room_read(room_id, &event_id).await?;
        if anchor.elapsed() >= Duration::from_secs(QA_DELIVERY_LONG_DWELL_SECS) {
            return Err(format!(
                "notification delivery cancelled: the read landed after {}s, so the dwell was not the thing that cancelled the send",
                QA_DELIVERY_LONG_DWELL_SECS
            ));
        }
        settle(
            self.conn_a,
            Duration::from_secs(QA_DELIVERY_LONG_DWELL_SECS) + Duration::from_millis(1200),
        )
        .await;
        if self.stub.count() != requests_before {
            return Err(format!(
                "notification delivery cancelled: the channel sent for a room read inside the dwell ({} request(s))",
                self.stub.count() - requests_before
            ));
        }
        println!("notification_delivery_read_cancels=ok");

        // The same room, unread again: a fresh episode, and a fresh dwell.
        let rearm_event = self
            .send_from_b(room_key_b, "qa-delivery-rearmed", QA_DELIVERY_REARMED_BODY)
            .await?;
        let rearm_unread = self
            .wait_room_unread(room_id, "notification delivery rearmed unread")
            .await?;
        self.wait_requests(requests_before + 1, "notification delivery rearmed request")
            .await?;
        let request = self.request(requests_before)?;
        assert_delivery_body(
            &request.body,
            &DeliveryRequestExpectation {
                label: "notification delivery rearm",
                headline: "Direct message in",
                room_label: label,
                unread: rearm_unread,
                room_id,
                event_id: &rearm_event,
                message_body: QA_DELIVERY_REARMED_BODY,
                user_a: self.user_a,
                user_b: self.user_b,
            },
        )?;
        println!("notification_delivery_rearm=ok");
        self.delivered_event = Some(rearm_event);
        Ok(())
    }

    /// Assertion 4: a delivery inside `min_interval_secs` is held, and released when
    /// the window reopens.
    async fn assert_min_interval_holds(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
        label: &str,
    ) -> Result<(), String> {
        let delivered_event = self.delivered_event.clone().ok_or_else(|| {
            "notification delivery min interval: no delivered event to read".to_owned()
        })?;
        self.mark_room_read(room_id, &delivered_event).await?;
        self.settings.dwell_secs = QA_DELIVERY_DWELL_SECS;
        self.settings.min_interval_secs = QA_DELIVERY_MIN_INTERVAL_SECS;
        self.apply_settings("notification delivery coalesce window")
            .await?;
        let requests_before = self.stub.count();

        let event_id = self
            .send_from_b(room_key_b, "qa-delivery-held", QA_DELIVERY_HELD_BODY)
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery held unread")
            .await?;
        // Far longer than the one second dwell: the room is due and the coalescing
        // window is the only thing that can be holding it.
        settle(self.conn_a, Duration::from_millis(2500)).await;
        if self.stub.count() != requests_before {
            return Err(format!(
                "notification delivery min interval: the channel sent inside min_interval_secs ({} request(s))",
                self.stub.count() - requests_before
            ));
        }

        self.settings.min_interval_secs = 0;
        self.apply_settings("notification delivery reopen window")
            .await?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery released request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_delivery_body(
            &request.body,
            &DeliveryRequestExpectation {
                label: "notification delivery min interval",
                headline: "Direct message in",
                room_label: label,
                unread,
                room_id,
                event_id: &event_id,
                message_body: QA_DELIVERY_HELD_BODY,
                user_a: self.user_a,
                user_b: self.user_b,
            },
        )?;
        println!("notification_delivery_min_interval=ok");
        Ok(())
    }

    /// Assertion 5: the watch list, for a room, and for a room that joins a watched
    /// Space after the list was written.
    async fn assert_watch_list(&mut self) -> Result<(), String> {
        // Every category trigger off: from here only the watch list can qualify.
        self.settings.direct_messages = false;
        self.settings.mentions = false;
        self.settings.keywords = false;
        self.settings.dwell_secs = QA_DELIVERY_DWELL_SECS;
        self.settings.min_interval_secs = 0;

        let watched_room_id = create_room_for_qa(
            self.conn_a,
            "QA Watched Room",
            false,
            "notification delivery watched room",
        )
        .await?;
        wait_for_room_in_room_list(
            self.conn_a,
            &watched_room_id,
            "notification delivery watched room list",
        )
        .await?;
        invite_user_for_qa(
            self.conn_a,
            &watched_room_id,
            self.user_b,
            "notification delivery watched room invite",
        )
        .await?;
        wait_for_invite_in_snapshot(
            self.conn_b,
            &watched_room_id,
            None,
            "notification delivery watched room B invite",
        )
        .await?;
        accept_invite_for_qa(
            self.conn_b,
            &watched_room_id,
            "notification delivery watched room accept",
        )
        .await?;
        let watched_key_b = TimelineKey {
            account_key: self.account_key_b.clone(),
            kind: TimelineKind::Room {
                room_id: watched_room_id.clone(),
            },
        };
        subscribe_timeline_for_qa(
            self.conn_b,
            &watched_key_b,
            "notification delivery watched room send",
        )
        .await?;

        self.settings.watched = vec![NotificationDeliveryTarget::Room {
            room_id: watched_room_id.clone(),
        }];
        self.apply_settings("notification delivery watch room")
            .await?;
        let requests_before = self.stub.count();

        let event_id = self
            .send_from_b(
                &watched_key_b,
                "qa-delivery-watched-room",
                QA_DELIVERY_WATCHED_ROOM_BODY,
            )
            .await?;
        let unread = self
            .wait_room_unread(
                &watched_room_id,
                "notification delivery watched room unread",
            )
            .await?;
        let label = self.room_label(&watched_room_id)?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery watched room request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_delivery_body(
            &request.body,
            &DeliveryRequestExpectation {
                label: "notification delivery watched room",
                headline: "Message in",
                room_label: &label,
                unread,
                room_id: &watched_room_id,
                event_id: &event_id,
                message_body: QA_DELIVERY_WATCHED_ROOM_BODY,
                user_a: self.user_a,
                user_b: self.user_b,
            },
        )?;
        println!("notification_delivery_watched_room=ok");

        // A Space the watch list names before the room exists inside it: the room
        // joins afterwards, so only the Space target can explain the delivery.
        let space_id = create_space_for_qa(
            self.conn_a,
            "QA Watched Space",
            "notification delivery watched space",
        )
        .await?;
        wait_for_space_in_space_list(
            self.conn_a,
            &space_id,
            "notification delivery watched space list",
        )
        .await?;
        self.settings.watched = vec![NotificationDeliveryTarget::Space {
            space_id: space_id.clone(),
        }];
        self.apply_settings("notification delivery watch space")
            .await?;

        let child_room_id = create_room_for_qa(
            self.conn_a,
            "QA Watched Space Room",
            false,
            "notification delivery watched space room",
        )
        .await?;
        invite_user_for_qa(
            self.conn_a,
            &child_room_id,
            self.user_b,
            "notification delivery watched space room invite",
        )
        .await?;
        wait_for_invite_in_snapshot(
            self.conn_b,
            &child_room_id,
            None,
            "notification delivery watched space room B invite",
        )
        .await?;
        accept_invite_for_qa(
            self.conn_b,
            &child_room_id,
            "notification delivery watched space room accept",
        )
        .await?;
        let child_key_b = TimelineKey {
            account_key: self.account_key_b.clone(),
            kind: TimelineKind::Room {
                room_id: child_room_id.clone(),
            },
        };
        subscribe_timeline_for_qa(
            self.conn_b,
            &child_key_b,
            "notification delivery watched space room send",
        )
        .await?;
        set_space_child_for_qa(
            self.conn_a,
            &space_id,
            &child_room_id,
            "notification delivery watched space child",
        )
        .await?;
        self.wait_parent_space(&child_room_id, &space_id).await?;

        let requests_before = self.stub.count();
        let event_id = self
            .send_from_b(
                &child_key_b,
                "qa-delivery-watched-space",
                QA_DELIVERY_WATCHED_SPACE_BODY,
            )
            .await?;
        let unread = self
            .wait_room_unread(&child_room_id, "notification delivery watched space unread")
            .await?;
        let label = self.room_label(&child_room_id)?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery watched space request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_delivery_body(
            &request.body,
            &DeliveryRequestExpectation {
                label: "notification delivery watched space",
                headline: "Message in",
                room_label: &label,
                unread,
                room_id: &child_room_id,
                event_id: &event_id,
                message_body: QA_DELIVERY_WATCHED_SPACE_BODY,
                user_a: self.user_a,
                user_b: self.user_b,
            },
        )?;
        println!("notification_delivery_watched_space=ok");
        Ok(())
    }

    /// Assertion 6: the secret stays in the credential store.
    async fn assert_secret_hygiene(&self) -> Result<(), String> {
        let secret = self.secret_value();
        let settings =
            serde_json::to_string(&self.conn_a.snapshot().settings.values).map_err(|_| {
                "notification delivery hygiene: settings snapshot is not serializable".to_owned()
            })?;
        if settings.contains(&secret) {
            return Err(
                "notification delivery hygiene: the settings snapshot carries the sink secret"
                    .to_owned(),
            );
        }
        let diagnostics = serde_json::to_string(&koushi_diagnostics::snapshot()).map_err(|_| {
            "notification delivery hygiene: diagnostics are not serializable".to_owned()
        })?;
        if diagnostics.contains(&secret) {
            return Err(
                "notification delivery hygiene: the diagnostics stream carries the sink secret"
                    .to_owned(),
            );
        }
        if self.stub.count() != QA_DELIVERY_EXPECTED_REQUESTS {
            return Err(format!(
                "notification delivery hygiene: expected {} requests at the loopback stub, saw {}",
                QA_DELIVERY_EXPECTED_REQUESTS,
                self.stub.count()
            ));
        }
        for request in self.stub.requests() {
            if request.body.contains(&secret)
                || request
                    .idempotency_key
                    .as_deref()
                    .is_some_and(|key| key.contains(&secret))
            {
                return Err(
                    "notification delivery hygiene: a captured request carries the sink secret"
                        .to_owned(),
                );
            }
            if !request.authorization_matches {
                return Err(
                    "notification delivery hygiene: a captured request reached the stub with the wrong credential"
                        .to_owned(),
                );
            }
        }
        println!("notification_delivery_secret_hygiene=ok");
        Ok(())
    }

    /// The second half of the credential-binding fix: changing what a stored
    /// credential authenticates must take the sink out of service, must not send the
    /// old credential anywhere, and must take that credential out of the store.
    ///
    /// The room is left unread and its dwell is allowed to elapse, so a sink that
    /// stayed configured would deliver and would be caught. The changed endpoint is a
    /// second loopback stub, so a send would be visible here rather than reaching a
    /// name the lane does not own.
    async fn assert_secret_binding(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        if !store_holds_secret(self.secret)? {
            return Err(
                "notification delivery binding: the store never held the credential this assertion removes"
                    .to_owned(),
            );
        }
        let moved = QaDeliveryStub::start(self.secret)?;
        for sink in &mut self.settings.sinks {
            if let NotificationSinkConfig::Resend { endpoint } = &mut sink.config {
                *endpoint = moved.endpoint().to_owned();
            }
        }
        // Put the categories back and retire the watch list, so the fresh message
        // below qualifies on its own merits. Only the sink's configuration can then
        // keep the credential on this device.
        self.settings.direct_messages = true;
        self.settings.mentions = true;
        self.settings.keywords = true;
        self.settings.watched.clear();
        self.apply_settings("notification delivery change endpoint")
            .await?;

        settle(self.conn_a, Duration::from_millis(1500)).await;
        let still_configured = self
            .conn_a
            .snapshot()
            .notification_delivery
            .sinks
            .iter()
            .any(|sink| sink.sink_id == self.sink_id && sink.secret_configured);
        if still_configured {
            return Err(
                "notification delivery binding: the sink still reads as configured after the endpoint its credential was entered for changed"
                    .to_owned(),
            );
        }

        // A fresh unread transition, so the only reason nothing leaves the device is
        // that the credential no longer belongs to this endpoint.
        let delivered_event = self.delivered_event.clone().ok_or_else(|| {
            "notification delivery binding: no delivered event to read".to_owned()
        })?;
        self.mark_room_read(room_id, &delivered_event).await?;
        self.send_from_b(room_key_b, "qa-delivery-binding", QA_DELIVERY_BINDING_BODY)
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery binding unread")
            .await?;
        if unread == 0 {
            return Err(
                "notification delivery binding: the room did not go unread again".to_owned(),
            );
        }
        settle(self.conn_a, Duration::from_millis(2500)).await;
        if moved.count() != 0 {
            return Err(format!(
                "notification delivery binding: {} request(s) carried the stored credential to an endpoint it was not entered for",
                moved.count()
            ));
        }
        if self.stub.count() != QA_DELIVERY_EXPECTED_REQUESTS {
            return Err(format!(
                "notification delivery binding: the original endpoint saw {} extra request(s)",
                self.stub.count() - QA_DELIVERY_EXPECTED_REQUESTS
            ));
        }
        if store_holds_secret(self.secret)? {
            return Err(
                "notification delivery binding: the credential store still holds a credential for an endpoint it no longer matches"
                    .to_owned(),
            );
        }
        println!("notification_delivery_secret_binding=ok");
        Ok(())
    }

    /// Send one message as user B and return the event id the homeserver accepted.
    ///
    /// B viewing its own message immediately afterwards is what a real client does,
    /// and the receipt event is what makes A's client re-derive its unread boundary
    /// from A's own receipts. The count A reports is still A's own; the receipt only
    /// stops the projection from sitting on a stale boundary.
    async fn send_from_b(
        &mut self,
        key: &TimelineKey,
        transaction_id: &str,
        body: &str,
    ) -> Result<String, String> {
        let event_id =
            send_text_and_wait_event(self.conn_b, key, transaction_id, body, transaction_id)
                .await?;
        send_main_timeline_read_receipt(
            self.conn_b,
            key,
            &event_id,
            &format!("{transaction_id} (receipt)"),
        )
        .await?;
        Ok(event_id)
    }

    fn secret_value(&self) -> String {
        self.secret.to_owned()
    }

    fn room_label(&self, room_id: &str) -> Result<String, String> {
        self.conn_a
            .snapshot()
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .map(|room| room.display_label.clone())
            .filter(|label| !label.is_empty())
            .ok_or_else(|| "notification delivery: the room has no display label".to_owned())
    }

    fn room_unread(&self, room_id: &str) -> Result<u64, String> {
        self.conn_a
            .snapshot()
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .map(room_activity_unread_count)
            .ok_or_else(|| "notification delivery: the room is not in the room list".to_owned())
    }

    async fn wait_room_unread(&mut self, room_id: &str, label: &str) -> Result<u64, String> {
        let room = room_id.to_owned();
        let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
        loop {
            let observed = self
                .conn_a
                .snapshot()
                .rooms
                .iter()
                .find(|summary| summary.room_id == room)
                .map(|summary| {
                    (
                        room_activity_unread_count(summary),
                        summary.unread_count,
                        summary.notification_count,
                        summary.marked_unread,
                    )
                });
            match observed {
                Some((unread, _, _, _)) if unread > 0 => return Ok(unread),
                other => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(format!(
                            "{label}: timed out with room_projection={other:?} requests={}",
                            self.stub.count()
                        ));
                    }
                }
            }
            let _ = tokio::time::timeout(Duration::from_millis(25), self.conn_a.recv_event()).await;
        }
    }

    async fn mark_room_read(&mut self, room_id: &str, event_id: &str) -> Result<(), String> {
        let request_id = self.conn_a.next_request_id();
        self.conn_a
            .command(CoreCommand::Room(RoomCommand::MarkRoomAsRead {
                request_id,
                room_id: room_id.to_owned(),
                event_id: event_id.to_owned(),
            }))
            .await
            .map_err(|error| format!("notification delivery: submit mark read failed: {error}"))?;
        let room = room_id.to_owned();
        let conn = &mut *self.conn_a;
        wait_for_condition(
            conn,
            "notification delivery room read",
            EVENT_TIMEOUT,
            move |state| {
                state
                    .rooms
                    .iter()
                    .find(|summary| summary.room_id == room)
                    .map(room_activity_unread_count)
                    .filter(|unread| *unread == 0)
                    .map(|_| ())
            },
        )
        .await
    }

    async fn wait_parent_space(&mut self, room_id: &str, space_id: &str) -> Result<(), String> {
        let room = room_id.to_owned();
        let space = space_id.to_owned();
        let conn = &mut *self.conn_a;
        wait_for_condition(
            conn,
            "notification delivery parent Space projection",
            ROOM_LIST_EVENT_TIMEOUT,
            move |state| {
                state
                    .rooms
                    .iter()
                    .any(|summary| {
                        summary.room_id == room
                            && summary.parent_space_ids.iter().any(|id| id == &space)
                    })
                    .then_some(())
            },
        )
        .await
    }

    async fn wait_requests(&mut self, expected: usize, label: &str) -> Result<(), String> {
        let stub = self.stub;
        let conn = &mut *self.conn_a;
        wait_for_condition(conn, label, EVENT_TIMEOUT, |_| {
            (stub.count() >= expected).then_some(())
        })
        .await
    }

    fn request(&self, index: usize) -> Result<QaDeliveryStubRequest, String> {
        self.stub
            .requests()
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("notification delivery: no request at index {index}"))
    }
}

/// What one request must say about the room, and what it must never say.
struct DeliveryRequestExpectation<'a> {
    label: &'a str,
    headline: &'a str,
    room_label: &'a str,
    unread: u64,
    room_id: &'a str,
    event_id: &'a str,
    message_body: &'a str,
    user_a: &'a str,
    user_b: &'a str,
}

/// Assert the body names the room by the label the badge shows, with its counts,
/// and carries no Matrix identifier and no message text.
fn assert_delivery_body(
    body: &str,
    expectation: &DeliveryRequestExpectation<'_>,
) -> Result<(), String> {
    let label = expectation.label;
    let named = format!("{} {}", expectation.headline, expectation.room_label);
    if !body.contains(&named) {
        return Err(format!(
            "{label}: the body did not name the room by its safe display label"
        ));
    }
    if !body.contains(&format!("{} unread", expectation.unread)) {
        return Err(format!(
            "{label}: the body did not carry the room's unread count"
        ));
    }
    for (what, needle) in [
        ("the room id", expectation.room_id),
        ("the event id", expectation.event_id),
        ("the message text", expectation.message_body),
        ("the device owner's account id", expectation.user_a),
        ("the sender's account id", expectation.user_b),
    ] {
        if body.contains(needle) {
            return Err(format!("{label}: the body carried {what}"));
        }
    }
    if carries_matrix_identifier(body) {
        return Err(format!("{label}: the body carried a Matrix identifier"));
    }
    Ok(())
}

/// Whether any whitespace-delimited token in `body` is a Matrix identifier, which
/// is what the lane's private-data rule forbids in anything it emits.
fn carries_matrix_identifier(body: &str) -> bool {
    body.split_whitespace().any(|token| {
        let token = token.trim_end_matches([':', ',', '.']);
        matches!(token.chars().next(), Some('@' | '!' | '$')) && token.contains(':')
    })
}

/// The settings every phase starts from: one enabled sink pointing at the stub,
/// and the channel off until a phase turns it on.
fn delivery_settings(stub: &QaDeliveryStub, sink_id: &str) -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        enabled: false,
        direct_messages: true,
        mentions: true,
        keywords: true,
        only_when_unfocused: true,
        dwell_secs: QA_DELIVERY_DWELL_SECS,
        min_interval_secs: 0,
        prefix_chars: 0,
        prefix_consent: None,
        watched: Vec::new(),
        sinks: vec![NotificationSinkSettings {
            id: sink_id.to_owned(),
            enabled: true,
            destination: "recipient@example.invalid".to_owned(),
            from_address: "sender@example.invalid".to_owned(),
            sender_name: "Koushi QA".to_owned(),
            config: NotificationSinkConfig::Resend {
                endpoint: stub.endpoint().to_owned(),
            },
            // Rust owns this field: it is written when a credential is entered for a
            // sink and dropped when the sink stops matching it. The lane carries
            // whatever the app stored rather than asserting a value it cannot own.
            secret_binding: None,
        }],
    }
}

/// The delivery block as the lane owns it.
///
/// `secret_binding` is Rust's own field: the lane carries the stored value through a
/// patch, but it cannot predict whether canonicalization keeps or drops it, so an
/// equality check over what the lane chose must ignore it.
fn lane_visible_delivery(settings: &NotificationDeliverySettings) -> NotificationDeliverySettings {
    let mut copy = settings.clone();
    for sink in &mut copy.sinks {
        sink.secret_binding = None;
    }
    copy
}

/// Whether the QA file credential store still holds `secret` anywhere.
///
/// The lane runs against the file backend by construction, which writes a credential
/// as a plain file, so a plaintext walk is an honest read of "is it still there". The
/// search only ever compares against the lane's own synthetic value and prints nothing
/// about any file it reads.
fn store_holds_secret(secret: &str) -> Result<bool, String> {
    let Ok(dir) = std::env::var(super::registry::ENV_FILE_CREDENTIAL_STORE_DIR) else {
        return Err(
            "notification delivery binding: the QA file credential store is not configured"
                .to_owned(),
        );
    };
    let mut pending = vec![std::path::PathBuf::from(dir)];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                pending.push(entry.path());
            }
            continue;
        }
        if let Ok(contents) = std::fs::read(&path)
            && contents
                .windows(secret.len())
                .any(|window| window == secret.as_bytes())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Wait for a condition over the app snapshot, draining the event stream while it
/// polls so a later event waiter cannot observe a lagged stream.
async fn wait_for_condition<T>(
    conn: &mut CoreConnection,
    label: &str,
    timeout: Duration,
    mut check: impl FnMut(&AppState) -> Option<T>,
) -> Result<T, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(value) = check(&conn.snapshot()) {
            return Ok(value);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("{label}: timed out"));
        }
        let _ = tokio::time::timeout(Duration::from_millis(25), conn.recv_event()).await;
    }
}

/// Hold the actor's clock for `duration` while draining its events, which is how
/// the lane waits for a request that must not arrive.
async fn settle(conn: &mut CoreConnection, duration: Duration) {
    let deadline = tokio::time::Instant::now() + duration;
    while tokio::time::Instant::now() < deadline {
        let _ = tokio::time::timeout(Duration::from_millis(25), conn.recv_event()).await;
    }
}

/// One HTTP request a sink made, reduced to what the lane asserts and nothing that
/// could carry the credential or a room's private data.
#[derive(Clone)]
struct QaDeliveryStubRequest {
    authorization_present: bool,
    authorization_matches: bool,
    idempotency_key: Option<String>,
    body: String,
}

/// The provider stand-in: a loopback listener that answers any request with a 2xx,
/// so the sink records `Delivered`, and keeps what the lane asserts about.
struct QaDeliveryStub {
    endpoint: String,
    requests: Arc<Mutex<Vec<QaDeliveryStubRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl QaDeliveryStub {
    fn start(expected_secret: &str) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| {
            "notification delivery: the provider stub could not bind loopback".to_owned()
        })?;
        let address = listener
            .local_addr()
            .map_err(|_| "notification delivery: the stub has no address".to_owned())?;
        listener.set_nonblocking(true).map_err(|_| {
            "notification delivery: the stub could not become non-blocking".to_owned()
        })?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let expected = format!("Bearer {expected_secret}");
        let thread_requests = Arc::clone(&requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Some(request) = read_stub_request(stream, &expected)
                            && let Ok(mut recorded) = thread_requests.lock()
                        {
                            recorded.push(request);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            endpoint: format!("http://127.0.0.1:{}{QA_STUB_PATH}", address.port()),
            requests,
            shutdown,
            handle: Some(handle),
        })
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn count(&self) -> usize {
        self.requests.lock().map(|held| held.len()).unwrap_or(0)
    }

    fn requests(&self) -> Vec<QaDeliveryStubRequest> {
        self.requests
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }
}

impl Drop for QaDeliveryStub {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Read one request and answer it. The expected bearer is compared and discarded;
/// the secret itself is never stored.
fn read_stub_request(mut stream: TcpStream, expected: &str) -> Option<QaDeliveryStubRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    read_stub_line(&mut reader)?;
    let mut content_length = 0_usize;
    let mut authorization_present = false;
    let mut authorization_matches = false;
    let mut idempotency_key = None;
    loop {
        let line = read_stub_line(&mut reader)?;
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => content_length = value.parse().unwrap_or(0),
            "authorization" => {
                authorization_present = true;
                authorization_matches = value == expected;
            }
            "idempotency-key" => idempotency_key = Some(value.to_owned()),
            _ => {}
        }
    }
    let mut body = vec![0_u8; content_length.min(1024 * 1024)];
    if !body.is_empty() {
        reader.read_exact(&mut body).ok()?;
    }
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"id\":\"qa-stub\"}",
    );
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
    Some(QaDeliveryStubRequest {
        authorization_present,
        authorization_matches,
        idempotency_key,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn read_stub_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    Some(line)
}
