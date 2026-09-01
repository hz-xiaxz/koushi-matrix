//! Test-only support for issue #532 room-subscription residency checks.
//!
//! This module is intentionally absent from default builds.  The harness is
//! filled in with production actor probes as each RED check is admitted; it
//! never contains a second residency policy.

#![cfg(any(test, feature = "test-hooks"))]

use std::sync::Arc;
use std::time::Duration;

use crate::account::{AccountActor, AccountActorHandle};
use crate::composer_draft_lifecycle::ComposerDraftLeaseRegistry;
use crate::link_preview::LinkPreviewContext;
use crate::room::{
    RoomActor, RoomActorHandle, RoomMessage, RoomOperationKind, RoomOperationTestControl,
};
use crate::store::StoreActor;
use crate::timeline::{
    RoomMembershipTransition, RoomMembershipTransitionKind, TimelineManagerActor,
    VisibleRoomObservation,
};
use crate::{RequestId, RuntimeConnectionId};
use koushi_protocol::TimelineKey;
use koushi_protocol::command::RoomCommand;
use koushi_protocol::event::{CoreEvent, RoomEvent};
use koushi_protocol::failure::{CoreFailure, RoomFailureKind};
use koushi_store::{CredentialStoreBackend, FileCredentialStore};

use koushi_diagnostics::DiagnosticValue;
use koushi_sdk::MatrixClientSession;
use koushi_state::{SessionAuthenticationMethod, SessionInfo};
use matrix_sdk_ui::room_list_service::RoomListService;
use tempfile::{TempDir, tempdir};
use tokio::sync::{broadcast, mpsc, oneshot};

/// A private-safe, synthetic snapshot used by the residency integration lane.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencySnapshot {
    pub desired_rooms: Vec<String>,
    pub active_rooms: Vec<String>,
    pub tombstoned_rooms: Vec<String>,
    pub actor_count: usize,
    pub lease_count: usize,
    pub sdk_generation: u64,
    pub last_trigger: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencyOperationProbe {
    pub old_manager_alive: bool,
    pub replacement_completed: bool,
    pub acknowledgement_before_replacement: bool,
    pub settlement_before_replacement: bool,
    pub late_terminal_after_replacement: bool,
    pub mismatch_probe: bool,
    pub sdk_call_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencyBindingProbe {
    pub room_session: Option<String>,
    pub bound_session: Option<String>,
    pub pointer_equal: bool,
    pub mismatch_probe: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencyAckLossProbe {
    pub operation_failed_sdk_count: usize,
    pub room_left_count: usize,
    pub success_action_count: usize,
    pub ack_diagnostic_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencyGateProbe {
    pub accepting_after_close: bool,
    pub active_count_after_close: usize,
    pub new_admission_rejected: bool,
    pub drain_completed: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSubscriptionResidencyTeardownProbe {
    pub binding_cleared: bool,
    pub post_clear_admission_rejected: bool,
    pub post_clear_failure_is_sdk: bool,
    pub operation_control_reached_count: usize,
    pub shutdown_incomplete_while_gap_held: bool,
    pub shutdown_incomplete_while_permit_held: bool,
    pub acknowledgement_before_shutdown: bool,
    pub settlement_before_shutdown: bool,
    pub shutdown_completed: bool,
    pub matching_terminal_count: usize,
    pub no_late_terminal: bool,
}

struct ResidencyAccountFixture {
    _credential_dir: TempDir,
    _data_dir: TempDir,
    handle: AccountActorHandle,
    _action_rx: mpsc::Receiver<Vec<koushi_state::AppAction>>,
    _event_rx: broadcast::Receiver<CoreEvent>,
}

impl ResidencyAccountFixture {
    fn spawn() -> Self {
        let credential_dir = tempdir().expect("credential tempdir");
        let data_dir = tempdir().expect("data tempdir");
        let store = StoreActor::with_backend(
            CredentialStoreBackend::FileDir(FileCredentialStore::new(credential_dir.path())),
            data_dir.path(),
        );
        let (action_tx, action_rx) = mpsc::channel(16);
        let (event_tx, event_rx) = broadcast::channel(16);
        let handle = AccountActor::spawn(
            store,
            action_tx,
            event_tx,
            LinkPreviewContext::default(),
            Arc::new(ComposerDraftLeaseRegistry::new()),
        );
        Self {
            _credential_dir: credential_dir,
            _data_dir: data_dir,
            handle,
            _action_rx: action_rx,
            _event_rx: event_rx,
        }
    }
}

/// Test-only wrapper for the real core runtime and its actor tree.
///
/// The wrapper deliberately has no policy or fake state.  Its public surface
/// is the eventual set of probes/barriers over the production actors.
pub struct RoomSubscriptionResidencyHarness {
    session: Option<Arc<MatrixClientSession>>,
    room_list_service: Option<Arc<RoomListService>>,
    manager: Option<TimelineManagerActor>,
    room_actor: Option<RoomActorHandle>,
    _room_action_rx: Option<mpsc::Receiver<Vec<koushi_state::AppAction>>>,
    _room_event_rx: Option<broadcast::Receiver<CoreEvent>>,
    next_request_sequence: u64,
}

async fn wait_for_room_operation_terminal(
    event_rx: &mut broadcast::Receiver<CoreEvent>,
    request_id: RequestId,
    kind: RoomOperationKind,
    room_id: &str,
) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match event_rx.recv().await {
                Ok(CoreEvent::Room(event)) => match (kind, event) {
                    (
                        RoomOperationKind::LeaveRoom,
                        RoomEvent::RoomLeft {
                            request_id: event_request_id,
                            room_id: event_room_id,
                        },
                    ) if event_request_id == request_id && event_room_id == room_id => {
                        return true;
                    }
                    (
                        RoomOperationKind::DeclineInvite,
                        RoomEvent::InviteDeclined {
                            request_id: event_request_id,
                            room_id: event_room_id,
                        },
                    ) if event_request_id == request_id && event_room_id == room_id => {
                        return true;
                    }
                    _ => {}
                },
                Ok(CoreEvent::OperationFailed {
                    request_id: event_request_id,
                    failure: CoreFailure::RoomOperationFailed { .. },
                }) if event_request_id == request_id => return false,
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    panic!("residency event receiver lagged")
                }
                Err(broadcast::error::RecvError::Closed) => {
                    panic!("residency event receiver closed")
                }
            }
        }
    })
    .await
    .expect("room operation terminal watchdog")
}

fn membership_transitions(states: &[(&str, &str)]) -> Vec<RoomMembershipTransition> {
    states
        .iter()
        .filter_map(|(room_id, state)| {
            let kind = match *state {
                "left" => RoomMembershipTransitionKind::Left,
                "joined" => RoomMembershipTransitionKind::Joined,
                "invited" => RoomMembershipTransitionKind::Invited,
                _ => return None,
            };
            Some(RoomMembershipTransition {
                room_id: room_id.parse().expect("synthetic room id"),
                kind,
            })
        })
        .collect()
}

async fn run_held_room_operation(
    room_actor: &RoomActorHandle,
    manager: &mut TimelineManagerActor,
    event_rx: &mut broadcast::Receiver<CoreEvent>,
    kind: RoomOperationKind,
    request_id: RequestId,
    room_id: &str,
    succeeds: bool,
) -> bool {
    let (reached_tx, mut reached_rx) = oneshot::channel();
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        room_actor.install_room_operation_test_control(RoomOperationTestControl {
            kind,
            reached: reached_tx,
            completion: completion_rx,
        }),
        "RoomActor must accept the held membership result"
    );
    let command = match kind {
        RoomOperationKind::LeaveRoom => RoomCommand::LeaveRoom {
            request_id,
            room_id: room_id.to_owned(),
        },
        RoomOperationKind::DeclineInvite => RoomCommand::DeclineInvite {
            request_id,
            room_id: room_id.to_owned(),
        },
        _ => unreachable!("harness leave helper only drives leave/decline"),
    };
    assert!(
        room_actor.send(RoomMessage::Command(command)).await,
        "RoomActor must receive the real membership command"
    );

    let mut terminal = Box::pin(wait_for_room_operation_terminal(
        event_rx, request_id, kind, room_id,
    ));
    tokio::select! {
        biased;
        reached = &mut reached_rx => {
            reached.expect("membership SDK-boundary reach sender");
            if succeeds {
                completion_tx
                    .send(Ok(room_id.to_owned()))
                    .expect("release successful membership result");
                let pump = manager.room_subscription_residency_test_pump_next_ingress();
                let ((), terminal) = tokio::join!(pump, terminal);
                terminal
            } else {
                completion_tx
                    .send(Err(koushi_sdk::MatrixRoomOperationError::RoomUnavailable))
                    .expect("release failed membership result");
                terminal.await
            }
        }
        terminal_result = &mut terminal => {
            room_actor.clear_room_operation_test_control();
            terminal_result
        }
    }
}

impl RoomSubscriptionResidencyHarness {
    /// Wrap a real manager around the caller's live RoomListService.
    pub async fn with_room_list_service(
        session: Arc<MatrixClientSession>,
        room_list_service: Arc<RoomListService>,
    ) -> Self {
        let manager = TimelineManagerActor::room_subscription_residency_test_manager(
            room_list_service.clone(),
        );
        let residency_handle = manager.room_subscription_residency_test_handle();
        let (room_action_tx, room_action_rx) = mpsc::channel(crate::runtime::ACTION_QUEUE_CAPACITY);
        let (room_event_tx, room_event_rx) =
            broadcast::channel(crate::runtime::EVENT_QUEUE_CAPACITY);
        let room_actor = RoomActor::spawn(
            room_action_tx,
            room_event_tx,
            crate::SlidingSyncDiagnostics::default(),
        );
        room_actor.bind_timeline_residency(session.clone(), residency_handle);
        assert!(
            room_actor
                .send(RoomMessage::SessionEstablished {
                    session: session.clone(),
                })
                .await
        );
        assert!(room_actor.wait_for_session(&session).await);
        Self {
            session: Some(session),
            room_list_service: Some(room_list_service),
            manager: Some(manager),
            room_actor: Some(room_actor),
            _room_action_rx: Some(room_action_rx),
            _room_event_rx: Some(room_event_rx),
            next_request_sequence: 1,
        }
    }

    pub async fn admit_timeline_key(&mut self, key: TimelineKey) {
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_admit_key(key)
            .await;
    }

    pub async fn admit_build_failure(&mut self, room_id: &str) {
        let room_id = room_id.parse().expect("synthetic test room id must parse");
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_admit_build_failure(room_id)
            .await;
    }

    pub async fn unsubscribe(&mut self, key: TimelineKey) {
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_unsubscribe(key)
            .await;
    }

    // Scaffold-only probes. Their production-backed bodies are admitted with
    // each RED assertion below.
    pub async fn sync_started(&mut self, core_generation: u64) {
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_sync_started(core_generation)
            .await;
    }

    pub async fn offer_restore(&mut self, core_generation: u64, room_ids: &[&str], proven: bool) {
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_offer_restore(core_generation, room_ids, proven)
            .await;
    }

    pub async fn observe_visible(&mut self, core_generation: u64, room_ids: &[&str]) {
        let room_ids = room_ids
            .iter()
            .map(|room_id| VisibleRoomObservation {
                room_id: (*room_id).to_owned(),
                non_left: true,
            })
            .collect();
        self.observe_visible_entries_from_room_actor(core_generation, true, room_ids)
            .await;
    }

    pub async fn observe_visible_entries(
        &mut self,
        core_generation: u64,
        entries: &[(&str, bool)],
    ) {
        let room_ids = entries
            .iter()
            .map(|(room_id, non_left)| VisibleRoomObservation {
                room_id: (*room_id).to_owned(),
                non_left: *non_left,
            })
            .collect();
        self.observe_visible_entries_from_room_actor(core_generation, true, room_ids)
            .await;
    }

    async fn observe_visible_entries_from_room_actor(
        &mut self,
        core_generation: u64,
        reconciliation_is_complete: bool,
        room_ids: Vec<VisibleRoomObservation>,
    ) {
        let forwarded = self
            .room_actor
            .as_ref()
            .expect("residency room actor")
            .room_subscription_residency_test_observe_visible(
                core_generation,
                reconciliation_is_complete,
                room_ids,
            )
            .await;
        if forwarded {
            self.manager
                .as_mut()
                .expect("residency manager")
                .room_subscription_residency_test_pump_next_ingress()
                .await;
        }
    }

    pub async fn seed_sdk_subscriptions(&mut self, room_ids: &[&str]) {
        let rooms = room_ids
            .iter()
            .map(|room_id| {
                room_id
                    .parse::<matrix_sdk::ruma::OwnedRoomId>()
                    .expect("synthetic room id")
            })
            .collect::<Vec<_>>();
        let refs = rooms
            .iter()
            .map(|room_id| room_id.as_ref())
            .collect::<Vec<_>>();
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_seed_sdk_subscriptions(&refs)
            .await;
    }

    pub async fn expire_sdk_subscriptions(&mut self) {
        self.manager
            .as_mut()
            .expect("residency manager")
            .room_subscription_residency_test_expire_sdk_subscriptions()
            .await;
    }

    pub fn clear_residency_binding(&self) {
        self.room_actor
            .as_ref()
            .expect("residency room actor")
            .clear_timeline_residency();
    }

    fn next_request_id(&mut self) -> RequestId {
        let request_id = RequestId {
            connection_id: RuntimeConnectionId(532),
            sequence: self.next_request_sequence,
        };
        self.next_request_sequence += 1;
        request_id
    }

    pub async fn leave_room(&mut self, room_id: &str, succeeds: bool) -> bool {
        let request_id = self.next_request_id();
        run_held_room_operation(
            self.room_actor.as_ref().expect("residency room actor"),
            self.manager.as_mut().expect("residency manager"),
            self._room_event_rx
                .as_mut()
                .expect("residency room event receiver"),
            RoomOperationKind::LeaveRoom,
            request_id,
            room_id,
            succeeds,
        )
        .await
    }

    pub async fn decline_invite(&mut self, room_id: &str, succeeds: bool) -> bool {
        let request_id = self.next_request_id();
        run_held_room_operation(
            self.room_actor.as_ref().expect("residency room actor"),
            self.manager.as_mut().expect("residency manager"),
            self._room_event_rx
                .as_mut()
                .expect("residency room event receiver"),
            RoomOperationKind::DeclineInvite,
            request_id,
            room_id,
            succeeds,
        )
        .await
    }

    pub async fn lost_leave_acknowledgement(&mut self) -> RoomSubscriptionResidencyAckLossProbe {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let ack_diagnostic_before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .filter(|record| {
                record.event.source == "core.room"
                    && record.event.stage == "residency_ack"
                    && record.event.fields.iter().any(|field| {
                        field.key == "reason"
                            && matches!(&field.value, DiagnosticValue::Token("manager_unavailable"))
                    })
            })
            .count();
        let room_id = "!resident-lost-leave-ack:example.invalid".to_owned();
        let request_id = RequestId {
            connection_id: RuntimeConnectionId(532),
            sequence: 1,
        };
        let (reached_tx, reached_rx) = oneshot::channel();
        let (completion_tx, completion_rx) = oneshot::channel();
        assert!(
            self.room_actor
                .as_ref()
                .expect("residency room actor")
                .install_room_operation_test_control(RoomOperationTestControl {
                    kind: RoomOperationKind::LeaveRoom,
                    reached: reached_tx,
                    completion: completion_rx,
                }),
            "RoomActor must accept the held leave control"
        );
        assert!(
            self.room_actor
                .as_ref()
                .expect("residency room actor")
                .send(RoomMessage::Command(RoomCommand::LeaveRoom {
                    request_id,
                    room_id: room_id.clone(),
                }))
                .await,
            "RoomActor must receive the real LeaveRoom command"
        );
        tokio::time::timeout(Duration::from_secs(5), reached_rx)
            .await
            .expect("leave SDK-boundary reach watchdog")
            .expect("leave SDK-boundary reach sender");

        // The real RoomActor still owns the residency sender, but dropping the
        // direct test manager closes its message receiver before SDK success is
        // released. This is the acknowledgement-loss boundary.
        drop(self.manager.take());
        completion_tx
            .send(Ok(room_id.clone()))
            .expect("release held successful leave result");

        let event_rx = self
            ._room_event_rx
            .as_mut()
            .expect("residency room event receiver");
        let action_rx = self
            ._room_action_rx
            .as_mut()
            .expect("residency room action receiver");
        let mut operation_failed_sdk_count = 0;
        let mut room_left_count = 0;
        let mut success_action_count = 0;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    biased;
                    event = event_rx.recv() => match event {
                        Ok(CoreEvent::OperationFailed {
                            request_id: event_request_id,
                            failure: CoreFailure::RoomOperationFailed {
                                kind: RoomFailureKind::Sdk,
                            },
                        }) if event_request_id == request_id => {
                            operation_failed_sdk_count += 1;
                            break;
                        }
                        Ok(CoreEvent::Room(RoomEvent::RoomLeft {
                            request_id: event_request_id,
                            room_id: event_room_id,
                        })) if event_request_id == request_id && event_room_id == room_id => {
                            room_left_count += 1;
                            break;
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            panic!("residency event receiver lagged")
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            panic!("residency event receiver closed")
                        }
                    },
                    action = action_rx.recv() => match action {
                        Some(actions) => {
                            success_action_count += actions.iter().filter(|action| {
                                matches!(
                                    action,
                                    koushi_state::AppAction::SpaceOrderPreferenceRemoved {
                                        space_id,
                                    } if space_id == &room_id
                                )
                            }).count();
                        }
                        None => panic!("residency action receiver closed"),
                    },
                }
            }
        })
        .await
        .expect("leave acknowledgement-loss terminal watchdog");

        while let Ok(event) = event_rx.try_recv() {
            match event {
                CoreEvent::OperationFailed {
                    request_id: event_request_id,
                    failure:
                        CoreFailure::RoomOperationFailed {
                            kind: RoomFailureKind::Sdk,
                        },
                } if event_request_id == request_id => {
                    operation_failed_sdk_count += 1;
                }
                CoreEvent::Room(RoomEvent::RoomLeft {
                    request_id: event_request_id,
                    room_id: event_room_id,
                }) if event_request_id == request_id && event_room_id == room_id => {
                    room_left_count += 1;
                }
                _ => {}
            }
        }

        while let Ok(actions) = action_rx.try_recv() {
            success_action_count += actions
                .iter()
                .filter(|action| {
                    matches!(
                        action,
                        koushi_state::AppAction::SpaceOrderPreferenceRemoved { space_id }
                            if space_id == &room_id
                    )
                })
                .count();
        }
        let ack_diagnostic_count = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .filter(|record| {
                record.event.source == "core.room"
                    && record.event.stage == "residency_ack"
                    && record.event.fields.iter().any(|field| {
                        field.key == "reason"
                            && matches!(&field.value, DiagnosticValue::Token("manager_unavailable"))
                    })
            })
            .count()
            .saturating_sub(ack_diagnostic_before);

        RoomSubscriptionResidencyAckLossProbe {
            operation_failed_sdk_count,
            room_left_count,
            success_action_count,
            ack_diagnostic_count,
        }
    }

    async fn membership_sequence_through_room_actor(
        &mut self,
        core_generation: u64,
        states: &[(&str, &str)],
    ) {
        let forwarded = self
            .room_actor
            .as_ref()
            .expect("residency room actor")
            .room_subscription_residency_test_observe_membership(
                core_generation,
                membership_transitions(states),
            )
            .await;
        if forwarded {
            self.manager
                .as_mut()
                .expect("residency manager")
                .room_subscription_residency_test_pump_next_ingress()
                .await;
        }
    }

    pub async fn membership_sequence(&mut self, core_generation: u64, states: &[(&str, &str)]) {
        self.membership_sequence_through_room_actor(core_generation, states)
            .await;
    }

    pub async fn stale_membership_sequence(
        &mut self,
        core_generation: u64,
        states: &[(&str, &str)],
    ) {
        self.membership_sequence_through_room_actor(core_generation, states)
            .await;
    }

    pub async fn local_rejoin_replacement_fence(
        &mut self,
    ) -> RoomSubscriptionResidencyOperationProbe {
        let cases = [
            (
                RoomOperationKind::DeclineInvite,
                1_u64,
                "!resident-rejoin-decline:example.invalid",
            ),
            (
                RoomOperationKind::AcceptInvite,
                2_u64,
                "!resident-rejoin-accept:example.invalid",
            ),
            (
                RoomOperationKind::JoinRoom,
                3_u64,
                "!resident-rejoin-room:example.invalid",
            ),
            (
                RoomOperationKind::JoinDirectoryRoom,
                4_u64,
                "!resident-rejoin-directory:example.invalid",
            ),
        ];
        let mut old_manager_alive = true;
        let mut replacement_completed = true;
        let mut acknowledgement_before_replacement = true;
        let mut settlement_before_replacement = true;
        let mut no_late_terminal = true;
        let mut mismatch_probe = true;
        let mut sdk_call_count = 0;

        for (kind, sequence, room_id) in cases {
            enum ReplacementRace {
                TerminalBeforeReplacement,
                ReplacementBeforeTerminal(bool),
            }

            let mut account = ResidencyAccountFixture::spawn();
            let account_handle = account.handle.clone();
            let resident_session = self.session.as_ref().expect("resident test session");
            let client = resident_session.client();
            let session_info = |device_id: &str| SessionInfo {
                homeserver: resident_session.info.homeserver.clone(),
                user_id: resident_session.info.user_id.clone(),
                device_id: device_id.to_owned(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            };
            let session_a = Arc::new(MatrixClientSession::from_client_for_testing(
                client.clone(),
                session_info("A"),
            ));
            let session_b = Arc::new(MatrixClientSession::from_client_for_testing(
                client,
                session_info("B"),
            ));
            let sessions_are_distinct = !Arc::ptr_eq(&session_a, &session_b);
            mismatch_probe &= sessions_are_distinct;
            assert!(
                sessions_are_distinct,
                "rejoin sessions must be pointer-distinct"
            );

            assert!(
                account_handle
                    .install_residency_test_session(session_a)
                    .await,
                "AccountActor must install the real session A"
            );

            let (reached_tx, reached_rx) = oneshot::channel();
            let (completion_tx, completion_rx) = oneshot::channel();
            assert!(
                account_handle
                    .configure_room_operation_test_control(RoomOperationTestControl {
                        kind,
                        reached: reached_tx,
                        completion: completion_rx,
                    })
                    .await,
                "RoomActor must accept the matching room-operation control"
            );

            let request_id = RequestId {
                connection_id: RuntimeConnectionId(532),
                sequence,
            };
            let command = match kind {
                RoomOperationKind::DeclineInvite => RoomCommand::DeclineInvite {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::AcceptInvite => RoomCommand::AcceptInvite {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::JoinRoom => RoomCommand::JoinRoom {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::JoinDirectoryRoom => RoomCommand::JoinDirectoryRoom {
                    request_id,
                    room_id_or_alias: room_id.to_owned(),
                    via_servers: Vec::new(),
                },
                RoomOperationKind::LeaveRoom => unreachable!("local rejoin case table"),
            };
            assert!(
                account_handle.residency_test_room_command(command).await,
                "AccountActor must route the real room command"
            );

            tokio::time::timeout(Duration::from_secs(5), reached_rx)
                .await
                .expect("room-operation SDK-boundary reach watchdog")
                .expect("room-operation SDK-boundary reach sender");
            sdk_call_count += 1;

            let replacement_handle = account_handle.clone();
            let mut replacement = tokio::spawn(async move {
                replacement_handle
                    .install_residency_test_session(session_b)
                    .await
            });
            let mut replacement_result = None;
            tokio::select! {
                biased;
                result = &mut replacement => {
                    replacement_result = Some(result.expect("replacement install task"));
                }
                _ = tokio::task::yield_now() => {}
            }
            let held_before_completion = replacement_result.is_none() && !replacement.is_finished();
            old_manager_alive &= held_before_completion;

            let expected_room_id = room_id.to_owned();
            let is_matching_terminal = |event: &CoreEvent| match (kind, event) {
                (
                    RoomOperationKind::DeclineInvite,
                    CoreEvent::Room(RoomEvent::InviteDeclined {
                        request_id: event_request_id,
                        room_id: event_room_id,
                    }),
                )
                | (
                    RoomOperationKind::AcceptInvite,
                    CoreEvent::Room(RoomEvent::InviteAccepted {
                        request_id: event_request_id,
                        room_id: event_room_id,
                    }),
                )
                | (
                    RoomOperationKind::JoinRoom | RoomOperationKind::JoinDirectoryRoom,
                    CoreEvent::Room(RoomEvent::RoomJoined {
                        request_id: event_request_id,
                        room_id: event_room_id,
                    }),
                ) => event_request_id == &request_id && event_room_id == &expected_room_id,
                _ => false,
            };

            completion_tx
                .send(Ok(expected_room_id.clone()))
                .expect("release successful room operation");

            let event_rx = &mut account._event_rx;
            let mut matching_terminal_count = 0;
            let terminal_before_replacement = if replacement_result.is_some() {
                false
            } else {
                match tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        tokio::select! {
                            biased;
                            event = event_rx.recv() => match event {
                                Ok(event) if is_matching_terminal(&event) => {
                                    break ReplacementRace::TerminalBeforeReplacement;
                                }
                                Ok(_) => {}
                                Err(broadcast::error::RecvError::Lagged(_)) => {
                                    panic!("residency event receiver lagged")
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    panic!("residency event receiver closed")
                                }
                            },
                            result = &mut replacement => {
                                break ReplacementRace::ReplacementBeforeTerminal(
                                    result.expect("replacement install task"),
                                );
                            }
                        }
                    }
                })
                .await
                .expect("room terminal/replacement ordering watchdog")
                {
                    ReplacementRace::TerminalBeforeReplacement => {
                        matching_terminal_count += 1;
                        true
                    }
                    ReplacementRace::ReplacementBeforeTerminal(completed) => {
                        replacement_result = Some(completed);
                        false
                    }
                }
            };

            let replacement_completed_for_case = match replacement_result {
                Some(completed) => completed,
                None => tokio::time::timeout(Duration::from_secs(5), &mut replacement)
                    .await
                    .expect("replacement completion watchdog")
                    .expect("replacement install task"),
            };
            replacement_completed &= replacement_completed_for_case;
            acknowledgement_before_replacement &= terminal_before_replacement;
            settlement_before_replacement &= terminal_before_replacement;

            assert!(
                tokio::time::timeout(
                    Duration::from_secs(5),
                    account.handle.shutdown_for_testing()
                )
                .await
                .expect("account shutdown queue-barrier watchdog"),
                "AccountActor must acknowledge ordered shutdown"
            );
            loop {
                match event_rx.try_recv() {
                    Ok(event) if is_matching_terminal(&event) => matching_terminal_count += 1,
                    Ok(_) => {}
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        panic!("residency event receiver lagged")
                    }
                    Err(broadcast::error::TryRecvError::Closed) => break,
                }
            }
            let late_terminal_after_replacement = if terminal_before_replacement {
                matching_terminal_count > 1
            } else {
                matching_terminal_count > 0
            };
            no_late_terminal &= !late_terminal_after_replacement;
            assert_eq!(
                matching_terminal_count, 1,
                "each local rejoin must emit one matching terminal"
            );
        }

        assert_eq!(sdk_call_count, cases.len());
        RoomSubscriptionResidencyOperationProbe {
            old_manager_alive,
            replacement_completed,
            acknowledgement_before_replacement,
            settlement_before_replacement,
            late_terminal_after_replacement: !no_late_terminal,
            mismatch_probe,
            sdk_call_count,
        }
    }

    pub async fn failed_operations_before_replacement(
        &mut self,
    ) -> RoomSubscriptionResidencyOperationProbe {
        let cases = [
            (
                RoomOperationKind::LeaveRoom,
                1_u64,
                "!resident-failure-leave:example.invalid",
            ),
            (
                RoomOperationKind::DeclineInvite,
                2_u64,
                "!resident-failure-decline:example.invalid",
            ),
            (
                RoomOperationKind::AcceptInvite,
                3_u64,
                "!resident-failure-accept:example.invalid",
            ),
            (
                RoomOperationKind::JoinRoom,
                4_u64,
                "!resident-failure-join:example.invalid",
            ),
            (
                RoomOperationKind::JoinDirectoryRoom,
                5_u64,
                "!resident-failure-directory:example.invalid",
            ),
        ];
        let mut old_manager_alive = true;
        let mut replacement_completed = true;
        let mut acknowledgement_before_replacement = true;
        let mut settlement_before_replacement = true;
        let mut no_late_terminal = true;
        let mut mismatch_probe = true;
        let mut sdk_call_count = 0;

        for (kind, sequence, room_id) in cases {
            let mut account = ResidencyAccountFixture::spawn();
            let account_handle = account.handle.clone();
            let resident_session = self.session.as_ref().expect("resident test session");
            let client = resident_session.client();
            let session_info = |device_id: &str| SessionInfo {
                homeserver: resident_session.info.homeserver.clone(),
                user_id: resident_session.info.user_id.clone(),
                device_id: device_id.to_owned(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            };
            let session_a = Arc::new(MatrixClientSession::from_client_for_testing(
                client.clone(),
                session_info("A"),
            ));
            let session_b = Arc::new(MatrixClientSession::from_client_for_testing(
                client,
                session_info("B"),
            ));
            let sessions_are_distinct = !Arc::ptr_eq(&session_a, &session_b);
            mismatch_probe &= sessions_are_distinct;
            assert!(
                sessions_are_distinct,
                "failure-operation sessions must be pointer-distinct"
            );

            assert!(
                account_handle
                    .install_residency_test_session(session_a)
                    .await,
                "AccountActor must install the real session A"
            );

            let (reached_tx, reached_rx) = oneshot::channel();
            let (completion_tx, completion_rx) = oneshot::channel();
            assert!(
                account_handle
                    .configure_room_operation_test_control(RoomOperationTestControl {
                        kind,
                        reached: reached_tx,
                        completion: completion_rx,
                    })
                    .await,
                "RoomActor must accept the matching room-operation control"
            );

            let request_id = RequestId {
                connection_id: RuntimeConnectionId(532),
                sequence,
            };
            let command = match kind {
                RoomOperationKind::LeaveRoom => RoomCommand::LeaveRoom {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::DeclineInvite => RoomCommand::DeclineInvite {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::AcceptInvite => RoomCommand::AcceptInvite {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::JoinRoom => RoomCommand::JoinRoom {
                    request_id,
                    room_id: room_id.to_owned(),
                },
                RoomOperationKind::JoinDirectoryRoom => RoomCommand::JoinDirectoryRoom {
                    request_id,
                    room_id_or_alias: room_id.to_owned(),
                    via_servers: Vec::new(),
                },
            };
            assert!(
                account_handle.residency_test_room_command(command).await,
                "AccountActor must route the real room command"
            );

            tokio::time::timeout(Duration::from_secs(5), reached_rx)
                .await
                .expect("room-operation SDK-boundary reach watchdog")
                .expect("room-operation SDK-boundary reach sender");
            sdk_call_count += 1;

            let replacement_handle = account_handle.clone();
            let mut replacement = tokio::spawn(async move {
                replacement_handle
                    .install_residency_test_session(session_b)
                    .await
            });
            let replacement_held = tokio::select! {
                biased;
                result = &mut replacement => {
                    result.expect("replacement install task");
                    false
                }
                _ = tokio::task::yield_now() => !replacement.is_finished(),
            };
            assert!(
                replacement_held,
                "held failure must keep replacement behind the old manager"
            );
            old_manager_alive &= replacement_held;

            completion_tx
                .send(Err(koushi_sdk::MatrixRoomOperationError::RoomUnavailable))
                .expect("release held room-operation failure");

            let is_directory_join = kind == RoomOperationKind::JoinDirectoryRoom;
            let matches_failure = |event: &CoreEvent| {
                matches!(
                    event,
                    CoreEvent::OperationFailed {
                        request_id: event_request_id,
                        ..
                    } if event_request_id == &request_id
                )
            };
            let matches_directory_failure = |actions: &[koushi_state::AppAction]| {
                actions.iter().any(|action| {
                    matches!(
                        action,
                        koushi_state::AppAction::DirectoryJoinFailed {
                            request_id: action_request_id,
                            room_id_or_alias: action_target,
                            via_servers,
                            ..
                        } if *action_request_id == request_id.sequence
                            && action_target == room_id
                            && via_servers.is_empty()
                    )
                })
            };

            let event_rx = &mut account._event_rx;
            let action_rx = &mut account._action_rx;
            let mut directory_failure_action_observed = !is_directory_join;
            let mut matching_terminal_count = 0;
            let mut terminal_before_replacement = true;
            let mut replacement_result = None;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    tokio::select! {
                        biased;
                        action = action_rx.recv(), if is_directory_join && !directory_failure_action_observed => {
                            match action {
                                Some(actions) if matches_directory_failure(&actions) => {
                                    directory_failure_action_observed = true;
                                }
                                Some(_) => {}
                                None => panic!("account action receiver closed"),
                            }
                        }
                        event = event_rx.recv() => match event {
                            Ok(event) if matches_failure(&event) => {
                                assert!(
                                    directory_failure_action_observed,
                                    "directory failure action must precede OperationFailed"
                                );
                                matching_terminal_count += 1;
                                break;
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                panic!("residency event receiver lagged")
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                panic!("residency event receiver closed")
                            }
                        },
                        result = &mut replacement, if replacement_result.is_none() => {
                            terminal_before_replacement = false;
                            replacement_result = Some(result.expect("replacement install task"));
                            break;
                        }
                    }
                }
            })
            .await
            .expect("room terminal/replacement ordering watchdog");

            let replacement_completed_for_case = match replacement_result {
                Some(completed) => completed,
                None => tokio::time::timeout(Duration::from_secs(5), &mut replacement)
                    .await
                    .expect("replacement completion watchdog")
                    .expect("replacement install task"),
            };
            replacement_completed &= replacement_completed_for_case;
            acknowledgement_before_replacement &= terminal_before_replacement;
            settlement_before_replacement &= terminal_before_replacement;

            assert_eq!(
                matching_terminal_count, 1,
                "each held failure must emit one matching OperationFailed"
            );
            assert!(
                directory_failure_action_observed,
                "directory failure must queue its reducer action"
            );
            assert!(
                tokio::time::timeout(
                    Duration::from_secs(5),
                    account_handle.shutdown_for_testing()
                )
                .await
                .expect("account shutdown queue-barrier watchdog"),
                "AccountActor must acknowledge ordered shutdown"
            );
            loop {
                match event_rx.try_recv() {
                    Ok(event) if matches_failure(&event) => matching_terminal_count += 1,
                    Ok(_) => {}
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        panic!("residency event receiver lagged")
                    }
                    Err(broadcast::error::TryRecvError::Closed) => break,
                }
            }
            let late_terminal_after_replacement = matching_terminal_count > 1;
            no_late_terminal &= !late_terminal_after_replacement;
            assert_eq!(
                matching_terminal_count, 1,
                "replacement must not duplicate OperationFailed"
            );
        }

        assert_eq!(sdk_call_count, 5);
        assert!(old_manager_alive);
        assert!(replacement_completed);
        assert!(acknowledgement_before_replacement);
        assert!(settlement_before_replacement);
        assert!(no_late_terminal);
        assert!(mismatch_probe);
        RoomSubscriptionResidencyOperationProbe {
            old_manager_alive,
            replacement_completed,
            acknowledgement_before_replacement,
            settlement_before_replacement,
            late_terminal_after_replacement: !no_late_terminal,
            mismatch_probe,
            sdk_call_count,
        }
    }

    pub async fn inflight_leave_replacement(&mut self) -> RoomSubscriptionResidencyOperationProbe {
        enum TerminalOrder {
            TerminalBeforeReplacement,
            ReplacementBeforeTerminal(bool),
        }

        let mut account = ResidencyAccountFixture::spawn();
        let account_handle = account.handle.clone();
        let resident_session = self.session.as_ref().expect("resident test session");
        let client = resident_session.client();
        let session_info = |device_id: &str| SessionInfo {
            homeserver: resident_session.info.homeserver.clone(),
            user_id: resident_session.info.user_id.clone(),
            device_id: device_id.to_owned(),
            authentication_method: SessionAuthenticationMethod::Unknown,
        };
        let session_a = Arc::new(MatrixClientSession::from_client_for_testing(
            client.clone(),
            session_info("A"),
        ));
        let session_b = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            session_info("B"),
        ));
        let mismatch_probe = !Arc::ptr_eq(&session_a, &session_b);

        assert!(
            account_handle
                .install_residency_test_session(session_a.clone())
                .await,
            "AccountActor must install the real session A"
        );

        let (reached_tx, reached_rx) = oneshot::channel();
        let (completion_tx, completion_rx) = oneshot::channel();
        assert!(
            account_handle
                .configure_room_operation_test_control(RoomOperationTestControl {
                    kind: RoomOperationKind::LeaveRoom,
                    reached: reached_tx,
                    completion: completion_rx,
                })
                .await,
            "RoomActor must accept the leave-operation control"
        );

        let request_id = RequestId {
            connection_id: RuntimeConnectionId(532),
            sequence: 1,
        };
        let room_id = "!resident-inflight-leave:example.invalid".to_owned();
        assert!(
            account_handle
                .residency_test_room_command(RoomCommand::LeaveRoom {
                    request_id,
                    room_id: room_id.clone(),
                })
                .await,
            "AccountActor must route the real LeaveRoom command"
        );

        let mut sdk_call_count = 0;
        tokio::time::timeout(Duration::from_secs(5), reached_rx)
            .await
            .expect("leave SDK-boundary reach watchdog")
            .expect("leave SDK-boundary reach sender");
        sdk_call_count += 1;

        let replacement_handle = account_handle.clone();
        let mut replacement = tokio::spawn(async move {
            replacement_handle
                .install_residency_test_session(session_b)
                .await
        });
        let old_manager_alive = tokio::select! {
            biased;
            result = &mut replacement => {
                panic!(
                    "replacement completed while the leave was held: {}",
                    result.expect("replacement install task")
                );
            }
            _ = tokio::task::yield_now() => !replacement.is_finished(),
        };

        completion_tx
            .send(Ok(room_id.clone()))
            .expect("release successful leave result");

        let event_rx = &mut account._event_rx;
        let terminal_order = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    biased;
                    event = event_rx.recv() => match event {
                        Ok(CoreEvent::Room(RoomEvent::RoomLeft {
                            request_id: event_request_id,
                            room_id: event_room_id,
                        })) if event_request_id == request_id && event_room_id == room_id => {
                            break TerminalOrder::TerminalBeforeReplacement;
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            panic!("residency event receiver lagged")
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            panic!("residency event receiver closed")
                        }
                    },
                    result = &mut replacement => {
                        break TerminalOrder::ReplacementBeforeTerminal(
                            result.expect("replacement install task"),
                        );
                    }
                }
            }
        })
        .await
        .expect("leave terminal/replacement ordering watchdog");

        let terminal_before_replacement =
            matches!(terminal_order, TerminalOrder::TerminalBeforeReplacement);
        let replacement_completed = match terminal_order {
            TerminalOrder::TerminalBeforeReplacement => {
                (&mut replacement).await.expect("replacement install task")
            }
            TerminalOrder::ReplacementBeforeTerminal(completed) => completed,
        };
        let acknowledgement_before_replacement = terminal_before_replacement;
        let settlement_before_replacement = terminal_before_replacement;

        tokio::task::yield_now().await;
        let mut matching_terminal_count = 0;
        loop {
            match event_rx.try_recv() {
                Ok(CoreEvent::Room(RoomEvent::RoomLeft {
                    request_id: event_request_id,
                    room_id: event_room_id,
                })) if event_request_id == request_id && event_room_id == room_id => {
                    matching_terminal_count += 1;
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    panic!("residency event receiver lagged")
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        assert!(
            account_handle.shutdown_for_testing().await,
            "AccountActor must acknowledge ordered shutdown"
        );
        loop {
            match event_rx.try_recv() {
                Ok(CoreEvent::Room(RoomEvent::RoomLeft {
                    request_id: event_request_id,
                    room_id: event_room_id,
                })) if event_request_id == request_id && event_room_id == room_id => {
                    matching_terminal_count += 1;
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    panic!("residency event receiver lagged")
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        let late_terminal_after_replacement = matching_terminal_count != 0;

        RoomSubscriptionResidencyOperationProbe {
            old_manager_alive,
            replacement_completed,
            acknowledgement_before_replacement,
            settlement_before_replacement,
            late_terminal_after_replacement,
            mismatch_probe,
            sdk_call_count,
        }
    }

    pub async fn pre_sync_mismatch_probe(&mut self) -> RoomSubscriptionResidencyBindingProbe {
        let _diagnostic_lock = koushi_diagnostics::test_support::lock();
        let mismatch_diagnostic_before = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .filter(|record| {
                record.event.source == "core.room"
                    && record.event.stage == "residency_admission"
                    && record.event.fields.iter().any(|field| {
                        field.key == "reason"
                            && matches!(
                                &field.value,
                                koushi_diagnostics::DiagnosticValue::Token("session_mismatch")
                            )
                    })
            })
            .count();
        let account = ResidencyAccountFixture::spawn();
        let resident_session = self.session.as_ref().expect("resident test session");
        let client = resident_session.client();
        let session_info = |device_id: &str| SessionInfo {
            homeserver: resident_session.info.homeserver.clone(),
            user_id: resident_session.info.user_id.clone(),
            device_id: device_id.to_owned(),
            authentication_method: SessionAuthenticationMethod::Unknown,
        };
        let session_a = Arc::new(MatrixClientSession::from_client_for_testing(
            client.clone(),
            session_info("A"),
        ));
        let session_b = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            session_info("B"),
        ));

        assert!(
            account
                .handle
                .install_residency_test_session(session_a.clone())
                .await,
            "AccountActor must install session A through its real session path"
        );

        let (reached_tx, reached_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        assert!(
            account
                .handle
                .configure_residency_install_gap(reached_tx, release_rx)
                .await,
            "AccountActor must install the real replacement barrier"
        );

        let install_handle = account.handle.clone();
        let session_b_for_install = session_b.clone();
        let install_b = tokio::spawn(async move {
            install_handle
                .install_residency_test_session(session_b_for_install)
                .await
        });
        let (room_session, bound_session) =
            tokio::time::timeout(Duration::from_secs(5), reached_rx)
                .await
                .expect("replacement install gap")
                .expect("replacement install gap sender");
        let room_session = room_session.expect("RoomActor must retain session A in the gap");
        let bound_session = bound_session.expect("replacement manager must bind session B");
        assert!(Arc::ptr_eq(&room_session, &session_a));
        assert!(Arc::ptr_eq(&bound_session, &session_b));
        let pointer_equal = Arc::ptr_eq(&room_session, &bound_session);
        assert!(
            !pointer_equal,
            "replacement binding must not reuse RoomActor session"
        );

        assert!(
            account
                .handle
                .residency_test_room_command_at_install_gap(RoomCommand::LeaveRoom {
                    request_id: RequestId {
                        connection_id: RuntimeConnectionId(1),
                        sequence: 1,
                    },
                    room_id: "!resident-pre-sync-mismatch:example.invalid".to_owned(),
                })
                .await,
            "real LeaveRoom command must reach RoomActor during the install gap"
        );
        let mismatch_diagnostic_after = koushi_diagnostics::test_support::detail_snapshot()
            .records
            .into_iter()
            .filter(|record| {
                record.event.source == "core.room"
                    && record.event.stage == "residency_admission"
                    && record.event.fields.iter().any(|field| {
                        field.key == "reason"
                            && matches!(
                                &field.value,
                                koushi_diagnostics::DiagnosticValue::Token("session_mismatch")
                            )
                    })
            })
            .count();
        let mismatch_probe = mismatch_diagnostic_after == mismatch_diagnostic_before + 1;

        release_tx
            .send(())
            .expect("release replacement install gap");
        assert!(
            install_b.await.expect("replacement install task"),
            "AccountActor must finish installing session B"
        );
        assert!(
            account.handle.shutdown_for_testing().await,
            "AccountActor must shut down after the real probe"
        );

        RoomSubscriptionResidencyBindingProbe {
            room_session: Some(room_session.info.device_id.clone()),
            bound_session: Some(bound_session.info.device_id.clone()),
            pointer_equal,
            mismatch_probe,
        }
    }

    pub async fn account_teardown_probe(&mut self) -> RoomSubscriptionResidencyTeardownProbe {
        enum ShutdownRace {
            TerminalBeforeShutdown,
            ShutdownBeforeTerminal(bool),
        }

        let mut account = ResidencyAccountFixture::spawn();
        let account_handle = account.handle.clone();
        let resident_session = self.session.as_ref().expect("resident test session");
        let client = resident_session.client();
        let session_a = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver: resident_session.info.homeserver.clone(),
                user_id: resident_session.info.user_id.clone(),
                device_id: "A".to_owned(),
                authentication_method: SessionAuthenticationMethod::Unknown,
            },
        ));
        assert!(
            account_handle
                .install_residency_test_session(session_a)
                .await,
            "AccountActor must install the real session A"
        );

        let room_id = "!resident-teardown-held:example.invalid".to_owned();
        let second_room_id = "!resident-teardown-rejected:example.invalid".to_owned();
        let first_request_id = RequestId {
            connection_id: RuntimeConnectionId(532),
            sequence: 1,
        };
        let second_request_id = RequestId {
            connection_id: RuntimeConnectionId(532),
            sequence: 2,
        };
        let (operation_reached_tx, operation_reached_rx) = oneshot::channel();
        let (operation_completion_tx, operation_completion_rx) = oneshot::channel();
        assert!(
            account_handle
                .configure_room_operation_test_control(RoomOperationTestControl {
                    kind: RoomOperationKind::LeaveRoom,
                    reached: operation_reached_tx,
                    completion: operation_completion_rx,
                })
                .await,
            "RoomActor must accept the held leave control"
        );
        assert!(
            account_handle
                .residency_test_room_command(RoomCommand::LeaveRoom {
                    request_id: first_request_id,
                    room_id: room_id.clone(),
                })
                .await,
            "AccountActor must route the real first LeaveRoom command"
        );
        tokio::time::timeout(Duration::from_secs(5), operation_reached_rx)
            .await
            .expect("held leave SDK-boundary reach watchdog")
            .expect("held leave SDK-boundary reach sender");

        let (gap_reached_tx, gap_reached_rx) = oneshot::channel();
        let (gap_release_tx, gap_release_rx) = oneshot::channel();
        assert!(
            account_handle
                .configure_residency_teardown_gap(gap_reached_tx, gap_release_rx)
                .await,
            "AccountActor must accept the teardown gap"
        );
        let shutdown_handle = account_handle.clone();
        let mut shutdown =
            tokio::spawn(async move { shutdown_handle.shutdown_for_testing().await });
        let binding_cleared = tokio::time::timeout(Duration::from_secs(5), gap_reached_rx)
            .await
            .expect("teardown gap reach watchdog")
            .expect("teardown gap reach sender");
        let shutdown_incomplete_while_gap_held = !shutdown.is_finished();

        assert!(
            account_handle
                .residency_test_room_command_direct(RoomCommand::LeaveRoom {
                    request_id: second_request_id,
                    room_id: second_room_id,
                })
                .await,
            "second LeaveRoom must be queued directly through RoomActor"
        );
        gap_release_tx.send(()).expect("release teardown gap");
        tokio::task::yield_now().await;
        let shutdown_incomplete_while_permit_held = !shutdown.is_finished();
        operation_completion_tx
            .send(Ok(room_id.clone()))
            .expect("release successful first leave result");

        let event_rx = &mut account._event_rx;
        let action_rx = &mut account._action_rx;
        let mut settlement_observed = false;
        let mut matching_terminal_count = 0;
        let mut post_clear_admission_rejected = false;
        let mut post_clear_failure_is_sdk = false;
        let terminal_order = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    biased;
                    action = action_rx.recv(), if !settlement_observed => match action {
                        Some(actions) => {
                            settlement_observed |= actions.iter().any(|action| {
                                matches!(
                                    action,
                                    koushi_state::AppAction::SpaceOrderPreferenceRemoved {
                                        space_id,
                                    } if space_id == &room_id
                                )
                            });
                        }
                        None => panic!("account action receiver closed"),
                    },
                    event = event_rx.recv() => match event {
                        Ok(CoreEvent::Room(RoomEvent::RoomLeft {
                            request_id,
                            room_id: event_room_id,
                        })) if request_id == first_request_id && event_room_id == room_id => {
                            matching_terminal_count += 1;
                            break ShutdownRace::TerminalBeforeShutdown;
                        }
                        Ok(CoreEvent::OperationFailed {
                            request_id,
                            failure,
                        }) if request_id == second_request_id => {
                            post_clear_admission_rejected = true;
                            post_clear_failure_is_sdk = matches!(
                                failure,
                                CoreFailure::RoomOperationFailed {
                                    kind: RoomFailureKind::Sdk,
                                }
                            );
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            panic!("residency event receiver lagged")
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            panic!("residency event receiver closed")
                        }
                    },
                    result = &mut shutdown => {
                        break ShutdownRace::ShutdownBeforeTerminal(
                            result.expect("account shutdown task"),
                        );
                    }
                }
            }
        })
        .await
        .expect("teardown terminal/shutdown ordering watchdog");
        let acknowledgement_before_shutdown =
            matches!(terminal_order, ShutdownRace::TerminalBeforeShutdown);
        let shutdown_completed = match terminal_order {
            ShutdownRace::TerminalBeforeShutdown => {
                tokio::time::timeout(Duration::from_secs(5), &mut shutdown)
                    .await
                    .expect("account shutdown completion watchdog")
                    .expect("account shutdown task")
            }
            ShutdownRace::ShutdownBeforeTerminal(completed) => completed,
        };

        while let Ok(event) = event_rx.try_recv() {
            match event {
                CoreEvent::Room(RoomEvent::RoomLeft {
                    request_id,
                    room_id: event_room_id,
                }) if request_id == first_request_id && event_room_id == room_id => {
                    matching_terminal_count += 1;
                }
                CoreEvent::OperationFailed {
                    request_id,
                    failure,
                } if request_id == second_request_id => {
                    post_clear_admission_rejected = true;
                    post_clear_failure_is_sdk = matches!(
                        failure,
                        CoreFailure::RoomOperationFailed {
                            kind: RoomFailureKind::Sdk,
                        }
                    );
                }
                _ => {}
            }
        }
        let operation_control_reached_count =
            account_handle.residency_test_room_operation_reached_count();
        let no_late_terminal = matching_terminal_count == 1;
        let settlement_before_shutdown = settlement_observed && acknowledgement_before_shutdown;

        RoomSubscriptionResidencyTeardownProbe {
            binding_cleared,
            post_clear_admission_rejected,
            post_clear_failure_is_sdk,
            operation_control_reached_count,
            shutdown_incomplete_while_gap_held,
            shutdown_incomplete_while_permit_held,
            acknowledgement_before_shutdown,
            settlement_before_shutdown,
            shutdown_completed,
            matching_terminal_count,
            no_late_terminal,
        }
    }

    pub async fn final_permit_drop_probe(&mut self) -> RoomSubscriptionResidencyGateProbe {
        let (accepting, active_count, new_admission_rejected, drain_completed) =
            TimelineManagerActor::room_subscription_residency_test_gate_probe().await;
        RoomSubscriptionResidencyGateProbe {
            accepting_after_close: accepting,
            active_count_after_close: active_count,
            new_admission_rejected,
            drain_completed,
        }
    }

    pub async fn timeline_setup_precedes_room_observation(&mut self) -> bool {
        let (Some(session), Some(room_list_service)) =
            (self.session.clone(), self.room_list_service.clone())
        else {
            return false;
        };
        crate::sync::room_subscription_residency_start_order_for_testing(session, room_list_service)
            .await
    }

    pub async fn replace_account_and_restore(&mut self, room_ids: &[&str]) {
        let room_list_service = self
            .room_list_service
            .clone()
            .expect("residency manager service");
        self.manager =
            Some(TimelineManagerActor::room_subscription_residency_test_manager(room_list_service));
        let residency_handle = self
            .manager
            .as_ref()
            .expect("replacement residency manager")
            .room_subscription_residency_test_handle();
        if let (Some(room_actor), Some(session)) = (self.room_actor.as_ref(), self.session.clone())
        {
            room_actor.bind_timeline_residency(session, residency_handle);
        }
        let manager = self
            .manager
            .as_mut()
            .expect("replacement residency manager");
        manager
            .room_subscription_residency_test_sync_started(100)
            .await;
        manager
            .room_subscription_residency_test_offer_restore(100, room_ids, true)
            .await;
    }

    pub fn snapshot(&self) -> RoomSubscriptionResidencySnapshot {
        let (
            desired_rooms,
            active_rooms,
            tombstoned_rooms,
            actor_count,
            lease_count,
            sdk_generation,
        ) = self
            .manager
            .as_ref()
            .expect("residency manager")
            .room_subscription_residency_test_snapshot();
        RoomSubscriptionResidencySnapshot {
            desired_rooms,
            active_rooms,
            tombstoned_rooms,
            actor_count,
            lease_count,
            sdk_generation,
            ..RoomSubscriptionResidencySnapshot::default()
        }
    }
}
