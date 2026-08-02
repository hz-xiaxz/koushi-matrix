//! RoomActor: room list normalization and room operations.
//!
//! ## Ownership
//! `RoomActor` is owned by `AccountActor`. Its task handle lives inside
//! `AccountActor`; colocated as a child task per the spec
//! ("Actor Deployment And Supervision — boundaries define ownership, not one
//! task per actor").
//!
//! ## Room list normalization (canon: overview.md RoomActor bullet)
//! Constructing ad-hoc `RoomListService` instances is PROHIBITED: they are
//! not driven by the sync loop, race the running `SyncService`, and return
//! entries without the live service's `required_state` (e.g. `m.room.create`
//! for space classification — deterministically broken on Conduit).
//!
//! `RoomMessage::SyncStarted` carries the backend handle:
//! - `Some(Arc<RoomListService>)` on the SyncService backend — the ONE live
//!   service owned by the running `SyncService` (`sync_service
//!   .room_list_service()`). The actor subscribes to its `all_rooms()`
//!   entries stream (`entries_with_dynamic_adapters` with the non-left filter)
//!   and KEEPS CONSUMING it, re-normalizing on each joined/invited diff batch
//!   (Async rule 1: actors relay the SDK's observable streams).
//! - `None` on the LegacySync backend — the actor normalizes from
//!   `client.joined_rooms()` and relays `client
//!   .subscribe_to_all_room_updates()` (which fires on the legacy backend
//!   because it feeds the base client), coalescing pending batches into one
//!   re-normalization per wakeup.
//!
//! Snapshots are projected as generation-fenced room-list bootstrap actions +
//! `RoomEvent::RoomListUpdated`.
//!
//! Operation-triggered refreshes after the actor's own mutations remain: on
//! the SyncService path "refresh" means "re-normalize from the live service's
//! current entries" (a refresh request to the observation loop), never "new
//! service"; on the LegacySync path it is a joined_rooms re-normalization.
//!
//! Per Async rule 9: "Because the local QA matrix includes homeservers without
//! MSC4186, this legacy room-list path is a fully implemented, QA-gated
//! product path, not a stub."
//!
//! ## Room operations
//! `CreateRoom`, `CreateSpace`, `SetSpaceChild`, `InviteUser`, `JoinRoom`,
//! `LeaveRoom`, and `ForgetRoom` call `koushi-sdk` primitives and emit
//! domain events with `request_id`. Errors are classified into
//! `RoomFailureKind` (never raw SDK text).
//!
//! ## SelectSpace / SelectRoom
//! Pure navigation — project `AppAction::SelectSpace` / `AppAction::SelectRoom`
//! through the action channel. Core applies the navigation state update here
//! and does not consume reducer effects in this actor.
//!
//! ## Security
//! Raw SDK error text never appears in events or AppState. All errors are
//! classified into `RoomFailureKind`.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_sdk::{
    MatrixClientSession, MatrixCreateRoomOptions, MatrixCreateRoomParentSpace,
    MatrixCreateRoomVisibility, MatrixJoinedMemberSnapshot, MatrixPreviewJoinability,
    MatrixPreviewMembership, MatrixPublicRoomDirectoryQuery, MatrixPublicRoomDirectoryRoom,
    MatrixRoomHistoryVisibility, MatrixRoomJoinRule, MatrixRoomListRoom, MatrixRoomListSnapshot,
    MatrixRoomListSpace, MatrixRoomMemberRole, MatrixRoomMemberSummary, MatrixRoomModerationAction,
    MatrixRoomOperationError, MatrixRoomPermissionFacts, MatrixRoomPreview,
    MatrixRoomSettingChange, MatrixRoomSettingsSnapshot, MatrixRoomTagKind, MatrixRoomTags,
    MatrixSpaceMemberEntry, MatrixSpaceMembersProjection, MatrixUserTrustState,
};
use koushi_state::{
    AppAction, AvatarImage, AvatarThumbnailState, BasicOperationRequest,
    DirectoryPreviewJoinability, DirectoryPreviewMembership, DirectoryQuery, DirectoryRoomPreview,
    DirectoryRoomSummary, INVITE_ALREADY_IN_SPACE_MESSAGE, InviteDestination,
    InviteDestinationResult, InviteDestinationResultKind, InvitePreview, InviteScopeSelection,
    MentionCandidatesCompleteness, MentionCandidatesFailureKind, MentionSurface,
    OperationFailureKind, PinnedEvent, PinnedEventState, RoomHistoryVisibility, RoomJoinRule,
    RoomMemberRole, RoomMemberSummary, RoomMentionPermission, RoomModerationAction,
    RoomListFailureKind, RoomListSource, RoomNotificationMode,
    RoomPermissionFacts, RoomSettingChange, RoomSettingsSnapshot, RoomSummary, RoomTagInfo,
    RoomTagKind, RoomTags, SpaceMemberEntry, SpaceMemberInviteOutcome,
    SpaceMemberMembership, SpaceMembersProjection, SpaceSummary, UserProfile, UserTrustState,
};
#[cfg(test)]
use koushi_state::{ProfileResolutionInput, ProfileResolutionSource, resolve_people_label};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::command::{CreateRoomOptions, CreateRoomVisibility, RoomCommand};
use crate::event::{CoreEvent, ReportKind, RoomEvent};
use crate::executor;
use crate::failure::{CoreFailure, RoomFailureKind};
use crate::ids::{RequestId, RuntimeConnectionId};
use crate::mention_candidates::{MentionMemberInput, project_candidates};
use crate::unread_trace;

/// Fixed, content-free messages recorded in `AppState.errors` when a basic
/// operation fails. Raw SDK errors are classified into `RoomFailureKind` for the
/// transport `OperationFailed` event and never placed in product state.
const CREATE_ROOM_FAILED_MESSAGE: &str = "Room creation failed";
const CREATE_SPACE_FAILED_MESSAGE: &str = "Space creation failed";
const LINK_SPACE_CHILD_FAILED_MESSAGE: &str = "Linking the room to the space failed";

type SpaceChildLinkKey = (String, String);

const SPACE_MEMBER_REFRESH_CONNECTION_ID: RuntimeConnectionId = RuntimeConnectionId(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingSpaceChildLink {
    space_id: String,
    child_room_id: String,
    via_server: String,
}

/// Messages sent to the RoomActor from AccountActor / SyncActor.
pub enum RoomMessage {
    /// Route a `RoomCommand` to the actor.
    Command(RoomCommand),
    /// A store-backed session was established (login/restore/switch).
    /// Enables room operations; does NOT start the room-list observation —
    /// that starts on `SyncStarted` when the backend (and its live
    /// `RoomListService`, if any) is known.
    SessionEstablished { session: Arc<MatrixClientSession> },
    /// Sync started. Sent by `SyncActor` after the backend is launched.
    /// `room_list_service` is the ONE live service owned by the running
    /// `SyncService` (`Some` on the SyncService backend, `None` on
    /// LegacySync). Ad-hoc `RoomListService` instances are prohibited
    /// (canon, overview.md RoomActor bullet).
    SyncStarted {
        session: Arc<MatrixClientSession>,
        room_list_service: Option<Arc<matrix_sdk_ui::room_list_service::RoomListService>>,
        source: RoomListSource,
        backend_generation: u64,
    },
    /// The current SyncService generation has proved connectivity. The RoomActor
    /// reprojects the service's current entries so an empty snapshot is now
    /// authoritative rather than provisional.
    RoomListBootstrapProven {
        source: RoomListSource,
        backend_generation: u64,
    },
    /// Sync stopped: tear down any active room list subscription.
    SyncStopped,
    /// A backend task ended. The source/generation fence prevents a delayed
    /// stop from failing a replacement backend that already started.
    BackendSyncStopped {
        source: RoomListSource,
        backend_generation: u64,
    },
    /// The active account is logging out/switching/resetting while the
    /// RoomActor stays alive for future sessions.
    SessionCleared,
    /// Observer relay: parent-only space links discovered in a room-list
    /// snapshot. RoomActor owns dedupe, server writes, and retry policy.
    MissingSpaceChildLinks { links: Vec<MissingSpaceChildLink> },
    /// Account-data aliases are identity presentation inputs, never mention
    /// eligibility inputs. Reproject only already-demanded joined members.
    LocalUserAliasesUpdated { aliases: BTreeMap<String, String> },
    MentionMembersRefreshed {
        room_id: String,
        session_generation: u64,
        refresh_generation: u64,
        result: Result<MatrixJoinedMemberSnapshot, MatrixRoomOperationError>,
    },
    /// Base-client room updates can include membership state changes. Only
    /// demanded rooms are recomputed; `None` means the broadcast lagged and
    /// every demanded room must be self-healed.
    MentionMembershipChanged { room_ids: Option<BTreeSet<String>> },
    /// Completion of a local-only sync-driven Space-member projection.
    SpaceMembersProjectionRefreshed {
        request_id: RequestId,
        session_generation: u64,
        demand_generation: u64,
        refresh_generation: u64,
        space_id: String,
        generation: u64,
        result: Result<MatrixSpaceMembersProjection, MatrixRoomOperationError>,
    },
    /// A sync update changed the authoritative `m.room.pinned_events` state.
    /// The actor reloads only the affected rooms so external pin/unpin actions
    /// become visible without polling every room.
    PinnedEventsChanged { room_ids: BTreeSet<String> },
    /// Ordered shutdown.
    Shutdown,
}

/// Handle to the RoomActor background task (owned by AccountActor).
pub struct RoomActorHandle {
    pub(crate) tx: mpsc::Sender<RoomMessage>,
    task: executor::JoinHandle<()>,
}

impl RoomActorHandle {
    pub async fn send(&self, msg: RoomMessage) -> bool {
        self.tx.send(msg).await.is_ok()
    }

    /// Wait for the actor task to complete (used in ordered shutdown).
    pub async fn join(self) {
        let _ = self.task.await;
    }
}

/// Handle on the spawned room-list observation loop: oneshot stop signal plus
/// the task handle so teardown can await completion (same pattern as
/// `sync.rs` `legacy_stop_tx`). Operation-triggered refreshes are always sent
/// to the observation loop so command handling never blocks on room-list
/// normalization.
struct RoomListObservation {
    stop_tx: oneshot::Sender<()>,
    task: executor::JoinHandle<()>,
    refresh_tx: mpsc::Sender<()>,
    generation: u64,
    source: RoomListSource,
    authoritative: Arc<AtomicBool>,
}

pub struct RoomActor {
    session: Option<Arc<MatrixClientSession>>,
    observation: Option<RoomListObservation>,
    room_list_generation: u64,
    room_list_source: Option<RoomListSource>,
    room_list_backend_generation: Option<u64>,
    known_room_ids: Arc<RwLock<BTreeSet<String>>>,
    attempted_space_child_repairs: Arc<RwLock<BTreeSet<SpaceChildLinkKey>>>,
    mention_demands: HashMap<(String, MentionSurface), MentionDemand>,
    mention_member_snapshots: HashMap<String, MatrixJoinedMemberSnapshot>,
    mention_refresh_generations: HashMap<String, u64>,
    mention_refresh_sequence: u64,
    mention_session_generation: u64,
    mention_local_aliases: BTreeMap<String, String>,
    space_member_demand: Option<SpaceMemberDemand>,
    space_member_demand_generation: u64,
    space_member_refresh_sequence: u64,
    space_member_session_generation: u64,
    space_member_refresh_in_flight: Option<SpaceMemberRefreshFence>,
    space_member_refresh_pending: bool,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    self_tx: mpsc::Sender<RoomMessage>,
    command_rx: mpsc::Receiver<RoomMessage>,
}

#[derive(Clone)]
struct MentionDemand {
    request_id: RequestId,
    generation: u64,
    query: String,
}

#[derive(Clone)]
struct SpaceMemberDemand {
    space_id: String,
    generation: u64,
    child_room_ids: BTreeSet<String>,
    demand_generation: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct SpaceMemberRefreshFence {
    request_id: RequestId,
    session_generation: u64,
    demand_generation: u64,
    refresh_generation: u64,
}

impl RoomActor {
    pub fn spawn(
        action_tx: mpsc::Sender<Vec<AppAction>>,
        event_tx: broadcast::Sender<CoreEvent>,
    ) -> RoomActorHandle {
        let (tx, command_rx) = mpsc::channel(crate::runtime::ACTOR_MESSAGE_QUEUE_CAPACITY);
        let actor = RoomActor {
            session: None,
            observation: None,
            room_list_generation: 0,
            room_list_source: None,
            room_list_backend_generation: None,
            known_room_ids: Arc::new(RwLock::new(BTreeSet::new())),
            attempted_space_child_repairs: Arc::new(RwLock::new(BTreeSet::new())),
            mention_demands: HashMap::new(),
            mention_member_snapshots: HashMap::new(),
            mention_refresh_generations: HashMap::new(),
            mention_refresh_sequence: 0,
            mention_session_generation: 0,
            mention_local_aliases: BTreeMap::new(),
            space_member_demand: None,
            space_member_demand_generation: 0,
            space_member_refresh_sequence: 0,
            space_member_session_generation: 0,
            space_member_refresh_in_flight: None,
            space_member_refresh_pending: false,
            action_tx,
            event_tx,
            self_tx: tx.clone(),
            command_rx,
        };
        let task = executor::spawn(actor.run());
        RoomActorHandle { tx, task }
    }

    async fn run(mut self) {
        while let Some(msg) = self.command_rx.recv().await {
            match msg {
                RoomMessage::Shutdown => {
                    self.stop_observation().await;
                    break;
                }
                RoomMessage::Command(command) => {
                    self.handle_command(command).await;
                }
                RoomMessage::SessionEstablished { session } => {
                    // Room operations become available; observation starts
                    // later on SyncStarted (backend then known).
                    self.reset_space_member_session();
                    self.session = Some(session);
                    self.clear_known_rooms();
                    self.clear_space_child_repair_attempts();
                    self.clear_mention_candidates();
                }
                RoomMessage::SyncStarted {
                    session,
                    room_list_service,
                    source,
                    backend_generation,
                } => {
                    // Guard against two observation loops running: a previous
                    // loop (from an earlier SyncStarted) is stopped before the
                    // replacement is spawned.
                    self.stop_observation().await;
                    self.reset_space_member_session();
                    self.session = Some(session.clone());
                    self.room_list_generation = self.room_list_generation.wrapping_add(1).max(1);
                    self.room_list_source = Some(source);
                    self.room_list_backend_generation = Some(backend_generation);
                    // Keep the actor-known room book across backend handoff so
                    // cached rows remain actionable while the new generation
                    // is still loading. SessionEstablished/SessionCleared own
                    // the account-bound reset of this book.
                    self.clear_space_child_repair_attempts();
                    self.reduce_reliable(vec![AppAction::RoomListBootstrapStarted {
                        generation: self.room_list_generation,
                        source,
                    }])
                    .await;
                    match room_list_service {
                        Some(service) => {
                            // SyncService backend: relay the live service's
                            // entries stream. Its first diff batch (Reset with
                            // the current entries) provides the initial
                            // snapshot, so no separate initial refresh is
                            // needed.
                            self.start_live_observation(
                                session,
                                service,
                                self.room_list_generation,
                                source,
                            );
                        }
                        None => {
                            // LegacySync backend: relay the base client's
                            // room update broadcast (Async rule 1). Request
                            // the initial snapshot through the observation
                            // loop so SyncStarted never blocks this actor.
                            self.start_legacy_observation(
                                self.room_list_generation,
                                source,
                            );
                            self.refresh_room_list();
                        }
                    }
                }
                RoomMessage::RoomListBootstrapProven {
                    source,
                    backend_generation,
                } => {
                    if self.room_list_source == Some(source)
                        && self.room_list_backend_generation == Some(backend_generation)
                        && let Some(observation) = &self.observation
                        && observation.source == source
                        && observation.generation == self.room_list_generation
                    {
                        observation.authoritative.store(true, Ordering::Release);
                        let _ = observation.refresh_tx.try_send(());
                    }
                }
                RoomMessage::SyncStopped => {
                    self.stop_observation().await;
                    self.reset_space_member_session();
                    self.clear_known_rooms();
                    self.clear_space_child_repair_attempts();
                    self.room_list_source = None;
                    self.room_list_backend_generation = None;
                }
                RoomMessage::BackendSyncStopped {
                    source,
                    backend_generation,
                } => {
                    if self.room_list_source == Some(source)
                        && self.room_list_backend_generation == Some(backend_generation)
                    {
                        self.stop_observation().await;
                        self.reduce_reliable(vec![AppAction::RoomListBootstrapFailed {
                            generation: self.room_list_generation,
                            source,
                            kind: RoomListFailureKind::Stopped,
                        }])
                        .await;
                        self.room_list_source = None;
                        self.room_list_backend_generation = None;
                    }
                }
                RoomMessage::SessionCleared => {
                    self.stop_observation().await;
                    self.reset_space_member_session();
                    self.session = None;
                    self.clear_known_rooms();
                    self.clear_space_child_repair_attempts();
                    self.clear_mention_candidates();
                }
                RoomMessage::MissingSpaceChildLinks { links } => {
                    self.handle_missing_space_child_links(links).await;
                }
                RoomMessage::LocalUserAliasesUpdated { aliases } => {
                    self.handle_mention_local_aliases_updated(aliases).await;
                }
                RoomMessage::MentionMembersRefreshed {
                    room_id,
                    session_generation,
                    refresh_generation,
                    result,
                } => {
                    self.handle_mention_members_refreshed(
                        room_id,
                        session_generation,
                        refresh_generation,
                        result,
                    )
                    .await;
                }
                RoomMessage::MentionMembershipChanged { room_ids } => {
                    self.handle_mention_membership_changed(room_ids).await;
                }
                RoomMessage::SpaceMembersProjectionRefreshed {
                    request_id,
                    session_generation,
                    demand_generation,
                    refresh_generation,
                    space_id,
                    generation,
                    result,
                } => {
                    self.handle_space_members_projection_refreshed(
                        request_id,
                        session_generation,
                        demand_generation,
                        refresh_generation,
                        space_id,
                        generation,
                        result,
                    )
                    .await;
                }
                RoomMessage::PinnedEventsChanged { room_ids } => {
                    self.handle_pinned_events_changed(room_ids).await;
                }
            }
        }
    }

    /// Spawn the live-service observation loop (SyncService backend): relay
    /// the ONE live `RoomListService`'s entries stream and re-normalize on
    /// each diff batch.
    fn start_live_observation(
        &mut self,
        session: Arc<MatrixClientSession>,
        service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
        generation: u64,
        source: RoomListSource,
    ) {
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let (refresh_tx, refresh_rx) = mpsc::channel::<()>(8);
        let authoritative = Arc::new(AtomicBool::new(false));
        let task = executor::spawn(run_live_room_list_observation(
            session,
            service,
            self.known_room_ids.clone(),
            self.self_tx.clone(),
            self.action_tx.clone(),
            self.event_tx.clone(),
            refresh_rx,
            stop_rx,
            generation,
            source,
            authoritative.clone(),
        ));
        self.observation = Some(RoomListObservation {
            stop_tx,
            task,
            refresh_tx,
            generation,
            source,
            authoritative,
        });
    }

    /// Spawn the legacy room-list observation loop (LegacySync backend) for
    /// the current session.
    fn start_legacy_observation(&mut self, generation: u64, source: RoomListSource) {
        let Some(session) = &self.session else {
            return;
        };
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let (refresh_tx, refresh_rx) = mpsc::channel::<()>(8);
        let authoritative = Arc::new(AtomicBool::new(true));
        let task = executor::spawn(run_legacy_room_list_observation(
            session.clone(),
            self.known_room_ids.clone(),
            self.self_tx.clone(),
            self.action_tx.clone(),
            self.event_tx.clone(),
            refresh_rx,
            stop_rx,
            generation,
            source,
            authoritative.clone(),
        ));
        self.observation = Some(RoomListObservation {
            stop_tx,
            task,
            refresh_tx,
            generation,
            source,
            authoritative,
        });
    }

    async fn handle_missing_space_child_links(&mut self, links: Vec<MissingSpaceChildLink>) {
        let Some(session) = self.session.clone() else {
            return;
        };

        for link in links {
            let key = (link.space_id.clone(), link.child_room_id.clone());
            let already_repaired = self
                .attempted_space_child_repairs
                .read()
                .map(|attempts| attempts.contains(&key))
                .unwrap_or(true);
            if already_repaired {
                continue;
            }

            match koushi_sdk::set_space_child(
                &session,
                &link.space_id,
                &link.child_room_id,
                &link.via_server,
            )
            .await
            {
                Ok(()) => {
                    if let Ok(mut attempts) = self.attempted_space_child_repairs.write() {
                        attempts.insert(key);
                    }
                    self.refresh_room_list();
                }
                Err(error) => {
                    let _kind = classify_room_error(&error);
                }
            }
        }
    }

    /// Stop the observation loop (if running) and wait for it to exit.
    async fn stop_observation(&mut self) {
        if let Some(observation) = self.observation.take() {
            let _ = observation.stop_tx.send(());
            let _ = observation.task.await;
        }
    }

    async fn handle_command(&mut self, command: RoomCommand) {
        match command {
            RoomCommand::CreateRoom {
                request_id,
                options,
            } => {
                self.handle_create_room(request_id, options).await;
            }
            RoomCommand::CreatePublicDirectoryRoom {
                request_id,
                name,
                alias_localpart,
            } => {
                self.handle_create_public_directory_room(request_id, name, alias_localpart)
                    .await;
            }
            RoomCommand::CreateSpace { request_id, name } => {
                self.handle_create_space(request_id, name).await;
            }
            RoomCommand::SetSpaceChild {
                request_id,
                space_id,
                child_room_id,
                via_server,
            } => {
                self.handle_set_space_child(request_id, space_id, child_room_id, via_server)
                    .await;
            }
            RoomCommand::InviteUser {
                request_id,
                room_id,
                user_id,
            } => {
                self.handle_invite_user(request_id, room_id, user_id).await;
            }
            RoomCommand::LoadSpaceMembers {
                request_id,
                space_id,
                generation,
            } => {
                self.handle_load_space_members(request_id, space_id, generation)
                    .await;
            }
            RoomCommand::InviteUserToSpace {
                request_id,
                space_id,
                user_id,
                generation,
            } => {
                self.handle_invite_user_to_space(request_id, space_id, user_id, generation)
                    .await;
            }
            RoomCommand::CancelSpaceInvite {
                request_id,
                space_id,
                user_id,
                generation,
            } => {
                self.handle_cancel_space_invite(request_id, space_id, user_id, generation)
                    .await;
            }
            RoomCommand::InviteTargets {
                request_id,
                room_id,
                user_ids,
                scope,
            } => {
                self.handle_invite_targets(request_id, room_id, user_ids, scope)
                    .await;
            }
            RoomCommand::AcceptInvite {
                request_id,
                room_id,
            } => {
                self.handle_accept_invite(request_id, room_id).await;
            }
            RoomCommand::DeclineInvite {
                request_id,
                room_id,
            } => {
                self.handle_decline_invite(request_id, room_id).await;
            }
            RoomCommand::StartDirectMessage {
                request_id,
                user_id,
            } => {
                self.handle_start_direct_message(request_id, user_id).await;
            }
            RoomCommand::JoinRoom {
                request_id,
                room_id,
            } => {
                self.handle_join_room(request_id, room_id).await;
            }
            RoomCommand::LeaveRoom {
                request_id,
                room_id,
            } => {
                self.handle_leave_room(request_id, room_id).await;
            }
            RoomCommand::ForgetRoom {
                request_id,
                room_id,
            } => {
                self.handle_forget_room(request_id, room_id).await;
            }
            RoomCommand::SetTag {
                request_id,
                room_id,
                tag,
                order,
            } => {
                self.handle_set_tag(request_id, room_id, tag, order).await;
            }
            RoomCommand::RemoveTag {
                request_id,
                room_id,
                tag,
            } => {
                self.handle_remove_tag(request_id, room_id, tag).await;
            }
            RoomCommand::PinEvent {
                request_id,
                room_id,
                event_id,
            } => {
                self.handle_pin_event(request_id, room_id, event_id).await;
            }
            RoomCommand::UnpinEvent {
                request_id,
                room_id,
                event_id,
            } => {
                self.handle_unpin_event(request_id, room_id, event_id).await;
            }
            RoomCommand::RefreshPinnedEvents {
                request_id,
                room_id,
            } => {
                self.handle_refresh_pinned_events(request_id, room_id).await;
            }
            RoomCommand::QueryDirectory { request_id, query } => {
                self.handle_query_directory(request_id, query).await;
            }
            RoomCommand::PreviewJoinTarget {
                request_id,
                room_id_or_alias,
                via_servers,
            } => {
                self.handle_preview_join_target(request_id, room_id_or_alias, via_servers)
                    .await;
            }
            RoomCommand::DismissDirectoryPreview { request_id: _ } => {
                self.reduce_reliable(vec![AppAction::DirectoryPreviewDismissed])
                    .await;
            }
            RoomCommand::JoinDirectoryRoom {
                request_id,
                room_id_or_alias,
                via_servers,
            } => {
                self.handle_join_directory_room(request_id, room_id_or_alias, via_servers)
                    .await;
            }
            RoomCommand::LoadRoomSettings {
                request_id,
                room_id,
            } => {
                self.handle_load_room_settings(request_id, room_id).await;
            }
            RoomCommand::QueryMentionCandidates {
                request_id,
                account_key,
                room_id,
                surface,
                query,
            } => {
                self.handle_query_mention_candidates(
                    request_id,
                    account_key,
                    room_id,
                    surface,
                    query,
                )
                .await;
            }
            RoomCommand::ReshareRoomKey {
                request_id,
                room_id,
            } => {
                self.handle_reshare_room_key(request_id, room_id).await;
            }
            RoomCommand::UpdateRoomSetting {
                request_id,
                room_id,
                change,
            } => {
                self.handle_update_room_setting(request_id, room_id, change)
                    .await;
            }
            RoomCommand::ModerateRoomMember {
                request_id,
                room_id,
                target_user_id,
                action,
                reason,
            } => {
                self.handle_moderate_room_member(
                    request_id,
                    room_id,
                    target_user_id,
                    action,
                    reason,
                )
                .await;
            }
            RoomCommand::UpdateRoomMemberRole {
                request_id,
                room_id,
                target_user_id,
                power_level,
            } => {
                self.handle_update_room_member_role(
                    request_id,
                    room_id,
                    target_user_id,
                    power_level,
                )
                .await;
            }
            RoomCommand::SelectSpace {
                request_id: _,
                space_id,
            } => {
                // Pure navigation: project to reducer; no domain event.
                // request_id correlation via StateChanged is implicit per spec.
                // One-shot navigation MUST be delivered reliably (see reduce_reliable).
                self.reduce_reliable(vec![AppAction::SelectSpace { space_id }])
                    .await;
            }
            RoomCommand::ReorderSpaces {
                request_id: _,
                space_ids,
            } => {
                // Pure navigation preference: project to reducer; no domain event.
                // One-shot navigation MUST be delivered reliably (see reduce_reliable).
                self.reduce_reliable(vec![AppAction::ReorderSpaces { space_ids }])
                    .await;
            }
            RoomCommand::SelectRoom {
                request_id: _,
                room_id,
            } => {
                // Pure navigation: project to reducer; no domain event.
                // Core updates navigation state here and does not consume
                // reducer effects in this actor. One-shot navigation MUST be
                // delivered reliably: a dropped SelectRoom is the large-account
                // "room selection did not complete" bug (see reduce_reliable).
                self.reduce_reliable(vec![AppAction::SelectRoom { room_id }])
                    .await;
            }
            RoomCommand::MarkRoomAsRead {
                request_id,
                room_id,
                event_id,
            } => {
                self.handle_mark_room_as_read(request_id, room_id, event_id)
                    .await;
            }
            RoomCommand::MarkRoomAsUnread {
                request_id,
                room_id,
                unread,
            } => {
                self.handle_mark_room_as_unread(request_id, room_id, unread)
                    .await;
            }
            RoomCommand::SetRoomNotificationMode {
                request_id,
                room_id,
                mode,
            } => {
                self.handle_set_room_notification_mode(request_id, room_id, mode)
                    .await;
            }
            RoomCommand::ReportContent {
                request_id,
                room_id,
                event_id,
                reason,
            } => {
                self.handle_report_content(request_id, room_id, event_id, reason)
                    .await;
            }
            RoomCommand::ReportRoom {
                request_id,
                room_id,
                reason,
            } => {
                self.handle_report_room(request_id, room_id, reason).await;
            }
        }
    }

    async fn handle_create_room(&self, request_id: RequestId, options: CreateRoomOptions) {
        trace_room_operation("create_room", "start", request_id);
        let Some(session) = &self.session else {
            trace_room_operation("create_room", "session_required", request_id);
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        let name = options.name.clone();
        let parent_space = options.parent_space.clone();
        // Drive the basic-operation state machine: Idle -> CreatingRoom. The
        // reducer guards re-entry; `request_id.sequence` is the correlation id
        // the settle action below must match.
        self.reduce_reliable(vec![AppAction::BasicOperationRequested {
            request_id: request_id.sequence,
            request: BasicOperationRequest::CreateRoom { name: name.clone() },
        }])
        .await;
        match koushi_sdk::create_room(session, matrix_create_room_options(options)).await {
            Ok(room_id) => {
                trace_room_operation("create_room", "succeeded", request_id);
                self.link_created_room_to_parent_space(
                    session,
                    parent_space.as_ref(),
                    &room_id,
                    request_id,
                )
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomCreated {
                    request_id,
                    room_id,
                }));
                self.reduce_reliable(vec![AppAction::BasicOperationSucceeded {
                    request_id: request_id.sequence,
                }])
                .await;
                // Reflect the actor's own mutation immediately instead of
                // waiting for the next sync round-trip.
                self.refresh_room_list();
            }
            Err(error) => {
                trace_room_operation("create_room", "failed", request_id);
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                self.reduce_reliable(vec![AppAction::BasicOperationFailed {
                    request_id: request_id.sequence,
                    message: CREATE_ROOM_FAILED_MESSAGE.to_owned(),
                }])
                .await;
            }
        }
    }

    async fn link_created_room_to_parent_space(
        &self,
        session: &MatrixClientSession,
        parent_space: Option<&crate::command::CreateRoomParentSpace>,
        room_id: &str,
        request_id: RequestId,
    ) {
        let Some(parent_space) = parent_space else {
            return;
        };
        let Ok(via_server) = koushi_sdk::room_id_server_name(room_id) else {
            return;
        };

        match koushi_sdk::set_space_child(session, &parent_space.space_id, room_id, &via_server)
            .await
        {
            Ok(()) => {
                self.mark_space_child_link_attempted(&parent_space.space_id, room_id);
                self.emit(CoreEvent::Room(RoomEvent::SpaceChildSet {
                    request_id,
                    space_id: parent_space.space_id.clone(),
                    child_room_id: room_id.to_owned(),
                }));
            }
            Err(_) => {}
        }
    }

    async fn handle_create_public_directory_room(
        &self,
        request_id: RequestId,
        name: String,
        alias_localpart: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::create_public_directory_room(session, &name, &alias_localpart).await {
            Ok(room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomCreated {
                    request_id,
                    room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_create_space(&self, request_id: RequestId, name: String) {
        trace_room_operation("create_space", "start", request_id);
        let Some(session) = &self.session else {
            trace_room_operation("create_space", "session_required", request_id);
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        // Drive the basic-operation state machine: Idle -> CreatingSpace.
        self.reduce_reliable(vec![AppAction::BasicOperationRequested {
            request_id: request_id.sequence,
            request: BasicOperationRequest::CreateSpace { name: name.clone() },
        }])
        .await;
        match koushi_sdk::create_space(session, &name).await {
            Ok(space_id) => {
                trace_room_operation("create_space", "succeeded", request_id);
                self.emit(CoreEvent::Room(RoomEvent::SpaceCreated {
                    request_id,
                    space_id,
                }));
                self.reduce_reliable(vec![AppAction::BasicOperationSucceeded {
                    request_id: request_id.sequence,
                }])
                .await;
                // Reflect the actor's own mutation immediately.
                self.refresh_room_list();
            }
            Err(error) => {
                trace_room_operation("create_space", "failed", request_id);
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                self.reduce_reliable(vec![AppAction::BasicOperationFailed {
                    request_id: request_id.sequence,
                    message: CREATE_SPACE_FAILED_MESSAGE.to_owned(),
                }])
                .await;
            }
        }
    }

    async fn handle_set_space_child(
        &self,
        request_id: RequestId,
        space_id: String,
        child_room_id: String,
        via_server: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        // Drive the basic-operation state machine: Idle -> LinkingSpaceChild.
        self.reduce_reliable(vec![AppAction::BasicOperationRequested {
            request_id: request_id.sequence,
            request: BasicOperationRequest::LinkSpaceChild {
                space_id: space_id.clone(),
                child_room_id: child_room_id.clone(),
            },
        }])
        .await;
        match koushi_sdk::set_space_child(session, &space_id, &child_room_id, &via_server).await {
            Ok(()) => {
                self.emit(CoreEvent::Room(RoomEvent::SpaceChildSet {
                    request_id,
                    space_id,
                    child_room_id,
                }));
                self.reduce_reliable(vec![AppAction::BasicOperationSucceeded {
                    request_id: request_id.sequence,
                }])
                .await;
                // Reflect the actor's own mutation immediately.
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                self.reduce_reliable(vec![AppAction::BasicOperationFailed {
                    request_id: request_id.sequence,
                    message: LINK_SPACE_CHILD_FAILED_MESSAGE.to_owned(),
                }])
                .await;
            }
        }
    }

    async fn handle_invite_user(&self, request_id: RequestId, room_id: String, user_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::invite_user_to_room(session, &room_id, &user_id).await {
            Ok(()) => {
                self.emit(CoreEvent::Room(RoomEvent::UserInvited {
                    request_id,
                    room_id,
                    user_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_load_space_members(
        &mut self,
        request_id: RequestId,
        space_id: String,
        generation: u64,
    ) {
        // Keep a same-Space/generation demand installed while this explicit
        // load is in flight so a failed retry cannot lose sync refreshes. A
        // different demand still supersedes the previous Space immediately.
        if should_clear_space_member_demand(
            self.space_member_demand.as_ref(),
            &space_id,
            generation,
        ) {
            self.clear_space_member_demand();
        }
        let Some(session) = self.session.clone() else {
            let kind = OperationFailureKind::Sdk;
            self.reduce_reliable(vec![AppAction::SpaceMembersLoadFailed {
                request_id: request_id.sequence,
                space_id,
                generation,
                kind,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::matrix_space_members_projection(&session, &space_id).await {
            Ok(raw_projection) => {
                self.install_space_member_demand(
                    &space_id,
                    generation,
                    &raw_projection.child_room_ids,
                );
                let profile_updates = user_profiles_from_space_projection(&raw_projection);
                let projection = state_space_members_projection(raw_projection.clone(), generation);
                record_core_space_members_projection_with_raw(
                    "load",
                    generation,
                    &raw_projection,
                    &projection,
                    "success",
                );
                self.reduce_reliable(vec![AppAction::SpaceMembersProjectionReconciled {
                    request_id: request_id.sequence,
                    projection: projection.clone(),
                    profiles: profile_updates,
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::SpaceMembersLoaded {
                    request_id,
                    generation,
                    joined_count: projection.space_joined.len(),
                    invited_count: projection.space_invited.len(),
                    child_room_only_count: projection.child_room_only.len(),
                    incomplete_child_room_count: projection.incomplete_child_room_count,
                }));
            }
            Err(error) => {
                let kind = operation_failure_kind(classify_room_error(&error));
                record_core_space_members_load_failure("load", generation);
                self.reduce_reliable(vec![AppAction::SpaceMembersLoadFailed {
                    request_id: request_id.sequence,
                    space_id,
                    generation,
                    kind,
                }])
                .await;
                self.emit_failure(
                    request_id,
                    CoreFailure::RoomOperationFailed {
                        kind: classify_room_error(&error),
                    },
                );
            }
        }
    }

    async fn handle_space_membership_changed(&mut self, room_ids: Option<&BTreeSet<String>>) {
        let Some(demand) = self.space_member_demand.clone() else {
            return;
        };
        if !space_members_update_affects_demand(&demand.space_id, &demand.child_room_ids, room_ids)
        {
            return;
        }
        if self.space_member_refresh_in_flight.is_some() {
            self.space_member_refresh_pending = true;
            return;
        }
        self.start_space_member_refresh(demand);
    }

    fn start_space_member_refresh(&mut self, demand: SpaceMemberDemand) {
        let Some(session) = self.session.clone() else {
            return;
        };

        self.space_member_refresh_sequence =
            self.space_member_refresh_sequence.wrapping_add(1).max(1);
        let refresh_generation = self.space_member_refresh_sequence;
        let request_id = RequestId {
            connection_id: SPACE_MEMBER_REFRESH_CONNECTION_ID,
            sequence: refresh_generation,
        };
        let session_generation = self.space_member_session_generation;
        let fence = SpaceMemberRefreshFence {
            request_id,
            session_generation,
            demand_generation: demand.demand_generation,
            refresh_generation,
        };
        self.space_member_refresh_in_flight = Some(fence);

        let room_tx = self.self_tx.clone();
        let space_id = demand.space_id.clone();
        let generation = demand.generation;
        let demand_generation = demand.demand_generation;
        let _ = executor::spawn(async move {
            let result = koushi_sdk::matrix_space_members_projection(&session, &space_id).await;
            let _ = room_tx
                .send(RoomMessage::SpaceMembersProjectionRefreshed {
                    request_id,
                    session_generation,
                    demand_generation,
                    refresh_generation,
                    space_id,
                    generation,
                    result,
                })
                .await;
        });
    }

    async fn handle_space_members_projection_refreshed(
        &mut self,
        request_id: RequestId,
        session_generation: u64,
        demand_generation: u64,
        refresh_generation: u64,
        space_id: String,
        generation: u64,
        result: Result<MatrixSpaceMembersProjection, MatrixRoomOperationError>,
    ) {
        let Some(demand) = self.space_member_demand.clone() else {
            return;
        };
        let is_current = space_member_refresh_fence_is_current(
            self.space_member_refresh_in_flight,
            SpaceMemberRefreshFence {
                request_id,
                session_generation,
                demand_generation,
                refresh_generation,
            },
            self.space_member_session_generation,
            demand.demand_generation,
            &space_id,
            generation,
            &demand.space_id,
            demand.generation,
        );
        if !is_current {
            record_space_member_refresh_event("stale_completion_ignored", false);
            return;
        }

        self.space_member_refresh_in_flight = None;
        let should_refresh_again = std::mem::take(&mut self.space_member_refresh_pending);
        match result {
            Ok(raw_projection) => {
                let profiles = user_profiles_from_space_projection(&raw_projection);
                let projection = state_space_members_projection(raw_projection.clone(), generation);
                record_core_space_members_projection_with_raw(
                    "sync_refresh",
                    generation,
                    &raw_projection,
                    &projection,
                    "success",
                );
                self.reduce_reliable(vec![
                    AppAction::SpaceMembersBackgroundProjectionReconciled {
                        request_id: request_id.sequence,
                        space_id: space_id.clone(),
                        generation,
                        projection,
                        profiles,
                    },
                ])
                .await;
                self.install_space_member_demand(
                    &space_id,
                    generation,
                    &raw_projection.child_room_ids,
                );
            }
            Err(_error) => {
                // A background lookup failure is deliberately silent at the
                // state layer: the last-known projection remains visible and
                // the next relevant sync update may retry it.
                record_core_space_members_load_failure("sync_refresh", generation);
            }
        }

        if should_refresh_again {
            if let Some(demand) = self.space_member_demand.clone() {
                self.start_space_member_refresh(demand);
            }
        }
    }

    async fn handle_invite_user_to_space(
        &self,
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    ) {
        let (outcome, reconciliation) = match &self.session {
            None => (
                SpaceMemberInviteOutcome::Failed(OperationFailureKind::Sdk),
                None,
            ),
            Some(session) => {
                match koushi_sdk::invite_user_to_room(session, &space_id, &user_id).await {
                    Ok(()) => {
                        let fallback = SpaceMemberInviteOutcome::Invited;
                        match reconcile_space_invite_outcome(
                            session,
                            &space_id,
                            &user_id,
                            generation,
                            fallback.clone(),
                        )
                        .await
                        {
                            Some(reconciliation) => {
                                (reconciliation.outcome.clone(), Some(reconciliation))
                            }
                            None => (fallback, None),
                        }
                    }
                    Err(error) => {
                        let failure_kind = operation_failure_kind(classify_room_error(&error));
                        let fallback = SpaceMemberInviteOutcome::Failed(failure_kind);
                        match reconcile_space_invite_outcome(
                            session,
                            &space_id,
                            &user_id,
                            generation,
                            fallback.clone(),
                        )
                        .await
                        {
                            Some(reconciliation) => {
                                (reconciliation.outcome.clone(), Some(reconciliation))
                            }
                            None => (fallback, None),
                        }
                    }
                }
            }
        };
        record_core_space_members_operation("invite", generation, &outcome);
        let mut actions = Vec::new();
        if let Some(reconciliation) = reconciliation {
            actions.push(AppAction::SpaceMembersProjectionReconciled {
                request_id: request_id.sequence,
                projection: reconciliation.projection,
                profiles: reconciliation.profiles,
            });
        }
        actions.push(AppAction::SpaceMemberInviteSettled {
            request_id: request_id.sequence,
            space_id,
            user_id,
            generation,
            outcome: outcome.clone(),
        });
        self.reduce_reliable(actions).await;
        self.emit(CoreEvent::Room(RoomEvent::SpaceMemberInviteSettled {
            request_id,
            generation,
            outcome,
        }));
    }

    async fn handle_cancel_space_invite(
        &self,
        request_id: RequestId,
        space_id: String,
        user_id: String,
        generation: u64,
    ) {
        let (outcome, reconciliation) = match &self.session {
            None => (
                SpaceMemberInviteOutcome::Failed(OperationFailureKind::Sdk),
                None,
            ),
            Some(session) => {
                let outcome =
                    match koushi_sdk::cancel_space_invite(session, &space_id, &user_id).await {
                        Ok(koushi_sdk::MatrixSpaceInviteCancellationOutcome::Cancelled) => {
                            SpaceMemberInviteOutcome::Cancelled
                        }
                        Ok(koushi_sdk::MatrixSpaceInviteCancellationOutcome::NotInvited) => {
                            SpaceMemberInviteOutcome::NotInvited
                        }
                        Err(error) => SpaceMemberInviteOutcome::Failed(operation_failure_kind(
                            classify_room_error(&error),
                        )),
                    };
                let reconciliation =
                    reconcile_space_invite_cancellation(session, &space_id, generation).await;
                (outcome, reconciliation)
            }
        };
        record_core_space_members_operation("cancel", generation, &outcome);
        let mut actions = Vec::new();
        if let Some(reconciliation) = reconciliation {
            actions.push(AppAction::SpaceMembersProjectionReconciled {
                request_id: request_id.sequence,
                projection: reconciliation.projection,
                profiles: reconciliation.profiles,
            });
        }
        actions.push(AppAction::SpaceMemberInviteCancellationSettled {
            request_id: request_id.sequence,
            space_id,
            user_id,
            generation,
            outcome: outcome.clone(),
        });
        self.reduce_reliable(actions).await;
        self.emit(CoreEvent::Room(
            RoomEvent::SpaceMemberInviteCancellationSettled {
                request_id,
                generation,
                outcome,
            },
        ));
    }

    async fn handle_invite_targets(
        &self,
        request_id: RequestId,
        room_id: String,
        user_ids: Vec<String>,
        scope: InviteScopeSelection,
    ) {
        self.reduce_reliable(vec![AppAction::InviteBatchRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            user_ids: user_ids.clone(),
            scope: scope.clone(),
        }])
        .await;

        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            self.reduce_reliable(vec![AppAction::InviteBatchFailed {
                request_id: request_id.sequence,
                room_id,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            return;
        };

        let mut results = Vec::new();
        let mut any_invited = false;

        for user_id in user_ids {
            if let InviteScopeSelection::ParentSpaceAndRoom { space_id } = &scope {
                match invite_target_to_space_if_needed(session, space_id, &user_id).await {
                    InviteTargetOutcome::Invited => {
                        any_invited = true;
                        results.push(InviteDestinationResult {
                            user_id: user_id.clone(),
                            destination: InviteDestination::Space {
                                space_id: space_id.clone(),
                            },
                            kind: InviteDestinationResultKind::Invited,
                            message: None,
                        });
                    }
                    InviteTargetOutcome::AlreadyInSpace => {
                        results.push(InviteDestinationResult {
                            user_id: user_id.clone(),
                            destination: InviteDestination::Space {
                                space_id: space_id.clone(),
                            },
                            kind: InviteDestinationResultKind::AlreadyInSpace,
                            message: Some(INVITE_ALREADY_IN_SPACE_MESSAGE.to_owned()),
                        });
                    }
                    InviteTargetOutcome::Failed => {
                        results.push(InviteDestinationResult {
                            user_id: user_id.clone(),
                            destination: InviteDestination::Space {
                                space_id: space_id.clone(),
                            },
                            kind: InviteDestinationResultKind::Failed,
                            message: None,
                        });
                    }
                }
            }

            match koushi_sdk::invite_user_to_room(session, &room_id, &user_id).await {
                Ok(()) => {
                    any_invited = true;
                    results.push(InviteDestinationResult {
                        user_id: user_id.clone(),
                        destination: InviteDestination::Room {
                            room_id: room_id.clone(),
                        },
                        kind: InviteDestinationResultKind::Invited,
                        message: None,
                    });
                }
                Err(_error) => {
                    results.push(InviteDestinationResult {
                        user_id,
                        destination: InviteDestination::Room {
                            room_id: room_id.clone(),
                        },
                        kind: InviteDestinationResultKind::Failed,
                        message: None,
                    });
                }
            }
        }

        self.reduce_reliable(vec![AppAction::InviteBatchCompleted {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            results: results.clone(),
        }])
        .await;
        self.emit(CoreEvent::Room(RoomEvent::InviteBatchCompleted {
            request_id,
            room_id,
            results,
        }));
        if any_invited {
            self.refresh_room_list();
        }
    }

    async fn handle_accept_invite(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::join_room_by_id(session, &room_id).await {
            Ok(joined_room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::InviteAccepted {
                    request_id,
                    room_id: joined_room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_decline_invite(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::leave_room(session, &room_id).await {
            Ok(declined_room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::InviteDeclined {
                    request_id,
                    room_id: declined_room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_start_direct_message(&self, request_id: RequestId, user_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::start_direct_message(session, &user_id).await {
            Ok(room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::DirectMessageStarted {
                    request_id,
                    room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_join_room(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::join_room_by_id(session, &room_id).await {
            Ok(joined_room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomJoined {
                    request_id,
                    room_id: joined_room_id,
                }));
                // Reflect the actor's own mutation immediately.
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_query_directory(&self, request_id: RequestId, query: DirectoryQuery) {
        self.reduce_reliable(vec![AppAction::DirectoryQueryRequested {
            request_id: request_id.sequence,
            query: query.clone(),
        }])
        .await;
        let Some(session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryQueryFailed {
                request_id: request_id.sequence,
                query,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let sdk_query = MatrixPublicRoomDirectoryQuery {
            term: query.term.clone(),
            server_name: query.server_name.clone(),
            limit: query.limit,
            since: query.since.clone(),
        };
        match koushi_sdk::query_public_room_directory(session, sdk_query).await {
            Ok(result) => {
                let rooms: Vec<DirectoryRoomSummary> = result
                    .rooms
                    .into_iter()
                    .map(directory_room_summary_from_sdk)
                    .collect();
                self.reduce_reliable(vec![AppAction::DirectoryQuerySucceeded {
                    request_id: request_id.sequence,
                    query: query.clone(),
                    rooms: rooms.clone(),
                    next_batch: result.next_batch.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::DirectoryQueryCompleted {
                    request_id,
                    query,
                    rooms,
                    next_batch: result.next_batch,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryQueryFailed {
                    request_id: request_id.sequence,
                    query,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_preview_join_target(
        &self,
        request_id: RequestId,
        room_id_or_alias: String,
        via_servers: Vec<String>,
    ) {
        self.reduce_reliable(vec![AppAction::DirectoryPreviewRequested {
            request_id: request_id.sequence,
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        }])
        .await;
        let Some(session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryPreviewFailed {
                request_id: request_id.sequence,
                room_id_or_alias,
                via_servers,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let target = koushi_sdk::MatrixJoinTarget {
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        };
        match koushi_sdk::preview_join_target(session, &target).await {
            Ok(preview) => {
                let room = directory_room_preview_from_sdk(preview);
                self.reduce_reliable(vec![AppAction::DirectoryPreviewLoaded {
                    request_id: request_id.sequence,
                    room: room.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::DirectoryPreviewLoaded {
                    request_id,
                    room,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryPreviewFailed {
                    request_id: request_id.sequence,
                    room_id_or_alias,
                    via_servers,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_join_directory_room(
        &self,
        request_id: RequestId,
        room_id_or_alias: String,
        via_servers: Vec<String>,
    ) {
        self.reduce_reliable(vec![AppAction::DirectoryJoinRequested {
            request_id: request_id.sequence,
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        }])
        .await;
        let Some(session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                request_id: request_id.sequence,
                room_id_or_alias,
                via_servers,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let join_target = koushi_sdk::MatrixJoinTarget {
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        };
        match koushi_sdk::join_room_target(session, &join_target).await {
            Ok(room_id) => {
                self.reduce_reliable(vec![AppAction::DirectoryJoinSucceeded {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomJoined {
                    request_id,
                    room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                    request_id: request_id.sequence,
                    room_id_or_alias,
                    via_servers,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_load_room_settings(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
                    room_id,
                    settings: settings.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomSettingsLoaded {
                    request_id,
                    settings,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_reshare_room_key(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::reshare_room_key(session, &room_id).await {
            Ok(()) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomKeyReshared {
                    request_id,
                    room_id,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_update_room_setting(
        &self,
        request_id: RequestId,
        room_id: String,
        change: RoomSettingChange,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !settings.permissions.can_edit_settings {
            self.reduce_reliable(vec![AppAction::RoomSettingUpdateRequested {
                request_id: request_id.sequence,
                room_id,
                change,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomSettingUpdateRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            change: change.clone(),
        }])
        .await;

        match koushi_sdk::update_room_setting(session, &room_id, room_setting_change_to_sdk(change))
            .await
        {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![AppAction::RoomSettingUpdateSucceeded {
                    request_id: request_id.sequence,
                    room_id,
                    settings: settings.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomSettingUpdated {
                    request_id,
                    settings,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomSettingUpdateFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_moderate_room_member(
        &self,
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        action: RoomModerationAction,
        reason: Option<String>,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !room_moderation_allowed(&settings.permissions, action) {
            self.reduce_reliable(vec![AppAction::RoomModerationRequested {
                request_id: request_id.sequence,
                room_id,
                target_user_id,
                action,
                reason,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomModerationRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            target_user_id: target_user_id.clone(),
            action,
            reason: reason.clone(),
        }])
        .await;

        match koushi_sdk::moderate_room_member(
            session,
            &room_id,
            &target_user_id,
            room_moderation_action_to_sdk(action),
            reason.as_deref(),
        )
        .await
        {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::RoomModerationSucceeded {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                    target_user_id: target_user_id.clone(),
                    action,
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomMemberModerated {
                    request_id,
                    room_id,
                    target_user_id,
                    action,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomModerationFailed {
                    request_id: request_id.sequence,
                    room_id,
                    target_user_id,
                    action,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_update_room_member_role(
        &self,
        request_id: RequestId,
        room_id: String,
        target_user_id: String,
        power_level: i64,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let settings = match koushi_sdk::get_room_settings_snapshot(session, &room_id).await {
            Ok(settings) => room_settings_snapshot_from_sdk(settings),
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
                return;
            }
        };
        self.reduce_reliable(vec![AppAction::RoomSettingsSnapshotLoaded {
            room_id: room_id.clone(),
            settings: settings.clone(),
        }])
        .await;
        if !settings.permissions.can_edit_roles {
            self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateRequested {
                request_id: request_id.sequence,
                room_id,
                target_user_id,
                power_level,
            }])
            .await;
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::Forbidden,
                },
            );
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            target_user_id: target_user_id.clone(),
            power_level,
        }])
        .await;

        match koushi_sdk::update_room_member_power_level(
            session,
            &room_id,
            &target_user_id,
            power_level,
        )
        .await
        {
            Ok(settings) => {
                let settings = room_settings_snapshot_from_sdk(settings);
                self.reduce_reliable(vec![
                    AppAction::RoomSettingsSnapshotLoaded {
                        room_id: room_id.clone(),
                        settings,
                    },
                    AppAction::RoomMemberRoleUpdateRequested {
                        request_id: request_id.sequence,
                        room_id: room_id.clone(),
                        target_user_id: target_user_id.clone(),
                        power_level,
                    },
                    AppAction::RoomMemberRoleUpdateSucceeded {
                        request_id: request_id.sequence,
                        room_id: room_id.clone(),
                        target_user_id: target_user_id.clone(),
                        power_level,
                    },
                ])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomMemberRoleUpdated {
                    request_id,
                    room_id,
                    target_user_id,
                    power_level,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomMemberRoleUpdateFailed {
                    request_id: request_id.sequence,
                    room_id,
                    target_user_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_leave_room(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::leave_room(session, &room_id).await {
            Ok(left_room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomLeft {
                    request_id,
                    room_id: left_room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_forget_room(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::forget_room(session, &room_id).await {
            Ok(forgotten_room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomForgotten {
                    request_id,
                    room_id: forgotten_room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_set_tag(
        &self,
        request_id: RequestId,
        room_id: String,
        tag: RoomTagKind,
        order: Option<f64>,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::set_room_tag(session, &room_id, sdk_room_tag_kind(tag), order).await {
            Ok(()) => {
                let info = room_tag_info_from_order(order);
                if self
                    .action_tx
                    .send(vec![AppAction::RoomTagSet {
                        room_id: room_id.clone(),
                        tag,
                        info,
                    }])
                    .await
                    .is_err()
                {
                    self.emit_failure(
                        request_id,
                        CoreFailure::RoomOperationFailed {
                            kind: RoomFailureKind::Sdk,
                        },
                    );
                    return;
                }
                // `set_is_favourite` / `set_is_low_priority` only send the
                // tag mutation to the server; the SDK room-list snapshot may
                // remain stale until the next sync. Keep the immediate state
                // projection in the reducer action above instead of refreshing
                // and potentially overwriting it with old tags.
                self.emit(CoreEvent::Room(RoomEvent::RoomTagSet {
                    request_id,
                    room_id,
                    tag,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_remove_tag(&self, request_id: RequestId, room_id: String, tag: RoomTagKind) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match koushi_sdk::remove_room_tag(session, &room_id, sdk_room_tag_kind(tag)).await {
            Ok(()) => {
                if self
                    .action_tx
                    .send(vec![AppAction::RoomTagRemoved {
                        room_id: room_id.clone(),
                        tag,
                    }])
                    .await
                    .is_err()
                {
                    self.emit_failure(
                        request_id,
                        CoreFailure::RoomOperationFailed {
                            kind: RoomFailureKind::Sdk,
                        },
                    );
                    return;
                }
                // See `handle_set_tag`: the reducer owns the immediate state
                // projection, while the next sync snapshot becomes canonical.
                self.emit(CoreEvent::Room(RoomEvent::RoomTagRemoved {
                    request_id,
                    room_id,
                    tag,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_pin_event(&self, request_id: RequestId, room_id: String, event_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        self.reduce_reliable(vec![AppAction::PinEventRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::pin_event(session, &room_id, &event_id).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::PinEventCompleted {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::PinEventCompleted {
                    request_id,
                    room_id: room_id.clone(),
                }));
                self.project_pinned_events_after_success(request_id, room_id)
                    .await;
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::PinEventFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_unpin_event(&self, request_id: RequestId, room_id: String, event_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        self.reduce_reliable(vec![AppAction::UnpinEventRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::unpin_event(session, &room_id, &event_id).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::UnpinEventCompleted {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::UnpinEventCompleted {
                    request_id,
                    room_id: room_id.clone(),
                }));
                self.project_pinned_events_after_success(request_id, room_id)
                    .await;
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::UnpinEventFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_refresh_pinned_events(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match load_pinned_events_for_room(session, &room_id).await {
            Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_pinned_events_changed(&self, room_ids: BTreeSet<String>) {
        let Some(session) = &self.session else {
            return;
        };
        for room_id in room_ids {
            match load_pinned_events_for_room(session, &room_id).await {
                Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
                Err(_kind) => {
                    // A background sync refresh has no request to fail. Keep
                    // the previous projection and wait for the next state
                    // update; only classified failure state may cross the
                    // Core boundary.
                }
            }
        }
    }

    async fn project_pinned_events_after_success(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            return;
        };
        match load_pinned_events_for_room(session, &room_id).await {
            Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn project_pinned_events(&self, room_id: String, pinned: Vec<PinnedEvent>) {
        self.reduce_reliable(vec![AppAction::RoomPinnedEventsUpdated {
            room_id: room_id.clone(),
            pinned: pinned.clone(),
        }])
        .await;
        self.emit(CoreEvent::Room(RoomEvent::PinnedEventsUpdated {
            room_id,
            pinned,
        }));
    }

    /// Request a room-list refresh and projection into AppState via the action
    /// channel. Also emits `RoomEvent::RoomListUpdated` as a discrete event.
    ///
    /// On the SyncService path this requests a re-normalization from the live
    /// service's current entries (inside the observation loop) — NEVER a new
    /// `RoomListService`. On the LegacySync path, the same request is handled
    /// by the legacy observation loop and coalesced there. Before sync starts,
    /// a detached one-shot refresh is spawned so room commands never await
    /// room-list normalization on the actor command loop.
    fn refresh_room_list(&self) {
        if let Some(observation) = &self.observation {
            let _ = observation.refresh_tx.try_send(());
            return;
        }
        if let Some(session) = self.session.clone() {
            let known_room_ids = self.known_room_ids.clone();
            let room_tx = self.self_tx.clone();
            let action_tx = self.action_tx.clone();
            let event_tx = self.event_tx.clone();
            let generation = self.room_list_generation;
            let source = self.room_list_source.unwrap_or(RoomListSource::Cache);
            let _ = executor::spawn(async move {
                refresh_room_list_from_joined_rooms(
                    &session,
                    &known_room_ids,
                    &room_tx,
                    &action_tx,
                    &event_tx,
                    generation,
                    source,
                    false,
                )
                .await;
            });
        }
    }

    async fn handle_mark_room_as_read(
        &self,
        request_id: RequestId,
        room_id: String,
        event_id: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        unread_trace::trace_mark_read(
            "mark_read_requested",
            request_id.sequence,
            &room_id,
            Some(event_id.as_str()),
        );
        self.reduce_reliable(vec![AppAction::RoomMarkedAsReadRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::mark_room_as_read(session, &room_id, &event_id).await {
            Ok(()) => {
                unread_trace::trace_mark_read(
                    "mark_read_success",
                    request_id.sequence,
                    &room_id,
                    Some(event_id.as_str()),
                );
                self.reduce_reliable(vec![
                    AppAction::FullyReadMarkerUpdated {
                        room_id: room_id.clone(),
                        event_id: Some(event_id.clone()),
                    },
                    AppAction::RoomMarkedAsReadSucceeded {
                        request_id: request_id.sequence,
                        room_id: room_id.clone(),
                    },
                ])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::MarkedAsRead {
                    request_id,
                    room_id: room_id.clone(),
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                unread_trace::trace_mark_read(
                    "mark_read_failed",
                    request_id.sequence,
                    &room_id,
                    Some(event_id.as_str()),
                );
                self.reduce_reliable(vec![AppAction::RoomMarkedAsReadFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_mark_room_as_unread(
        &self,
        request_id: RequestId,
        room_id: String,
        unread: bool,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        self.reduce_reliable(vec![AppAction::RoomMarkedAsUnreadRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            unread,
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::mark_room_as_unread(session, &room_id, unread).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::RoomMarkedAsUnreadSucceeded {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                    unread,
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::MarkedAsUnread {
                    request_id,
                    room_id: room_id.clone(),
                    unread,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomMarkedAsUnreadFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_set_room_notification_mode(
        &self,
        request_id: RequestId,
        room_id: String,
        mode: RoomNotificationMode,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }

        self.reduce_reliable(vec![AppAction::RoomNotificationModeSet {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            mode,
        }])
        .await;
        match koushi_sdk::set_room_notification_mode(session, &room_id, mode).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::RoomNotificationModeCompleted {
                    request_id: request_id.sequence,
                    room_id,
                }])
                .await;
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::RoomNotificationModeFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_report_content(
        &self,
        request_id: RequestId,
        room_id: String,
        event_id: String,
        reason: Option<String>,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::report_content(session, &room_id, &event_id, reason).await {
            Ok(()) => {
                self.emit(CoreEvent::Room(RoomEvent::ReportCompleted {
                    request_id,
                    kind: ReportKind::Event,
                }));
            }
            Err(error) => {
                self.emit_failure(
                    request_id,
                    CoreFailure::ReportOperationFailed {
                        kind: classify_report_error(&error),
                    },
                );
            }
        }
    }

    async fn handle_report_room(&self, request_id: RequestId, room_id: String, reason: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::report_room(session, &room_id, reason).await {
            Ok(()) => {
                self.emit(CoreEvent::Room(RoomEvent::ReportCompleted {
                    request_id,
                    kind: ReportKind::Room,
                }));
            }
            Err(error) => {
                self.emit_failure(
                    request_id,
                    CoreFailure::ReportOperationFailed {
                        kind: classify_report_error(&error),
                    },
                );
            }
        }
    }

    fn clear_known_rooms(&self) {
        if let Ok(mut known_room_ids) = self.known_room_ids.write() {
            known_room_ids.clear();
        }
    }

    fn clear_space_child_repair_attempts(&self) {
        if let Ok(mut attempts) = self.attempted_space_child_repairs.write() {
            attempts.clear();
        }
    }

    fn mark_space_child_link_attempted(&self, space_id: &str, child_room_id: &str) {
        if let Ok(mut attempts) = self.attempted_space_child_repairs.write() {
            attempts.insert((space_id.to_owned(), child_room_id.to_owned()));
        }
    }

    fn ensure_known_room_for_message_interaction(
        &self,
        request_id: RequestId,
        room_id: &str,
    ) -> bool {
        let known = self
            .known_room_ids
            .read()
            .map(|known_room_ids| known_room_ids.contains(room_id))
            .unwrap_or(false);
        if !known {
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::NotFound,
                },
            );
        }
        known
    }

    async fn handle_query_mention_candidates(
        &mut self,
        request_id: RequestId,
        account_key: crate::AccountKey,
        room_id: String,
        surface: MentionSurface,
        query: String,
    ) {
        let Some(session) = self.session.clone() else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        if account_key.0 != session.info.user_id {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        }

        let key = (room_id.clone(), surface);
        let generation = self
            .mention_demands
            .get(&key)
            .map_or(1, |demand| demand.generation.wrapping_add(1).max(1));
        self.mention_demands.insert(
            key,
            MentionDemand {
                request_id,
                generation,
                query: query.clone(),
            },
        );
        self.reduce_reliable(vec![AppAction::MentionCandidatesDemanded {
            request_id: request_id.sequence,
            generation,
            room_id: room_id.clone(),
            surface,
            query: query.clone(),
        }])
        .await;
        record_mention_candidate_event(
            "requested",
            surface,
            MentionCandidatesCompleteness::Loading,
            0,
            "accepted",
        );

        match session.joined_member_snapshot_no_sync(&room_id).await {
            Ok(snapshot) => {
                self.mention_member_snapshots
                    .insert(room_id.clone(), snapshot.clone());
                self.publish_mention_projection(&room_id, surface, &snapshot)
                    .await;
                if !snapshot.complete {
                    self.start_mention_member_refresh(session, room_id);
                }
            }
            Err(error) => {
                self.publish_mention_failure(&room_id, surface, mention_failure_kind(&error))
                    .await;
            }
        }
    }

    async fn handle_mention_members_refreshed(
        &mut self,
        room_id: String,
        session_generation: u64,
        refresh_generation: u64,
        result: Result<MatrixJoinedMemberSnapshot, MatrixRoomOperationError>,
    ) {
        if session_generation != self.mention_session_generation
            || self.mention_refresh_generations.get(&room_id) != Some(&refresh_generation)
            || self.session.is_none()
        {
            record_mention_candidate_event(
                "member_refresh_settled",
                MentionSurface::Main,
                MentionCandidatesCompleteness::Failed,
                0,
                "stale",
            );
            return;
        }
        self.mention_refresh_generations.remove(&room_id);
        let demanded_surfaces = self
            .mention_demands
            .keys()
            .filter_map(|(demanded_room_id, surface)| {
                (demanded_room_id == &room_id).then_some(*surface)
            })
            .collect::<Vec<_>>();
        match result {
            Ok(snapshot) => {
                self.mention_member_snapshots
                    .insert(room_id.clone(), snapshot.clone());
                for surface in demanded_surfaces {
                    self.publish_mention_projection(&room_id, surface, &snapshot)
                        .await;
                }
            }
            Err(error) => {
                let kind = mention_failure_kind(&error);
                for surface in demanded_surfaces {
                    self.publish_mention_failure(&room_id, surface, kind).await;
                }
            }
        }
    }

    fn start_mention_member_refresh(&mut self, session: Arc<MatrixClientSession>, room_id: String) {
        if self.mention_refresh_generations.contains_key(&room_id) {
            return;
        }
        self.mention_refresh_sequence = self.mention_refresh_sequence.wrapping_add(1).max(1);
        let refresh_generation = self.mention_refresh_sequence;
        self.mention_refresh_generations
            .insert(room_id.clone(), refresh_generation);
        let session_generation = self.mention_session_generation;
        let self_tx = self.self_tx.clone();
        record_mention_candidate_event(
            "member_refresh_started",
            MentionSurface::Main,
            MentionCandidatesCompleteness::Loading,
            0,
            "started",
        );
        executor::spawn(async move {
            let result = session.refresh_joined_member_snapshot(&room_id).await;
            let _ = self_tx
                .send(RoomMessage::MentionMembersRefreshed {
                    room_id,
                    session_generation,
                    refresh_generation,
                    result,
                })
                .await;
        });
    }

    async fn handle_mention_membership_changed(&mut self, room_ids: Option<BTreeSet<String>>) {
        self.handle_space_membership_changed(room_ids.as_ref())
            .await;

        let demanded_rooms = self
            .mention_demands
            .keys()
            .filter_map(|(room_id, _)| {
                room_ids
                    .as_ref()
                    .is_none_or(|updated| updated.contains(room_id))
                    .then_some(room_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let Some(session) = self.session.clone() else {
            return;
        };
        for room_id in demanded_rooms {
            self.mention_member_snapshots.remove(&room_id);
            // An update supersedes an in-flight refresh for this room. Its
            // completion is fenced by the replacement refresh generation.
            self.mention_refresh_generations.remove(&room_id);
            match session.joined_member_snapshot_no_sync(&room_id).await {
                Ok(snapshot) => {
                    self.mention_member_snapshots
                        .insert(room_id.clone(), snapshot.clone());
                    let surfaces = self
                        .mention_demands
                        .keys()
                        .filter_map(|(demanded_room_id, surface)| {
                            (demanded_room_id == &room_id).then_some(*surface)
                        })
                        .collect::<Vec<_>>();
                    for surface in surfaces {
                        self.publish_mention_projection(&room_id, surface, &snapshot)
                            .await;
                    }
                    if !snapshot.complete {
                        self.start_mention_member_refresh(session.clone(), room_id);
                    }
                }
                Err(error) => {
                    let kind = mention_failure_kind(&error);
                    let surfaces = self
                        .mention_demands
                        .keys()
                        .filter_map(|(demanded_room_id, surface)| {
                            (demanded_room_id == &room_id).then_some(*surface)
                        })
                        .collect::<Vec<_>>();
                    for surface in surfaces {
                        self.publish_mention_failure(&room_id, surface, kind).await;
                    }
                }
            }
        }
    }

    async fn handle_mention_local_aliases_updated(&mut self, aliases: BTreeMap<String, String>) {
        self.mention_local_aliases = aliases;
        let demanded_targets = self.mention_demands.keys().cloned().collect::<Vec<_>>();
        for (room_id, surface) in demanded_targets {
            if let Some(snapshot) = self.mention_member_snapshots.get(&room_id).cloned() {
                self.publish_mention_projection(&room_id, surface, &snapshot)
                    .await;
            }
        }
    }

    async fn publish_mention_projection(
        &self,
        room_id: &str,
        surface: MentionSurface,
        snapshot: &MatrixJoinedMemberSnapshot,
    ) {
        let Some(demand) = self
            .mention_demands
            .get(&(room_id.to_owned(), surface))
            .cloned()
        else {
            return;
        };
        let permission = match snapshot.room_mention_allowed {
            Some(true) => RoomMentionPermission::Allowed,
            Some(false) => RoomMentionPermission::Denied,
            None => RoomMentionPermission::Unknown,
        };
        let projection = project_candidates(
            &demand.query,
            snapshot
                .members
                .iter()
                .map(|member| MentionMemberInput {
                    user_id: member.user_id.clone(),
                    room_display_name: member.display_name.clone(),
                    profile_display_name: None,
                    local_alias: self.mention_local_aliases.get(&member.user_id).cloned(),
                    avatar_mxc_uri: member.avatar_url.clone(),
                })
                .collect(),
            permission,
        );
        let room_mention_allowed = if projection.room_mention_included {
            RoomMentionPermission::Allowed
        } else if permission == RoomMentionPermission::Unknown {
            RoomMentionPermission::Unknown
        } else {
            RoomMentionPermission::Denied
        };
        let candidate_count = projection.candidates.len();
        self.reduce_reliable(vec![AppAction::MentionCandidatesProjected {
            request_id: demand.request_id.sequence,
            generation: demand.generation,
            room_id: room_id.to_owned(),
            surface,
            query: demand.query,
            completeness: if snapshot.complete {
                MentionCandidatesCompleteness::Complete
            } else {
                MentionCandidatesCompleteness::Partial
            },
            candidates: projection.candidates,
            room_mention_allowed,
        }])
        .await;
        record_mention_candidate_event(
            "projected",
            surface,
            if snapshot.complete {
                MentionCandidatesCompleteness::Complete
            } else {
                MentionCandidatesCompleteness::Partial
            },
            candidate_count,
            "success",
        );
    }

    async fn publish_mention_failure(
        &self,
        room_id: &str,
        surface: MentionSurface,
        kind: MentionCandidatesFailureKind,
    ) {
        let Some(demand) = self
            .mention_demands
            .get(&(room_id.to_owned(), surface))
            .cloned()
        else {
            return;
        };
        self.reduce_reliable(vec![AppAction::MentionCandidatesFailed {
            request_id: demand.request_id.sequence,
            generation: demand.generation,
            room_id: room_id.to_owned(),
            surface,
            query: demand.query,
            kind,
        }])
        .await;
        record_mention_candidate_event(
            "projected",
            surface,
            MentionCandidatesCompleteness::Failed,
            0,
            match kind {
                MentionCandidatesFailureKind::Network => "network",
                MentionCandidatesFailureKind::Forbidden => "forbidden",
                MentionCandidatesFailureKind::Sdk => "sdk",
            },
        );
    }

    fn clear_mention_candidates(&mut self) {
        self.mention_demands.clear();
        self.mention_member_snapshots.clear();
        self.mention_refresh_generations.clear();
        self.mention_local_aliases.clear();
        self.mention_refresh_sequence = 0;
        self.mention_session_generation = self.mention_session_generation.wrapping_add(1);
    }

    fn reset_space_member_session(&mut self) {
        self.space_member_session_generation =
            self.space_member_session_generation.wrapping_add(1).max(1);
        self.clear_space_member_demand();
    }

    fn clear_space_member_demand(&mut self) {
        self.space_member_demand = None;
        self.space_member_demand_generation =
            self.space_member_demand_generation.wrapping_add(1).max(1);
        self.space_member_refresh_in_flight = None;
        self.space_member_refresh_pending = false;
        self.space_member_refresh_sequence = 0;
    }

    fn install_space_member_demand(
        &mut self,
        space_id: &str,
        generation: u64,
        child_room_ids: &[String],
    ) {
        self.space_member_demand_generation =
            self.space_member_demand_generation.wrapping_add(1).max(1);
        let child_room_ids = child_room_ids.iter().cloned().collect::<BTreeSet<_>>();
        let child_room_count = child_room_ids.len();
        self.space_member_demand = Some(SpaceMemberDemand {
            space_id: space_id.to_owned(),
            generation,
            child_room_ids,
            demand_generation: self.space_member_demand_generation,
        });
        record_space_member_demand_event("installed", generation, child_room_count);
    }

    fn emit(&self, event: CoreEvent) {
        let _ = self.event_tx.send(event);
    }

    fn emit_failure(&self, request_id: RequestId, failure: CoreFailure) {
        self.emit(CoreEvent::OperationFailed {
            request_id,
            failure,
        });
    }

    /// Reliable projection for one-shot, non-re-projected actions (navigation,
    /// command results) that MUST NOT be dropped under large-account sync load.
    /// Backpressures instead of dropping; the AppActor drains the action inbox
    /// continuously, so this does not deadlock.
    async fn reduce_reliable(&self, actions: Vec<AppAction>) {
        let _ = self.action_tx.send(actions).await;
    }
}

// ---------------------------------------------------------------------------
// Room list refresh + observation loop
// ---------------------------------------------------------------------------

/// Maximum number of room-list entries requested from the live service's
/// dynamic entries adapter (mirrors the auth snapshot limit).
const ROOM_LIST_ENTRIES_LIMIT: usize = 4096;

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum LiveObserverTestEvent {
    RlsProjected {
        wake_count: u64,
        entries_len: usize,
    },
    BaseBatch {
        wake_count: u64,
        update_count: u64,
        lagged: bool,
        projection_required: bool,
    },
    BaseProjected {
        wake_count: u64,
        rls_wake_count: u64,
        entries_len: usize,
        action_delivered: bool,
    },
    BaseClosed,
}

#[cfg(test)]
fn emit_live_observer_test_event(
    tx: &Option<mpsc::UnboundedSender<LiveObserverTestEvent>>,
    event: LiveObserverTestEvent,
) {
    if let Some(tx) = tx {
        let _ = tx.send(event);
    }
}

/// Normalize a snapshot and project it as a generation-fenced room-list action +
/// `RoomEvent::RoomListUpdated`.
async fn project_room_list_snapshot(
    snapshot: &koushi_sdk::MatrixRoomListSnapshot,
    known_room_ids: &Arc<RwLock<BTreeSet<String>>>,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    event_tx: &broadcast::Sender<CoreEvent>,
    generation: u64,
    source: RoomListSource,
    authoritative: bool,
) -> bool {
    let spaces = normalize_spaces(snapshot);
    let rooms = normalize_rooms(snapshot);
    let invites = normalize_invites(snapshot);
    let user_profiles = normalize_user_profiles(snapshot);
    unread_trace::trace_room_list_snapshot(&rooms);
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "core.room", "room_list_projection")
            .field(DiagnosticField::token(
                "source",
                room_list_source_label(source),
            ))
            .field(DiagnosticField::count("generation", generation))
            .field(DiagnosticField::boolean("authoritative", authoritative))
            .field(DiagnosticField::count("rooms_count", rooms.len() as u64))
            .field(DiagnosticField::count("spaces_count", spaces.len() as u64))
            .field(DiagnosticField::count("invites_count", invites.len() as u64)),
    );
    let projected_rooms = rooms.clone();
    let snapshot_action = if authoritative {
        AppAction::RoomListSnapshotAuthoritative {
            generation,
            source,
            spaces,
            rooms,
            invites,
        }
    } else {
        AppAction::RoomListSnapshotProvisional {
            generation,
            source,
            spaces,
            rooms,
            invites,
        }
    };
    let delivered = action_tx
        .send(vec![
            snapshot_action,
            AppAction::UserProfilesUpdated {
                profiles: user_profiles,
            },
        ])
        .await
        .is_ok();
    let has_payload = !projected_rooms.is_empty()
        || !snapshot.spaces.is_empty()
        || !snapshot.invites.is_empty();
    if delivered {
        if authoritative || has_payload {
            replace_known_room_ids(known_room_ids, &projected_rooms);
            let _ = event_tx.send(CoreEvent::Room(RoomEvent::RoomListUpdated));
        }
    }
    delivered
}

fn room_list_source_label(source: RoomListSource) -> &'static str {
    match source {
        RoomListSource::Cache => "cache",
        RoomListSource::SyncService => "sync_service",
        RoomListSource::Legacy => "legacy",
    }
}

/// LegacySync-path refresh: normalize from `client.joined_rooms()` and
/// project. Never constructs a `RoomListService` (canon prohibition).
async fn refresh_room_list_from_joined_rooms(
    session: &MatrixClientSession,
    known_room_ids: &Arc<RwLock<BTreeSet<String>>>,
    room_tx: &mpsc::Sender<RoomMessage>,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    event_tx: &broadcast::Sender<CoreEvent>,
    generation: u64,
    source: RoomListSource,
    authoritative: bool,
) {
    let snapshot = koushi_sdk::room_list_snapshot_from_sdk_rooms_with_invites(
        session,
        session.client().joined_rooms(),
    )
    .await;
    relay_missing_space_child_links(&snapshot, room_tx).await;
    project_room_list_snapshot(
        &snapshot,
        known_room_ids,
        action_tx,
        event_tx,
        generation,
        source,
        authoritative,
    )
    .await;
}

/// SyncService-path observation loop (Async rule 1: relay the SDK's
/// observable streams). Subscribes to the live `RoomListService`'s
/// `all_rooms()` entries stream (`entries_with_dynamic_adapters` with the
/// non-left filter — the same shape the live service drives with its
/// `required_state`, including `m.room.create` for space classification) and
/// KEEPS CONSUMING it: the current entry vector is maintained by applying
/// each `VectorDiff` batch, and every visible joined/invited batch triggers a
/// re-normalization. The base client's committed room-update broadcast is a
/// second wake source for invite membership changes that do not alter the
/// bounded entries head; it never owns or drives another network sync.
/// The first batch (a Reset with the current entries) doubles as the initial
/// snapshot. A refresh request (operation-triggered) re-normalizes from the
/// current entries without touching the service. Exits on the oneshot stop
/// signal or when the stream ends.
async fn run_live_room_list_observation(
    session: Arc<MatrixClientSession>,
    service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
    known_room_ids: Arc<RwLock<BTreeSet<String>>>,
    room_tx: mpsc::Sender<RoomMessage>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    refresh_rx: mpsc::Receiver<()>,
    stop_rx: oneshot::Receiver<()>,
    generation: u64,
    source: RoomListSource,
    authoritative: Arc<AtomicBool>,
) {
    let room_updates_rx = session.client().subscribe_to_all_room_updates();
    #[cfg(test)]
    run_live_room_list_observation_with_sources(
        session,
        service,
        known_room_ids,
        room_tx,
        action_tx,
        event_tx,
        refresh_rx,
        stop_rx,
        generation,
        source,
        authoritative,
        ROOM_LIST_ENTRIES_LIMIT,
        room_updates_rx,
        None,
    )
    .await;
    #[cfg(not(test))]
    run_live_room_list_observation_with_sources(
        session,
        service,
        known_room_ids,
        room_tx,
        action_tx,
        event_tx,
        refresh_rx,
        stop_rx,
        generation,
        source,
        authoritative,
        ROOM_LIST_ENTRIES_LIMIT,
        room_updates_rx,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn run_live_room_list_observation_with_sources(
    session: Arc<MatrixClientSession>,
    service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
    known_room_ids: Arc<RwLock<BTreeSet<String>>>,
    room_tx: mpsc::Sender<RoomMessage>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    mut refresh_rx: mpsc::Receiver<()>,
    mut stop_rx: oneshot::Receiver<()>,
    generation: u64,
    source: RoomListSource,
    authoritative: Arc<AtomicBool>,
    entries_limit: usize,
    mut room_updates_rx: broadcast::Receiver<matrix_sdk_base::sync::RoomUpdates>,
    #[cfg(test)] test_event_tx: Option<mpsc::UnboundedSender<LiveObserverTestEvent>>,
) {
    use futures_util::StreamExt as _;

    let all_rooms = match service.all_rooms().await {
        Ok(all_rooms) => all_rooms,
        Err(_) => {
            record(
                DiagnosticEvent::new(DiagnosticLevel::Error, "core.room", "live_observer_exit")
                    .field(DiagnosticField::token("reason", "all_rooms_error")),
            );
            return;
        }
    };
    let (entries, entries_controller) = all_rooms.entries_with_dynamic_adapters(entries_limit);
    entries_controller.set_filter(Box::new(
        matrix_sdk_ui::room_list_service::filters::new_filter_non_left(),
    ));
    let mut entries = Box::pin(entries);
    let mut room_updates_closed = false;
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "core.room", "live_observer_started").field(
            DiagnosticField::count("entries_limit", entries_limit as u64),
        ),
    );

    // Current filtered entry vector, maintained by applying each diff batch.
    let mut current: eyeball_im::Vector<matrix_sdk_ui::room_list_service::RoomListItem> =
        eyeball_im::Vector::new();
    // `None` until the entries stream's initial Reset (or an explicit refresh)
    // has established the first projection. IDs remain private process state
    // and are never included in diagnostics.
    let mut projected_invite_ids: Option<BTreeSet<String>> = None;
    let mut rls_wake_count = 0_u64;
    let mut base_wake_count = 0_u64;

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                record_live_observer_exit(
                    DiagnosticLevel::Debug,
                    "stopped",
                    rls_wake_count,
                    base_wake_count,
                );
                break;
            },
            maybe_refresh = refresh_rx.recv() => {
                if maybe_refresh.is_none() {
                    record_live_observer_exit(
                        DiagnosticLevel::Error,
                        "refresh_channel_closed",
                        rls_wake_count,
                        base_wake_count,
                    );
                    break;
                }
                // Operation-triggered refresh: drain coalesced requests, then
                // re-normalize from the live service's CURRENT entries.
                while refresh_rx.try_recv().is_ok() {}
                projected_invite_ids = Some(normalize_and_project_entries(
                    &session,
                    &current,
                    &known_room_ids,
                    &room_tx,
                    &action_tx,
                    &event_tx,
                    generation,
                    source,
                    &authoritative,
                ).await.invite_ids);
            }
            maybe_diffs = entries.next() => match maybe_diffs {
                None => {
                    record_live_observer_exit(
                        DiagnosticLevel::Error,
                        "entries_stream_ended",
                        rls_wake_count,
                        base_wake_count,
                    );
                    break;
                },
                Some(diffs) => {
                    rls_wake_count = rls_wake_count.saturating_add(1);
                    for diff in diffs {
                        diff.apply(&mut current);
                    }
                    if rls_wake_count.is_power_of_two() {
                        record(
                            DiagnosticEvent::new(
                                DiagnosticLevel::Debug,
                                "core.room",
                                "live_observer_wake_milestone",
                            )
                            .field(DiagnosticField::token("source", "rls_diff"))
                            .field(DiagnosticField::count("wake_count", rls_wake_count))
                            .field(DiagnosticField::count("entries_count", current.len() as u64)),
                        );
                    }
                    projected_invite_ids = Some(normalize_and_project_entries(
                        &session,
                        &current,
                        &known_room_ids,
                        &room_tx,
                        &action_tx,
                        &event_tx,
                        generation,
                        source,
                        &authoritative,
                    ).await.invite_ids);
                    #[cfg(test)]
                    emit_live_observer_test_event(
                        &test_event_tx,
                        LiveObserverTestEvent::RlsProjected {
                            wake_count: rls_wake_count,
                            entries_len: current.len(),
                        },
                    );
                }
            },
            room_update = room_updates_rx.recv(), if !room_updates_closed => {
                let mut update_count = 0_u64;
                let mut lagged = false;
                let mut invite_update_observed = false;
                let mut updated_joined_room_ids = BTreeSet::new();
                let mut pinned_event_room_ids = BTreeSet::new();
                match room_update {
                    Ok(updates) => {
                        update_count = 1;
                        invite_update_observed = !updates.invited.is_empty();
                        updated_joined_room_ids.extend(
                            updates.joined.keys().map(ToString::to_string),
                        );
                        pinned_event_room_ids.extend(crate::room::pinned_event_room_ids(&updates));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => lagged = true,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        room_updates_closed = true;
                    }
                }
                loop {
                    match room_updates_rx.try_recv() {
                        Ok(updates) => {
                            update_count = update_count.saturating_add(1);
                            invite_update_observed |= !updates.invited.is_empty();
                            updated_joined_room_ids.extend(
                                updates.joined.keys().map(ToString::to_string),
                            );
                            pinned_event_room_ids.extend(crate::room::pinned_event_room_ids(&updates));
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                            lagged = true;
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                            room_updates_closed = true;
                            break;
                        }
                    }
                }

                if room_updates_closed {
                    record(
                        DiagnosticEvent::new(
                            DiagnosticLevel::Warn,
                            "core.room",
                            "live_observer_auxiliary_closed",
                        )
                        .field(DiagnosticField::token("source", "base_room_updates"))
                        .field(DiagnosticField::count("rls_wake_count", rls_wake_count))
                        .field(DiagnosticField::count("base_wake_count", base_wake_count)),
                    );
                    #[cfg(test)]
                    emit_live_observer_test_event(
                        &test_event_tx,
                        LiveObserverTestEvent::BaseClosed,
                    );
                }

                base_wake_count = base_wake_count.saturating_add(1);
                let current_invite_ids = current_invite_membership(&session);
                let invite_membership_changed = projected_invite_ids
                    .as_ref()
                    .is_some_and(|projected| projected != &current_invite_ids);
                let projection_required = invite_projection_required(
                    projected_invite_ids.as_ref(),
                    &current_invite_ids,
                    invite_update_observed,
                    lagged,
                );
                if base_wake_count.is_power_of_two() {
                    record(
                        DiagnosticEvent::new(
                            DiagnosticLevel::Debug,
                            "core.room",
                            "live_observer_wake_milestone",
                        )
                        .field(DiagnosticField::token("source", "base_room_updates"))
                        .field(DiagnosticField::count("wake_count", base_wake_count))
                        .field(DiagnosticField::count("drained_update_count", update_count))
                        .field(DiagnosticField::boolean("lagged", lagged))
                        .field(DiagnosticField::boolean(
                            "invite_update_observed",
                            invite_update_observed,
                        ))
                        .field(DiagnosticField::boolean(
                            "initial_projection_complete",
                            projected_invite_ids.is_some(),
                        ))
                        .field(DiagnosticField::boolean(
                            "invite_membership_changed",
                            invite_membership_changed,
                        ))
                        .field(DiagnosticField::boolean(
                            "projection_required",
                            projection_required,
                        )),
                    );
                }
                #[cfg(test)]
                emit_live_observer_test_event(
                    &test_event_tx,
                    LiveObserverTestEvent::BaseBatch {
                        wake_count: base_wake_count,
                        update_count,
                        lagged,
                        projection_required,
                    },
                );
                if lagged {
                    record(
                        DiagnosticEvent::new(
                            DiagnosticLevel::Warn,
                            "core.room",
                            "live_observer_base_lagged",
                        )
                        .field(DiagnosticField::count("rls_wake_count", rls_wake_count))
                        .field(DiagnosticField::count("base_wake_count", base_wake_count))
                        .field(DiagnosticField::count("drained_update_count", update_count)),
                    );
                }
                if lagged || !updated_joined_room_ids.is_empty() {
                    let _ = room_tx
                        .send(RoomMessage::MentionMembershipChanged {
                            room_ids: (!lagged).then_some(updated_joined_room_ids),
                        })
                        .await;
                }
                if !pinned_event_room_ids.is_empty() {
                    let _ = room_tx
                        .send(RoomMessage::PinnedEventsChanged {
                            room_ids: pinned_event_room_ids,
                        })
                        .await;
                }

                if projection_required {
                    record(
                        DiagnosticEvent::new(
                            DiagnosticLevel::Debug,
                            "core.room",
                            "live_observer_invite_projection",
                        )
                        .field(DiagnosticField::count("rls_wake_count", rls_wake_count))
                        .field(DiagnosticField::count("base_wake_count", base_wake_count))
                        .field(DiagnosticField::count("drained_update_count", update_count))
                        .field(DiagnosticField::boolean("lagged", lagged))
                        .field(DiagnosticField::boolean(
                            "invite_update_observed",
                            invite_update_observed,
                        ))
                        .field(DiagnosticField::boolean(
                            "invite_membership_changed",
                            invite_membership_changed,
                        )),
                    );
                    let projection = normalize_and_project_entries(
                        &session,
                        &current,
                        &known_room_ids,
                        &room_tx,
                        &action_tx,
                        &event_tx,
                        generation,
                        source,
                        &authoritative,
                    ).await;
                    let action_delivered = projection.action_delivered;
                    projected_invite_ids = Some(projection.invite_ids);
                    record(
                        DiagnosticEvent::new(
                            DiagnosticLevel::Debug,
                            "core.room",
                            "live_observer_invite_projection_completed",
                        )
                        .field(DiagnosticField::count("rls_wake_count", rls_wake_count))
                        .field(DiagnosticField::count("base_wake_count", base_wake_count))
                        .field(DiagnosticField::boolean(
                            "action_delivered",
                            action_delivered,
                        )),
                    );
                    #[cfg(test)]
                    emit_live_observer_test_event(
                        &test_event_tx,
                        LiveObserverTestEvent::BaseProjected {
                            wake_count: base_wake_count,
                            rls_wake_count,
                            entries_len: current.len(),
                            action_delivered,
                        },
                    );
                }
            }
        }
    }
}

fn record_live_observer_exit(
    level: DiagnosticLevel,
    reason: &'static str,
    rls_wake_count: u64,
    base_wake_count: u64,
) {
    record(
        DiagnosticEvent::new(level, "core.room", "live_observer_exit")
            .field(DiagnosticField::token("reason", reason))
            .field(DiagnosticField::count("rls_wake_count", rls_wake_count))
            .field(DiagnosticField::count("base_wake_count", base_wake_count)),
    );
}

fn current_invite_membership(session: &MatrixClientSession) -> BTreeSet<String> {
    session
        .client()
        .invited_rooms()
        .into_iter()
        .map(|room| room.room_id().to_string())
        .collect()
}

fn invite_projection_required(
    projected_invite_ids: Option<&BTreeSet<String>>,
    current_invite_ids: &BTreeSet<String>,
    invite_update_observed: bool,
    lagged: bool,
) -> bool {
    projected_invite_ids.is_some_and(|projected| {
        projected != current_invite_ids || invite_update_observed || lagged
    })
}

struct RoomListProjectionResult {
    invite_ids: BTreeSet<String>,
    action_delivered: bool,
}

/// Normalize the live service's current entries and project the result.
async fn normalize_and_project_entries(
    session: &MatrixClientSession,
    current: &eyeball_im::Vector<matrix_sdk_ui::room_list_service::RoomListItem>,
    known_room_ids: &Arc<RwLock<BTreeSet<String>>>,
    room_tx: &mpsc::Sender<RoomMessage>,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    event_tx: &broadcast::Sender<CoreEvent>,
    generation: u64,
    source: RoomListSource,
    authoritative: &Arc<AtomicBool>,
) -> RoomListProjectionResult {
    // Collect before the await: mapping lazily across the await trips a
    // higher-ranked lifetime check on the iterator closure.
    let mut rooms = Vec::with_capacity(current.len());
    for item in current.iter() {
        rooms.push(item.clone().into_inner());
    }
    let snapshot = koushi_sdk::room_list_snapshot_from_sdk_rooms_with_invites(session, rooms).await;
    let projected_invite_ids = snapshot
        .invites
        .iter()
        .map(|invite| invite.room_id.clone())
        .collect();
    relay_missing_space_child_links(&snapshot, room_tx).await;
    let action_delivered = project_room_list_snapshot(
        &snapshot,
        known_room_ids,
        action_tx,
        event_tx,
        generation,
        source,
        authoritative.load(Ordering::Acquire),
    )
    .await;
    RoomListProjectionResult {
        invite_ids: projected_invite_ids,
        action_delivered,
    }
}

async fn relay_missing_space_child_links(
    snapshot: &MatrixRoomListSnapshot,
    room_tx: &mpsc::Sender<RoomMessage>,
) {
    let links = missing_space_child_links(snapshot);
    if !links.is_empty() {
        let _ = room_tx
            .send(RoomMessage::MissingSpaceChildLinks { links })
            .await;
    }
}

fn missing_space_child_links(snapshot: &MatrixRoomListSnapshot) -> Vec<MissingSpaceChildLink> {
    let mut links = Vec::new();
    for room in &snapshot.rooms {
        for space in &snapshot.spaces {
            if room_has_parent_without_space_child(room, space)
                && let Ok(via_server) = koushi_sdk::room_id_server_name(&room.room_id)
            {
                links.push(MissingSpaceChildLink {
                    space_id: space.space_id.clone(),
                    child_room_id: room.room_id.clone(),
                    via_server,
                });
            }
        }
    }
    links.sort_by(|left, right| {
        left.space_id
            .cmp(&right.space_id)
            .then_with(|| left.child_room_id.cmp(&right.child_room_id))
    });
    links.dedup_by(|left, right| {
        left.space_id == right.space_id && left.child_room_id == right.child_room_id
    });
    links
}

fn room_has_parent_without_space_child(
    room: &MatrixRoomListRoom,
    space: &MatrixRoomListSpace,
) -> bool {
    room.parent_space_ids
        .iter()
        .any(|space_id| space_id == &space.space_id)
        && !space
            .child_room_ids
            .iter()
            .any(|child_room_id| child_room_id == &room.room_id)
}

/// LegacySync-path observation loop (Async rule 1: relay the SDK's observable
/// streams). Subscribes to `client.subscribe_to_all_room_updates()`, which
/// fires on the legacy backend because it feeds the base client. Each
/// received batch coalesces any additionally pending batches into one
/// re-normalization; `Lagged` triggers a single refresh because the snapshot
/// is self-healing. Exits on the oneshot stop signal (same pattern as
/// `sync.rs` `legacy_stop_tx`) or when the SDK closes the broadcast.
async fn run_legacy_room_list_observation(
    session: Arc<MatrixClientSession>,
    known_room_ids: Arc<RwLock<BTreeSet<String>>>,
    room_tx: mpsc::Sender<RoomMessage>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    mut refresh_rx: mpsc::Receiver<()>,
    mut stop_rx: oneshot::Receiver<()>,
    generation: u64,
    source: RoomListSource,
    authoritative: Arc<AtomicBool>,
) {
    use tokio::sync::broadcast::error::RecvError;

    let mut updates_rx = session.client().subscribe_to_all_room_updates();
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            maybe_refresh = refresh_rx.recv() => {
                if maybe_refresh.is_none() {
                    break;
                }
                // Operation-triggered refresh: drain coalesced requests, then
                // normalize from the SDK's current joined-room snapshot.
                while refresh_rx.try_recv().is_ok() {}
                refresh_room_list_from_joined_rooms(
                    &session,
                    &known_room_ids,
                    &room_tx,
                    &action_tx,
                    &event_tx,
                    generation,
                    source,
                    authoritative.load(Ordering::Acquire),
                ).await;
            }
            result = updates_rx.recv() => match result {
                Ok(batch) => {
                    let mut updated_joined_room_ids = batch
                        .joined
                        .keys()
                        .map(ToString::to_string)
                        .collect::<BTreeSet<_>>();
                    let mut pinned_event_room_ids = crate::room::pinned_event_room_ids(&batch);
                    // Coalesce: drain any additionally pending update batches;
                    // one refresh covers them all.
                    while let Ok(batch) = updates_rx.try_recv() {
                        updated_joined_room_ids.extend(
                            batch.joined.keys().map(ToString::to_string),
                        );
                        pinned_event_room_ids.extend(crate::room::pinned_event_room_ids(&batch));
                    }
                    if !updated_joined_room_ids.is_empty() {
                        let _ = room_tx
                            .send(RoomMessage::MentionMembershipChanged {
                                room_ids: Some(updated_joined_room_ids),
                            })
                            .await;
                    }
                    if !pinned_event_room_ids.is_empty() {
                        let _ = room_tx
                            .send(RoomMessage::PinnedEventsChanged {
                                room_ids: pinned_event_room_ids,
                            })
                            .await;
                    }
                    refresh_room_list_from_joined_rooms(
                        &session,
                        &known_room_ids,
                        &room_tx,
                        &action_tx,
                        &event_tx,
                        generation,
                        source,
                        authoritative.load(Ordering::Acquire),
                    ).await;
                }
                Err(RecvError::Lagged(_)) => {
                    // The snapshot is self-healing: refresh once.
                    let _ = room_tx
                        .send(RoomMessage::MentionMembershipChanged { room_ids: None })
                        .await;
                    refresh_room_list_from_joined_rooms(
                        &session,
                        &known_room_ids,
                        &room_tx,
                        &action_tx,
                        &event_tx,
                        generation,
                        source,
                        authoritative.load(Ordering::Acquire),
                    ).await;
                }
                Err(RecvError::Closed) => break,
            },
        }
    }
}

fn state_contains_pinned_events(state: &matrix_sdk_base::sync::State) -> bool {
    let events = match state {
        matrix_sdk_base::sync::State::Before(events)
        | matrix_sdk_base::sync::State::After(events) => events,
    };
    events.iter().any(|event| {
        serde_json::from_str::<serde_json::Value>(event.json().get())
            .ok()
            .and_then(|json| {
                json.get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("m.room.pinned_events")
    })
}

fn pinned_event_room_ids(updates: &matrix_sdk_base::sync::RoomUpdates) -> BTreeSet<String> {
    updates
        .joined
        .iter()
        .filter(|(_, update)| state_contains_pinned_events(&update.state))
        .map(|(room_id, _)| room_id.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Normalization helpers: auth snapshot → state DTOs
// ---------------------------------------------------------------------------

/// Convert `MatrixRoomListSnapshot` spaces into `SpaceSummary` values with
/// child room id lists. Homeservers may sync one side of the Matrix space
/// relationship before the other, so the projection uses both the space's
/// `m.space.child` state and rooms' `m.space.parent` state.
fn normalize_spaces(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<SpaceSummary> {
    snapshot
        .spaces
        .iter()
        .map(|space| {
            let child_room_ids = normalize_space_child_room_ids(snapshot, space);
            SpaceSummary {
                space_id: space.space_id.clone(),
                display_name: space.display_name.clone(),
                avatar: avatar_from_mxc_uri(space.avatar_mxc_uri.as_deref()),
                child_room_ids,
            }
        })
        .collect()
}

fn normalize_space_child_room_ids(
    snapshot: &koushi_sdk::MatrixRoomListSnapshot,
    space: &koushi_sdk::MatrixRoomListSpace,
) -> Vec<String> {
    let mut child_room_ids = BTreeSet::new();
    child_room_ids.extend(space.child_room_ids.iter().cloned());
    child_room_ids.extend(
        snapshot
            .rooms
            .iter()
            .filter(|room| room.parent_space_ids.iter().any(|id| id == &space.space_id))
            .map(|room| room.room_id.clone()),
    );
    child_room_ids.into_iter().collect()
}

/// Convert `MatrixRoomListSnapshot` rooms into `RoomSummary` values.
fn normalize_rooms(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<RoomSummary> {
    let mut rooms: Vec<RoomSummary> = snapshot
        .rooms
        .iter()
        .map(|room| {
            let display_label = room
                .display_name
                .trim()
                .is_empty()
                .then(|| room.room_id.clone())
                .unwrap_or_else(|| room.display_name.trim().to_owned());
            RoomSummary {
                room_id: room.room_id.clone(),
                display_name: room.display_name.clone(),
                display_label: display_label.clone(),
                original_display_label: display_label,
                avatar: avatar_from_mxc_uri(room.avatar_mxc_uri.as_deref()),
                is_dm: room.is_dm,
                dm_user_ids: room.dm_user_ids.clone(),
                tags: normalize_room_tags(&room.tags),
                unread_count: room.unread_count,
                notification_count: room.notification_count,
                highlight_count: room.highlight_count,
                marked_unread: room.marked_unread,
                recency_stamp: room.recency_stamp,
                conversation_activity: room.conversation_activity.map(|activity| {
                    koushi_state::ConversationActivity {
                        timestamp_ms: activity.timestamp_ms,
                        source: match activity.source {
                            koushi_sdk::MatrixConversationActivitySource::Message => {
                                koushi_state::ConversationActivitySource::Message
                            }
                            koushi_sdk::MatrixConversationActivitySource::EncryptedMessage => {
                                koushi_state::ConversationActivitySource::EncryptedMessage
                            }
                            koushi_sdk::MatrixConversationActivitySource::ThreadReply => {
                                koushi_state::ConversationActivitySource::ThreadReply
                            }
                        },
                    }
                }),
                latest_event: room.latest_event.as_ref().map(|event| {
                    koushi_state::RoomLatestEventSummary {
                        event_id: event.event_id.clone(),
                        relation_type: event.relation_type.clone(),
                        relation_event_id: event.relation_event_id.clone(),
                        sender_id: event.sender_id.clone(),
                        sender_label: event.sender_label.clone(),
                        sender_avatar: avatar_from_mxc_uri(event.sender_avatar_mxc_uri.as_deref()),
                        preview: event.preview.clone(),
                        timestamp_ms: event.timestamp_ms,
                    }
                }),
                parent_space_ids: normalize_room_parent_space_ids(snapshot, room),
                dm_space_ids: Vec::new(),
                is_encrypted: room.is_encrypted,
                joined_members: room.joined_members,
            }
        })
        .collect();
    let space_members: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        snapshot
            .spaces
            .iter()
            .map(|s| {
                (
                    s.space_id.clone(),
                    s.member_user_ids.iter().cloned().collect(),
                )
            })
            .collect();
    assign_dm_space_ids(&mut rooms, &space_members);
    rooms
}

fn normalize_room_parent_space_ids(
    snapshot: &koushi_sdk::MatrixRoomListSnapshot,
    room: &koushi_sdk::MatrixRoomListRoom,
) -> Vec<String> {
    let mut parent_space_ids: BTreeSet<String> = room.parent_space_ids.iter().cloned().collect();
    parent_space_ids.extend(
        snapshot
            .spaces
            .iter()
            .filter(|space| space.child_room_ids.iter().any(|id| id == &room.room_id))
            .map(|space| space.space_id.clone()),
    );
    parent_space_ids.into_iter().collect()
}

/// Populate `dm_space_ids` on each `RoomSummary` in `rooms`.
///
/// For each DM room, `dm_space_ids` is set to the sorted list of space IDs
/// (keys of `space_members`) whose member set contains at least one of
/// `room.dm_user_ids`. Non-DM rooms always get an empty `dm_space_ids`.
///
/// The result is deterministically ordered because `space_members` is a
/// `BTreeMap` and iteration yields keys in ascending order.
pub fn assign_dm_space_ids(
    rooms: &mut [koushi_state::RoomSummary],
    space_members: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    for room in rooms.iter_mut() {
        if !room.is_dm {
            room.dm_space_ids = Vec::new();
            continue;
        }
        room.dm_space_ids = space_members
            .iter()
            .filter(|(_space_id, members)| room.dm_user_ids.iter().any(|uid| members.contains(uid)))
            .map(|(space_id, _)| space_id.clone())
            .collect();
    }
}

fn normalize_room_tags(tags: &MatrixRoomTags) -> RoomTags {
    RoomTags {
        favourite: tags.favourite.as_ref().map(|info| RoomTagInfo {
            order: info.order.clone(),
        }),
        low_priority: tags.low_priority.as_ref().map(|info| RoomTagInfo {
            order: info.order.clone(),
        }),
    }
}

fn normalize_user_profiles(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<UserProfile> {
    snapshot
        .user_profiles
        .iter()
        .map(|profile| {
            let display_label = profile
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|display_name| !display_name.is_empty())
                .unwrap_or(profile.user_id.as_str())
                .to_owned();
            UserProfile {
                user_id: profile.user_id.clone(),
                display_name: profile.display_name.clone(),
                display_label: display_label.clone(),
                original_display_label: display_label,
                mention_search_terms: user_profile_mention_search_terms(
                    &profile.user_id,
                    profile.display_name.as_deref(),
                ),
                avatar: avatar_from_mxc_uri(profile.avatar_mxc_uri.as_deref()),
            }
        })
        .collect()
}

fn state_space_members_projection(
    projection: MatrixSpaceMembersProjection,
    generation: u64,
) -> SpaceMembersProjection {
    SpaceMembersProjection {
        space_id: projection.space_id,
        generation,
        space_joined: projection
            .space_joined
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::SpaceJoined))
            .collect(),
        space_invited: projection
            .space_invited
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::SpaceInvited))
            .collect(),
        child_room_only: projection
            .child_room_only
            .into_iter()
            .map(|entry| state_space_member_entry(entry, SpaceMemberMembership::ChildRoomOnly))
            .collect(),
        child_room_count: projection.child_room_count,
        complete_child_room_count: projection.complete_child_room_count,
        incomplete_child_room_count: projection.incomplete_child_room_count,
    }
}

fn state_space_member_entry(
    entry: MatrixSpaceMemberEntry,
    membership: SpaceMemberMembership,
) -> SpaceMemberEntry {
    let display_name = entry
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let display_label = display_name
        .clone()
        .unwrap_or_else(|| "Unknown user".to_owned());
    SpaceMemberEntry {
        user_id: entry.user_id,
        display_name,
        display_label: display_label.clone(),
        original_display_label: display_label,
        avatar_url: entry.avatar_url,
        power_level: entry.power_level,
        role: match entry.role {
            koushi_sdk::MatrixRoomMemberRole::Creator => koushi_state::RoomMemberRole::Creator,
            koushi_sdk::MatrixRoomMemberRole::Administrator => {
                koushi_state::RoomMemberRole::Administrator
            }
            koushi_sdk::MatrixRoomMemberRole::Moderator => koushi_state::RoomMemberRole::Moderator,
            koushi_sdk::MatrixRoomMemberRole::User => koushi_state::RoomMemberRole::User,
        },
        membership,
        child_room_ids: entry.child_room_ids,
        invite_pending: false,
    }
}

/// Feed non-empty room observations into the account-scoped profile cache.
/// This is deliberately emitted alongside the Space projection, before the
/// projection action is reduced, so receipt/Seen payloads with no label can
/// resolve from `ProfileState.users` without requiring Space membership.
fn user_profiles_from_space_projection(
    projection: &MatrixSpaceMembersProjection,
) -> Vec<UserProfile> {
    let mut profiles = BTreeMap::<String, UserProfile>::new();
    for entry in projection
        .space_joined
        .iter()
        .chain(projection.space_invited.iter())
        .chain(projection.child_room_only.iter())
        .chain(projection.child_room_profiles.iter())
    {
        let has_display_name = entry
            .display_name
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if !has_display_name && entry.avatar_url.is_none() {
            continue;
        }
        let next = UserProfile {
            user_id: entry.user_id.clone(),
            display_name: entry
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            display_label: entry
                .display_name
                .clone()
                .unwrap_or_else(|| "Unknown user".to_owned()),
            original_display_label: entry
                .display_name
                .clone()
                .unwrap_or_else(|| "Unknown user".to_owned()),
            mention_search_terms: Vec::new(),
            avatar: avatar_from_mxc_uri(entry.avatar_url.as_deref()),
        };
        profiles
            .entry(entry.user_id.clone())
            .and_modify(|existing| {
                if existing.display_name.is_none() && next.display_name.is_some() {
                    existing.display_name = next.display_name.clone();
                    existing.display_label = next.display_label.clone();
                    existing.original_display_label = next.original_display_label.clone();
                }
                if existing.avatar.is_none() && next.avatar.is_some() {
                    existing.avatar = next.avatar.clone();
                }
            })
            .or_insert(next);
    }
    profiles.into_values().collect()
}

struct SpaceInviteReconciliation {
    projection: SpaceMembersProjection,
    profiles: Vec<UserProfile>,
    outcome: SpaceMemberInviteOutcome,
}

struct SpaceInviteProjectionReconciliation {
    projection: SpaceMembersProjection,
    profiles: Vec<UserProfile>,
}

async fn reconcile_space_invite_outcome(
    session: &MatrixClientSession,
    space_id: &str,
    user_id: &str,
    generation: u64,
    fallback: SpaceMemberInviteOutcome,
) -> Option<SpaceInviteReconciliation> {
    let Ok(raw_projection) = koushi_sdk::matrix_space_members_projection(session, space_id).await
    else {
        return None;
    };
    let outcome = if raw_projection
        .space_joined
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        SpaceMemberInviteOutcome::AlreadyJoined
    } else if raw_projection
        .space_invited
        .iter()
        .any(|entry| entry.user_id == user_id)
    {
        SpaceMemberInviteOutcome::AlreadyInvited
    } else {
        fallback
    };
    let profiles = user_profiles_from_space_projection(&raw_projection);
    let projection = state_space_members_projection(raw_projection, generation);
    Some(SpaceInviteReconciliation {
        projection,
        profiles,
        outcome,
    })
}

async fn reconcile_space_invite_cancellation(
    session: &MatrixClientSession,
    space_id: &str,
    generation: u64,
) -> Option<SpaceInviteProjectionReconciliation> {
    let Ok(raw_projection) = koushi_sdk::matrix_space_members_projection(session, space_id).await
    else {
        return None;
    };
    let profiles = user_profiles_from_space_projection(&raw_projection);
    let projection = state_space_members_projection(raw_projection, generation);
    Some(SpaceInviteProjectionReconciliation {
        projection,
        profiles,
    })
}

fn record_core_space_members_projection(
    trigger: &'static str,
    generation: u64,
    projection: &SpaceMembersProjection,
    outcome: &'static str,
) {
    record_core_space_members_projection_with_metrics(
        trigger, generation, projection, None, outcome,
    );
}

fn record_core_space_members_projection_with_raw(
    trigger: &'static str,
    generation: u64,
    raw_projection: &MatrixSpaceMembersProjection,
    projection: &SpaceMembersProjection,
    outcome: &'static str,
) {
    record_core_space_members_projection_with_metrics(
        trigger,
        generation,
        projection,
        Some(raw_projection),
        outcome,
    );
}

fn record_core_space_members_projection_with_metrics(
    trigger: &'static str,
    generation: u64,
    projection: &SpaceMembersProjection,
    raw_projection: Option<&MatrixSpaceMembersProjection>,
    outcome: &'static str,
) {
    let output_count = projection.space_joined.len()
        + projection.space_invited.len()
        + projection.child_room_only.len();
    let mut event = DiagnosticEvent::new(
        DiagnosticLevel::Debug,
        "core.space_members_projection",
        "projection",
    )
    .field(DiagnosticField::token("trigger", trigger))
    .field(DiagnosticField::count("generation", generation))
    .field(DiagnosticField::count(
        "space_joined_count",
        projection.space_joined.len() as u64,
    ))
    .field(DiagnosticField::count(
        "space_invited_count",
        projection.space_invited.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_only_count",
        projection.child_room_only.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_count",
        projection.child_room_count as u64,
    ))
    .field(DiagnosticField::count(
        "complete_child_room_count",
        projection.complete_child_room_count as u64,
    ))
    .field(DiagnosticField::count(
        "incomplete_child_room_count",
        projection.incomplete_child_room_count as u64,
    ))
    .field(DiagnosticField::count("output_count", output_count as u64))
    .field(DiagnosticField::count(
        "space_joined_output_count",
        projection.space_joined.len() as u64,
    ))
    .field(DiagnosticField::count(
        "space_invited_output_count",
        projection.space_invited.len() as u64,
    ))
    .field(DiagnosticField::count(
        "child_room_only_output_count",
        projection.child_room_only.len() as u64,
    ))
    .field(DiagnosticField::boolean(
        "incomplete",
        projection.incomplete_child_room_count > 0,
    ))
    .field(DiagnosticField::token("outcome", outcome));

    if let Some(raw_projection) = raw_projection {
        let input_count = raw_projection.space_joined_input_count
            + raw_projection.space_invited_input_count
            + raw_projection.child_join_input_count;
        event = event
            .field(DiagnosticField::count("input_count", input_count as u64))
            .field(DiagnosticField::count(
                "space_joined_input_count",
                raw_projection.space_joined_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "space_invited_input_count",
                raw_projection.space_invited_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "child_join_input_count",
                raw_projection.child_join_input_count as u64,
            ))
            .field(DiagnosticField::count(
                "deduplicated_count",
                raw_projection.duplicate_child_membership_count as u64,
            ))
            .field(DiagnosticField::count(
                "child_join_union_count",
                raw_projection.child_join_union_count as u64,
            ))
            .field(DiagnosticField::token("input_tracking_status", "tracked"));
    } else {
        event = event
            .field(DiagnosticField::token("input_count", "not_tracked"))
            .field(DiagnosticField::token("deduplicated_count", "not_tracked"))
            .field(DiagnosticField::token(
                "input_tracking_status",
                "not_tracked",
            ));
    }

    record(event.field(DiagnosticField::token("freshness_status", "not_tracked")));
}

fn space_members_update_affects_demand(
    space_id: &str,
    child_room_ids: &BTreeSet<String>,
    updated_room_ids: Option<&BTreeSet<String>>,
) -> bool {
    updated_room_ids.map_or(true, |updated| {
        updated.contains(space_id)
            || updated
                .iter()
                .any(|room_id| child_room_ids.contains(room_id))
    })
}

fn should_clear_space_member_demand(
    demand: Option<&SpaceMemberDemand>,
    space_id: &str,
    generation: u64,
) -> bool {
    demand.is_some_and(|demand| demand.space_id != space_id || demand.generation != generation)
}

fn space_members_refresh_is_current(
    result_space_id: &str,
    result_generation: u64,
    demanded_space_id: &str,
    demanded_generation: u64,
) -> bool {
    result_space_id == demanded_space_id && result_generation == demanded_generation
}

fn space_member_refresh_fence_is_current(
    active_fence: Option<SpaceMemberRefreshFence>,
    expected_fence: SpaceMemberRefreshFence,
    current_session_generation: u64,
    current_demand_generation: u64,
    result_space_id: &str,
    result_generation: u64,
    demanded_space_id: &str,
    demanded_generation: u64,
) -> bool {
    active_fence == Some(expected_fence)
        && current_session_generation == expected_fence.session_generation
        && current_demand_generation == expected_fence.demand_generation
        && space_members_refresh_is_current(
            result_space_id,
            result_generation,
            demanded_space_id,
            demanded_generation,
        )
}

fn record_space_member_demand_event(
    outcome: &'static str,
    generation: u64,
    child_room_count: usize,
) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "demand",
        )
        .field(DiagnosticField::token("outcome", outcome))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::count(
            "child_room_count",
            child_room_count as u64,
        )),
    );
}

fn record_space_member_refresh_event(outcome: &'static str, applied: bool) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "background_refresh",
        )
        .field(DiagnosticField::token("outcome", outcome))
        .field(DiagnosticField::boolean("applied", applied)),
    );
}

fn record_core_space_members_load_failure(trigger: &'static str, generation: u64) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "projection",
        )
        .field(DiagnosticField::token("trigger", trigger))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::token("outcome", "lookup_failed"))
        .field(DiagnosticField::token(
            "space_joined_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token(
            "space_invited_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token(
            "child_count_availability",
            "counts_unavailable",
        ))
        .field(DiagnosticField::token("input_count", "counts_unavailable"))
        .field(DiagnosticField::token("output_count", "counts_unavailable"))
        .field(DiagnosticField::token("freshness_status", "not_tracked")),
    );
}

fn record_core_space_members_operation(
    trigger: &'static str,
    generation: u64,
    outcome: &SpaceMemberInviteOutcome,
) {
    let outcome_token = match outcome {
        SpaceMemberInviteOutcome::Invited => "invited",
        SpaceMemberInviteOutcome::AlreadyInvited => "already_invited",
        SpaceMemberInviteOutcome::AlreadyJoined => "already_joined",
        SpaceMemberInviteOutcome::Cancelled => "cancelled",
        SpaceMemberInviteOutcome::NotInvited => "not_invited",
        SpaceMemberInviteOutcome::Failed(_) => "failed",
    };
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.space_members_projection",
            "invite_settled",
        )
        .field(DiagnosticField::token("trigger", trigger))
        .field(DiagnosticField::count("generation", generation))
        .field(DiagnosticField::token("outcome", outcome_token)),
    );
}

#[cfg(test)]
fn record_core_profile_resolution(projection: &SpaceMembersProjection) {
    let entries = projection
        .space_joined
        .iter()
        .chain(projection.space_invited.iter())
        .chain(projection.child_room_only.iter());
    let mut counts = [0_u64; 7];
    let input_count = entries
        .map(|entry| {
            let (relevant_room_label, space_room_label) =
                match entry.membership {
                    SpaceMemberMembership::ChildRoomOnly => (
                        entry.display_name.as_deref().filter(|label| {
                            !label.trim().is_empty() && label.trim() != "Unknown user"
                        }),
                        None,
                    ),
                    SpaceMemberMembership::SpaceJoined | SpaceMemberMembership::SpaceInvited => (
                        None,
                        entry.display_name.as_deref().filter(|label| {
                            !label.trim().is_empty() && label.trim() != "Unknown user"
                        }),
                    ),
                };
            let resolution = resolve_people_label(ProfileResolutionInput {
                local_alias: None,
                relevant_room_label,
                space_room_label,
                payload_label: None,
                cached_label: None,
                local_homeserver_label: None,
            });
            let index = match resolution.source {
                ProfileResolutionSource::LocalAlias => 0,
                ProfileResolutionSource::RelevantRoom => 1,
                ProfileResolutionSource::SpaceRoom => 2,
                ProfileResolutionSource::Payload => 3,
                ProfileResolutionSource::GlobalCache => 4,
                ProfileResolutionSource::LocalHomeserver => 5,
                ProfileResolutionSource::Unresolved => 6,
            };
            counts[index] += 1;
        })
        .count() as u64;
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.profile_resolution",
            "space_member_projection",
        )
        .field(DiagnosticField::count("input_count", input_count))
        .field(DiagnosticField::count("output_count", input_count))
        .field(DiagnosticField::count("local_alias_count", counts[0]))
        .field(DiagnosticField::count("relevant_room_count", counts[1]))
        .field(DiagnosticField::count("space_room_count", counts[2]))
        .field(DiagnosticField::count("payload_count", counts[3]))
        .field(DiagnosticField::count("global_cache_count", counts[4]))
        .field(DiagnosticField::count("local_homeserver_count", counts[5]))
        .field(DiagnosticField::count("unresolved_count", counts[6]))
        .field(DiagnosticField::token(
            "cache_stale_hit_status",
            "not_tracked",
        ))
        .field(DiagnosticField::token(
            "cache_freshness_status",
            "not_tracked",
        )),
    );
}

fn user_profile_mention_search_terms(user_id: &str, display_name: Option<&str>) -> Vec<String> {
    let mut terms = Vec::new();
    if let Some(display_name) = display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        terms.push(display_name.to_owned());
    }
    if !terms.iter().any(|term| term == user_id) {
        terms.push(user_id.to_owned());
    }
    terms
}

async fn load_pinned_events_for_room(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<Vec<PinnedEvent>, RoomFailureKind> {
    let event_ids = koushi_sdk::load_pinned_event_ids(session, room_id)
        .await
        .map_err(|error| classify_room_error(&error))?;
    Ok(load_pinned_events(session, room_id, event_ids).await)
}

async fn load_pinned_events(
    session: &MatrixClientSession,
    room_id: &str,
    event_ids: Vec<String>,
) -> Vec<PinnedEvent> {
    let Ok(parsed_room_id) = matrix_sdk::ruma::RoomId::parse(room_id) else {
        return event_ids
            .into_iter()
            .map(pinned_event_unavailable)
            .collect();
    };
    let Some(room) = session.client().get_room(&parsed_room_id) else {
        return event_ids
            .into_iter()
            .map(pinned_event_unavailable)
            .collect();
    };

    let mut seen = HashSet::new();
    let mut projected = Vec::new();
    for event_id in event_ids {
        if !seen.insert(event_id.clone()) {
            continue;
        }
        let Ok(parsed_event_id) = matrix_sdk::ruma::EventId::parse(&event_id) else {
            projected.push(pinned_event_unavailable(event_id));
            continue;
        };
        let pinned = match room.load_or_fetch_event(&parsed_event_id, None).await {
            Ok(event) => pinned_event_from_raw(event_id.clone(), event.raw().json().get()),
            Err(_) => pinned_event_unavailable(event_id),
        };
        projected.push(pinned);
    }
    projected
}

fn pinned_event_unavailable(event_id: String) -> PinnedEvent {
    PinnedEvent {
        event_id,
        sender: None,
        sender_label: None,
        body_preview: None,
        redacted: false,
        timestamp_ms: None,
        state: PinnedEventState::Unavailable,
        thread_root_event_id: None,
    }
}

fn pinned_event_from_raw(event_id: String, raw_json: &str) -> PinnedEvent {
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return pinned_event_unavailable(event_id);
    };
    let event_id = raw
        .get("event_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or(event_id);
    let sender = raw
        .get("sender")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let timestamp_ms = raw
        .get("origin_server_ts")
        .and_then(serde_json::Value::as_u64);
    let redacted = raw
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("redacted_because"))
        .is_some();
    let content = raw.get("content").unwrap_or(&serde_json::Value::Null);
    let thread_root_event_id = content
        .get("m.relates_to")
        .and_then(|relation| relation.get("rel_type"))
        .and_then(serde_json::Value::as_str)
        .filter(|rel_type| *rel_type == "m.thread")
        .and_then(|_| content.get("m.relates_to"))
        .and_then(|relation| relation.get("event_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let is_encrypted =
        raw.get("type").and_then(serde_json::Value::as_str) == Some("m.room.encrypted");
    let body_preview = content
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    PinnedEvent {
        event_id,
        sender,
        sender_label: None,
        body_preview,
        redacted,
        timestamp_ms,
        state: if is_encrypted {
            PinnedEventState::UnableToDecrypt
        } else {
            PinnedEventState::Ready
        },
        thread_root_event_id,
    }
}

fn replace_known_room_ids(known_room_ids: &Arc<RwLock<BTreeSet<String>>>, rooms: &[RoomSummary]) {
    if let Ok(mut known_room_ids) = known_room_ids.write() {
        *known_room_ids = rooms.iter().map(|room| room.room_id.clone()).collect();
    }
}

/// Convert `MatrixRoomListSnapshot` invites into Rust-owned invite previews.
fn normalize_invites(snapshot: &koushi_sdk::MatrixRoomListSnapshot) -> Vec<InvitePreview> {
    snapshot
        .invites
        .iter()
        .map(|invite| InvitePreview {
            room_id: invite.room_id.clone(),
            display_name: invite.display_name.clone(),
            avatar: avatar_from_mxc_uri(invite.avatar_mxc_uri.as_deref()),
            topic: invite.topic.clone(),
            inviter_display_name: invite.inviter_display_name.clone(),
            inviter_user_id: invite.inviter_user_id.clone(),
            is_dm: invite.is_dm,
        })
        .collect()
}

fn directory_room_summary_from_sdk(room: MatrixPublicRoomDirectoryRoom) -> DirectoryRoomSummary {
    DirectoryRoomSummary {
        room_id: room.room_id,
        canonical_alias: room.canonical_alias,
        room_type: room.room_type,
        name: room.name,
        topic: room.topic,
        avatar_url: room.avatar_url,
        joined_members: room.joined_members,
        world_readable: room.world_readable,
        guest_can_join: room.guest_can_join,
    }
}

fn directory_room_preview_from_sdk(preview: MatrixRoomPreview) -> DirectoryRoomPreview {
    DirectoryRoomPreview {
        room_id: preview.room_id,
        canonical_alias: preview.canonical_alias,
        room_type: preview.room_type,
        name: preview.name,
        topic: preview.topic,
        joined_members: preview.joined_members,
        joinability: match preview.joinability {
            MatrixPreviewJoinability::Open => DirectoryPreviewJoinability::Open,
            MatrixPreviewJoinability::InviteOnly => DirectoryPreviewJoinability::InviteOnly,
            MatrixPreviewJoinability::Restricted => DirectoryPreviewJoinability::Restricted,
            MatrixPreviewJoinability::Unknown => DirectoryPreviewJoinability::Unknown,
        },
        membership: match preview.membership {
            MatrixPreviewMembership::Joined => DirectoryPreviewMembership::Joined,
            MatrixPreviewMembership::Invited => DirectoryPreviewMembership::Invited,
            MatrixPreviewMembership::None => DirectoryPreviewMembership::None,
        },
    }
}

fn matrix_create_room_options(options: CreateRoomOptions) -> MatrixCreateRoomOptions {
    MatrixCreateRoomOptions {
        name: options.name,
        topic: options.topic,
        alias_localpart: options.alias_localpart,
        encrypted: options.encrypted,
        visibility: match options.visibility {
            CreateRoomVisibility::Private => MatrixCreateRoomVisibility::Private,
            CreateRoomVisibility::Public => MatrixCreateRoomVisibility::Public,
        },
        parent_space: options
            .parent_space
            .map(|parent| MatrixCreateRoomParentSpace {
                space_id: parent.space_id,
                via_server: parent.via_server,
            }),
    }
}

fn room_settings_snapshot_from_sdk(settings: MatrixRoomSettingsSnapshot) -> RoomSettingsSnapshot {
    let share_link = koushi_state::room_settings_share_link(
        &settings.room_id,
        settings.canonical_alias.as_deref(),
        &settings.alternate_aliases,
    );
    RoomSettingsSnapshot {
        room_id: settings.room_id,
        name: settings.name,
        topic: settings.topic,
        avatar_url: settings.avatar_url,
        canonical_alias: settings.canonical_alias,
        alternate_aliases: settings.alternate_aliases,
        share_link,
        join_rule: room_join_rule_from_sdk(settings.join_rule),
        history_visibility: room_history_visibility_from_sdk(settings.history_visibility),
        permissions: room_permission_facts_from_sdk(settings.permissions),
        members: settings
            .members
            .into_iter()
            .map(room_member_summary_from_sdk)
            .collect(),
    }
}

fn room_member_summary_from_sdk(member: MatrixRoomMemberSummary) -> RoomMemberSummary {
    let display_label = member
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .unwrap_or(member.user_id.as_str())
        .to_owned();
    RoomMemberSummary {
        user_id: member.user_id,
        display_name: member.display_name,
        display_label: display_label.clone(),
        original_display_label: display_label,
        avatar_url: member.avatar_url,
        power_level: member.power_level,
        role: room_member_role_from_sdk(member.role),
        user_trust: member.user_trust.map(user_trust_state_from_sdk),
    }
}

fn user_trust_state_from_sdk(state: MatrixUserTrustState) -> UserTrustState {
    match state {
        MatrixUserTrustState::Unverified => UserTrustState::Unverified,
        MatrixUserTrustState::Verified => UserTrustState::Verified,
        MatrixUserTrustState::IdentityReset => UserTrustState::IdentityReset,
    }
}

fn room_member_role_from_sdk(role: MatrixRoomMemberRole) -> RoomMemberRole {
    match role {
        MatrixRoomMemberRole::Creator => RoomMemberRole::Creator,
        MatrixRoomMemberRole::Administrator => RoomMemberRole::Administrator,
        MatrixRoomMemberRole::Moderator => RoomMemberRole::Moderator,
        MatrixRoomMemberRole::User => RoomMemberRole::User,
    }
}

fn room_join_rule_from_sdk(join_rule: MatrixRoomJoinRule) -> RoomJoinRule {
    match join_rule {
        MatrixRoomJoinRule::Public => RoomJoinRule::Public,
        MatrixRoomJoinRule::Invite => RoomJoinRule::Invite,
        MatrixRoomJoinRule::Knock => RoomJoinRule::Knock,
        MatrixRoomJoinRule::Restricted => RoomJoinRule::Restricted,
        MatrixRoomJoinRule::Private => RoomJoinRule::Private,
    }
}

fn room_join_rule_to_sdk(join_rule: RoomJoinRule) -> MatrixRoomJoinRule {
    match join_rule {
        RoomJoinRule::Public => MatrixRoomJoinRule::Public,
        RoomJoinRule::Invite => MatrixRoomJoinRule::Invite,
        RoomJoinRule::Knock => MatrixRoomJoinRule::Knock,
        RoomJoinRule::Restricted => MatrixRoomJoinRule::Restricted,
        RoomJoinRule::Private => MatrixRoomJoinRule::Private,
    }
}

fn room_history_visibility_from_sdk(
    history_visibility: MatrixRoomHistoryVisibility,
) -> RoomHistoryVisibility {
    match history_visibility {
        MatrixRoomHistoryVisibility::WorldReadable => RoomHistoryVisibility::WorldReadable,
        MatrixRoomHistoryVisibility::Shared => RoomHistoryVisibility::Shared,
        MatrixRoomHistoryVisibility::Invited => RoomHistoryVisibility::Invited,
        MatrixRoomHistoryVisibility::Joined => RoomHistoryVisibility::Joined,
    }
}

fn room_history_visibility_to_sdk(
    history_visibility: RoomHistoryVisibility,
) -> MatrixRoomHistoryVisibility {
    match history_visibility {
        RoomHistoryVisibility::WorldReadable => MatrixRoomHistoryVisibility::WorldReadable,
        RoomHistoryVisibility::Shared => MatrixRoomHistoryVisibility::Shared,
        RoomHistoryVisibility::Invited => MatrixRoomHistoryVisibility::Invited,
        RoomHistoryVisibility::Joined => MatrixRoomHistoryVisibility::Joined,
    }
}

fn room_permission_facts_from_sdk(permissions: MatrixRoomPermissionFacts) -> RoomPermissionFacts {
    RoomPermissionFacts {
        can_edit_settings: permissions.can_edit_settings,
        can_edit_roles: permissions.can_edit_roles,
        can_invite: permissions.can_invite,
        can_kick: permissions.can_kick,
        can_ban: permissions.can_ban,
        can_unban: permissions.can_unban,
    }
}

fn room_setting_change_to_sdk(change: RoomSettingChange) -> MatrixRoomSettingChange {
    match change {
        RoomSettingChange::Name(name) => MatrixRoomSettingChange::Name(name),
        RoomSettingChange::Topic(topic) => MatrixRoomSettingChange::Topic(topic),
        RoomSettingChange::AvatarUrl(avatar_url) => MatrixRoomSettingChange::AvatarUrl(avatar_url),
        RoomSettingChange::JoinRule(join_rule) => {
            MatrixRoomSettingChange::JoinRule(room_join_rule_to_sdk(join_rule))
        }
        RoomSettingChange::HistoryVisibility(history_visibility) => {
            MatrixRoomSettingChange::HistoryVisibility(room_history_visibility_to_sdk(
                history_visibility,
            ))
        }
    }
}

fn room_moderation_action_to_sdk(action: RoomModerationAction) -> MatrixRoomModerationAction {
    match action {
        RoomModerationAction::Kick => MatrixRoomModerationAction::Kick,
        RoomModerationAction::Ban => MatrixRoomModerationAction::Ban,
        RoomModerationAction::Unban => MatrixRoomModerationAction::Unban,
    }
}

fn room_moderation_allowed(
    permissions: &RoomPermissionFacts,
    action: RoomModerationAction,
) -> bool {
    match action {
        RoomModerationAction::Kick => permissions.can_kick,
        RoomModerationAction::Ban => permissions.can_ban,
        RoomModerationAction::Unban => permissions.can_unban,
    }
}

fn avatar_from_mxc_uri(mxc_uri: Option<&str>) -> Option<AvatarImage> {
    mxc_uri.map(|mxc_uri| AvatarImage {
        mxc_uri: mxc_uri.to_owned(),
        thumbnail: AvatarThumbnailState::NotRequested,
    })
}

fn sdk_room_tag_kind(tag: RoomTagKind) -> MatrixRoomTagKind {
    match tag {
        RoomTagKind::Favourite => MatrixRoomTagKind::Favourite,
        RoomTagKind::LowPriority => MatrixRoomTagKind::LowPriority,
    }
}

fn room_tag_info_from_order(order: Option<f64>) -> RoomTagInfo {
    RoomTagInfo {
        order: order.map(|order| order.to_string()),
    }
}

fn operation_failure_kind(kind: RoomFailureKind) -> OperationFailureKind {
    match kind {
        RoomFailureKind::Forbidden => OperationFailureKind::Forbidden,
        RoomFailureKind::Network => OperationFailureKind::Network,
        RoomFailureKind::NotFound => OperationFailureKind::NotFound,
        RoomFailureKind::Sdk => OperationFailureKind::Sdk,
    }
}

enum InviteTargetOutcome {
    Invited,
    AlreadyInSpace,
    Failed,
}

async fn invite_target_to_space_if_needed(
    session: &MatrixClientSession,
    space_id: &str,
    user_id: &str,
) -> InviteTargetOutcome {
    match koushi_sdk::room_has_active_member_no_sync(session, space_id, user_id).await {
        Ok(true) => return InviteTargetOutcome::AlreadyInSpace,
        Ok(false) => {}
        Err(_error) => return InviteTargetOutcome::Failed,
    }

    match koushi_sdk::invite_user_to_room(session, space_id, user_id).await {
        Ok(()) => InviteTargetOutcome::Invited,
        Err(_error) => InviteTargetOutcome::Failed,
    }
}

// ---------------------------------------------------------------------------
// Error classification (never raw SDK text in public events)
// ---------------------------------------------------------------------------

/// Map a `MatrixRoomOperationError` to a coarse `RoomFailureKind`.
/// The spec defines: Forbidden / NotFound / Network / Sdk.
/// Raw SDK error text must never appear in public events.
pub(crate) fn classify_room_error(error: &MatrixRoomOperationError) -> RoomFailureKind {
    use koushi_sdk::MatrixRoomOperationFailureKind;
    match error {
        MatrixRoomOperationError::InvalidRoomSetting => RoomFailureKind::Sdk,
        MatrixRoomOperationError::InvalidRoomId
        | MatrixRoomOperationError::InvalidRoomAlias
        | MatrixRoomOperationError::InvalidEventId
        | MatrixRoomOperationError::InvalidUserId
        | MatrixRoomOperationError::InvalidServerName
        | MatrixRoomOperationError::RoomUnavailable => RoomFailureKind::NotFound,
        MatrixRoomOperationError::Sdk(kind) => match kind {
            MatrixRoomOperationFailureKind::Forbidden
            | MatrixRoomOperationFailureKind::AuthenticationRequired => RoomFailureKind::Forbidden,
            MatrixRoomOperationFailureKind::Http => RoomFailureKind::Network,
            MatrixRoomOperationFailureKind::Sdk
            | MatrixRoomOperationFailureKind::Encryption
            | MatrixRoomOperationFailureKind::Store
            | MatrixRoomOperationFailureKind::WrongRoomState => RoomFailureKind::Sdk,
        },
    }
}

fn mention_failure_kind(error: &MatrixRoomOperationError) -> MentionCandidatesFailureKind {
    match classify_room_error(error) {
        RoomFailureKind::Forbidden => MentionCandidatesFailureKind::Forbidden,
        RoomFailureKind::Network => MentionCandidatesFailureKind::Network,
        RoomFailureKind::NotFound | RoomFailureKind::Sdk => MentionCandidatesFailureKind::Sdk,
    }
}

fn classify_report_error(
    error: &koushi_sdk::MatrixReportError,
) -> crate::failure::ReportFailureKind {
    use crate::failure::ReportFailureKind;
    use koushi_sdk::MatrixReportFailureKind;
    match error.failure_kind() {
        MatrixReportFailureKind::Forbidden => ReportFailureKind::Forbidden,
        MatrixReportFailureKind::Network => ReportFailureKind::Network,
        MatrixReportFailureKind::InvalidUserId => ReportFailureKind::InvalidUserId,
        MatrixReportFailureKind::InvalidRoomId => ReportFailureKind::InvalidRoomId,
        MatrixReportFailureKind::InvalidEventId => ReportFailureKind::InvalidEventId,
        MatrixReportFailureKind::Sdk => ReportFailureKind::Sdk,
    }
}

fn trace_room_operation(kind: &'static str, stage: &'static str, request_id: RequestId) {
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "core.room", stage)
            .field(DiagnosticField::token("operation", kind))
            .field(DiagnosticField::request_id(
                "request_id",
                request_id.connection_id.0,
                request_id.sequence,
            )),
    );
}

fn record_mention_candidate_event(
    stage: &'static str,
    surface: MentionSurface,
    completeness: MentionCandidatesCompleteness,
    candidate_count: usize,
    outcome: &'static str,
) {
    let surface = match surface {
        MentionSurface::Main => "main",
        MentionSurface::Thread => "thread",
    };
    let completeness = match completeness {
        MentionCandidatesCompleteness::Loading => "loading",
        MentionCandidatesCompleteness::Partial => "partial",
        MentionCandidatesCompleteness::Complete => "complete",
        MentionCandidatesCompleteness::Failed => "failed",
    };
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "mention.candidates", stage)
            .field(DiagnosticField::token("surface", surface))
            .field(DiagnosticField::token("completeness", completeness))
            .field(DiagnosticField::count(
                "candidate_count",
                candidate_count as u64,
            ))
            .field(DiagnosticField::token("outcome", outcome)),
    );
}

// ---------------------------------------------------------------------------
// Unit tests (network-free)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use koushi_sdk::{
        MatrixConversationActivity, MatrixConversationActivitySource, MatrixInvitePreview,
        MatrixRoomListRoom, MatrixRoomListSnapshot, MatrixRoomListSpace, MatrixRoomMemberRole,
        MatrixRoomPermissionFacts, MatrixRoomSettingsSnapshot, MatrixRoomTagInfo, MatrixRoomTags,
    };
    use koushi_state::{RoomMemberRole, RoomTagInfo, RoomTagKind, SessionInfo};
    use tokio::sync::{broadcast, mpsc, oneshot};

    use super::*;
    use crate::command::RoomCommand;
    use crate::event::CoreEvent;
    use crate::failure::{CoreFailure, RoomFailureKind};
    use crate::ids::{RequestId, RuntimeConnectionId};

    fn make_request_id(seq: u64) -> RequestId {
        RequestId {
            connection_id: RuntimeConnectionId(1),
            sequence: seq,
        }
    }

    async fn wait_for_live_observer_test_event(
        rx: &mut mpsc::UnboundedReceiver<LiveObserverTestEvent>,
        label: &'static str,
        predicate: impl Fn(&LiveObserverTestEvent) -> bool,
    ) -> LiveObserverTestEvent {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let event = rx.recv().await.expect("live observer test channel");
                if predicate(&event) {
                    break event;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
    }

    struct LiveObserverTestHarness {
        action_rx: mpsc::Receiver<Vec<AppAction>>,
        test_event_rx: mpsc::UnboundedReceiver<LiveObserverTestEvent>,
        _refresh_tx: mpsc::Sender<()>,
        stop_tx: oneshot::Sender<()>,
        task: tokio::task::JoinHandle<()>,
    }

    impl LiveObserverTestHarness {
        async fn next_actions(&mut self, label: &'static str) -> Vec<AppAction> {
            tokio::time::timeout(Duration::from_secs(1), self.action_rx.recv())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
                .expect("action channel should stay open")
        }

        async fn expect_event(&mut self, label: &'static str, expected: LiveObserverTestEvent) {
            let actual =
                wait_for_live_observer_test_event(&mut self.test_event_rx, label, |event| {
                    event == &expected
                })
                .await;
            assert_eq!(actual, expected);
        }

        async fn stop(self) {
            let _ = self.stop_tx.send(());
            self.task.await.expect("observer task");
        }
    }

    async fn spawn_live_observer_test_harness(
        client: matrix_sdk::Client,
        homeserver: String,
        entries_limit: usize,
        room_updates_rx: broadcast::Receiver<matrix_sdk_base::sync::RoomUpdates>,
    ) -> LiveObserverTestHarness {
        let service = Arc::new(
            matrix_sdk_ui::room_list_service::RoomListService::new(client.clone())
                .await
                .expect("room list service"),
        );
        let session = Arc::new(MatrixClientSession::from_client_for_testing(
            client,
            SessionInfo {
                homeserver,
                user_id: "@observer:example.invalid".to_owned(),
                device_id: "OBSERVER".to_owned(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        ));
        let known_room_ids = Arc::new(RwLock::new(BTreeSet::new()));
        let (room_tx, _room_rx) = mpsc::channel(4);
        let (action_tx, action_rx) = mpsc::channel(8);
        let (event_tx, _event_rx) = broadcast::channel(8);
        let (refresh_tx, refresh_rx) = mpsc::channel(1);
        let (stop_tx, stop_rx) = oneshot::channel();
        let (test_event_tx, test_event_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_live_room_list_observation_with_sources(
            session,
            service,
            known_room_ids,
            room_tx,
            action_tx,
            event_tx,
            refresh_rx,
            stop_rx,
            1,
            RoomListSource::Legacy,
            Arc::new(AtomicBool::new(true)),
            entries_limit,
            room_updates_rx,
            Some(test_event_tx),
        ));
        LiveObserverTestHarness {
            action_rx,
            test_event_rx,
            _refresh_tx: refresh_tx,
            stop_tx,
            task,
        }
    }

    #[test]
    fn room_operation_records_without_environment_switch() {
        trace_room_operation("create_room", "test_always_on", make_request_id(999));
        assert!(koushi_diagnostics::snapshot().records.iter().any(|record| {
            record.event.source == "core.room" && record.event.stage == "test_always_on"
        }));
    }

    #[test]
    fn invite_projection_policy_self_heals_after_lag_and_skips_ordinary_updates() {
        let projected = BTreeSet::from(["!invite:example.invalid".to_owned()]);
        let changed = BTreeSet::from(["!other-invite:example.invalid".to_owned()]);

        assert!(!invite_projection_required(
            Some(&projected),
            &projected,
            false,
            false,
        ));
        assert!(invite_projection_required(
            Some(&projected),
            &projected,
            true,
            false,
        ));
        assert!(invite_projection_required(
            Some(&projected),
            &projected,
            false,
            true,
        ));
        assert!(invite_projection_required(
            Some(&projected),
            &changed,
            false,
            false,
        ));
        assert!(!invite_projection_required(None, &changed, true, true,));
    }

    #[test]
    fn space_member_sync_updates_are_relevant_only_for_the_demanded_scope() {
        let child_room_ids = BTreeSet::from([
            "!child-a:example.invalid".to_owned(),
            "!child-b:example.invalid".to_owned(),
        ]);
        let space_update = BTreeSet::from(["!space:example.invalid".to_owned()]);
        let child_update = BTreeSet::from(["!child-a:example.invalid".to_owned()]);
        let unrelated_update = BTreeSet::from(["!unrelated:example.invalid".to_owned()]);

        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&space_update),
        ));
        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&child_update),
        ));
        assert!(!space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&unrelated_update),
        ));
        assert!(space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            None,
        ));
        assert!(!space_members_update_affects_demand(
            "!space:example.invalid",
            &child_room_ids,
            Some(&BTreeSet::new()),
        ));
    }

    #[test]
    fn stale_space_member_refreshes_are_rejected_by_space_and_generation() {
        assert!(space_members_refresh_is_current(
            "!space:example.invalid",
            4,
            "!space:example.invalid",
            4,
        ));
        assert!(!space_members_refresh_is_current(
            "!space:example.invalid",
            3,
            "!space:example.invalid",
            4,
        ));
        assert!(!space_members_refresh_is_current(
            "!old-space:example.invalid",
            4,
            "!space:example.invalid",
            4,
        ));
    }

    #[test]
    fn space_member_reload_clears_only_a_different_demand() {
        let demand = SpaceMemberDemand {
            space_id: "!space:example.invalid".to_owned(),
            generation: 4,
            child_room_ids: BTreeSet::new(),
            demand_generation: 1,
        };

        assert!(!should_clear_space_member_demand(
            Some(&demand),
            "!space:example.invalid",
            4,
        ));
        assert!(should_clear_space_member_demand(
            Some(&demand),
            "!other-space:example.invalid",
            4,
        ));
        assert!(should_clear_space_member_demand(
            Some(&demand),
            "!space:example.invalid",
            5,
        ));
        assert!(!should_clear_space_member_demand(
            None,
            "!space:example.invalid",
            4,
        ));
    }

    #[test]
    fn stale_space_member_refreshes_are_rejected_by_session_demand_and_request_fences() {
        let fence = SpaceMemberRefreshFence {
            request_id: make_request_id(1),
            session_generation: 2,
            demand_generation: 3,
            refresh_generation: 4,
        };
        let current = |active_fence, session_generation, demand_generation, request_id| {
            space_member_refresh_fence_is_current(
                active_fence,
                SpaceMemberRefreshFence {
                    request_id,
                    ..fence
                },
                session_generation,
                demand_generation,
                "!space:example.invalid",
                4,
                "!space:example.invalid",
                4,
            )
        };

        assert!(current(Some(fence), 2, 3, make_request_id(1)));
        assert!(!current(Some(fence), 1, 3, make_request_id(1)));
        assert!(!current(Some(fence), 2, 9, make_request_id(1)));
        assert!(!current(Some(fence), 2, 3, make_request_id(2)));
    }

    #[test]
    fn existing_membership_change_message_routes_to_space_refresh() {
        let source = include_str!("room.rs");
        let handler = source
            .split("async fn handle_mention_membership_changed")
            .nth(1)
            .expect("membership change handler exists")
            .split("async fn handle_mention_local_aliases_updated")
            .next()
            .expect("membership change handler boundary exists");

        assert!(
            handler.contains("handle_space_membership_changed"),
            "the existing MentionMembershipChanged message must refresh demanded Space members"
        );
    }

    // --- Error classification ---

    #[test]
    fn forbidden_sdk_error_classifies_as_forbidden() {
        let error =
            MatrixRoomOperationError::Sdk(koushi_sdk::MatrixRoomOperationFailureKind::Forbidden);
        assert_eq!(classify_room_error(&error), RoomFailureKind::Forbidden);
    }

    #[test]
    fn auth_required_sdk_error_classifies_as_forbidden() {
        let error = MatrixRoomOperationError::Sdk(
            koushi_sdk::MatrixRoomOperationFailureKind::AuthenticationRequired,
        );
        assert_eq!(classify_room_error(&error), RoomFailureKind::Forbidden);
    }

    #[test]
    fn http_sdk_error_classifies_as_network() {
        let error = MatrixRoomOperationError::Sdk(koushi_sdk::MatrixRoomOperationFailureKind::Http);
        assert_eq!(classify_room_error(&error), RoomFailureKind::Network);
    }

    #[test]
    fn invalid_room_id_classifies_as_not_found() {
        let error = MatrixRoomOperationError::InvalidRoomId;
        assert_eq!(classify_room_error(&error), RoomFailureKind::NotFound);
    }

    #[test]
    fn room_unavailable_classifies_as_not_found() {
        let error = MatrixRoomOperationError::RoomUnavailable;
        assert_eq!(classify_room_error(&error), RoomFailureKind::NotFound);
    }

    #[test]
    fn sdk_error_classifies_as_sdk() {
        let error = MatrixRoomOperationError::Sdk(koushi_sdk::MatrixRoomOperationFailureKind::Sdk);
        assert_eq!(classify_room_error(&error), RoomFailureKind::Sdk);
    }

    #[test]
    fn mark_room_as_read_success_updates_fully_read_marker_before_clearing_counts() {
        let source = include_str!("room.rs");
        let handler = source
            .split("async fn handle_mark_room_as_read")
            .nth(1)
            .expect("handle_mark_room_as_read should exist")
            .split("async fn handle_mark_room_as_unread")
            .next()
            .expect("handle_mark_room_as_unread should follow handle_mark_room_as_read");
        let success_arm = handler
            .split("Ok(()) => {")
            .nth(1)
            .expect("mark read success arm should exist")
            .split("Err(error) => {")
            .next()
            .expect("mark read error arm should follow success arm");

        assert!(
            success_arm.contains("AppAction::FullyReadMarkerUpdated"),
            "mark-room-as-read success must update local fully-read state so stale room-list snapshots cannot resurrect unread counts"
        );
        assert!(
            success_arm.contains("AppAction::RoomMarkedAsReadSucceeded"),
            "mark-room-as-read success must still clear room summary unread counts"
        );
        assert!(
            success_arm.find("FullyReadMarkerUpdated")
                < success_arm.find("RoomMarkedAsReadSucceeded"),
            "fully-read marker should be reduced before unread counts are cleared"
        );
    }

    #[test]
    fn room_settings_snapshot_mapping_preserves_role_power_and_role_permission_facts() {
        let settings = MatrixRoomSettingsSnapshot {
            room_id: "!room:example.invalid".to_owned(),
            name: Some("Private room".to_owned()),
            topic: Some("Private topic".to_owned()),
            avatar_url: Some("mxc://example.invalid/avatar".to_owned()),
            canonical_alias: Some("#private:example.invalid".to_owned()),
            alternate_aliases: vec!["#alternate:example.invalid".to_owned()],
            join_rule: MatrixRoomJoinRule::Invite,
            history_visibility: MatrixRoomHistoryVisibility::Shared,
            permissions: MatrixRoomPermissionFacts {
                can_edit_settings: true,
                can_edit_roles: true,
                can_invite: true,
                can_kick: true,
                can_ban: false,
                can_unban: false,
            },
            members: vec![MatrixRoomMemberSummary {
                user_id: "@member:example.invalid".to_owned(),
                display_name: Some("Private member".to_owned()),
                avatar_url: Some("mxc://example.invalid/member-avatar".to_owned()),
                power_level: Some(50),
                role: MatrixRoomMemberRole::Moderator,
                user_trust: None,
            }],
        };

        let mapped = room_settings_snapshot_from_sdk(settings);

        assert!(mapped.permissions.can_edit_roles);
        assert!(mapped.permissions.can_invite);
        assert_eq!(
            mapped.share_link.as_deref(),
            Some("https://matrix.to/#/%23private%3Aexample.invalid")
        );
        let member = mapped.members.first().expect("member summary");
        assert_eq!(member.power_level, Some(50));
        assert_eq!(member.role, RoomMemberRole::Moderator);
        let debug = format!("{mapped:?}");
        assert!(!debug.contains("Private room"), "{debug}");
        assert!(!debug.contains("Private topic"), "{debug}");
        assert!(!debug.contains("@member:example.invalid"), "{debug}");
        assert!(!debug.contains("mxc://example.invalid"), "{debug}");
    }

    // --- Room list normalization: spaces ---

    #[test]
    fn normalize_rooms_preserves_typed_conversation_activity_and_opaque_recency() {
        let snapshot = MatrixRoomListSnapshot {
            rooms: vec![MatrixRoomListRoom {
                room_id: "!dm:example.test".to_owned(),
                display_name: "Synthetic DM".to_owned(),
                avatar_mxc_uri: None,
                is_dm: true,
                dm_user_ids: vec!["@member:example.test".to_owned()],
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: Some(9),
                conversation_activity: Some(MatrixConversationActivity {
                    timestamp_ms: 42,
                    source: MatrixConversationActivitySource::EncryptedMessage,
                }),
                latest_event: None,
                parent_space_ids: Vec::new(),
                is_encrypted: true,
                joined_members: 2,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        let rooms = normalize_rooms(&snapshot);
        let room = rooms.first().expect("normalized room");

        assert_eq!(room.recency_stamp, Some(9));
        assert_eq!(
            room.conversation_activity,
            Some(koushi_state::ConversationActivity {
                timestamp_ms: 42,
                source: koushi_state::ConversationActivitySource::EncryptedMessage,
            })
        );
    }

    #[test]
    fn normalize_spaces_with_child_rooms() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space1:example.test".to_owned(),
                display_name: "My Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: Vec::new(),
                member_user_ids: Vec::new(),
            }],
            rooms: vec![
                MatrixRoomListRoom {
                    room_id: "!room1:example.test".to_owned(),
                    display_name: "Room 1".to_owned(),
                    avatar_mxc_uri: None,
                    is_dm: false,
                    dm_user_ids: Vec::new(),
                    tags: MatrixRoomTags::default(),
                    unread_count: 0,
                    notification_count: 0,
                    highlight_count: 0,
                    marked_unread: false,
                    recency_stamp: None,
                    conversation_activity: None,
                    latest_event: None,
                    parent_space_ids: vec!["!space1:example.test".to_owned()],
                    is_encrypted: false,
                    joined_members: 0,
                },
                MatrixRoomListRoom {
                    room_id: "!room2:example.test".to_owned(),
                    display_name: "Room 2".to_owned(),
                    avatar_mxc_uri: None,
                    is_dm: false,
                    dm_user_ids: Vec::new(),
                    tags: MatrixRoomTags::default(),
                    unread_count: 0,
                    notification_count: 0,
                    highlight_count: 0,
                    marked_unread: false,
                    recency_stamp: None,
                    conversation_activity: None,
                    latest_event: None,
                    parent_space_ids: vec![],
                    is_encrypted: false,
                    joined_members: 0,
                },
            ],
            ..MatrixRoomListSnapshot::default()
        };
        let spaces = normalize_spaces(&snapshot);
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].space_id, "!space1:example.test");
        assert_eq!(spaces[0].child_room_ids, vec!["!room1:example.test"]);
    }

    #[test]
    fn missing_space_child_links_detects_parent_only_relationship() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space:example.test".to_owned(),
                display_name: "My Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: Vec::new(),
                member_user_ids: Vec::new(),
            }],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "Room".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec!["!space:example.test".to_owned()],
                is_encrypted: true,
                joined_members: 1,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        assert_eq!(
            missing_space_child_links(&snapshot),
            vec![MissingSpaceChildLink {
                space_id: "!space:example.test".to_owned(),
                child_room_id: "!room:example.test".to_owned(),
                via_server: "example.test".to_owned(),
            }]
        );
    }

    #[test]
    fn missing_space_child_links_skips_reciprocal_relationship() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space:example.test".to_owned(),
                display_name: "My Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: vec!["!room:example.test".to_owned()],
                member_user_ids: Vec::new(),
            }],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "Room".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec!["!space:example.test".to_owned()],
                is_encrypted: true,
                joined_members: 1,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        assert!(missing_space_child_links(&snapshot).is_empty());
    }

    #[test]
    fn normalize_spaces_uses_direct_space_child_state() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space1:example.test".to_owned(),
                display_name: "My Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: vec!["!room1:example.test".to_owned()],
                member_user_ids: Vec::new(),
            }],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room1:example.test".to_owned(),
                display_name: "Room 1".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: Vec::new(),
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        let spaces = normalize_spaces(&snapshot);

        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].child_room_ids, vec!["!room1:example.test"]);
    }

    #[test]
    fn normalize_spaces_no_children() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space:example.test".to_owned(),
                display_name: "Empty Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: Vec::new(),
                member_user_ids: Vec::new(),
            }],
            rooms: vec![],
            ..MatrixRoomListSnapshot::default()
        };
        let spaces = normalize_spaces(&snapshot);
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].child_room_ids, Vec::<String>::new());
    }

    #[test]
    fn normalize_spaces_preserves_avatar_mxc_as_unrequested_thumbnail() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space:example.test".to_owned(),
                display_name: "Space".to_owned(),
                avatar_mxc_uri: Some("mxc://example.test/space-avatar".to_owned()),
                child_room_ids: Vec::new(),
                member_user_ids: Vec::new(),
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let spaces = normalize_spaces(&snapshot);

        let avatar = spaces[0].avatar.as_ref().expect("space avatar");
        assert_eq!(avatar.mxc_uri, "mxc://example.test/space-avatar");
        assert_eq!(avatar.thumbnail, AvatarThumbnailState::NotRequested);
    }

    // --- Room list normalization: rooms ---

    #[test]
    fn normalize_rooms_preserves_dm_and_unread() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!dm:example.test".to_owned(),
                display_name: "Alice".to_owned(),
                avatar_mxc_uri: None,
                is_dm: true,
                dm_user_ids: vec!["@alice:example.test".to_owned()],
                tags: MatrixRoomTags::default(),
                unread_count: 3,
                notification_count: 3,
                highlight_count: 1,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec![],
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let rooms = normalize_rooms(&snapshot);
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].room_id, "!dm:example.test");
        assert!(rooms[0].is_dm);
        assert_eq!(rooms[0].unread_count, 3);
        assert_eq!(rooms[0].notification_count, 3);
        assert_eq!(rooms[0].highlight_count, 1);
    }

    #[test]
    fn normalize_rooms_non_dm() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "General".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec!["!space:example.test".to_owned()],
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let rooms = normalize_rooms(&snapshot);
        assert_eq!(rooms.len(), 1);
        assert!(!rooms[0].is_dm);
        assert_eq!(rooms[0].parent_space_ids, vec!["!space:example.test"]);
        assert_eq!(rooms[0].notification_count, 0);
        assert_eq!(rooms[0].highlight_count, 0);
    }

    #[test]
    fn normalize_rooms_uses_direct_space_child_state_as_parent() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "!space:example.test".to_owned(),
                display_name: "Space".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: vec!["!room:example.test".to_owned()],
                member_user_ids: Vec::new(),
            }],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "General".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: Vec::new(),
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        let rooms = normalize_rooms(&snapshot);

        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].parent_space_ids, vec!["!space:example.test"]);
    }

    #[test]
    fn normalize_rooms_assigns_dm_space_ids_by_counterpart_membership() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![MatrixRoomListSpace {
                space_id: "space-a".to_owned(),
                display_name: "Space A".to_owned(),
                avatar_mxc_uri: None,
                child_room_ids: Vec::new(),
                member_user_ids: vec!["@alice".to_owned()],
            }],
            rooms: vec![
                MatrixRoomListRoom {
                    room_id: "dm-alice".to_owned(),
                    display_name: "Alice".to_owned(),
                    avatar_mxc_uri: None,
                    is_dm: true,
                    dm_user_ids: vec!["@alice".to_owned()],
                    tags: MatrixRoomTags::default(),
                    unread_count: 0,
                    notification_count: 0,
                    highlight_count: 0,
                    marked_unread: false,
                    recency_stamp: None,
                    conversation_activity: None,
                    latest_event: None,
                    parent_space_ids: Vec::new(),
                    is_encrypted: false,
                    joined_members: 0,
                },
                MatrixRoomListRoom {
                    room_id: "dm-bob".to_owned(),
                    display_name: "Bob".to_owned(),
                    avatar_mxc_uri: None,
                    is_dm: true,
                    dm_user_ids: vec!["@bob".to_owned()],
                    tags: MatrixRoomTags::default(),
                    unread_count: 0,
                    notification_count: 0,
                    highlight_count: 0,
                    marked_unread: false,
                    recency_stamp: None,
                    conversation_activity: None,
                    latest_event: None,
                    parent_space_ids: Vec::new(),
                    is_encrypted: false,
                    joined_members: 0,
                },
            ],
            ..MatrixRoomListSnapshot::default()
        };
        let rooms = normalize_rooms(&snapshot);
        let alice_room = rooms.iter().find(|r| r.room_id == "dm-alice").unwrap();
        let bob_room = rooms.iter().find(|r| r.room_id == "dm-bob").unwrap();
        assert_eq!(alice_room.dm_space_ids, vec!["space-a"]);
        assert_eq!(bob_room.dm_space_ids, Vec::<String>::new());
    }

    #[test]
    fn normalize_rooms_preserves_avatar_mxc_as_unrequested_thumbnail() {
        let snapshot = MatrixRoomListSnapshot {
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "General".to_owned(),
                avatar_mxc_uri: Some("mxc://example.test/room-avatar".to_owned()),
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec![],
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let rooms = normalize_rooms(&snapshot);

        let avatar = rooms[0].avatar.as_ref().expect("room avatar");
        assert_eq!(avatar.mxc_uri, "mxc://example.test/room-avatar");
        assert_eq!(avatar.thumbnail, AvatarThumbnailState::NotRequested);
    }

    #[test]
    fn normalize_invites_preserves_preview_fields() {
        let snapshot = MatrixRoomListSnapshot {
            invites: vec![MatrixInvitePreview {
                room_id: "!invite:example.test".to_owned(),
                display_name: "Project invite".to_owned(),
                avatar_mxc_uri: None,
                topic: Some("Project topic".to_owned()),
                inviter_display_name: Some("Inviter".to_owned()),
                inviter_user_id: Some("@inviter:example.test".to_owned()),
                is_dm: true,
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let invites = normalize_invites(&snapshot);

        assert_eq!(invites.len(), 1);
        assert_eq!(invites[0].room_id, "!invite:example.test");
        assert_eq!(invites[0].display_name, "Project invite");
        assert_eq!(invites[0].topic.as_deref(), Some("Project topic"));
        assert_eq!(invites[0].inviter_display_name.as_deref(), Some("Inviter"));
        assert!(invites[0].is_dm);
    }

    #[test]
    fn normalize_invites_preserves_avatar_mxc_as_unrequested_thumbnail() {
        let snapshot = MatrixRoomListSnapshot {
            invites: vec![MatrixInvitePreview {
                room_id: "!invite:example.test".to_owned(),
                display_name: "Invite".to_owned(),
                avatar_mxc_uri: Some("mxc://example.test/invite-avatar".to_owned()),
                topic: None,
                inviter_display_name: None,
                inviter_user_id: None,
                is_dm: false,
            }],
            ..MatrixRoomListSnapshot::default()
        };
        let invites = normalize_invites(&snapshot);

        let avatar = invites[0].avatar.as_ref().expect("invite avatar");
        assert_eq!(avatar.mxc_uri, "mxc://example.test/invite-avatar");
        assert_eq!(avatar.thumbnail, AvatarThumbnailState::NotRequested);
    }

    #[test]
    fn normalize_user_profiles_preserves_member_profile_fields() {
        let snapshot = MatrixRoomListSnapshot {
            user_profiles: vec![koushi_sdk::MatrixUserProfile {
                user_id: "@alice:example.test".to_owned(),
                display_name: Some("Alice".to_owned()),
                avatar_mxc_uri: Some("mxc://example.test/alice".to_owned()),
            }],
            ..MatrixRoomListSnapshot::default()
        };

        let profiles = normalize_user_profiles(&snapshot);

        assert_eq!(
            profiles,
            vec![UserProfile {
                user_id: "@alice:example.test".to_owned(),
                display_name: Some("Alice".to_owned()),
                display_label: "Alice".to_owned(),
                original_display_label: "Alice".to_owned(),
                mention_search_terms: vec!["Alice".to_owned(), "@alice:example.test".to_owned(),],
                avatar: Some(AvatarImage {
                    mxc_uri: "mxc://example.test/alice".to_owned(),
                    thumbnail: AvatarThumbnailState::NotRequested,
                }),
            }]
        );
    }

    #[tokio::test]
    async fn project_room_list_snapshot_updates_user_profiles() {
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let known_room_ids = Arc::new(RwLock::new(BTreeSet::new()));
        let snapshot = MatrixRoomListSnapshot {
            user_profiles: vec![koushi_sdk::MatrixUserProfile {
                user_id: "@alice:example.test".to_owned(),
                display_name: Some("Alice".to_owned()),
                avatar_mxc_uri: None,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        project_room_list_snapshot(
            &snapshot,
            &known_room_ids,
            &action_tx,
            &event_tx,
            1,
            RoomListSource::Legacy,
            true,
        )
        .await;

        let actions = action_rx.recv().await.expect("actions");
        assert!(
            matches!(
                actions.as_slice(),
                [
                    AppAction::RoomListSnapshotAuthoritative { .. },
                    AppAction::UserProfilesUpdated { profiles },
                ] if profiles == &vec![UserProfile {
                    user_id: "@alice:example.test".to_owned(),
                    display_name: Some("Alice".to_owned()),
                    display_label: "Alice".to_owned(),
                    original_display_label: "Alice".to_owned(),
                    mention_search_terms: vec![
                        "Alice".to_owned(),
                        "@alice:example.test".to_owned(),
                    ],
                    avatar: None,
                }]
            ),
            "expected UserProfilesUpdated action, got {actions:?}"
        );
    }

    #[tokio::test]
    async fn project_room_list_snapshot_holds_unproven_empty_and_preserves_known_rooms() {
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let known_room_ids = Arc::new(RwLock::new(BTreeSet::from([
            "!cached:example.test".to_owned(),
        ])));
        let snapshot = MatrixRoomListSnapshot::default();

        project_room_list_snapshot(
            &snapshot,
            &known_room_ids,
            &action_tx,
            &event_tx,
            1,
            RoomListSource::SyncService,
            false,
        )
        .await;

        let actions = action_rx.recv().await.expect("provisional actions");
        assert!(matches!(
            actions.as_slice(),
            [AppAction::RoomListSnapshotProvisional { rooms, invites, .. },
                AppAction::UserProfilesUpdated { .. }]
                if rooms.is_empty() && invites.is_empty()
        ));
        assert_eq!(
            known_room_ids
                .read()
                .expect("known rooms")
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["!cached:example.test".to_owned()]
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn live_room_list_observer_projects_committed_invite_without_entry_diff() {
        use matrix_sdk::{
            ruma::{events::AnySyncStateEvent, room_id, serde::Raw, user_id},
            test_utils::mocks::MatrixMockServer,
        };
        use matrix_sdk_test::{
            InvitedRoomBuilder, JoinedRoomBuilder, LeftRoomBuilder, event_factory::EventFactory,
        };

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let visible_room_id = room_id!("!visible-room:example.invalid");
        let visible_room_name: Raw<AnySyncStateEvent> = EventFactory::new()
            .room(visible_room_id)
            .sender(user_id!("@sender:example.invalid"))
            .room_name("AAAA visible room")
            .into();
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(visible_room_id).add_state_event(visible_room_name),
            )
            .await;
        let room_updates_rx = client.subscribe_to_all_room_updates();
        let mut harness =
            spawn_live_observer_test_harness(client.clone(), server.uri(), 1, room_updates_rx)
                .await;

        let initial = harness.next_actions("initial RLS projection").await;
        assert!(initial.iter().any(
            |action| matches!(action, AppAction::RoomListSnapshotAuthoritative { invites, .. } if invites.is_empty())
        ));
        harness
            .expect_event(
                "initial RLS projection",
                LiveObserverTestEvent::RlsProjected {
                    wake_count: 1,
                    entries_len: 1,
                },
            )
            .await;

        let invited_room_id = room_id!("!invite-without-list-diff:example.invalid");
        let invited_room_name = EventFactory::new()
            .room(invited_room_id)
            .sender(user_id!("@sender:example.invalid"))
            .room_name("ZZZZ hidden invite");
        server
            .sync_room(
                &client,
                InvitedRoomBuilder::new(invited_room_id).add_state_event(invited_room_name),
            )
            .await;

        harness
            .expect_event(
                "invite base batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 1,
                    update_count: 1,
                    lagged: false,
                    projection_required: true,
                },
            )
            .await;

        let updated = harness.next_actions("committed invite projection").await;
        assert!(updated.iter().any(|action| {
            matches!(
                action,
                AppAction::RoomListSnapshotAuthoritative { invites, .. }
                    if invites.iter().any(|invite| invite.room_id == invited_room_id.as_str())
            )
        }));
        harness
            .expect_event(
                "invite base projection",
                LiveObserverTestEvent::BaseProjected {
                    wake_count: 1,
                    rls_wake_count: 1,
                    entries_len: 1,
                    action_delivered: true,
                },
            )
            .await;

        let renamed_invite = EventFactory::new()
            .room(invited_room_id)
            .sender(user_id!("@sender:example.invalid"))
            .room_name("ZZZZ renamed invite");
        server
            .sync_room(
                &client,
                InvitedRoomBuilder::new(invited_room_id).add_state_event(renamed_invite),
            )
            .await;
        harness
            .expect_event(
                "invite metadata base batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 2,
                    update_count: 1,
                    lagged: false,
                    projection_required: true,
                },
            )
            .await;
        let metadata_updated = harness.next_actions("invite metadata projection").await;
        assert!(metadata_updated.iter().any(|action| {
            matches!(
                action,
                AppAction::RoomListSnapshotAuthoritative { invites, .. }
                    if invites.iter().any(|invite| {
                        invite.room_id == invited_room_id.as_str()
                            && invite.display_name == "ZZZZ renamed invite"
                    })
            )
        }));
        harness
            .expect_event(
                "invite metadata base projection",
                LiveObserverTestEvent::BaseProjected {
                    wake_count: 2,
                    rls_wake_count: 1,
                    entries_len: 1,
                    action_delivered: true,
                },
            )
            .await;

        server
            .sync_room(&client, LeftRoomBuilder::new(invited_room_id))
            .await;
        harness
            .expect_event(
                "invite removal base batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 3,
                    update_count: 1,
                    lagged: false,
                    projection_required: true,
                },
            )
            .await;
        let removed = harness.next_actions("invite removal projection").await;
        assert!(removed.iter().any(
            |action| matches!(action, AppAction::RoomListSnapshotAuthoritative { invites, .. } if invites.is_empty())
        ));
        harness
            .expect_event(
                "invite removal base projection",
                LiveObserverTestEvent::BaseProjected {
                    wake_count: 3,
                    rls_wake_count: 1,
                    entries_len: 1,
                    action_delivered: true,
                },
            )
            .await;
        assert!(matches!(
            harness.action_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let hidden_joined_room_id = room_id!("!hidden-joined-room:example.invalid");
        let hidden_joined_room_name: Raw<AnySyncStateEvent> = EventFactory::new()
            .room(hidden_joined_room_id)
            .sender(user_id!("@sender:example.invalid"))
            .room_name("ZZZY hidden joined room")
            .into();
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(hidden_joined_room_id)
                    .add_state_event(hidden_joined_room_name),
            )
            .await;
        harness
            .expect_event(
                "ordinary joined base batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 4,
                    update_count: 1,
                    lagged: false,
                    projection_required: false,
                },
            )
            .await;
        assert!(matches!(
            harness.action_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        harness.stop().await;
    }

    #[tokio::test]
    async fn live_room_list_observer_reconciles_once_after_lagged_base_updates() {
        use matrix_sdk::test_utils::mocks::MatrixMockServer;

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let (base_update_tx, base_update_rx) = broadcast::channel(1);
        let mut harness =
            spawn_live_observer_test_harness(client, server.uri(), 1, base_update_rx).await;

        harness.next_actions("initial empty RLS projection").await;
        harness
            .expect_event(
                "initial empty RLS projection",
                LiveObserverTestEvent::RlsProjected {
                    wake_count: 1,
                    entries_len: 0,
                },
            )
            .await;

        base_update_tx
            .send(matrix_sdk_base::sync::RoomUpdates::default())
            .expect("first base update");
        base_update_tx
            .send(matrix_sdk_base::sync::RoomUpdates::default())
            .expect("second base update should overrun capacity one");
        harness
            .expect_event(
                "lagged base batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 1,
                    update_count: 1,
                    lagged: true,
                    projection_required: true,
                },
            )
            .await;
        harness.next_actions("one lag self-heal projection").await;
        harness
            .expect_event(
                "lag self-heal projection",
                LiveObserverTestEvent::BaseProjected {
                    wake_count: 1,
                    rls_wake_count: 1,
                    entries_len: 0,
                    action_delivered: true,
                },
            )
            .await;

        base_update_tx
            .send(matrix_sdk_base::sync::RoomUpdates::default())
            .expect("post-lag fence update");
        harness
            .expect_event(
                "post-lag fence batch",
                LiveObserverTestEvent::BaseBatch {
                    wake_count: 2,
                    update_count: 1,
                    lagged: false,
                    projection_required: false,
                },
            )
            .await;
        assert!(matches!(
            harness.action_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        harness.stop().await;
    }

    #[tokio::test]
    async fn live_room_list_observer_keeps_entries_alive_after_base_receiver_closes() {
        use matrix_sdk::{ruma::room_id, test_utils::mocks::MatrixMockServer};
        use matrix_sdk_test::JoinedRoomBuilder;

        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let (base_update_tx, base_update_rx) = broadcast::channel(1);
        let mut harness =
            spawn_live_observer_test_harness(client.clone(), server.uri(), 2, base_update_rx).await;

        harness
            .next_actions("initial RLS projection before close")
            .await;
        harness
            .expect_event(
                "initial RLS projection before close",
                LiveObserverTestEvent::RlsProjected {
                    wake_count: 1,
                    entries_len: 0,
                },
            )
            .await;

        drop(base_update_tx);
        harness
            .expect_event("closed base receiver", LiveObserverTestEvent::BaseClosed)
            .await;

        let joined_room_id = room_id!("!joined-after-base-close:example.invalid");
        server
            .sync_room(&client, JoinedRoomBuilder::new(joined_room_id))
            .await;
        let actions = harness
            .next_actions("RLS projection after base close")
            .await;
        assert!(actions.iter().any(|action| {
            matches!(
                action,
                AppAction::RoomListSnapshotAuthoritative { rooms, .. }
                    | AppAction::RoomListSnapshotProvisional { rooms, .. }
                    if rooms.iter().any(|room| room.room_id == joined_room_id.as_str())
            )
        }));
        harness
            .expect_event(
                "RLS projection after base close",
                LiveObserverTestEvent::RlsProjected {
                    wake_count: 2,
                    entries_len: 1,
                },
            )
            .await;

        harness.stop().await;
    }

    #[tokio::test]
    async fn project_room_list_snapshot_does_not_update_known_rooms_when_actions_are_undelivered() {
        let (action_tx, action_rx) = mpsc::channel(1);
        drop(action_rx);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let known_room_ids = Arc::new(RwLock::new(BTreeSet::new()));
        let snapshot = MatrixRoomListSnapshot {
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room:example.test".to_owned(),
                display_name: "Private room".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags::default(),
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: Vec::new(),
                is_encrypted: false,
                joined_members: 0,
            }],
            ..MatrixRoomListSnapshot::default()
        };

        project_room_list_snapshot(
            &snapshot,
            &known_room_ids,
            &action_tx,
            &event_tx,
            1,
            RoomListSource::Legacy,
            true,
        )
        .await;

        assert!(
            known_room_ids.read().expect("known rooms").is_empty(),
            "RoomActor known-room book must advance only after reducer projection delivery"
        );
    }

    // --- SelectSpace / SelectRoom projection ---

    #[tokio::test]
    async fn select_space_projects_action() {
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        handle
            .send(RoomMessage::Command(RoomCommand::SelectSpace {
                request_id: make_request_id(1),
                space_id: Some("!space:example.test".to_owned()),
            }))
            .await;

        let actions = action_rx.recv().await.expect("actions");
        assert!(
            matches!(
                actions.as_slice(),
                [AppAction::SelectSpace {
                    space_id: Some(id)
                }] if id == "!space:example.test"
            ),
            "expected SelectSpace action, got {actions:?}"
        );
    }

    #[tokio::test]
    async fn reorder_spaces_projects_action() {
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        handle
            .send(RoomMessage::Command(RoomCommand::ReorderSpaces {
                request_id: make_request_id(1),
                space_ids: vec![
                    "!space-b:example.test".to_owned(),
                    "!space-a:example.test".to_owned(),
                ],
            }))
            .await;

        let actions = action_rx.recv().await.expect("actions");
        assert!(
            matches!(
                actions.as_slice(),
                [AppAction::ReorderSpaces { space_ids }]
                    if space_ids == &vec![
                        "!space-b:example.test".to_owned(),
                        "!space-a:example.test".to_owned()
                    ]
            ),
            "expected ReorderSpaces action, got {actions:?}"
        );
    }

    #[test]
    fn normalize_rooms_carries_sdk_room_tags() {
        let snapshot = MatrixRoomListSnapshot {
            spaces: vec![],
            rooms: vec![MatrixRoomListRoom {
                room_id: "!room1:example.test".to_owned(),
                display_name: "Room 1".to_owned(),
                avatar_mxc_uri: None,
                is_dm: false,
                dm_user_ids: Vec::new(),
                tags: MatrixRoomTags {
                    favourite: Some(MatrixRoomTagInfo {
                        order: Some("0.25".to_owned()),
                    }),
                    low_priority: None,
                },
                unread_count: 0,
                notification_count: 0,
                highlight_count: 0,
                marked_unread: false,
                recency_stamp: None,
                conversation_activity: None,
                latest_event: None,
                parent_space_ids: vec![],
                is_encrypted: false,
                joined_members: 0,
            }],
            invites: vec![],
            user_profiles: vec![],
        };

        let rooms = normalize_rooms(&snapshot);

        assert_eq!(
            rooms[0].tags.favourite,
            Some(RoomTagInfo {
                order: Some("0.25".to_owned())
            })
        );
        assert_eq!(rooms[0].tags.low_priority, None);
    }

    #[tokio::test]
    async fn select_room_projects_action() {
        let (action_tx, mut action_rx) = mpsc::channel(16);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        handle
            .send(RoomMessage::Command(RoomCommand::SelectRoom {
                request_id: make_request_id(2),
                room_id: "!room:example.test".to_owned(),
            }))
            .await;

        let actions = action_rx.recv().await.expect("actions");
        assert!(
            matches!(
                actions.as_slice(),
                [AppAction::SelectRoom { room_id }] if room_id == "!room:example.test"
            ),
            "expected SelectRoom action, got {actions:?}"
        );
    }

    // --- OperationFailed without session emits SessionRequired ---

    #[tokio::test]
    async fn create_room_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(3);
        handle
            .send(RoomMessage::Command(RoomCommand::CreateRoom {
                request_id,
                options: CreateRoomOptions {
                    name: "test room".to_owned(),
                    topic: None,
                    alias_localpart: None,
                    encrypted: false,
                    visibility: CreateRoomVisibility::Private,
                    parent_space: None,
                },
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn leave_room_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(4);
        handle
            .send(RoomMessage::Command(RoomCommand::LeaveRoom {
                request_id,
                room_id: "!room:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn forget_room_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(5);
        handle
            .send(RoomMessage::Command(RoomCommand::ForgetRoom {
                request_id,
                room_id: "!room:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn set_room_tag_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(6);
        handle
            .send(RoomMessage::Command(RoomCommand::SetTag {
                request_id,
                room_id: "!room:example.test".to_owned(),
                tag: RoomTagKind::Favourite,
                order: None,
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn remove_room_tag_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(7);
        handle
            .send(RoomMessage::Command(RoomCommand::RemoveTag {
                request_id,
                room_id: "!room:example.test".to_owned(),
                tag: RoomTagKind::LowPriority,
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pin_event_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(8);
        handle
            .send(RoomMessage::Command(RoomCommand::PinEvent {
                request_id,
                room_id: "!room:example.test".to_owned(),
                event_id: "$event:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unpin_event_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        let request_id = make_request_id(9);
        handle
            .send(RoomMessage::Command(RoomCommand::UnpinEvent {
                request_id,
                room_id: "!room:example.test".to_owned(),
                event_id: "$event:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[test]
    fn room_tag_success_path_does_not_refresh_from_stale_sdk_snapshot() {
        let source = include_str!("room.rs");
        let set_tag_body = source
            .split("async fn handle_set_tag")
            .nth(1)
            .expect("set tag handler")
            .split("async fn handle_remove_tag")
            .next()
            .expect("set tag body");
        let remove_tag_body = source
            .split("async fn handle_remove_tag")
            .nth(1)
            .expect("remove tag handler")
            .split("    /// Refresh the room list")
            .next()
            .expect("remove tag body");

        assert!(!set_tag_body.contains("refresh_room_list().await"));
        assert!(!remove_tag_body.contains("refresh_room_list().await"));
    }

    #[test]
    fn room_actor_command_loop_never_awaits_room_list_refresh() {
        let source = include_str!("room.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");

        assert!(
            !production_source.contains("refresh_room_list().await"),
            "RoomActor command handling must not await room-list normalization; it can block user-visible operations under large room lists"
        );
    }

    #[test]
    fn legacy_room_list_observation_accepts_explicit_refresh_requests() {
        let source = include_str!("room.rs");
        let legacy_body = source
            .split("async fn run_legacy_room_list_observation")
            .nth(1)
            .expect("legacy observation function")
            .split("// ---------------------------------------------------------------------------")
            .next()
            .expect("legacy observation body");

        assert!(legacy_body.contains("mut refresh_rx: mpsc::Receiver<()>"));
        assert!(legacy_body.contains("refresh_rx.recv()"));
        assert!(legacy_body.contains("while refresh_rx.try_recv().is_ok()"));
    }

    #[test]
    fn sync_started_legacy_starts_observation_before_refresh_request() {
        let source = include_str!("room.rs");
        let sync_started_body = source
            .split("RoomMessage::SyncStarted")
            .nth(2)
            .expect("SyncStarted match arm")
            .split("RoomMessage::SyncStopped")
            .next()
            .expect("SyncStarted body");

        let start = sync_started_body
            .find("self.start_legacy_observation(")
            .expect("legacy observation starts");
        let refresh = sync_started_body
            .find("self.refresh_room_list(")
            .expect("legacy refresh request");

        assert!(
            start < refresh,
            "Legacy refresh must be requested through the observation loop after it starts"
        );
        assert!(
            !sync_started_body.contains("self.clear_known_rooms();"),
            "backend handoff must retain the actor-known cached rooms until the new generation settles"
        );
    }

    #[test]
    fn create_room_links_parent_space_child_with_created_room_id_before_completion_event() {
        let source = include_str!("room.rs");
        let create_body = source
            .split("async fn handle_create_room")
            .nth(1)
            .expect("create room handler")
            .split("async fn handle_create_public_directory_room")
            .next()
            .expect("create room body");

        let link = create_body
            .find("link_created_room_to_parent_space")
            .expect("create room should link parent space with the newly created room id");
        let completion_event = create_body
            .find("RoomEvent::RoomCreated")
            .expect("create room completion event");

        assert!(
            link < completion_event,
            "m.space.child must be sent using the SDK-created room id before Tauri observes RoomCreated"
        );

        let link_helper = source
            .split("async fn link_created_room_to_parent_space")
            .nth(1)
            .expect("created-room space link helper")
            .split("async fn handle_create_public_directory_room")
            .next()
            .expect("created-room space link helper body");
        assert!(
            !link_helper.contains("emit_failure"),
            "linking a created room into a parent space is best-effort; the room already exists, so it must not turn RoomCreated into a Tauri-visible failure"
        );
    }

    #[test]
    fn room_list_observation_relays_parent_only_space_links_before_projection() {
        let source = include_str!("room.rs");
        let live_body = source
            .split("async fn normalize_and_project_entries")
            .nth(1)
            .expect("live normalize helper")
            .split("async fn run_legacy_room_list_observation")
            .next()
            .expect("live normalize body");
        let legacy_body = source
            .split("async fn refresh_room_list_from_joined_rooms")
            .nth(1)
            .expect("legacy refresh helper")
            .split("async fn run_live_room_list_observation")
            .next()
            .expect("legacy refresh body");

        for body in [live_body, legacy_body] {
            let relay = body
                .find("relay_missing_space_child_links")
                .expect("room-list snapshots should relay missing m.space.child state");
            let projection = body
                .find("project_room_list_snapshot")
                .expect("room-list snapshot projection");
            assert!(
                relay < projection,
                "observation should relay missing links before projection without owning the mutation policy"
            );
            assert!(
                !body.contains("koushi_sdk::set_space_child"),
                "room-list observers must not perform server writes directly"
            );
        }
    }

    #[test]
    fn missing_space_child_repairs_are_actor_owned_and_retryable() {
        let source = include_str!("room.rs");
        let actor_body = source
            .split("async fn handle_missing_space_child_links")
            .nth(1)
            .expect("RoomActor should own missing space-child repair handling")
            .split("async fn stop_observation")
            .next()
            .expect("repair handler should precede observation teardown");

        assert!(
            source.contains("RoomMessage::MissingSpaceChildLinks"),
            "observation must relay missing links to the RoomActor mailbox"
        );
        assert!(
            actor_body.contains("classify_room_error(&error)"),
            "RoomActor-owned repair failures must be classified"
        );
        let success = actor_body
            .find("attempts.insert(key)")
            .expect("successful repair should record the dedupe key");
        let call = actor_body
            .find("koushi_sdk::set_space_child")
            .expect("RoomActor should perform the repair write");
        assert!(
            call < success,
            "dedupe key must be recorded only after set_space_child succeeds so transient failures remain retryable"
        );
    }

    #[test]
    fn room_list_projection_is_reliable_before_known_room_book_advances() {
        let source = include_str!("room.rs");
        let projection_body = source
            .split("async fn project_room_list_snapshot")
            .nth(1)
            .expect("room-list projection helper")
            .split("/// LegacySync-path refresh")
            .next()
            .expect("room-list projection body");
        let send = projection_body
            .find(".send(vec![")
            .expect("room-list projection must use reliable action delivery");
        let known = projection_body
            .find("replace_known_room_ids")
            .expect("room-list projection should update the actor known-room book");

        assert!(
            !projection_body.contains("try_send(vec!["),
            "room-list projection must not drop reducer snapshots under action-channel pressure"
        );
        assert!(
            send < known,
            "RoomActor known-room book must advance only after reducer projection delivery"
        );
    }

    #[test]
    fn directory_join_selects_room_before_room_joined_event_is_emitted() {
        let source = include_str!("room.rs");
        let join_body = source
            .split("async fn handle_join_directory_room")
            .nth(1)
            .expect("directory join handler")
            .split("async fn handle_mark_room_as_read")
            .next()
            .expect("directory join body");
        let success_reduce = join_body
            .find("AppAction::DirectoryJoinSucceeded")
            .expect("directory join success reduction");
        let joined_event = join_body
            .find("RoomEvent::RoomJoined")
            .expect("directory join completion event");

        assert!(
            success_reduce < joined_event,
            "DirectoryJoinSucceeded must select the room before Tauri observes RoomJoined"
        );
    }

    #[test]
    fn pin_success_settles_pending_before_pinned_projection_reload() {
        let source = include_str!("room.rs");
        let pin_body = source
            .split("async fn handle_pin_event")
            .nth(1)
            .expect("pin handler")
            .split("async fn handle_unpin_event")
            .next()
            .expect("pin body");
        let unpin_body = source
            .split("async fn handle_unpin_event")
            .nth(1)
            .expect("unpin handler")
            .split("async fn project_pinned_events_after_success")
            .next()
            .expect("unpin body");
        let projection_body = source
            .split("async fn project_pinned_events_after_success")
            .nth(1)
            .expect("projection helper")
            .split("    /// Refresh the room list")
            .next()
            .expect("projection body");

        let pin_completion = pin_body
            .find("self.reduce_reliable(vec![AppAction::PinEventCompleted")
            .expect("pin completion action");
        let pin_reload = pin_body
            .find("project_pinned_events_after_success")
            .expect("pin projection reload");
        assert!(pin_completion < pin_reload);

        let unpin_completion = unpin_body
            .find("self.reduce_reliable(vec![AppAction::UnpinEventCompleted")
            .expect("unpin completion action");
        let unpin_reload = unpin_body
            .find("project_pinned_events_after_success")
            .expect("unpin projection reload");
        assert!(unpin_completion < unpin_reload);

        assert!(!projection_body.contains("AppAction::PinEventCompleted"));
        assert!(!projection_body.contains("AppAction::UnpinEventCompleted"));
    }

    #[test]
    fn pinned_raw_projection_preserves_event_order_metadata_and_thread_relation() {
        let event = pinned_event_from_raw(
            "$fallback:example.invalid".to_owned(),
            r#"{
                "event_id":"$reply:example.invalid",
                "sender":"@bob:example.invalid",
                "origin_server_ts":1800000000000,
                "type":"m.room.message",
                "content":{
                    "msgtype":"m.text",
                    "body":"Pinned reply",
                    "m.relates_to":{"rel_type":"m.thread","event_id":"$root:example.invalid"}
                }
            }"#,
        );

        assert_eq!(event.event_id, "$reply:example.invalid");
        assert_eq!(event.sender.as_deref(), Some("@bob:example.invalid"));
        assert_eq!(event.timestamp_ms, Some(1_800_000_000_000));
        assert_eq!(event.body_preview.as_deref(), Some("Pinned reply"));
        assert_eq!(
            event.thread_root_event_id.as_deref(),
            Some("$root:example.invalid")
        );
        assert_eq!(event.state, PinnedEventState::Ready);
    }

    #[test]
    fn pin_and_unpin_commands_require_actor_known_room_guard_before_sdk_call() {
        let source = include_str!("room.rs");
        let pin_body = source
            .split("async fn handle_pin_event")
            .nth(1)
            .expect("pin handler")
            .split("async fn handle_unpin_event")
            .next()
            .expect("pin body");
        let unpin_body = source
            .split("async fn handle_unpin_event")
            .nth(1)
            .expect("unpin handler")
            .split("async fn project_pinned_events_after_success")
            .next()
            .expect("unpin body");

        let pin_guard = pin_body
            .find("ensure_known_room_for_message_interaction")
            .expect("pin known-room guard");
        let pin_sdk = pin_body
            .find("koushi_sdk::pin_event")
            .expect("pin sdk call");
        assert!(pin_guard < pin_sdk);

        let unpin_guard = unpin_body
            .find("ensure_known_room_for_message_interaction")
            .expect("unpin known-room guard");
        let unpin_sdk = unpin_body
            .find("koushi_sdk::unpin_event")
            .expect("unpin sdk call");
        assert!(unpin_guard < unpin_sdk);
    }

    // --- request_id correlation on RoomEvents ---

    #[test]
    fn room_event_carries_request_id() {
        let request_id = make_request_id(10);
        let event = RoomEvent::RoomCreated {
            request_id,
            room_id: "!room:example.test".to_owned(),
        };
        match event {
            RoomEvent::RoomCreated {
                request_id: ev_id, ..
            } => assert_eq!(ev_id, request_id),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    // --- Observation lifecycle messages without a session are safe ---

    #[tokio::test]
    async fn session_lifecycle_messages_without_session_complete_cleanly() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, _event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(action_tx, event_tx);

        // No session, no observation loop: both must be no-ops, and the
        // actor task must still exit on Shutdown.
        assert!(handle.send(RoomMessage::SyncStopped).await);
        assert!(handle.send(RoomMessage::SessionCleared).await);
        assert!(handle.send(RoomMessage::Shutdown).await);
        tokio::time::timeout(std::time::Duration::from_secs(5), handle.join())
            .await
            .expect("actor task must exit after Shutdown");
    }

    // --- Normalization empty snapshot ---

    #[test]
    fn normalize_empty_snapshot() {
        let snapshot = MatrixRoomListSnapshot::default();
        assert!(normalize_spaces(&snapshot).is_empty());
        assert!(normalize_rooms(&snapshot).is_empty());
    }

    #[test]
    fn space_members_projection_load_path_emits_non_empty_child_profile_observations() {
        let raw = MatrixSpaceMembersProjection {
            space_id: "!space:example.invalid".to_owned(),
            child_room_ids: vec!["!child:example.invalid".to_owned()],
            space_joined: Vec::new(),
            space_invited: Vec::new(),
            child_room_only: vec![MatrixSpaceMemberEntry {
                user_id: "@child:example.invalid".to_owned(),
                display_name: Some("Child room profile".to_owned()),
                avatar_url: None,
                power_level: Some(0),
                role: MatrixRoomMemberRole::User,
                child_room_ids: vec!["!child:example.invalid".to_owned()],
            }],
            child_room_profiles: Vec::new(),
            space_joined_input_count: 0,
            space_invited_input_count: 0,
            child_join_input_count: 1,
            child_join_union_count: 1,
            duplicate_child_membership_count: 0,
            child_room_count: 1,
            complete_child_room_count: 1,
            incomplete_child_room_count: 0,
        };

        let profiles = user_profiles_from_space_projection(&raw);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].user_id, "@child:example.invalid");
        assert_eq!(
            profiles[0].display_name.as_deref(),
            Some("Child room profile")
        );

        let projection = state_space_members_projection(raw, 4);
        assert_eq!(
            projection.child_room_only[0].display_name.as_deref(),
            Some("Child room profile")
        );
    }

    #[test]
    fn space_member_load_failure_does_not_construct_an_empty_projection() {
        let source = include_str!("room.rs");
        let failure_path = source
            .split("async fn handle_load_space_members")
            .nth(1)
            .expect("Space load error branch exists")
            .split("async fn handle_invite_user_to_space")
            .next()
            .expect("Space load handler boundary exists")
            .split("Err(error) =>")
            .nth(1)
            .expect("Space load error branch exists")
            .split("self.reduce_reliable")
            .next()
            .expect("Space load failure must reduce a structured failure action");

        assert!(
            !failure_path.contains("SpaceMembersProjection {"),
            "a failed Space lookup must not be represented by an empty projection"
        );
        assert!(
            failure_path.contains("record_core_space_members_load_failure"),
            "core failure diagnostics must preserve unavailable-count semantics"
        );
    }

    #[test]
    fn background_space_member_lookup_failure_preserves_state_and_only_records_diagnostic() {
        let source = include_str!("room.rs");
        let failure_path = source
            .split("async fn handle_space_members_projection_refreshed")
            .nth(1)
            .expect("background refresh handler exists")
            .split("async fn handle_invite_user_to_space")
            .next()
            .expect("background refresh handler boundary exists")
            .split("Err(_error) =>")
            .nth(1)
            .expect("background lookup failure branch exists");

        assert!(failure_path.contains("record_core_space_members_load_failure"));
        assert!(!failure_path.contains("SpaceMembersBackgroundProjectionReconciled"));
        assert!(!failure_path.contains("SpaceMembersLoadFailed"));
    }

    #[test]
    fn cancel_space_invite_reconciles_a_fresh_projection_before_settling() {
        let source = include_str!("room.rs");
        let handler = source
            .split("async fn handle_cancel_space_invite")
            .nth(1)
            .expect("Space invite cancellation handler exists")
            .split("async fn handle_invite_targets")
            .next()
            .expect("Space invite cancellation handler boundary exists");
        let sdk_call = handler
            .find("koushi_sdk::cancel_space_invite")
            .expect("core must call the SDK cancellation helper");
        let reconcile = handler
            .find("reconcile_space_invite_cancellation")
            .expect("core must request a fresh Space projection");
        let settlement = handler
            .find("SpaceMemberInviteCancellationSettled")
            .expect("core must settle the cancellation action");
        assert!(sdk_call < reconcile);
        assert!(reconcile < settlement);

        let reconciliation = source
            .split("async fn reconcile_space_invite_cancellation")
            .nth(1)
            .expect("cancellation reconciliation helper exists")
            .split("fn record_core_space_members_projection")
            .next()
            .expect("cancellation reconciliation helper boundary exists");
        assert!(reconciliation.contains("koushi_sdk::matrix_space_members_projection"));
    }

    #[test]
    fn failed_space_member_diagnostics_do_not_fabricate_member_counts() {
        let before = koushi_diagnostics::snapshot().records.len();
        record_core_space_members_load_failure("sync_refresh", 7);
        let record = koushi_diagnostics::snapshot()
            .records
            .into_iter()
            .skip(before)
            .find(|record| {
                record.event.source == "core.space_members_projection"
                    && record.event.fields.iter().any(|field| {
                        field.key == "outcome"
                            && field.value
                                == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
                    })
            })
            .expect("Space load failure diagnostic");

        assert!(record.event.fields.iter().any(|field| {
            field.key == "outcome"
                && field.value == koushi_diagnostics::DiagnosticValue::Token("lookup_failed")
        }));
        for field in &record.event.fields {
            if matches!(
                field.key,
                "space_joined_count"
                    | "space_invited_count"
                    | "child_room_count"
                    | "child_room_only_count"
                    | "input_count"
                    | "output_count"
            ) {
                assert_ne!(
                    field.value,
                    koushi_diagnostics::DiagnosticValue::Count(0),
                    "failed Space diagnostics must not report member counts as zero"
                );
            }
        }
    }

    #[test]
    fn core_space_members_diagnostics_are_private_data_free() {
        let projection = SpaceMembersProjection {
            space_id: "!private:example.invalid".to_owned(),
            generation: 4,
            space_joined: vec![SpaceMemberEntry {
                user_id: "@alice:example.invalid".to_owned(),
                display_name: Some("Alice private".to_owned()),
                display_label: "Alice private".to_owned(),
                original_display_label: "Alice private".to_owned(),
                avatar_url: Some("mxc://example.invalid/avatar".to_owned()),
                power_level: Some(100),
                role: RoomMemberRole::Administrator,
                membership: SpaceMemberMembership::SpaceJoined,
                child_room_ids: Vec::new(),
                invite_pending: false,
            }],
            space_invited: Vec::new(),
            child_room_only: Vec::new(),
            child_room_count: 0,
            complete_child_room_count: 0,
            incomplete_child_room_count: 0,
        };
        record_core_space_members_projection("load", 4, &projection, "success");
        record_core_profile_resolution(&projection);

        let snapshot = koushi_diagnostics::snapshot();
        let encoded = serde_json::to_string(&snapshot).expect("diagnostics serialize");
        assert!(!encoded.contains("@alice:example.invalid"));
        assert!(!encoded.contains("Alice private"));
        assert!(!encoded.contains("mxc://example.invalid/avatar"));
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.space_members_projection")
        );
        assert!(
            snapshot
                .records
                .iter()
                .any(|record| record.event.source == "core.profile_resolution")
        );
    }
}
