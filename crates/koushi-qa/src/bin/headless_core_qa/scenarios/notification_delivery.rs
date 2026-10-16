//! #1427, #1460: device-local notification delivery, proved end to end against a
//! disposable local homeserver and loopback stand-ins for the providers.
//!
//! User A is the device under test; user B is the peer that produces the
//! attention. A configures one Resend destination whose endpoint is a stub
//! listener this lane owns on `127.0.0.1`, writes the sink credential through the
//! app's own typed command, and selects no target for it until the phases that
//! assert delivery. There is no channel switch (#1460): a destination with every
//! category off and no watch sends nothing. The stub records only what the lane
//! must assert: whether the request carried the expected bearer or the expected
//! SendKey path, the idempotency key, and the request body. It compares the
//! credential against the expected value without ever keeping it, so the lane
//! cannot leak the credential into its own artifacts.
//!
//! Every assertion is about what left the device and when:
//!
//! 1. a destination with no target produces no request at all, however long the
//!    room stays unread;
//! 2. with its categories on and a one second dwell, a direct message produces no
//!    request before the dwell and exactly one after, addressed to this lane's own
//!    stub, authorized by the sink credential, naming the room by the label the
//!    badge shows, and carrying none of the room id, the event id, the sender, or
//!    the message text;
//! 3. a read that lands inside the dwell cancels the delivery, and the same room
//!    made unread again delivers after a fresh dwell;
//! 4. a second delivery inside the destination's `min_interval_secs` is held, and
//!    is released - not dropped - when the window reopens;
//! 5. a room on the destination's watch list delivers while its category is off,
//!    and a room that joins a watched Space after the list was written delivers too;
//! 6. the sink secret reaches neither the settings snapshot, nor the diagnostics
//!    stream, nor this lane's own captured requests;
//! 7. the start of the room's newest message reaches the body, as a quoted line
//!    under the room's own line, only with the `Prefix` content mode and a consent
//!    bound to this sink kind, endpoint, and recipient; with no consent, with names
//!    only, and when the newest event yields no readable text, the body carries none
//!    of it and the delivery still succeeds, so missing text is never a failure;
//! 8. changing the recipient a consent was given for, or the endpoint, makes the app
//!    clear the stored consent and return the destination to names only, so the next
//!    delivery carries no message text;
//! 9. that message text reaches no serialized app-state snapshot, no settings file,
//!    no diagnostics stream, and no file this lane wrote;
//! 10. a second destination of kind ServerChan, pointed at its own loopback stub,
//!     takes a synthetic SendKey through the same typed command, loses a consent when
//!     the SendKey is saved again (a new recipient), and sends exactly one accepted
//!     request whose path carries the SendKey, whose form body carries no message
//!     text, and whose key reaches no snapshot, diagnostics stream, or capture.
//!
//! Private data: the lane prints tokens and counts only. Room ids, event ids,
//! account ids, room labels, message bodies, and credentials stay inside the
//! process, and the leak assertions are evaluated here so only a boolean reaches
//! the transcript.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use koushi_state::{
    AppState, NotificationDeliveryContent, NotificationDeliveryContentConsent,
    NotificationDeliveryOutcomeState, NotificationDeliveryPartnerLabel,
    NotificationDeliverySettings, NotificationDeliveryTarget, NotificationDeliveryTargets,
    NotificationDeliveryTiming, NotificationSinkConfig, NotificationSinkSettings, SettingsPatch,
    SettingsPatchScope, room_activity_unread_count,
};

use super::cleanup::cleanup_logged_in_runtime;
use super::event_wait::{
    send_main_timeline_read_receipt, subscribe_timeline_for_qa, wait_for_dm_room_in_room_list,
    wait_for_invite_in_snapshot, wait_for_media_send_flow_completion, wait_for_room_in_room_list,
    wait_for_space_in_space_list,
};
use super::fixtures::{
    accept_invite_for_qa, create_room_for_qa, create_space_for_qa, invite_user_for_qa,
    set_space_child_for_qa, start_direct_message_for_qa,
};
use super::participants::{
    QaParticipantLoginGate, QaParticipantLoginOutcome, authenticated_session_info,
    login_synced_participant_for_qa, qa_data_dir,
};
use super::registry::{EVENT_TIMEOUT, QaConfig, ROOM_LIST_EVENT_TIMEOUT};
use super::scenario_read_state::send_text_and_wait_event;
use super::{
    AppCommand, CoreCommand, CoreConnection, MentionIntent, RoomCommand, TimelineCommand,
    TimelineKey, TimelineKind, UploadMediaKind, UploadMediaRequest, build_formatted_message_draft,
};

/// The Resend destination every phase but the last configures. Sink ids are the
/// credential store's own alphabet, so the id can also be a file name component inside
/// the QA store.
const QA_DELIVERY_SINK_ID: &str = "qa-stub-sink";

/// The ServerChan destination the last phase adds next to the Resend one.
const QA_DELIVERY_SERVERCHAN_SINK_ID: &str = "qa-serverchan-sink";

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

/// The synthetic text the prefix assertions make the room's newest message.
///
/// Plain ASCII and unmistakable, so a search for it in a captured body or an artifact
/// is meaningful, and what the digest renders is what was sent.
const QA_DELIVERY_PREFIX_MARKER: &str = "qa-notification-prefix-alpine";

/// The synthetic caption of the non-text (file) message the prefix assertions make the
/// room's newest event.
///
/// It is the event's `body`, so it is exactly what a prefix would render if the channel
/// read any message body rather than only a readable text message.
const QA_DELIVERY_PREFIX_MEDIA_CAPTION: &str = "qa-notification-prefix-nontext";

/// Grapheme clusters of the newest message the `Prefix` mode may render, far more than
/// the marker, so a consented prefix renders all of it and names only renders none.
const QA_DELIVERY_PREFIX_GRAPHEMES: u32 = 80;
/// The content mode the prefix phases choose.
const QA_DELIVERY_PREFIX: NotificationDeliveryContent = NotificationDeliveryContent::Prefix {
    graphemes: QA_DELIVERY_PREFIX_GRAPHEMES,
};

/// A second synthetic mailbox, so changing where a consented sink delivers is a change
/// the lane owns and never a real address.
const QA_DELIVERY_PREFIX_OTHER_DESTINATION: &str = "other-recipient@example.invalid";

/// Run the delivery lane: two synthetic participants, one stub sink, and the phases
/// whose tokens are the lane's evidence.
pub(super) async fn run_notification_delivery_scenario(config: &QaConfig) -> Result<(), String> {
    let secret = format!("qa-notification-delivery-{}", std::process::id());
    // A ServerChan Turbo SendKey is `SCT` and letters and digits; this one is synthetic
    // and names no WeChat account.
    let sendkey = format!("SCTqa{}", std::process::id());
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
        sendkey: &sendkey,
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
    /// The synthetic ServerChan SendKey the last phase stores.
    sendkey: &'a str,
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

        // Install the destination with no target, then write its credential through
        // the app's own command. Both land before any phase that asserts delivery,
        // so a missing request is never explained by an unconfigured sink.
        {
            let sink = self.primary_sink_mut()?;
            sink.targets = NotificationDeliveryTargets::none();
            sink.timing.dwell_secs = QA_DELIVERY_DWELL_SECS;
            sink.timing.min_interval_secs = 0;
        }
        self.apply_settings("notification delivery install sink")
            .await?;
        self.store_secret().await?;

        self.assert_channel_off(&dm_room_id, &dm_key_b).await?;
        let label = self.dm_partner_name(&dm_room_id)?;
        self.assert_dwell_and_request(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_read_cancels(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_min_interval_holds(&dm_room_id, &dm_key_b, &label)
            .await?;
        self.assert_watch_list().await?;
        self.assert_secret_hygiene().await?;
        self.assert_secret_binding(&dm_room_id, &dm_key_b).await?;
        self.assert_prefix_text(&dm_room_id, &dm_key_b).await?;
        self.assert_serverchan(&dm_room_id, &dm_key_b).await?;

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
        self.sync_bindings();
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

    /// Write the Resend sink credential and wait for the projection to read the sink
    /// as configured, which is also the lane's evidence that the store took it.
    async fn store_secret(&mut self) -> Result<(), String> {
        let sink_id = self.sink_id.to_owned();
        let secret = self.secret_value();
        self.store_sink_secret(&sink_id, secret).await
    }

    /// Write one sink's credential through the app's typed command, and wait until the
    /// app has bound a credential revision this write minted - not the one before it -
    /// and the projection reads the sink as configured.
    async fn store_sink_secret(&mut self, sink_id: &str, secret: String) -> Result<(), String> {
        let previous = self
            .stored_sink(sink_id)
            .and_then(|sink| sink.secret_binding);
        let request_id = self.conn_a.next_request_id();
        self.conn_a
            .command(CoreCommand::App(
                AppCommand::SetNotificationDeliverySecret {
                    request_id,
                    sink_id: sink_id.to_owned(),
                    secret: koushi_state::AuthSecret::new(secret),
                },
            ))
            .await
            .map_err(|error| format!("notification delivery: store sink secret failed: {error}"))?;
        let sink_id = sink_id.to_owned();
        let conn = &mut *self.conn_a;
        wait_for_condition(
            conn,
            "notification delivery sink configured",
            EVENT_TIMEOUT,
            move |state| {
                let rebound = state
                    .settings
                    .values
                    .delivery
                    .sink(&sink_id)
                    .and_then(|sink| sink.secret_binding.clone())
                    .is_some_and(|binding| previous.as_deref() != Some(binding.as_str()));
                let configured = state
                    .notification_delivery
                    .sinks
                    .iter()
                    .any(|sink| sink.sink_id == sink_id && sink.secret_configured);
                (rebound && configured).then_some(())
            },
        )
        .await
    }

    /// Assertion 1: with no target selected, an unread direct message produces no
    /// request at all, however long the lane waits. The evidence token keeps its
    /// pre-#1460 name: "delivery off" is now a destination that sends for nothing.
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
                "notification delivery off: {} request(s) left the device while the destination had no target",
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
        {
            let sink = self.primary_sink_mut()?;
            sink.targets = NotificationDeliveryTargets::default();
            sink.timing.dwell_secs = QA_DELIVERY_DWELL_SECS;
            sink.timing.min_interval_secs = 0;
        }
        self.apply_settings("notification delivery select targets")
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
            headline: "Direct message from",
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
        self.primary_sink_mut()?.timing.dwell_secs = QA_DELIVERY_LONG_DWELL_SECS;
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
                headline: "Direct message from",
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
        {
            let timing = &mut self.primary_sink_mut()?.timing;
            timing.dwell_secs = QA_DELIVERY_DWELL_SECS;
            timing.min_interval_secs = QA_DELIVERY_MIN_INTERVAL_SECS;
        }
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

        self.primary_sink_mut()?.timing.min_interval_secs = 0;
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
                headline: "Direct message from",
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
        {
            let sink = self.primary_sink_mut()?;
            sink.targets.direct_messages = false;
            sink.targets.mentions = false;
            sink.targets.keywords = false;
            sink.timing.dwell_secs = QA_DELIVERY_DWELL_SECS;
            sink.timing.min_interval_secs = 0;
        }

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

        self.primary_sink_mut()?.targets.watched = vec![NotificationDeliveryTarget::Room {
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
        self.primary_sink_mut()?.targets.watched = vec![NotificationDeliveryTarget::Space {
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
        // Put the categories back and retire the watch list, so the fresh message
        // below qualifies on its own merits. Only the sink's configuration can then
        // keep the credential on this device.
        {
            let sink = self.primary_sink_mut()?;
            if let NotificationSinkConfig::Resend { endpoint } = &mut sink.config {
                *endpoint = moved.endpoint().to_owned();
            }
            sink.targets = NotificationDeliveryTargets::default();
        }
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

    /// The plaintext-prefix assertions: what the body carries, what a missing consent or
    /// a switched-off option withholds, what a newest event without readable text
    /// withholds, what a moved destination or endpoint drops, and where the text never
    /// reaches at all.
    ///
    /// The credential-binding phase above deliberately takes the sink out of service, so
    /// this phase re-enters the credential the way a user would before it asserts
    /// anything. Every sub-assertion reads the app's own snapshot for the prefix source
    /// the room list holds and for whether the stored consent survived, and the stub for
    /// what the delivery carried.
    async fn assert_prefix_text(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        self.restore_prefix_sink(room_id).await?;
        let destination = self.primary_sink()?.destination.clone();

        self.assert_prefix_rendered(room_id, room_key_b).await?;
        self.assert_prefix_without_consent(room_id, room_key_b)
            .await?;
        self.assert_prefix_option_off(room_id, room_key_b).await?;
        self.assert_prefix_without_readable_text(room_id, room_key_b)
            .await?;
        self.assert_prefix_consent_cleared(room_id, room_key_b)
            .await?;
        self.assert_prefix_hygiene(room_id)?;
        self.assert_prefix_endpoint_cleared(room_id, room_key_b, &destination)
            .await?;
        Ok(())
    }

    /// Put the sink back in service on this lane's own stub, with no target selected.
    ///
    /// The binding phase above moved the endpoint and retired the credential, so the
    /// prefix assertions re-enter it exactly as a user would: point the sink at this
    /// lane's stub again and save the credential. The sink selects nothing while the room
    /// that phase left unread is read, so this phase's first delivery is armed by its own
    /// message and never by that leftover.
    async fn restore_prefix_sink(&mut self, room_id: &str) -> Result<(), String> {
        let endpoint = self.stub.endpoint().to_owned();
        {
            let sink = self.primary_sink_mut()?;
            sink.targets = NotificationDeliveryTargets::none();
            sink.timing.dwell_secs = QA_DELIVERY_DWELL_SECS;
            sink.timing.min_interval_secs = 0;
            sink.content = NotificationDeliveryContent::NamesOnly;
            sink.content_consent = None;
            if let NotificationSinkConfig::Resend { endpoint: current } = &mut sink.config {
                *current = endpoint;
            }
        }
        self.apply_settings("notification delivery prefix restore sink")
            .await?;
        self.store_secret().await?;
        self.mark_room_read_to_latest(room_id).await
    }

    /// With a prefix chosen and a consent that covers this sink's kind, recipient, and
    /// endpoint, the delivered body quotes the room's newest message text under the room
    /// line.
    async fn assert_prefix_rendered(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let requests_before = self.stub.count();
        self.primary_sink_mut()?.targets = NotificationDeliveryTargets {
            direct_messages: true,
            ..NotificationDeliveryTargets::none()
        };
        self.set_primary_content(QA_DELIVERY_PREFIX, true)?;
        self.apply_settings("notification delivery prefix consent")
            .await?;
        if self.stored_prefix_consent().is_none() {
            return Err(
                "notification delivery prefix: a consent matching the sink was dropped".to_owned(),
            );
        }

        self.send_from_b(room_key_b, "qa-delivery-prefix", QA_DELIVERY_PREFIX_MARKER)
            .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix unread")
            .await?;
        self.wait_requests(requests_before + 1, "notification delivery prefix request")
            .await?;
        let request = self.request(requests_before)?;
        let label = self.dm_partner_name(room_id)?;
        assert_prefix_quoted(
            &request.body,
            "notification delivery prefix",
            &label,
            unread,
            QA_DELIVERY_PREFIX_MARKER,
        )?;
        println!("notification_delivery_prefix=ok");
        Ok(())
    }

    /// With a prefix chosen but no consent stored, the delivery still succeeds and carries
    /// none of the message text. The app reads the unconsented choice back as names only.
    async fn assert_prefix_without_consent(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let label = self.dm_partner_name(room_id)?;
        self.mark_room_read_to_latest(room_id).await?;
        let requests_before = self.stub.count();
        self.set_primary_content(QA_DELIVERY_PREFIX, false)?;
        self.apply_settings("notification delivery prefix without consent")
            .await?;
        if !self.stored_content_is_names_only_without_consent() {
            return Err(
                "notification delivery prefix no consent: an unconsented prefix did not read back as names only"
                    .to_owned(),
            );
        }

        self.send_from_b(
            room_key_b,
            "qa-delivery-prefix-no-consent",
            QA_DELIVERY_PREFIX_MARKER,
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix no consent unread")
            .await?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery prefix no consent request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_prefix_absent(
            &request,
            "notification delivery prefix no consent",
            &label,
            unread,
            QA_DELIVERY_PREFIX_MARKER,
        )?;
        // Positive control: the room list did hold the text, so what kept it out of the
        // body was the missing consent and never a missing message.
        self.assert_room_latest_text(room_id, QA_DELIVERY_PREFIX_MARKER)?;
        println!("notification_delivery_prefix_no_consent=ok");
        Ok(())
    }

    /// With names only chosen, a stored consent still renders nothing, and the delivery
    /// still succeeds.
    async fn assert_prefix_option_off(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let label = self.dm_partner_name(room_id)?;
        self.mark_room_read_to_latest(room_id).await?;
        let requests_before = self.stub.count();
        self.set_primary_content(NotificationDeliveryContent::NamesOnly, true)?;
        self.apply_settings("notification delivery prefix option off")
            .await?;
        // The acceptance is kept: what is off is the option, not the consent.
        let stored = self.stored_sink(self.sink_id);
        if !stored.is_some_and(|sink| {
            sink.content == NotificationDeliveryContent::NamesOnly && sink.content_consent.is_some()
        }) {
            return Err(
                "notification delivery prefix option off: the option and the stored consent did not read as off-but-consented"
                    .to_owned(),
            );
        }

        self.send_from_b(
            room_key_b,
            "qa-delivery-prefix-option-off",
            QA_DELIVERY_PREFIX_MARKER,
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix option off unread")
            .await?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery prefix option off request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_prefix_absent(
            &request,
            "notification delivery prefix option off",
            &label,
            unread,
            QA_DELIVERY_PREFIX_MARKER,
        )?;
        self.assert_room_latest_text(room_id, QA_DELIVERY_PREFIX_MARKER)?;
        println!("notification_delivery_prefix_option_off=ok");
        Ok(())
    }

    /// A newest event that yields no readable text still delivers, without a prefix.
    ///
    /// The event is a real file message whose caption is the marker, so its `body` is the
    /// marker while its message kind has no message text. A channel that read any body
    /// rather than only a readable text message would render it.
    async fn assert_prefix_without_readable_text(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let label = self.dm_partner_name(room_id)?;
        self.mark_room_read_to_latest(room_id).await?;
        let requests_before = self.stub.count();
        self.set_primary_content(QA_DELIVERY_PREFIX, true)?;
        self.apply_settings("notification delivery prefix non-text")
            .await?;

        self.send_media_from_b(
            room_key_b,
            "qa-delivery-prefix-media",
            QA_DELIVERY_PREFIX_MEDIA_CAPTION,
        )
        .await?;
        // Wait for the room list to read the file message as its newest event, so the
        // delivery is about that event and not the text message before it.
        self.wait_room_latest_preview(
            room_id,
            QA_DELIVERY_PREFIX_MEDIA_CAPTION,
            "notification delivery prefix media projection",
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix non-text unread")
            .await?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery prefix non-text request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_prefix_absent(
            &request,
            "notification delivery prefix non-text",
            &label,
            unread,
            QA_DELIVERY_PREFIX_MEDIA_CAPTION,
        )?;
        if request.body.contains(QA_DELIVERY_PREFIX_MARKER) {
            return Err(
                "notification delivery prefix non-text: the body carried the text message before the file message"
                    .to_owned(),
            );
        }
        // Positive controls: the room list's newest event carries the caption as its
        // preview and holds no readable text at all, so the absence is the event's own
        // kind.
        self.assert_room_latest_text_is_none(room_id)?;
        println!("notification_delivery_prefix_no_text=ok");
        Ok(())
    }

    /// Changing where a consented sink delivers drops the consent and returns the sink to
    /// names only, so the next delivery carries no message text.
    async fn assert_prefix_consent_cleared(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let label = self.dm_partner_name(room_id)?;
        self.mark_room_read_to_latest(room_id).await?;
        let requests_before = self.stub.count();
        // Set the option and a consent that covers the sink as it is, then change the
        // destination in the same patch: the app is the one that must drop the consent.
        self.set_primary_content(QA_DELIVERY_PREFIX, true)?;
        self.primary_sink_mut()?.destination = QA_DELIVERY_PREFIX_OTHER_DESTINATION.to_owned();
        self.apply_settings("notification delivery prefix changed destination")
            .await?;
        if !self.stored_content_is_names_only_without_consent() {
            return Err(
                "notification delivery prefix changed destination: the consent survived a different destination"
                    .to_owned(),
            );
        }

        self.send_from_b(
            room_key_b,
            "qa-delivery-prefix-destination",
            QA_DELIVERY_PREFIX_MARKER,
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix destination unread")
            .await?;
        self.wait_requests(
            requests_before + 1,
            "notification delivery prefix destination request",
        )
        .await?;
        let request = self.request(requests_before)?;
        assert_prefix_absent(
            &request,
            "notification delivery prefix changed destination",
            &label,
            unread,
            QA_DELIVERY_PREFIX_MARKER,
        )?;
        self.assert_room_latest_text(room_id, QA_DELIVERY_PREFIX_MARKER)?;
        println!("notification_delivery_prefix_consent_cleared=ok");
        Ok(())
    }

    /// The message text reaches no serialized artifact the run produces.
    ///
    /// The room-list `preview` is the sidebar's own serialized field and has always
    /// carried message text, so it is the one carrier this search excludes: the prefix
    /// source this feature adds is a separate field, and it must not join it. The
    /// positive control below reads that separate field from the live snapshot, so the
    /// search is over a state that really did hold the text.
    fn assert_prefix_hygiene(&self, room_id: &str) -> Result<(), String> {
        self.assert_room_latest_text(room_id, QA_DELIVERY_PREFIX_MARKER)?;
        let snapshot = self.conn_a.snapshot();

        let mut without_previews = snapshot.clone();
        for room in &mut without_previews.rooms {
            if let Some(latest) = &mut room.latest_event {
                latest.preview = None;
            }
        }
        let serialized = serde_json::to_string(&without_previews).map_err(|_| {
            "notification delivery prefix hygiene: the app-state snapshot is not serializable"
                .to_owned()
        })?;
        let full = serde_json::to_string(&snapshot).map_err(|_| {
            "notification delivery prefix hygiene: the app-state snapshot is not serializable"
                .to_owned()
        })?;
        let diagnostics = serde_json::to_string(&koushi_diagnostics::snapshot()).map_err(|_| {
            "notification delivery prefix hygiene: the diagnostics are not serializable".to_owned()
        })?;
        let settings_path = qa_data_dir("notification-delivery-a")
            .join("settings")
            .join("settings.json");
        let settings_file = std::fs::read(&settings_path).map_err(|_| {
            "notification delivery prefix hygiene: the settings file was not readable".to_owned()
        })?;

        for marker in [QA_DELIVERY_PREFIX_MARKER, QA_DELIVERY_PREFIX_MEDIA_CAPTION] {
            if serialized.contains(marker) {
                return Err(
                    "notification delivery prefix hygiene: the app-state snapshot carried message text"
                        .to_owned(),
                );
            }
            // The exact field shape this feature adds, searched in the untouched snapshot:
            // `preview` and the prefix source are different fields, so a marker under
            // `message_text` is the prefix source serializing itself.
            if full.contains(&format!("\"message_text\":\"{marker}\"")) {
                return Err(
                    "notification delivery prefix hygiene: the app-state snapshot serialized the prefix source"
                        .to_owned(),
                );
            }
            if diagnostics.contains(marker) {
                return Err(
                    "notification delivery prefix hygiene: the diagnostics stream carried message text"
                        .to_owned(),
                );
            }
            if bytes_contain(&settings_file, marker) {
                return Err(
                    "notification delivery prefix hygiene: the settings file carried message text"
                        .to_owned(),
                );
            }
        }

        let credential_root =
            std::env::var(super::registry::ENV_FILE_CREDENTIAL_STORE_DIR).map_err(|_| {
                "notification delivery prefix hygiene: the QA file credential store is not configured"
                    .to_owned()
            })?;
        for marker in [QA_DELIVERY_PREFIX_MARKER, QA_DELIVERY_PREFIX_MEDIA_CAPTION] {
            if directory_holds_text(std::path::Path::new(&credential_root), marker) {
                return Err(
                    "notification delivery prefix hygiene: a file the lane wrote carried message text"
                        .to_owned(),
                );
            }
        }
        println!("notification_delivery_prefix_hygiene=ok");
        Ok(())
    }

    /// Changing the endpoint a consent was given for drops that consent too.
    ///
    /// The endpoint is also what the stored credential was entered for, so the sink goes
    /// out of service as well: a fresh unread transition must leave no request at either
    /// endpoint, which is stronger than a body without a prefix.
    async fn assert_prefix_endpoint_cleared(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
        destination: &str,
    ) -> Result<(), String> {
        let moved = QaDeliveryStub::start(self.secret)?;
        self.mark_room_read_to_latest(room_id).await?;
        let requests_before = self.stub.count();
        // Bring the destination back and store a consent that covers that configuration,
        // so the endpoint change below is the one thing the next patch changes.
        self.primary_sink_mut()?.destination = destination.to_owned();
        self.set_primary_content(QA_DELIVERY_PREFIX, true)?;
        self.apply_settings("notification delivery prefix restore destination")
            .await?;
        if self.stored_prefix_consent().is_none() {
            return Err(
                "notification delivery prefix restore destination: a consent matching the sink was dropped"
                    .to_owned(),
            );
        }
        if let NotificationSinkConfig::Resend { endpoint } = &mut self.primary_sink_mut()?.config {
            *endpoint = moved.endpoint().to_owned();
        }
        self.apply_settings("notification delivery prefix changed endpoint")
            .await?;
        if !self.stored_content_is_names_only_without_consent() {
            return Err(
                "notification delivery prefix changed endpoint: the consent survived a different endpoint"
                    .to_owned(),
            );
        }

        self.send_from_b(
            room_key_b,
            "qa-delivery-prefix-endpoint",
            QA_DELIVERY_PREFIX_MARKER,
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery prefix endpoint unread")
            .await?;
        if unread == 0 {
            return Err(
                "notification delivery prefix changed endpoint: the room did not go unread again"
                    .to_owned(),
            );
        }
        settle(self.conn_a, Duration::from_millis(2500)).await;
        if moved.count() != 0 || self.stub.count() != requests_before {
            return Err(
                "notification delivery prefix changed endpoint: a request left the device for an endpoint the consent did not cover"
                    .to_owned(),
            );
        }
        println!("notification_delivery_prefix_endpoint_cleared=ok");
        Ok(())
    }

    /// The ServerChan Turbo destination (#1460), as a second sink beside the Resend one.
    ///
    /// The SendKey is synthetic and saved through the app's own typed command, and the
    /// provider is a loopback stub, so nothing leaves the machine. The phase asserts
    /// that saving a SendKey drops a text consent (the key's owner is the recipient),
    /// that the one request it makes carries the key only in its path and sends names
    /// and counts as a form, that the app reads the stub's answer as accepted, and that
    /// the key reaches no serialized artifact.
    async fn assert_serverchan(
        &mut self,
        room_id: &str,
        room_key_b: &TimelineKey,
    ) -> Result<(), String> {
        let stub = QaDeliveryStub::start(self.sendkey)?;
        self.mark_room_read_to_latest(room_id).await?;
        let resend_before = self.stub.count();
        self.primary_sink_mut()?.targets = NotificationDeliveryTargets::none();
        self.settings.sinks.push(NotificationSinkSettings {
            id: QA_DELIVERY_SERVERCHAN_SINK_ID.to_owned(),
            destination: String::new(),
            from_address: String::new(),
            sender_name: String::new(),
            config: NotificationSinkConfig::ServerChan {
                endpoint: stub.endpoint().to_owned(),
            },
            secret_binding: None,
            targets: NotificationDeliveryTargets {
                direct_messages: true,
                ..NotificationDeliveryTargets::none()
            },
            content: NotificationDeliveryContent::NamesOnly,
            content_consent: None,
            timing: NotificationDeliveryTiming {
                dwell_secs: QA_DELIVERY_DWELL_SECS,
                min_interval_secs: 0,
                only_when_unfocused: true,
            },
            direct_partner_label: NotificationDeliveryPartnerLabel::LocalAlias,
        });
        self.apply_settings("notification delivery serverchan install sink")
            .await?;
        self.store_sink_secret(QA_DELIVERY_SERVERCHAN_SINK_ID, self.sendkey.to_owned())
            .await?;
        println!("notification_delivery_serverchan_configured=ok");

        // Accept text for the SendKey bound right now, then save a SendKey again. Every
        // save mints a new credential revision, which names a different recipient, so
        // the app must drop the consent and return the destination to names only.
        self.set_sink_content(QA_DELIVERY_SERVERCHAN_SINK_ID, QA_DELIVERY_PREFIX, true)?;
        self.apply_settings("notification delivery serverchan consent")
            .await?;
        if self
            .stored_sink(QA_DELIVERY_SERVERCHAN_SINK_ID)
            .and_then(|sink| sink.content_consent)
            .is_none()
        {
            return Err(
                "notification delivery serverchan: a consent matching the bound SendKey was dropped"
                    .to_owned(),
            );
        }
        self.store_sink_secret(QA_DELIVERY_SERVERCHAN_SINK_ID, self.sendkey.to_owned())
            .await?;
        if !self
            .stored_sink(QA_DELIVERY_SERVERCHAN_SINK_ID)
            .is_some_and(|sink| {
                sink.content == NotificationDeliveryContent::NamesOnly
                    && sink.content_consent.is_none()
            })
        {
            return Err(
                "notification delivery serverchan: saving a SendKey kept the text consent"
                    .to_owned(),
            );
        }
        // The lane's copy follows the app: names only, no consent.
        self.set_sink_content(
            QA_DELIVERY_SERVERCHAN_SINK_ID,
            NotificationDeliveryContent::NamesOnly,
            false,
        )?;
        println!("notification_delivery_serverchan_sendkey_drops_consent=ok");

        self.send_from_b(
            room_key_b,
            "qa-delivery-serverchan",
            QA_DELIVERY_PREFIX_MARKER,
        )
        .await?;
        let unread = self
            .wait_room_unread(room_id, "notification delivery serverchan unread")
            .await?;
        {
            let stub = &stub;
            let conn = &mut *self.conn_a;
            wait_for_condition(
                conn,
                "notification delivery serverchan request",
                EVENT_TIMEOUT,
                |_| (stub.count() >= 1).then_some(()),
            )
            .await?;
        }
        let sink_id = QA_DELIVERY_SERVERCHAN_SINK_ID;
        wait_for_condition(
            &mut *self.conn_a,
            "notification delivery serverchan accepted",
            EVENT_TIMEOUT,
            |state| {
                state
                    .notification_delivery
                    .sinks
                    .iter()
                    .find(|sink| sink.sink_id == sink_id)
                    .and_then(|sink| sink.last_outcome)
                    .filter(|outcome| {
                        matches!(outcome, NotificationDeliveryOutcomeState::Accepted { .. })
                    })
                    .map(|_| ())
            },
        )
        .await?;
        settle(self.conn_a, Duration::from_millis(1500)).await;
        if stub.count() != 1 {
            return Err(format!(
                "notification delivery serverchan: expected 1 request at the ServerChan stub, saw {}",
                stub.count()
            ));
        }
        if self.stub.count() != resend_before {
            return Err(
                "notification delivery serverchan: the Resend sink sent although it selected nothing"
                    .to_owned(),
            );
        }
        let request = stub
            .requests()
            .into_iter()
            .next()
            .ok_or_else(|| "notification delivery serverchan: no request".to_owned())?;
        if !request.path_carries_secret || !request.form_encoded || request.authorization_present {
            return Err(
                "notification delivery serverchan: the request was not a form POST to {endpoint}/{SendKey}.send"
                    .to_owned(),
            );
        }
        let (Some(title), Some(desp)) =
            (request.form_title.as_deref(), request.form_desp.as_deref())
        else {
            return Err(
                "notification delivery serverchan: the form carried no title or desp".to_owned(),
            );
        };
        if !title.starts_with("Direct message from")
            || !desp.contains("Direct message from")
            || !desp.contains(&format!("{unread} unread"))
        {
            return Err(
                "notification delivery serverchan: the form did not name the room kind and its unread count"
                    .to_owned(),
            );
        }
        println!("notification_delivery_serverchan_request=ok");

        // Markdown escaping puts a backslash before each hyphen, so search for both
        // spellings of the marker.
        let escaped_marker = QA_DELIVERY_PREFIX_MARKER.replace('-', "\\-");
        let carries_text = [title, desp, request.body.as_str()].iter().any(|field| {
            field.contains(QA_DELIVERY_PREFIX_MARKER) || field.contains(&escaped_marker)
        });
        if carries_text
            || carries_matrix_identifier(title, Some(self.user_b))
            || carries_matrix_identifier(desp, Some(self.user_b))
        {
            return Err(
                "notification delivery serverchan: the form carried message text or a Matrix identifier without consent"
                    .to_owned(),
            );
        }
        // Positive control: the room list held the text, so the consent kept it out.
        self.assert_room_latest_text(room_id, QA_DELIVERY_PREFIX_MARKER)?;
        println!("notification_delivery_serverchan_no_text=ok");

        self.assert_serverchan_hygiene(&request)
    }

    /// The SendKey reaches only the request path: no snapshot, settings file,
    /// diagnostics record, captured body, or idempotency key carries it.
    fn assert_serverchan_hygiene(&self, request: &QaDeliveryStubRequest) -> Result<(), String> {
        let key = self.sendkey;
        let snapshot = serde_json::to_string(&self.conn_a.snapshot()).map_err(|_| {
            "notification delivery serverchan hygiene: the app-state snapshot is not serializable"
                .to_owned()
        })?;
        let diagnostics = serde_json::to_string(&koushi_diagnostics::snapshot()).map_err(|_| {
            "notification delivery serverchan hygiene: the diagnostics are not serializable"
                .to_owned()
        })?;
        let settings_path = qa_data_dir("notification-delivery-a")
            .join("settings")
            .join("settings.json");
        let settings_file = std::fs::read(&settings_path).map_err(|_| {
            "notification delivery serverchan hygiene: the settings file was not readable"
                .to_owned()
        })?;
        let leaked = snapshot.contains(key)
            || diagnostics.contains(key)
            || bytes_contain(&settings_file, key)
            || request.body.contains(key)
            || request
                .idempotency_key
                .as_deref()
                .is_some_and(|value| value.contains(key))
            || self
                .stub
                .requests()
                .iter()
                .any(|captured| captured.body.contains(key));
        if leaked || !request.path_mentions_secret {
            return Err(
                "notification delivery serverchan hygiene: the SendKey reached somewhere other than the request path"
                    .to_owned(),
            );
        }
        println!("notification_delivery_serverchan_secret_hygiene=ok");
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

    /// The name a digest gives a one-to-one direct message (#1492): the partner's full
    /// Matrix user id, because the scenario sets no local alias for them.
    fn dm_partner_name(&self, room_id: &str) -> Result<String, String> {
        let snapshot = self.conn_a.snapshot();
        let room = snapshot
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .ok_or_else(|| "notification delivery: the direct message is not listed".to_owned())?;
        match room.dm_user_ids.as_slice() {
            [partner] if room.is_dm && partner == self.user_b => Ok(partner.clone()),
            _ => Err(
                "notification delivery: the room is not a direct message with the sender"
                    .to_owned(),
            ),
        }
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

    /// The one sink this lane configures.
    fn primary_sink(&self) -> Result<&NotificationSinkSettings, String> {
        self.settings
            .sinks
            .first()
            .ok_or_else(|| "notification delivery prefix: the lane has no sink".to_owned())
    }

    fn primary_sink_mut(&mut self) -> Result<&mut NotificationSinkSettings, String> {
        self.settings
            .sinks
            .first_mut()
            .ok_or_else(|| "notification delivery prefix: the lane has no sink".to_owned())
    }

    /// Choose the primary sink's content mode, with or without a consent that covers the
    /// sink as it is configured right now - what the user gives when they accept text in
    /// plaintext. The timestamp is synthetic: this lane asserts the binding, not the clock.
    fn set_primary_content(
        &mut self,
        content: NotificationDeliveryContent,
        consented: bool,
    ) -> Result<(), String> {
        let sink_id = self.sink_id;
        self.set_sink_content(sink_id, content, consented)
    }

    fn set_sink_content(
        &mut self,
        sink_id: &str,
        content: NotificationDeliveryContent,
        consented: bool,
    ) -> Result<(), String> {
        // A ServerChan consent names the bound SendKey revision, so the lane's copy must
        // carry the binding the app stored before the consent is computed from it.
        self.sync_bindings();
        let sink = self
            .settings
            .sink_mut(sink_id)
            .ok_or_else(|| "notification delivery: the lane has no such sink".to_owned())?;
        sink.content = content;
        sink.content_consent = consented.then(|| sink.consent_for_current(0));
        Ok(())
    }

    /// Carry the app's stored `secret_binding` into the lane's copy. `UpdateSettings`
    /// replaces the whole delivery block, and the binding is Rust's own field rather
    /// than one the lane may choose, so a patch must carry it through, never clear it.
    fn sync_bindings(&mut self) {
        let current = self.conn_a.snapshot().settings.values.delivery.clone();
        for sink in &mut self.settings.sinks {
            sink.secret_binding = current
                .sink(&sink.id)
                .and_then(|existing| existing.secret_binding.clone());
        }
    }

    /// One sink as the app's own state holds it right now.
    fn stored_sink(&self, sink_id: &str) -> Option<NotificationSinkSettings> {
        self.conn_a
            .snapshot()
            .settings
            .values
            .delivery
            .sink(sink_id)
            .cloned()
    }

    /// The primary sink's content consent as the app's own state holds it, if any.
    fn stored_prefix_consent(&self) -> Option<NotificationDeliveryContentConsent> {
        self.stored_sink(self.sink_id)
            .and_then(|sink| sink.content_consent)
    }

    /// Whether the app holds the primary sink at names only with no consent, which is
    /// what Rust guarantees after a consent stops matching.
    fn stored_content_is_names_only_without_consent(&self) -> bool {
        self.stored_sink(self.sink_id).is_some_and(|sink| {
            sink.content == NotificationDeliveryContent::NamesOnly && sink.content_consent.is_none()
        })
    }

    /// The room's newest event id, which is what the room list holds right now.
    fn room_latest_event_id(&self, room_id: &str) -> Result<String, String> {
        self.conn_a
            .snapshot()
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .and_then(|room| room.latest_event.as_ref())
            .map(|latest| latest.event_id.clone())
            .ok_or_else(|| "notification delivery prefix: the room has no latest event".to_owned())
    }

    /// The delivery prefix source the room list holds for the room, if any. It is the
    /// field the feature adds and never serializes, so reading it here is the positive
    /// control a hygiene search needs to be about a state that held the text.
    fn room_latest_text(&self, room_id: &str) -> Option<String> {
        self.conn_a
            .snapshot()
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .and_then(|room| room.latest_event.as_ref())
            .and_then(|latest| latest.message_text.clone())
    }

    fn assert_room_latest_text(&self, room_id: &str, expected: &str) -> Result<(), String> {
        if self.room_latest_text(room_id).as_deref() != Some(expected) {
            return Err(
                "notification delivery prefix: the room list did not hold the message text the delivery was about"
                    .to_owned(),
            );
        }
        Ok(())
    }

    fn assert_room_latest_text_is_none(&self, room_id: &str) -> Result<(), String> {
        if self.room_latest_text(room_id).is_some() {
            return Err(
                "notification delivery prefix: the room list held readable text for an event that has none"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Mark the room read through its newest event, which is how every prefix assertion
    /// ends the episode before it arms the next one.
    async fn mark_room_read_to_latest(&mut self, room_id: &str) -> Result<(), String> {
        let event_id = self.room_latest_event_id(room_id)?;
        self.mark_room_read(room_id, &event_id).await
    }

    /// Wait until the room list's newest event carries `preview`, so a delivery is known
    /// to be about that event rather than the one before it.
    async fn wait_room_latest_preview(
        &mut self,
        room_id: &str,
        preview: &str,
        label: &str,
    ) -> Result<(), String> {
        let room = room_id.to_owned();
        let expected = preview.to_owned();
        let conn = &mut *self.conn_a;
        wait_for_condition(conn, label, EVENT_TIMEOUT, move |state| {
            state
                .rooms
                .iter()
                .find(|summary| summary.room_id == room)
                .and_then(|summary| summary.latest_event.as_ref())
                .filter(|latest| latest.preview.as_deref() == Some(expected.as_str()))
                .map(|_| ())
        })
        .await
    }

    /// Send one non-text (file) message as user B with `caption` as its body, and return
    /// the event id the homeserver accepted.
    ///
    /// A media message carries its caption in `body` but is not a text message, so the
    /// room list's `preview` shows the caption while the delivery prefix source has
    /// nothing: the newest event exists, is unread, and yields no readable text.
    async fn send_media_from_b(
        &mut self,
        key: &TimelineKey,
        transaction_id: &str,
        caption: &str,
    ) -> Result<String, String> {
        let info =
            authenticated_session_info(self.conn_b, "notification delivery prefix media account")?;
        let expected_account = koushi_core::store::session_key_id_from_info(&info);
        let request_id = self.conn_b.next_request_id();
        self.conn_b
            .command(CoreCommand::Timeline(TimelineCommand::UploadAndSendMedia {
                request_id,
                expected_account,
                key: key.clone(),
                transaction_id: transaction_id.to_owned(),
                request: UploadMediaRequest {
                    filename: "qa-notification-prefix-media.bin".to_owned(),
                    mime_type: "application/octet-stream".to_owned(),
                    bytes: b"koushi qa notification prefix fixture".to_vec(),
                    kind: UploadMediaKind::File,
                    compression: None,
                    thumbnail: None,
                    caption: Some(build_formatted_message_draft(
                        caption,
                        MentionIntent::default(),
                    )),
                },
            }))
            .await
            .map_err(|_| format!("{transaction_id}: media send submission failed"))?;
        let event_id = wait_for_media_send_flow_completion(
            self.conn_b,
            request_id,
            key,
            transaction_id,
            transaction_id,
        )
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
    // #1492: a direct message is named by its partner, so the partner's user id is the
    // one Matrix identifier the body may carry, and only as the room's name.
    let partner = (expectation.room_label == expectation.user_b).then_some(expectation.user_b);
    for (what, needle) in [
        ("the room id", Some(expectation.room_id)),
        ("the event id", Some(expectation.event_id)),
        ("the message text", Some(expectation.message_body)),
        ("the device owner's account id", Some(expectation.user_a)),
        (
            "the sender's account id",
            partner.is_none().then_some(expectation.user_b),
        ),
    ] {
        if needle.is_some_and(|needle| body.contains(needle)) {
            return Err(format!("{label}: the body carried {what}"));
        }
    }
    if carries_matrix_identifier(body, partner) {
        return Err(format!("{label}: the body carried a Matrix identifier"));
    }
    Ok(())
}

/// Whether any whitespace-delimited token in `body` is a Matrix identifier other than
/// `partner`, the direct-message partner a digest names (#1492). Every other identifier
/// is what the lane's private-data rule forbids in anything it emits.
fn carries_matrix_identifier(body: &str, partner: Option<&str>) -> bool {
    body.split_whitespace().any(|token| {
        let token = token.trim_end_matches([':', ',', '.']);
        matches!(token.chars().next(), Some('@' | '!' | '$'))
            && token.contains(':')
            && Some(token) != partner
    })
}

/// Assert the delivered body names the room with its own counts and quotes `marker`,
/// the room's newest message text, on a `> ` line under that room line.
fn assert_prefix_quoted(
    body: &str,
    context: &str,
    room_label: &str,
    unread: u64,
    marker: &str,
) -> Result<(), String> {
    let headline = format!("Direct message from {room_label}");
    let Some(marker_at) = body.find(&format!("> {marker}")) else {
        return Err(format!(
            "{context}: the body carried none of the room's newest message text as a quoted line"
        ));
    };
    let Some(headline_at) = body.find(&headline) else {
        return Err(format!(
            "{context}: the body did not name the room by its safe display label"
        ));
    };
    // #1460: the room's own line comes first and its message text is quoted under it.
    if marker_at <= headline_at {
        return Err(format!(
            "{context}: the message text was not quoted under the room's own line"
        ));
    }
    if !body.contains(&format!("{unread} unread")) {
        return Err(format!(
            "{context}: the body did not carry the room's unread count"
        ));
    }
    if carries_matrix_identifier(body, Some(room_label)) {
        return Err(format!("{context}: the body carried a Matrix identifier"));
    }
    Ok(())
}

/// Assert a delivery that must not carry message text still succeeded, and that
/// `marker` appears nowhere in what the stub recorded: the body it received or the
/// idempotency key it correlated the request by.
fn assert_prefix_absent(
    request: &QaDeliveryStubRequest,
    context: &str,
    room_label: &str,
    unread: u64,
    marker: &str,
) -> Result<(), String> {
    let headline = format!("Direct message from {room_label}");
    if !request.body.contains(&headline) {
        return Err(format!(
            "{context}: the delivery did not reach the stub naming the room"
        ));
    }
    if !request.body.contains(&format!("{unread} unread")) {
        return Err(format!(
            "{context}: the delivery did not reach the stub with the room's unread count"
        ));
    }
    if request.body.contains(marker) {
        return Err(format!("{context}: the received body carried message text"));
    }
    if request
        .idempotency_key
        .as_deref()
        .is_some_and(|key| key.contains(marker))
    {
        return Err(format!(
            "{context}: the received idempotency key carried message text"
        ));
    }
    Ok(())
}

/// Whether `needle` appears in `haystack` as raw bytes.
fn bytes_contain(haystack: &[u8], needle: &str) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
}

/// Whether any file under `dir` contains `needle` as raw bytes.
///
/// The lane's own artifacts are plain files, so a byte walk is an honest read of "did
/// this text reach a file this run wrote". It compares only against the lane's own
/// synthetic value and prints nothing about any file it reads.
fn directory_holds_text(dir: &std::path::Path, needle: &str) -> bool {
    let mut pending = vec![dir.to_path_buf()];
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
            && bytes_contain(&contents, needle)
        {
            return true;
        }
    }
    false
}

/// The settings every phase starts from: one Resend sink pointing at the stub, selecting
/// no target until a phase selects one.
fn delivery_settings(stub: &QaDeliveryStub, sink_id: &str) -> NotificationDeliverySettings {
    NotificationDeliverySettings {
        sinks: vec![NotificationSinkSettings {
            id: sink_id.to_owned(),
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
            targets: NotificationDeliveryTargets::none(),
            content: NotificationDeliveryContent::NamesOnly,
            content_consent: None,
            timing: NotificationDeliveryTiming {
                dwell_secs: QA_DELIVERY_DWELL_SECS,
                min_interval_secs: 0,
                only_when_unfocused: true,
            },
            direct_partner_label: NotificationDeliveryPartnerLabel::LocalAlias,
        }],
    }
}

/// The delivery block as the lane owns it.
///
/// `secret_binding` is Rust's own field: the lane carries the stored value through a
/// patch, but it cannot predict whether canonicalization keeps or drops it, so an
/// equality check over what the lane chose must ignore it. Canonicalization is also what
/// decides whether a prefix consent still covers the sink the lane configured, so the
/// expected value is canonicalized exactly the way the app canonicalizes the state: a
/// patch that changes a destination or an endpoint is expected to drop the consent, and
/// the equality check must expect that drop rather than fight it.
fn lane_visible_delivery(settings: &NotificationDeliverySettings) -> NotificationDeliverySettings {
    let mut copy = settings.clone();
    copy.canonicalize();
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
    /// Whether the request path is exactly ServerChan's `{endpoint}/{SendKey}.send` for
    /// the expected secret. The path itself is never kept: it carries the key.
    path_carries_secret: bool,
    /// Whether the expected secret appears anywhere in the request path.
    path_mentions_secret: bool,
    /// Whether the body was sent as `application/x-www-form-urlencoded`.
    form_encoded: bool,
    body: String,
    /// The decoded `title` and `desp` fields of a form body, if it was one.
    form_title: Option<String>,
    form_desp: Option<String>,
}

/// The provider stand-in: a loopback listener that answers any request with a 2xx and
/// ServerChan's `{"code":0}`, so either sink kind records `Accepted`, and keeps what the
/// lane asserts about.
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
        let expected_secret = expected_secret.to_owned();
        let thread_requests = Arc::clone(&requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Some(request) = read_stub_request(stream, &expected_secret)
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
fn read_stub_request(
    mut stream: TcpStream,
    expected_secret: &str,
) -> Option<QaDeliveryStubRequest> {
    let expected = format!("Bearer {expected_secret}");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let request_line = read_stub_line(&mut reader)?;
    let path = request_line.split_whitespace().nth(1).unwrap_or_default();
    let path_carries_secret = path == format!("{QA_STUB_PATH}/{expected_secret}.send");
    let path_mentions_secret = path.contains(expected_secret);
    let mut form_encoded = false;
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
            "content-type" => {
                form_encoded = value
                    .to_ascii_lowercase()
                    .starts_with("application/x-www-form-urlencoded");
            }
            "idempotency-key" => idempotency_key = Some(value.to_owned()),
            _ => {}
        }
    }
    let mut body = vec![0_u8; content_length.min(1024 * 1024)];
    if !body.is_empty() {
        reader.read_exact(&mut body).ok()?;
    }
    // Resend reads any 2xx as accepted; ServerChan also needs `code` 0.
    let answer = "{\"code\":0,\"id\":\"qa-stub\"}";
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
            answer.len()
        )
        .as_bytes(),
    );
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
    let body = String::from_utf8_lossy(&body).into_owned();
    let (form_title, form_desp) = if form_encoded {
        (form_field(&body, "title"), form_field(&body, "desp"))
    } else {
        (None, None)
    };
    Some(QaDeliveryStubRequest {
        authorization_present,
        authorization_matches,
        idempotency_key,
        path_carries_secret,
        path_mentions_secret,
        form_encoded,
        body,
        form_title,
        form_desp,
    })
}

/// One decoded field of an `application/x-www-form-urlencoded` body.
fn form_field(body: &str, name: &str) -> Option<String> {
    body.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (form_decode(key) == name).then(|| form_decode(value))
    })
}

/// Undo form encoding: `+` is a space and `%XX` is one byte.
fn form_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        decoded.push(byte);
                        index += 2;
                    }
                    None => decoded.push(b'%'),
                }
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn read_stub_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    Some(line)
}
