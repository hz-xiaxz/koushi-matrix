//! `scheduled_send` ownership for AccountActor.

use koushi_key::SessionKeyId;
use koushi_sdk::MatrixClientSession;
use koushi_state::{
    AppAction, ComposerDraftRevision, ScheduledSendCapability, ScheduledSendHandle,
    ScheduledSendItem,
};
use tokio::sync::mpsc;

use crate::failure::{CoreFailure, TimelineFailureKind};
use crate::ids::RequestId;
use crate::runtime::ForwardedComposerDraftPermit;
use crate::timeline::composer::build_room_message_content_from_composer_body;

use super::actor::AccountActor;

fn scheduled_dispatch_targets_active_session(
    active_session_key: Option<&SessionKeyId>,
    origin_session_key: &SessionKeyId,
) -> bool {
    active_session_key == Some(origin_session_key)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthoritativeRoomEncryption {
    Unknown,
    Unencrypted,
    Encrypted,
}

fn user_content_is_admitted(encryption: AuthoritativeRoomEncryption) -> bool {
    match encryption {
        AuthoritativeRoomEncryption::Unknown => false,
        AuthoritativeRoomEncryption::Unencrypted => true,
        AuthoritativeRoomEncryption::Encrypted => true,
    }
}

fn server_delayed_events_are_safe(encryption: AuthoritativeRoomEncryption) -> bool {
    encryption == AuthoritativeRoomEncryption::Unencrypted
}

pub(super) async fn admit_secure_backup_user_content(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<(matrix_sdk::Room, AuthoritativeRoomEncryption), TimelineFailureKind> {
    let room_id = matrix_sdk::ruma::RoomId::parse(room_id)
        .map_err(|_| TimelineFailureKind::SecureBackupRequired)?;
    let room = session
        .client()
        .get_room(&room_id)
        .ok_or(TimelineFailureKind::SecureBackupRequired)?;
    let encryption = match room.latest_encryption_state().await {
        Ok(state) if state.is_encrypted() => AuthoritativeRoomEncryption::Encrypted,
        Ok(_) => AuthoritativeRoomEncryption::Unencrypted,
        Err(_) => AuthoritativeRoomEncryption::Unknown,
    };
    user_content_is_admitted(encryption)
        .then_some((room, encryption))
        .ok_or(TimelineFailureKind::SecureBackupRequired)
}

fn build_scheduled_message_content(
    body: &str,
    thread_root_event_id: Option<&str>,
) -> Result<matrix_sdk::ruma::events::room::message::RoomMessageEventContent, TimelineFailureKind> {
    use matrix_sdk::ruma::events::{relation::Thread, room::message::Relation};

    let mut content = build_room_message_content_from_composer_body(
        body,
        koushi_state::MentionIntent::default(),
    )?;
    if let Some(root_event_id) = thread_root_event_id {
        let root_event_id = matrix_sdk::ruma::EventId::parse(root_event_id)
            .map_err(|_| TimelineFailureKind::Sdk)?;
        content.relates_to = Some(Relation::Thread(Thread::plain(
            root_event_id.clone(),
            root_event_id,
        )));
    }
    Ok(content)
}

async fn send_scheduled_acceptance_actions(
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    actions: Vec<AppAction>,
    mut composer_permit: ForwardedComposerDraftPermit,
) {
    composer_permit.acceptance_projection_reached();
    if action_tx.send(actions).await.is_ok() {
        composer_permit.acceptance_enqueued();
    }
}

impl AccountActor {
    pub(super) async fn handle_schedule_server_delayed_send(
        &self,
        request_id: RequestId,
        expected_account: SessionKeyId,
        scheduled_id: String,
        room_id: String,
        thread_root_event_id: Option<String>,
        body: String,
        send_at_ms: u64,
        draft_revision: ComposerDraftRevision,
        composer_permit: ForwardedComposerDraftPermit,
    ) {
        if self.session_key_id.as_ref() != Some(&expected_account) {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        }
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        let (_, encryption) = match admit_secure_backup_user_content(session, &room_id).await {
            Ok(admitted) => admitted,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::TimelineOperationFailed { kind });
                return;
            }
        };

        let capability = crate::scheduled_send::detect_capability(&session.client()).await;
        // The delayed-event endpoint accepts event content directly; a homeserver
        // cannot perform client-side room encryption. Encrypted rooms therefore
        // always use the local scheduler, whose eventual Room::send crosses the
        // exact-session secure-backup fence.
        if server_delayed_events_are_safe(encryption)
            && capability == ScheduledSendCapability::ServerDelayedEvents
        {
            match self
                .send_server_delayed_message(
                    session,
                    &room_id,
                    thread_root_event_id.as_deref(),
                    &body,
                    send_at_ms,
                )
                .await
            {
                Ok(delay_id) => {
                    send_scheduled_acceptance_actions(
                        &self.action_tx,
                        vec![
                            AppAction::ScheduledSendCapabilityChanged {
                                capability: ScheduledSendCapability::ServerDelayedEvents,
                            },
                            AppAction::ScheduledSendCreatedAtRevision {
                                item: ScheduledSendItem {
                                    scheduled_id,
                                    room_id,
                                    thread_root_event_id,
                                    body,
                                    send_at_ms,
                                    handle: ScheduledSendHandle::Server { delay_id },
                                    is_dispatching: false,
                                },
                                draft_revision,
                            },
                        ],
                        composer_permit,
                    )
                    .await;
                    return;
                }
                Err(()) => {}
            }
        }

        send_scheduled_acceptance_actions(
            &self.action_tx,
            vec![
                AppAction::ScheduledSendCapabilityChanged {
                    capability: ScheduledSendCapability::LocalFallback,
                },
                AppAction::ScheduledSendCreatedAtRevision {
                    item: ScheduledSendItem {
                        scheduled_id,
                        room_id,
                        thread_root_event_id,
                        body,
                        send_at_ms,
                        handle: ScheduledSendHandle::Local,
                        is_dispatching: false,
                    },
                    draft_revision,
                },
            ],
            composer_permit,
        )
        .await;
    }

    pub(super) async fn handle_dispatch_local_scheduled_send(
        &self,
        request_id: RequestId,
        origin_session_key: SessionKeyId,
        scheduled_id: String,
        room_id: String,
        thread_root_event_id: Option<String>,
        body: String,
    ) {
        let retry_at_ms = crate::scheduled_send::local_scheduled_send_retry_at_ms();
        if !scheduled_dispatch_targets_active_session(
            self.session_key_id.as_ref(),
            &origin_session_key,
        ) {
            self.retry_local_scheduled_send(scheduled_id, retry_at_ms)
                .await;
            return;
        }
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            self.retry_local_scheduled_send(scheduled_id, retry_at_ms)
                .await;
            return;
        };
        let room = match admit_secure_backup_user_content(session, &room_id).await {
            Ok((room, _)) => room,
            Err(_) => {
                self.retry_local_scheduled_send(scheduled_id, retry_at_ms)
                    .await;
                return;
            }
        };
        let content = match build_scheduled_message_content(&body, thread_root_event_id.as_deref())
        {
            Ok(content) => content,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::TimelineOperationFailed { kind });
                self.retry_local_scheduled_send(scheduled_id, retry_at_ms)
                    .await;
                return;
            }
        };
        let transaction_id = matrix_sdk::ruma::OwnedTransactionId::from(
            crate::scheduled_send::scheduled_send_transaction_id(&scheduled_id),
        );
        match room.send(content).with_transaction_id(transaction_id).await {
            Ok(_) => {
                self.send_actions(vec![AppAction::ScheduledSendDispatched { scheduled_id }])
                    .await;
            }
            Err(_) => {
                self.emit_failure(
                    request_id,
                    CoreFailure::TimelineOperationFailed {
                        kind: TimelineFailureKind::Sdk,
                    },
                );
                self.retry_local_scheduled_send(scheduled_id, retry_at_ms)
                    .await;
            }
        }
    }

    async fn retry_local_scheduled_send(&self, scheduled_id: String, retry_at_ms: u64) {
        self.send_actions(vec![AppAction::ScheduledSendDispatchFailed {
            scheduled_id,
            retry_at_ms,
        }])
        .await;
    }

    pub(super) async fn handle_cancel_server_delayed_send(
        &self,
        request_id: RequestId,
        scheduled_id: String,
        delay_id: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match self
            .update_server_delayed_event(
                session,
                delay_id,
                matrix_sdk::ruma::api::client::delayed_events::update_delayed_event::unstable::UpdateAction::Cancel,
            )
            .await
        {
            Ok(()) => {
                self.send_actions(vec![AppAction::ScheduledSendCancelled { scheduled_id }])
                    .await;
            }
            Err(()) => self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            ),
        }
    }

    pub(super) async fn handle_reschedule_server_delayed_send(
        &self,
        request_id: RequestId,
        scheduled_id: String,
        room_id: String,
        thread_root_event_id: Option<String>,
        body: String,
        delay_id: String,
        send_at_ms: u64,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        let (_, encryption) = match admit_secure_backup_user_content(session, &room_id).await {
            Ok(admitted) => admitted,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::TimelineOperationFailed { kind });
                return;
            }
        };

        if self
            .update_server_delayed_event(
                session,
                delay_id,
                matrix_sdk::ruma::api::client::delayed_events::update_delayed_event::unstable::UpdateAction::Cancel,
            )
            .await
            .is_err()
        {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
            return;
        }

        if encryption == AuthoritativeRoomEncryption::Encrypted {
            self.send_actions(vec![
                AppAction::ScheduledSendCapabilityChanged {
                    capability: ScheduledSendCapability::LocalFallback,
                },
                AppAction::ScheduledSendRescheduled {
                    scheduled_id,
                    body,
                    send_at_ms,
                    handle: ScheduledSendHandle::Local,
                },
            ])
            .await;
            return;
        }

        match self
            .send_server_delayed_message(
                session,
                &room_id,
                thread_root_event_id.as_deref(),
                &body,
                send_at_ms,
            )
            .await
        {
            Ok(delay_id) => {
                self.send_actions(vec![AppAction::ScheduledSendRescheduled {
                    scheduled_id,
                    body,
                    send_at_ms,
                    handle: ScheduledSendHandle::Server { delay_id },
                }])
                .await;
            }
            Err(()) => {
                self.send_actions(vec![
                    AppAction::ScheduledSendCapabilityChanged {
                        capability: ScheduledSendCapability::LocalFallback,
                    },
                    AppAction::ScheduledSendRescheduled {
                        scheduled_id,
                        body,
                        send_at_ms,
                        handle: ScheduledSendHandle::Local,
                    },
                ])
                .await;
            }
        }
    }

    async fn send_server_delayed_message(
        &self,
        session: &MatrixClientSession,
        room_id: &str,
        thread_root_event_id: Option<&str>,
        body: &str,
        send_at_ms: u64,
    ) -> Result<String, ()> {
        use matrix_sdk::ruma::TransactionId;
        use matrix_sdk::ruma::api::client::delayed_events::{
            DelayParameters, delayed_message_event,
        };

        let room_id = matrix_sdk::ruma::RoomId::parse(room_id).map_err(|_| ())?;
        let content =
            build_scheduled_message_content(body, thread_root_event_id).map_err(|_| ())?;
        let request = delayed_message_event::unstable::Request::new(
            room_id,
            TransactionId::new(),
            DelayParameters::Timeout {
                timeout: crate::scheduled_send::server_delay_timeout(
                    send_at_ms,
                    crate::time::current_epoch_ms(),
                ),
            },
            &content,
        )
        .map_err(|_| ())?;

        session
            .client()
            .send(request)
            .await
            .map(|response| response.delay_id)
            .map_err(|_| ())
    }

    async fn update_server_delayed_event(
        &self,
        session: &MatrixClientSession,
        delay_id: String,
        action: matrix_sdk::ruma::api::client::delayed_events::update_delayed_event::unstable::UpdateAction,
    ) -> Result<(), ()> {
        let request =
            matrix_sdk::ruma::api::client::delayed_events::update_delayed_event::unstable::Request::new(
                delay_id, action,
            );
        session
            .client()
            .send(request)
            .await
            .map(|_| ())
            .map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use koushi_key::SessionKeyId;
    use koushi_sdk::MatrixClientSession;
    use koushi_state::{AppAction, ScheduledSendHandle, ScheduledSendItem, SessionInfo};

    use tokio::sync::{mpsc, oneshot};

    use super::{
        AuthoritativeRoomEncryption, build_scheduled_message_content,
        scheduled_dispatch_targets_active_session, send_scheduled_acceptance_actions,
        server_delayed_events_are_safe, user_content_is_admitted,
    };
    use crate::account::actor::{AccountActorHandle, AccountMessage};
    use crate::account::test_support::{
        recv_account_action_with_sliding_sync_effects, spawn_actor_with_dirs_and_registry,
    };
    use crate::command::AccountCommand;
    use crate::composer_draft_lifecycle::ComposerDraftLeaseRegistry;

    use crate::ids::{RequestId, RuntimeConnectionId};

    use crate::runtime::ForwardedComposerDraftPermit;

    use matrix_sdk::test_utils::mocks::MatrixMockServer;

    use tempfile::tempdir;
    use wiremock::ResponseTemplate;

    #[test]
    fn secure_backup_room_admission_fails_closed_without_authoritative_metadata() {
        assert!(!user_content_is_admitted(
            AuthoritativeRoomEncryption::Unknown
        ));
    }

    #[test]
    fn encrypted_room_admission_does_not_wait_for_secure_backup() {
        assert!(user_content_is_admitted(
            AuthoritativeRoomEncryption::Unencrypted
        ));
        assert!(user_content_is_admitted(
            AuthoritativeRoomEncryption::Encrypted
        ));
    }

    #[test]
    fn server_delayed_events_are_used_only_for_authoritatively_unencrypted_rooms() {
        assert!(server_delayed_events_are_safe(
            AuthoritativeRoomEncryption::Unencrypted
        ));
        assert!(!server_delayed_events_are_safe(
            AuthoritativeRoomEncryption::Encrypted
        ));
        assert!(!server_delayed_events_are_safe(
            AuthoritativeRoomEncryption::Unknown
        ));
    }

    #[test]
    fn secure_backup_barrier_covers_normal_and_scheduled_user_content_routes() {
        for (source, start) in [
            (
                include_str!("routing.rs"),
                "async fn route_timeline_command_with_permit_and_formatting_options",
            ),
            (
                include_str!("scheduled_send.rs"),
                "async fn handle_schedule_server_delayed_send",
            ),
            (
                include_str!("scheduled_send.rs"),
                "async fn handle_dispatch_local_scheduled_send",
            ),
            (
                include_str!("scheduled_send.rs"),
                "async fn handle_reschedule_server_delayed_send",
            ),
        ] {
            let route = crate::account::test_source::item_body(source, start);
            assert!(
                route.contains("admit_secure_backup_user_content"),
                "{start} must use the authoritative AccountActor barrier"
            );
        }

        let reschedule = crate::account::test_source::item_body(
            include_str!("scheduled_send.rs"),
            "async fn handle_reschedule_server_delayed_send",
        );
        let barrier = reschedule
            .find("admit_secure_backup_user_content")
            .expect("reschedule barrier");
        let cancellation = reschedule
            .find("UpdateAction::Cancel")
            .expect("delayed-event cancellation");
        assert!(
            barrier < cancellation,
            "a rejected reschedule must leave the existing delayed event intact"
        );
    }

    #[test]
    fn local_scheduled_room_send_does_not_use_per_session_backup_durability_fence() {
        let handler = crate::account::test_source::item_body(
            include_str!("scheduled_send.rs"),
            "async fn handle_dispatch_local_scheduled_send",
        );
        assert!(
            !handler.contains(".require_backed_up_session()"),
            "direct Room::send must let Secure Backup upload follow asynchronously"
        );
    }

    #[tokio::test]
    async fn scheduled_acceptance_retains_exact_permit_until_reducer_delivery() {
        let account = SessionKeyId {
            homeserver: "https://schedule.example.test".to_owned(),
            user_id: "@schedule:example.test".to_owned(),
            device_id: "SCHEDULE".to_owned(),
        };
        let target = koushi_state::ComposerTarget::Main {
            room_id: "!scheduled:example.test".to_owned(),
        };
        let scope = crate::composer_draft_lifecycle::ComposerDraftScope {
            account: account.clone(),
            target: target.clone(),
        };
        let registry = Arc::new(crate::composer_draft_lifecycle::ComposerDraftLeaseRegistry::new());
        let renderer_generation = registry
            .begin_renderer_generation()
            .expect("begin renderer generation");
        let lease_id = registry
            .acquire(renderer_generation, scope.clone())
            .expect("acquire scheduled-send lease");
        let command_permit = registry
            .try_command_permit(renderer_generation, lease_id, &scope)
            .expect("admit scheduled-send command");
        let app_pending_permit = command_permit.clone();
        registry
            .release(renderer_generation, lease_id)
            .expect("release activation after command admission");

        let request_id = RequestId {
            connection_id: RuntimeConnectionId(44),
            sequence: 9,
        };
        let (rejected_tx, mut rejected_rx) = mpsc::unbounded_channel();
        let (acceptance_probe_tx, acceptance_probe_rx) = oneshot::channel();
        let forwarded_permit = ForwardedComposerDraftPermit::new_with_acceptance_probe(
            request_id,
            command_permit,
            rejected_tx,
            acceptance_probe_tx,
        );
        let (action_tx, mut action_rx) = mpsc::channel(1);
        action_tx
            .try_send(Vec::new())
            .expect("fill reducer action lane");
        let send = tokio::spawn(async move {
            send_scheduled_acceptance_actions(
                &action_tx,
                vec![AppAction::ScheduledSendCreatedAtRevision {
                    item: ScheduledSendItem {
                        scheduled_id: "scheduled-permit".to_owned(),
                        room_id: "!scheduled:example.test".to_owned(),
                        thread_root_event_id: None,
                        body: "synthetic body".to_owned(),
                        send_at_ms: 1,
                        handle: ScheduledSendHandle::Local,
                        is_dispatching: false,
                    },
                    draft_revision: 4.into(),
                }],
                forwarded_permit,
            )
            .await;
        });

        acceptance_probe_rx
            .await
            .expect("account schedule reached acceptance projection");
        assert_eq!(
            registry.protected_targets(&account),
            BTreeSet::from([target.clone()]),
            "the AccountActor schedule permit must protect the exact blocked target"
        );
        assert!(
            action_rx
                .recv()
                .await
                .expect("reducer lane marker")
                .is_empty(),
            "the first action only opens the deterministic reducer barrier"
        );
        let acceptance = action_rx.recv().await.expect("scheduled acceptance action");
        assert!(matches!(
            acceptance.as_slice(),
            [AppAction::ScheduledSendCreatedAtRevision { item, .. }]
                if item.scheduled_id == "scheduled-permit"
        ));
        send.await.expect("scheduled acceptance sender");
        assert_eq!(
            registry.protected_targets(&account),
            BTreeSet::from([target.clone()]),
            "the AppActor pending clone must outlive schedule acceptance enqueue"
        );

        let mut changes = registry.subscribe();
        changes.borrow_and_update();
        drop(app_pending_permit);
        changes
            .changed()
            .await
            .expect("matching reducer release notification");
        assert!(
            registry.protected_targets(&account).is_empty(),
            "the scheduled target becomes eligible only after reducer delivery"
        );
        assert!(
            rejected_rx.try_recv().is_err(),
            "successful scheduled acceptance must disarm rejection cleanup"
        );
    }

    #[tokio::test]
    async fn stale_server_delayed_schedule_is_rejected_after_account_replacement() {
        let server = MatrixMockServer::new().await;
        server
            .mock_versions()
            .with_feature(crate::scheduled_send::MSC4140_FEATURE, true)
            .with_feature("org.matrix.simplified_msc3575", true)
            .ok()
            .mount()
            .await;
        server
            .mock_room_send()
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "delay_id": "synthetic-delay" })),
            )
            .mount()
            .await;

        let old_account = SessionKeyId {
            homeserver: server.uri(),
            user_id: "@old-schedule:localhost".to_owned(),
            device_id: "OLD-SCHEDULE".to_owned(),
        };
        let replacement_account = SessionKeyId {
            homeserver: server.uri(),
            user_id: "@replacement-schedule:localhost".to_owned(),
            device_id: "REPLACEMENT-SCHEDULE".to_owned(),
        };
        let old_session = MatrixClientSession::from_client_for_testing(
            server.client_builder().build().await,
            SessionInfo {
                homeserver: old_account.homeserver.clone(),
                user_id: old_account.user_id.clone(),
                device_id: old_account.device_id.clone(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        );
        let replacement_session = MatrixClientSession::from_client_for_testing(
            server.client_builder().build().await,
            SessionInfo {
                homeserver: replacement_account.homeserver.clone(),
                user_id: replacement_account.user_id.clone(),
                device_id: replacement_account.device_id.clone(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        );
        let credential_dir = tempdir().expect("credential tempdir");
        let data_dir = tempdir().expect("data tempdir");
        let registry = Arc::new(ComposerDraftLeaseRegistry::new());
        let (handle, mut action_rx, _event_rx) = spawn_actor_with_dirs_and_registry(
            credential_dir.path(),
            data_dir.path(),
            registry.clone(),
        );

        configure_synthetic_oidc_session(
            &handle,
            old_session,
            RequestId {
                connection_id: RuntimeConnectionId(51),
                sequence: 1,
            },
        )
        .await;
        loop {
            let actions =
                recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx).await;
            if matches!(
                actions.as_slice(),
                [AppAction::LoginSucceeded { info, .. }]
                    if info.user_id == old_account.user_id
                        && info.device_id == old_account.device_id
            ) {
                break;
            }
        }

        let target = koushi_state::ComposerTarget::Main {
            room_id: "!scheduled-owner:localhost".to_owned(),
        };
        let scope = crate::composer_draft_lifecycle::ComposerDraftScope {
            account: old_account.clone(),
            target: target.clone(),
        };
        let renderer_generation = registry
            .begin_renderer_generation()
            .expect("begin old-account renderer generation");
        let lease_id = registry
            .acquire(renderer_generation, scope.clone())
            .expect("acquire old-account schedule lease");
        let command_permit = registry
            .try_command_permit(renderer_generation, lease_id, &scope)
            .expect("admit old-account schedule command");
        let app_pending_permit = command_permit.clone();
        registry
            .release(renderer_generation, lease_id)
            .expect("release activation after schedule admission");
        let request_id = RequestId {
            connection_id: RuntimeConnectionId(51),
            sequence: 3,
        };
        let (rejected_tx, mut rejected_rx) = mpsc::unbounded_channel();
        let forwarded_permit =
            ForwardedComposerDraftPermit::new(request_id, command_permit, rejected_tx);

        configure_synthetic_oidc_session(
            &handle,
            replacement_session,
            RequestId {
                connection_id: RuntimeConnectionId(51),
                sequence: 2,
            },
        )
        .await;
        loop {
            let actions =
                recv_account_action_with_sliding_sync_effects(&handle, &mut action_rx).await;
            if matches!(
                actions.as_slice(),
                [AppAction::LoginSucceeded { info, .. }]
                    if info.user_id == replacement_account.user_id
                        && info.device_id == replacement_account.device_id
            ) {
                break;
            }
        }
        assert!(
            handle
                .send(AccountMessage::ScheduleServerDelayedSend {
                    request_id,
                    expected_account: old_account.clone(),
                    scheduled_id: "stale-server-schedule".to_owned(),
                    room_id: "!scheduled-owner:localhost".to_owned(),
                    thread_root_event_id: None,
                    body: "synthetic stale scheduled body".to_owned(),
                    send_at_ms: crate::time::current_epoch_ms().saturating_add(60_000),
                    draft_revision: 4.into(),
                    composer_permit: forwarded_permit,
                })
                .await
        );
        let (barrier_tx, barrier_rx) = oneshot::channel();
        assert!(
            handle
                .send(AccountMessage::InspectSessionRuntime {
                    response: barrier_tx,
                })
                .await
        );
        barrier_rx
            .await
            .expect("account actor mailbox barrier after stale schedule");

        let delayed_request_count = server
            .received_requests()
            .await
            .expect("recorded Matrix requests")
            .into_iter()
            .filter(|request| {
                request
                    .url
                    .query_pairs()
                    .any(|(key, _)| key == "org.matrix.msc4140.delay")
            })
            .count();
        assert_eq!(
            delayed_request_count, 0,
            "an old-account schedule must not reach the replacement account SDK"
        );
        let queued_actions = std::iter::from_fn(|| action_rx.try_recv().ok())
            .flatten()
            .collect::<Vec<_>>();
        assert!(
            !queued_actions.iter().any(|action| matches!(
                action,
                AppAction::ScheduledSendCapabilityChanged { .. }
                    | AppAction::ScheduledSendCreatedAtRevision { .. }
            )),
            "an old-account schedule must not enqueue an acceptance for the replacement account"
        );
        assert_eq!(
            rejected_rx.try_recv(),
            Ok(request_id),
            "the rejected schedule must settle AppActor's pending permit"
        );
        assert_eq!(
            registry.protected_targets(&old_account),
            BTreeSet::from([target]),
            "the AppActor pending clone remains protected until rejection cleanup"
        );
        drop(app_pending_permit);
        assert!(
            registry.protected_targets(&old_account).is_empty(),
            "the rejected old-account permit must not leak target protection"
        );

        let _ = handle.send(AccountMessage::Shutdown).await;
    }

    #[test]
    fn scheduled_thread_message_content_preserves_the_thread_relation() {
        let content = build_scheduled_message_content(
            "scheduled thread body",
            Some("$thread-root:example.invalid"),
        )
        .expect("thread content should build");
        let value = serde_json::to_value(content).expect("content should serialize");

        assert_eq!(value["m.relates_to"]["rel_type"], "m.thread");
        assert_eq!(
            value["m.relates_to"]["event_id"],
            "$thread-root:example.invalid"
        );
    }

    #[test]
    fn scheduled_dispatch_targets_its_origin_session() {
        let origin = SessionKeyId {
            homeserver: "https://example.test".to_owned(),
            user_id: "@alice:example.test".to_owned(),
            device_id: "ALICE".to_owned(),
        };
        let switched = SessionKeyId {
            homeserver: "https://example.test".to_owned(),
            user_id: "@bob:example.test".to_owned(),
            device_id: "BOB".to_owned(),
        };

        assert!(scheduled_dispatch_targets_active_session(
            Some(&origin),
            &origin
        ));
        assert!(!scheduled_dispatch_targets_active_session(
            Some(&switched),
            &origin
        ));
        assert!(!scheduled_dispatch_targets_active_session(None, &origin));
    }

    async fn configure_synthetic_oidc_session(
        handle: &AccountActorHandle,
        session: MatrixClientSession,
        request_id: RequestId,
    ) {
        let homeserver = session.info.homeserver.clone();
        assert!(
            handle
                .send(AccountMessage::ConfigureTrustObservation {
                    observation: koushi_sdk::CurrentDeviceTrustObservation {
                        current: koushi_state::CurrentDeviceTrustState::Unknown,
                        updates: Box::pin(futures_util::stream::pending()),
                    },
                })
                .await
        );
        assert!(
            handle
                .send(AccountMessage::ConfigureOidcCompletion {
                    start_request_id: request_id,
                    homeserver,
                    session,
                })
                .await
        );
        assert!(
            handle
                .send(AccountMessage::Command(AccountCommand::CompleteOidcLogin {
                    request_id,
                    callback_url: "http://127.0.0.1/callback?code=fixture&state=fixture".to_owned(),
                    platform: koushi_state::DisplayPlatform::Linux,
                }))
                .await
        );
    }
}
