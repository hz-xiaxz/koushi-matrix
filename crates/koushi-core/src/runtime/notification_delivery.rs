//! #1427: the AppActor's half of the device-local delivery channel.
//!
//! The decision engine ([`crate::notification_delivery`]) answers when a room may be
//! delivered and what the message says; this file answers the three questions that need a
//! runtime: which rooms the projection currently reports, which sinks hold a credential, and
//! how one send ends. It owns the qualifying-set build, the channel's state, one timer on the
//! core executor's clock, and the spawned sends.
//!
//! A send is the only place a sink secret is read, and it is read inside the spawned task: the
//! value never enters `AppState`, the channel, the actor, a log line, or an event. The actor
//! only ever learns the typed outcome.

use std::time::Duration;

use koushi_notify::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
    NotificationSink, ResendSink, ServerChanSink, SmtpSettings, SmtpSink,
    validate_serverchan_sendkey,
};
use koushi_protocol::{CoreFailure, NotificationDeliveryFailureKind};
use koushi_state::{
    AppAction, NotificationDeliverySettings, NotificationSinkConfig, NotificationSinkSettings,
};

use super::AppActor;
use crate::executor::{self, AbortOnDrop};
use crate::notification_delivery::channel::{NotificationDeliveryJob, random_hex_token};
use crate::notification_delivery::digest::{NotificationDeliveryIntent, test_delivery_intent};
use crate::store::StoreActor;

/// How many sends may be in flight at once.
///
/// One per sink in the settings, which the settings loader already bounds; the cap is a
/// backstop so a hand-built settings value cannot spawn an unbounded number of tasks.
pub(super) const MAX_INFLIGHT_NOTIFICATION_DELIVERY_SENDS: usize = 8;

/// What one spawned send reports back to the actor.
///
/// The variants are exactly what the actor can act on: an outcome to record, or a reason no
/// outcome could be reached at all. Each carries the idempotency key of the attempt it settles,
/// so the channel matches it to that attempt and to nothing else. No variant carries the secret,
/// the destination, or the body.
pub(super) enum NotificationDeliverySendResult {
    /// The sink's own typed verdict.
    Outcome {
        sink_id: String,
        idempotency_key: String,
        outcome: NotificationDeliveryOutcome,
    },
    /// The credential store holds no secret for this sink any more. Nothing was sent, and the
    /// sink is re-read as unconfigured rather than left reading as ON.
    SecretMissing {
        sink_id: String,
        idempotency_key: String,
    },
    /// The credential store could not be read at all. Nothing was sent.
    StoreUnavailable {
        sink_id: String,
        idempotency_key: String,
    },
}

/// Send one intent through one sink, reading the credential here and nowhere else.
///
/// The secret is loaded onto this task's own stack, handed to the sink, and dropped with it: it
/// is never returned to the actor, never stored, and never rendered into a failure. A sink with
/// no stored secret is reported as such instead of being attempted with an empty one.
pub(super) async fn send_notification_delivery(
    intent: NotificationDeliveryIntent,
    sink_settings: NotificationSinkSettings,
    store: StoreActor,
) -> NotificationDeliverySendResult {
    let sink_id = intent.sink_id.clone();
    let idempotency_key = intent.idempotency_key.clone();
    let secret = match store.load_notification_sink_secret(&sink_id) {
        Ok(Some(secret)) => secret,
        Ok(None) => {
            return NotificationDeliverySendResult::SecretMissing {
                sink_id,
                idempotency_key,
            };
        }
        Err(_) => {
            return NotificationDeliverySendResult::StoreUnavailable {
                sink_id,
                idempotency_key,
            };
        }
    };
    // The configuration is re-read at send time, from the settings, so an edit between the
    // decision and the send cannot put the old body on a new destination.
    let sink = notification_sink_for(&sink_settings);
    let request = NotificationDeliveryRequest {
        subject: intent.subject,
        body: intent.body,
        destination: intent.destination,
        from_address: intent.from_address,
        sender_name: intent.sender_name,
        idempotency_key: intent.idempotency_key,
    };
    NotificationDeliverySendResult::Outcome {
        sink_id,
        idempotency_key,
        outcome: sink.send(&secret, &request).await,
    }
}

/// Build the sender for one sink's non-secret configuration.
fn notification_sink_for(settings: &NotificationSinkSettings) -> NotificationSink {
    match &settings.config {
        NotificationSinkConfig::Resend { endpoint } => {
            NotificationSink::Resend(ResendSink::with_endpoint(endpoint.clone()))
        }
        NotificationSinkConfig::ServerChan { endpoint } => {
            NotificationSink::ServerChan(ServerChanSink::with_endpoint(endpoint.clone()))
        }
        NotificationSinkConfig::Smtp {
            host,
            port,
            tls,
            username,
            ..
        } => NotificationSink::Smtp(SmtpSink::new(SmtpSettings {
            host: host.clone(),
            port: *port,
            tls: match tls {
                koushi_state::SmtpTls::Implicit => koushi_notify::SmtpTls::Implicit,
                koushi_state::SmtpTls::StartTls => koushi_notify::SmtpTls::StartTls,
            },
            username: username.clone(),
        })),
    }
}

impl AppActor {
    /// Whether the tab this account is rendered in is in the foreground.
    ///
    /// Per account tab, this is "this account's tab is focused", not "the process has a window
    /// somewhere": the runtime keeps one window focus flag per account tab, the adapter observes
    /// the selected tab's window and reports through `ObserveNativeWindowFocus`, and the
    /// attention projection already reads the same flag to suppress the OS notification. So this
    /// is not a new signal. An unknown focus reads as focused - `NativeAttentionContext::default()`
    /// starts there - and a destination whose timing says "only when inactive" then delivers
    /// nothing, so a device that has not said anything yet fails closed rather than mailing a
    /// notification the user is already looking at.
    fn notification_delivery_window_focused(&self) -> bool {
        self.state.native_attention_context.window_focused
    }

    /// The delay until the channel next needs the actor, following the composer-debounce and
    /// scheduled-send timers: one sleep on the core executor's clock, no task per room.
    pub(super) fn notification_delivery_wake_delay(&self) -> Option<Duration> {
        let wake_at = self
            .notification_delivery
            .next_wake_at(&self.state.settings.values.delivery)?;
        Some(
            wake_at
                .checked_duration_since(executor::Instant::now())
                .unwrap_or_default(),
        )
    }

    /// Observe the attention projection, feed the dwell, and start the sends that are due.
    ///
    /// This is the one entry point, called at the top of every actor loop iteration - the same
    /// place the badge and the toast candidate are already up to date - and from the delivery
    /// timer. It returns whether the delivery projection the frontend renders changed, so the
    /// caller publishes exactly once per change.
    pub(super) async fn observe_notification_delivery(&mut self) -> bool {
        let now = executor::Instant::now();
        let settings = self.state.settings.values.delivery.clone();
        self.notification_delivery.retain_sinks(&settings);
        let mut changed = false;
        if !self.notification_delivery.configured_sinks_match(&settings) {
            changed |= self.refresh_notification_delivery_configured().await;
        }
        if !settings.sinks.iter().any(|sink| sink.targets.has_any())
            && !self.notification_delivery.has_pending_work()
        {
            // No destination sends for anything and nothing is held: building the candidate set
            // would only prove that nothing qualifies.
            return changed;
        }

        // The candidate set is the badge's own per-room projection, so read/unread and mute
        // semantics cannot drift from the badge; each lane applies its destination's targets,
        // focus rule, and content mode on top of it. A room read on another device leaves the
        // set on the sync that reports the read, which is what cancels it here.
        let candidates = koushi_state::attention_room_candidates_for_state(&self.state);
        let focused = self.notification_delivery_window_focused();
        self.notification_delivery
            .observe(&settings, now, &candidates, focused);

        let mut jobs = self.notification_delivery.take_due_retries(&settings, now);
        jobs.extend(self.notification_delivery.take_send_jobs(&settings, now));
        self.spawn_notification_delivery_sends(jobs, &settings);
        changed | self.refresh_notification_delivery_projection().await
    }

    /// Read which sinks hold a usable credential, and publish the projection if it moved.
    ///
    /// The credential store is read here rather than on the dispatch path, and only when the
    /// settings' sink list or a sink's binding changed: the canon reads a secret when a sink is
    /// configured and when the delivery actor starts, never per observation.
    ///
    /// A stored credential is only usable for the server and account it was entered for, and the
    /// binding in the settings is the record of which those were. A sink with no binding therefore
    /// counts as unconfigured, and its stored credential is deleted here: this is not garbage
    /// collection, it is a credential that no longer belongs to any destination this sink has, and
    /// leaving it would let the next send offer the previous server's password to the new one.
    pub(super) async fn refresh_notification_delivery_configured(&mut self) -> bool {
        let settings = self.state.settings.values.delivery.clone();
        for sink in &settings.sinks {
            let store = self.composer_draft_store_actor.clone();
            let sink_id = sink.id.clone();
            let bound = sink.secret_binding.is_some();
            let (configured, deleted) = executor::spawn_blocking(move || {
                if !store
                    .notification_sink_secret_configured(&sink_id)
                    .unwrap_or(false)
                {
                    // No credential at all: nothing was retired, and nothing is configured.
                    return (false, false);
                }
                if !bound {
                    let _ = store.delete_notification_sink_secret(&sink_id);
                    return (false, true);
                }
                (true, false)
            })
            .await
            .unwrap_or((false, false));
            self.notification_delivery
                .record_credential(sink, configured);
            if deleted {
                self.notification_delivery.mark_credential_retired(&sink.id);
            } else if configured {
                // A credential that is usable for this configuration is not a retired one, whoever
                // saved it: the flag describes the record in the store, not the history of edits.
                self.notification_delivery
                    .clear_credential_retired(&sink.id);
            }
        }
        self.refresh_notification_delivery_projection().await
    }

    /// Record the outcome of one send and publish the projection if it moved.
    pub(super) async fn record_notification_delivery_outcome(
        &mut self,
        result: NotificationDeliverySendResult,
    ) -> bool {
        let settings = self.state.settings.values.delivery.clone();
        let at_ms = crate::time::current_epoch_ms();
        match result {
            NotificationDeliverySendResult::Outcome {
                sink_id,
                idempotency_key,
                outcome,
            } => {
                self.notification_delivery.record_outcome(
                    &settings,
                    executor::Instant::now(),
                    &sink_id,
                    &idempotency_key,
                    &outcome,
                    at_ms,
                );
            }
            NotificationDeliverySendResult::SecretMissing {
                sink_id,
                idempotency_key,
            } => {
                // The credential went away between the observation and the send. The sink is no
                // longer one this device can send through, and it must not keep reading as ON.
                self.notification_delivery
                    .abandon_inflight(&sink_id, &idempotency_key);
                for sink in &settings.sinks {
                    if sink.id == sink_id {
                        self.notification_delivery.record_credential(sink, false);
                    }
                }
            }
            NotificationDeliverySendResult::StoreUnavailable {
                sink_id,
                idempotency_key,
            } => {
                self.notification_delivery
                    .abandon_inflight(&sink_id, &idempotency_key);
                // A credential store that cannot be read is a transport failure from the
                // sender's point of view: no verdict was reached and nothing left the device.
                // Recording it keeps a broken store from leaving the sink reading as ON.
                for sink in &settings.sinks {
                    if sink.id == sink_id {
                        self.notification_delivery.record_credential(sink, false);
                    }
                }
                self.notification_delivery.record_test_outcome(
                    &sink_id,
                    &NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
                    at_ms,
                );
            }
        }
        self.refresh_notification_delivery_projection().await
    }

    /// Reduce the whole delivery projection into app state, and say whether it moved.
    ///
    /// The projection is app scope and never persisted: it is what this run of the app has
    /// observed about the credential store and the last sends, and a restart re-observes it.
    pub(super) async fn refresh_notification_delivery_projection(&mut self) -> bool {
        let settings = self.state.settings.values.delivery.clone();
        let projection = self.notification_delivery.projection(&settings);
        if self.state.notification_delivery == projection {
            return false;
        }
        let effects = koushi_state::reduce(
            &mut self.state,
            AppAction::NotificationDeliveryObserved {
                delivery: projection,
            },
        );
        self.handle_ui_event_effects(&effects).await;
        true
    }

    /// Spawn one task per send, keeping one owner per task so shutdown cannot leak one.
    fn spawn_notification_delivery_sends(
        &mut self,
        jobs: Vec<NotificationDeliveryJob>,
        settings: &NotificationDeliverySettings,
    ) {
        for job in jobs {
            let Some(sink) = settings.sink(&job.sink_id) else {
                continue;
            };
            if sink.secret_binding.is_none() {
                // Belt and braces: the digest only names a sink the channel read a credential for,
                // and that read requires a binding. A sink that lost its binding between the two
                // must not reach a relay with the credential the previous configuration stored.
                continue;
            }
            self.notification_delivery_sends
                .retain(|(_, task)| !task.get().is_finished());
            if self.notification_delivery_sends.len() >= MAX_INFLIGHT_NOTIFICATION_DELIVERY_SENDS {
                // The job's lane still names it in flight; release it so the window is retried
                // on a later observation instead of waiting for an outcome that never comes.
                self.notification_delivery
                    .abandon_inflight(&job.sink_id, &job.intent.idempotency_key);
                continue;
            }
            self.notification_delivery_sends.push((
                job.sink_id.clone(),
                spawn_notification_delivery_send(
                    job.intent,
                    sink.clone(),
                    self.composer_draft_store_actor.clone(),
                    self.notification_delivery_outcome_tx.clone(),
                ),
            ));
        }
    }

    /// Store one sink's secret and bind it to the configuration it was entered for, then publish
    /// the projection that says the sink is configured.
    ///
    /// The value is handed to the credential store inside a blocking task and is not kept here:
    /// the actor learns only that the write succeeded. The binding is written into the app-scoped
    /// settings, derived from the canonical configuration the state holds, so the user never types
    /// it and the frontend never sends it.
    pub(super) async fn set_notification_delivery_secret(
        &mut self,
        request_id: koushi_protocol::RequestId,
        sink_id: &str,
        secret: String,
    ) -> Result<(), CoreFailure> {
        let Some(sink) = self.state.settings.values.delivery.sink(sink_id) else {
            return Err(CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::UnknownSink,
            });
        };
        if matches!(sink.config, NotificationSinkConfig::ServerChan { .. })
            && validate_serverchan_sendkey(&secret).is_err()
        {
            // A ServerChan³ key or a value that is not a Turbo SendKey would only ever be
            // refused by the service, so it is refused here instead of being stored.
            return Err(CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::UnsupportedSendKey,
            });
        }
        // Every save mints a new revision, so the binding names this credential and not just
        // this configuration: a consent given for the previous SendKey no longer matches, and a
        // late outcome of a send made with the previous credential settles nothing.
        let binding = sink.credential_binding(&random_hex_token());
        self.abort_notification_delivery_sends(sink_id);
        let store = self.composer_draft_store_actor.clone();
        let save_sink_id = sink_id.to_owned();
        let saved = executor::spawn_blocking(move || {
            store.save_notification_sink_secret(&save_sink_id, &secret)
        })
        .await;
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error.to_core_failure()),
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        }
        self.write_notification_delivery_binding(request_id, sink_id, Some(binding))
            .await;
        // A new password is not a removed one, so the line the frontend draws for a retired
        // credential goes away as soon as the user saves a replacement.
        self.notification_delivery.clear_credential_retired(sink_id);
        // The verdict comes from the same read the observation uses, so a binding that did not
        // land leaves the sink unconfigured instead of ON behind a credential nothing can use.
        self.refresh_notification_delivery_configured().await;
        Ok(())
    }

    /// Forget one sink's secret and the binding that named its configuration.
    pub(super) async fn clear_notification_delivery_secret(
        &mut self,
        request_id: koushi_protocol::RequestId,
        sink_id: &str,
    ) -> Result<(), CoreFailure> {
        self.require_notification_delivery_sink(sink_id)?;
        self.abort_notification_delivery_sends(sink_id);
        let store = self.composer_draft_store_actor.clone();
        let delete_sink_id = sink_id.to_owned();
        let deleted = executor::spawn_blocking(move || {
            store.delete_notification_sink_secret(&delete_sink_id)
        })
        .await;
        match deleted {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error.to_core_failure()),
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        }
        self.write_notification_delivery_binding(request_id, sink_id, None)
            .await;
        // The user asked for this removal, so it is not something to explain back to them.
        self.notification_delivery.clear_credential_retired(sink_id);
        self.refresh_notification_delivery_configured().await;
        Ok(())
    }

    /// Abort every send in flight for one sink, before its credential is replaced or removed.
    ///
    /// A send reads its credential when it starts, so one that is still running after a save
    /// may be using the credential the user just replaced; dropping its owner aborts it. The
    /// lane it was settling is dropped with the binding change, so no outcome is waited for.
    fn abort_notification_delivery_sends(&mut self, sink_id: &str) {
        self.notification_delivery_sends
            .retain(|(owner, task)| owner != sink_id && !task.get().is_finished());
    }

    /// Write or clear one sink's credential binding in the app-scoped settings.
    ///
    /// This goes through the reducer like every other settings edit, and the patch is persisted
    /// under the command's own request id so the binding survives a restart. Clearing the field
    /// is how a cleared secret stops being usable: canonicalization accepts a binding only while
    /// it names the current configuration, and `None` never does.
    async fn write_notification_delivery_binding(
        &mut self,
        request_id: koushi_protocol::RequestId,
        sink_id: &str,
        binding: Option<String>,
    ) {
        let mut delivery = self.state.settings.values.delivery.clone();
        let Some(sink) = delivery.sinks.iter_mut().find(|sink| sink.id == sink_id) else {
            return;
        };
        if sink.secret_binding == binding {
            return;
        }
        sink.secret_binding = binding;
        let patch = koushi_state::SettingsPatch {
            delivery: Some(*delivery),
            ..koushi_state::SettingsPatch::default()
        };
        let effects = self
            .reduce_app_action(AppAction::SettingsUpdateRequested {
                request_id: request_id.sequence,
                patch,
            })
            .await;
        self.handle_app_effects(request_id, effects).await;
    }

    /// Refuse a sink the settings do not hold, because a secret written for it would have no
    /// owner and no sink to configure.
    fn require_notification_delivery_sink(&self, sink_id: &str) -> Result<(), CoreFailure> {
        if self.state.settings.values.delivery.sink(sink_id).is_some() {
            return Ok(());
        }
        Err(CoreFailure::NotificationDeliveryRejected {
            kind: NotificationDeliveryFailureKind::UnknownSink,
        })
    }

    /// Settle a delivery command: a success commits, and every outcome already went through the
    /// projection; a refusal is the command's only failure.
    ///
    /// Nothing is emitted for a success beyond the delta the projection published, because the
    /// projection *is* the answer to "did it work" and the frontend re-renders it.
    pub(super) async fn settle_notification_delivery_command(
        &mut self,
        request_id: koushi_protocol::RequestId,
        result: Result<(), CoreFailure>,
    ) {
        use koushi_protocol::event::{CoreEvent, IntentOutcome};
        match result {
            Ok(()) => {
                let before_state = self.snapshot_tx.borrow().state.clone();
                let published_generation = self.publish_state_change(&before_state);
                self.emit(CoreEvent::IntentLifecycle {
                    request_id,
                    outcome: IntentOutcome::Committed,
                    published_generation,
                });
            }
            Err(failure) => self.emit(CoreEvent::OperationFailed {
                request_id,
                failure,
            }),
        }
    }

    /// The window id for one test send, unique per press and per run so a provider never sees
    /// two test messages under one idempotency key.
    pub(super) fn next_notification_delivery_test_window(&mut self, sink_id: &str) -> String {
        self.notification_delivery.next_test_window_id(sink_id)
    }

    /// Send one canned test message through one sink, and record its typed outcome.
    ///
    /// A test send is not a delivery: it consumes no coalescing window, arms no retry, and
    /// leaves the dwell and the held rooms alone, so proving a sink works can never suppress or
    /// delay a real notification. It still goes through the same sink path and the same outcome
    /// projection, which is the only way the test can prove anything.
    pub(super) async fn dispatch_notification_delivery_test(
        &mut self,
        sink_id: &str,
        window_id: String,
    ) -> Result<(), CoreFailure> {
        let settings = self.state.settings.values.delivery.clone();
        let Some(sink) = settings.sink(sink_id).cloned() else {
            return Err(CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::UnknownSink,
            });
        };
        if sink.secret_binding.is_none() {
            // Nothing to prove and no credential this configuration may use: a test send would
            // either be attempted with a password entered for another server, which is exactly
            // what the binding exists to prevent, or fail for a reason the user cannot act on.
            return Err(CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::SecretMissing,
            });
        }
        let intent = test_delivery_intent(&sink, &window_id);
        let store = self.composer_draft_store_actor.clone();
        let configured = executor::spawn_blocking({
            let store = store.clone();
            let sink_id = sink_id.to_owned();
            move || {
                store
                    .notification_sink_secret_configured(&sink_id)
                    .unwrap_or(false)
            }
        })
        .await
        .unwrap_or(false);
        if !configured {
            // A sink with no secret has nothing to prove, and the product says so instead of
            // attempting a send the provider would reject.
            self.notification_delivery.record_credential(&sink, false);
            self.refresh_notification_delivery_projection().await;
            return Err(CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::SecretMissing,
            });
        }

        self.notification_delivery_sends
            .retain(|(_, task)| !task.get().is_finished());
        self.notification_delivery_sends.push((
            sink_id.to_owned(),
            spawn_notification_delivery_send(
                intent,
                sink,
                store,
                self.notification_delivery_outcome_tx.clone(),
            ),
        ));
        Ok(())
    }
}

/// Own one send: run it, report its result on the actor's outcome channel, and abort it if the
/// actor goes away before it finishes. The handle is what keeps the task from outliving the
/// actor, so it is always retained in `AppActor::notification_delivery_sends`.
fn spawn_notification_delivery_send(
    intent: NotificationDeliveryIntent,
    sink: NotificationSinkSettings,
    store: StoreActor,
    outcome_tx: tokio::sync::mpsc::Sender<NotificationDeliverySendResult>,
) -> AbortOnDrop<()> {
    AbortOnDrop::new(executor::spawn(async move {
        let result = send_notification_delivery(intent, sink, store).await;
        let _ = outcome_tx.send(result).await;
    }))
}
