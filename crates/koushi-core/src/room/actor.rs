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
//! for space classification when a homeserver omits the required room state).
//!
//! `RoomMessage::SyncStarted` carries the ONE live `RoomListService` owned by
//! the running `SyncService` (`sync_service.room_list_service()`). The actor
//! subscribes to its `all_rooms()` entries stream
//! (`entries_with_dynamic_adapters` with the non-left filter) and KEEPS
//! CONSUMING it, re-normalizing rooms and invites on each diff batch (Async
//! rule 1: actors relay the SDK's observable streams).
//!
//! Snapshots are projected as generation-fenced room-list bootstrap actions +
//! `RoomEvent::RoomListUpdated`.
//!
//! Operation-triggered refreshes after the actor's own mutations mean
//! "re-normalize from the live service's current entries" (a refresh request
//! to the observation loop), never "new service". Before sync starts, cached
//! rows remain reducer-owned; RoomActor does not synthesize a live snapshot
//! from the base client.
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
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "test-hooks")]
use std::sync::{Mutex, atomic::AtomicUsize};

use futures_util::FutureExt;
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
    DirectoryRoomSummary, EncryptionDebugOperationKind, EncryptionDebugOperationOutcome,
    INVITE_ALREADY_IN_SPACE_MESSAGE, InviteDestination, InviteDestinationResult,
    InviteDestinationResultKind, InvitePreview, InviteScopeSelection,
    MentionCandidatesCompleteness, MentionCandidatesFailureKind, MentionSurface,
    OperationFailureKind, PinnedEvent, PinnedEventState, RoomHistoryVisibility, RoomJoinRule,
    RoomListFailureKind, RoomListSource, RoomMemberRole, RoomMemberSummary, RoomMentionPermission,
    RoomModerationAction, RoomNotificationMode, RoomPermissionFacts, RoomSettingChange,
    RoomSettingsSnapshot, RoomSummary, RoomTagInfo, RoomTagKind, RoomTags, SpaceMemberEntry,
    SpaceMemberInviteOutcome, SpaceMemberMembership, SpaceMembersProjection, SpaceSummary,
    UserProfile, UserTrustState,
};
#[cfg(test)]
use koushi_state::{ProfileResolutionInput, ProfileResolutionSource, resolve_people_label};
use matrix_sdk::ruma::events::direct::DirectEvent;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::account_work::{AccountWorkKind, AccountWorkScheduler};
use crate::command::{CreateRoomOptions, CreateRoomVisibility, RoomCommand};
use crate::direct_message_classification::{DirectAccountDataSource, DirectClassificationState};
use crate::event::{
    CoreEvent, EncryptionDebugOperationOutcome as CoreEncryptionDebugOutcome, ReportKind,
    RoomEvent, RoomKeyReshareOutcome,
};
use crate::executor;
use crate::failure::{CoreFailure, RoomFailureKind};
use crate::ids::{RequestId, RuntimeConnectionId};
use crate::mention_candidates::{MentionMemberInput, project_candidates};
use crate::timeline::{
    RoomMembershipTransition, RoomMembershipTransitionKind, RoomRemovalCause,
    TimelineSubscriptionResidencyHandle, TimelineSubscriptionResidencyPermit,
    VisibleRoomObservation,
};
use crate::unread_trace;

/// Fixed, content-free messages recorded in `AppState.errors` when a basic
/// operation fails. Raw SDK errors are classified into `RoomFailureKind` for the
/// transport `OperationFailed` event and never placed in product state.
const CREATE_ROOM_FAILED_MESSAGE: &str = "Room creation failed";
const CREATE_SPACE_FAILED_MESSAGE: &str = "Space creation failed";
const LINK_SPACE_CHILD_FAILED_MESSAGE: &str = "Linking the room to the space failed";

type SpaceChildLinkKey = (String, String);

fn room_key_reshare_outcome_from_sdk(
    outcome: koushi_sdk::MatrixRoomKeyReshareOutcome,
) -> RoomKeyReshareOutcome {
    match outcome {
        koushi_sdk::MatrixRoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        } => RoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        },
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoSession => RoomKeyReshareOutcome::NoSession,
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoRecipients => {
            RoomKeyReshareOutcome::NoRecipients
        }
        koushi_sdk::MatrixRoomKeyReshareOutcome::StaleSession => {
            RoomKeyReshareOutcome::StaleSession
        }
    }
}

fn record_manual_room_key_reshare(outcome: &koushi_sdk::MatrixRoomKeyReshareOutcome) {
    let (token, request_count, recipient_count, failed_recipient_count) = match outcome {
        koushi_sdk::MatrixRoomKeyReshareOutcome::Sent {
            request_count,
            recipient_count,
            failed_recipient_count,
        } => (
            "sent",
            *request_count,
            *recipient_count,
            *failed_recipient_count,
        ),
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoSession => ("no_session", 0, 0, 0),
        koushi_sdk::MatrixRoomKeyReshareOutcome::NoRecipients => ("no_recipients", 0, 0, 0),
        koushi_sdk::MatrixRoomKeyReshareOutcome::StaleSession => ("cancelled", 0, 0, 0),
    };
    record(
        DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_reshare", "attempt")
            .field(DiagnosticField::token("trigger", "manual"))
            .field(DiagnosticField::token("outcome", token))
            .field(DiagnosticField::count(
                "request_count",
                request_count.try_into().unwrap_or(u64::MAX),
            ))
            .field(DiagnosticField::count(
                "recipient_count",
                recipient_count.try_into().unwrap_or(u64::MAX),
            ))
            .field(DiagnosticField::count(
                "failed_recipient_count",
                failed_recipient_count.try_into().unwrap_or(u64::MAX),
            )),
    );
}

const SPACE_MEMBER_REFRESH_CONNECTION_ID: RuntimeConnectionId = RuntimeConnectionId(0);
const ROOM_ACTOR_SHUTDOWN_SEND_TIMEOUT: Duration = Duration::from_secs(1);
// Long enough to cover the SDK encryption-debug operations' 10s monotonic
// deadline plus inline settlement/reset, so Shutdown never aborts the actor
// mid-join of an encryption-debug fence (issue #538).
const ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(30);
const ROOM_OBSERVATION_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

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
    #[cfg(feature = "test-hooks")]
    TestCommand {
        command: RoomCommand,
        processed: oneshot::Sender<()>,
    },
    /// A store-backed session was established (login/restore/switch).
    /// Enables room operations; does NOT start the room-list observation —
    /// that starts on `SyncStarted` when its live `RoomListService` is known.
    SessionEstablished { session: Arc<MatrixClientSession> },
    /// Sync started. Sent by `SyncActor` after the backend is launched.
    /// `room_list_service` is the ONE live service owned by the running
    /// `SyncService`. Ad-hoc `RoomListService` instances are prohibited.
    SyncStarted {
        session: Arc<MatrixClientSession>,
        room_list_service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
        source: RoomListSource,
        backend_generation: u64,
    },
    /// A committed response must not become connected until the matching
    /// response has been projected reliably. Empty accounts may omit the SDK
    /// room count, so that response is treated as a complete empty projection.
    ReconcileCommittedRange {
        source: RoomListSource,
        backend_generation: u64,
        response_sequence: u64,
        ack: oneshot::Sender<RoomListReconcileAck>,
    },
    /// Re-project the current entries after a committed Sliding Sync response.
    /// The source remains the single live `RoomListService`; this wake only
    /// closes the response/store publication window for membership changes.
    RefreshCommittedProjection {
        source: RoomListSource,
        backend_generation: u64,
    },
    /// Re-project after the base client reports an invite/leave membership
    /// update. The dynamic room-list stream can observe that update before
    /// the client room store publishes the corresponding room, so the actor
    /// performs the same bounded live-service refresh used by local room
    /// mutations.
    RefreshMembershipProjection {
        source: RoomListSource,
        room_generation: u64,
    },
    /// Authoritative room-list removal invalidated in-flight room operations.
    AuthoritativeRoomsRemoved { room_ids: BTreeSet<String> },
    /// Stop only the observation owned by this runtime generation and
    /// acknowledge after its task has joined.
    StopSyncObservation {
        backend_generation: u64,
        ack: oneshot::Sender<()>,
    },
    /// A backend task ended. The source/generation fence prevents a delayed
    /// stop from failing a replacement backend that already started.
    BackendSyncStopped {
        source: RoomListSource,
        backend_generation: u64,
    },
    /// The active account is logging out/switching/resetting while the
    /// RoomActor stays alive for future sessions. The oneshot acknowledges
    /// that the actor completed the encryption-debug cancel/join/settle
    /// sequence before clearing the session (issue #538).
    SessionCleared { ack: oneshot::Sender<()> },
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
    #[cfg(feature = "test-hooks")]
    TestVisibleRoomsObserved {
        core_generation: u64,
        reconciliation_is_complete: bool,
        room_ids: Vec<VisibleRoomObservation>,
        forwarded: oneshot::Sender<bool>,
    },
    #[cfg(feature = "test-hooks")]
    TestMembershipObserved {
        core_generation: u64,
        transitions: Vec<RoomMembershipTransition>,
        forwarded: oneshot::Sender<bool>,
    },
    #[cfg(feature = "test-hooks")]
    TestKnownRooms {
        room_ids: BTreeSet<String>,
        forwarded: oneshot::Sender<()>,
    },
    #[cfg(test)]
    InspectObservationGeneration {
        response: oneshot::Sender<Option<u64>>,
    },
    /// Ordered shutdown.
    Shutdown,
}

/// Closed membership-operation kinds used by the test-only result seam.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoomOperationKind {
    LeaveRoom,
    DeclineInvite,
    AcceptInvite,
    JoinRoom,
    JoinDirectoryRoom,
}

#[cfg(feature = "test-hooks")]
pub(crate) struct RoomOperationTestControl {
    pub(crate) kind: RoomOperationKind,
    pub(crate) reached: oneshot::Sender<()>,
    pub(crate) completion: oneshot::Receiver<Result<String, MatrixRoomOperationError>>,
}

#[cfg(feature = "test-hooks")]
pub(crate) struct EncryptionDebugTestControl {
    pub(crate) kind: EncryptionDebugOperationKind,
    pub(crate) reached: oneshot::Sender<()>,
    pub(crate) completion: oneshot::Receiver<CoreEncryptionDebugOutcome>,
}

#[cfg(feature = "test-hooks")]
type EncryptionDebugTestControlSlot = Arc<Mutex<Option<EncryptionDebugTestControl>>>;

#[cfg(feature = "test-hooks")]
fn take_encryption_debug_test_control(
    control: &mut Option<EncryptionDebugTestControl>,
    kind: EncryptionDebugOperationKind,
) -> Option<EncryptionDebugTestControl> {
    if control.as_ref().is_some_and(|control| control.kind == kind) {
        control.take()
    } else {
        None
    }
}

#[cfg(feature = "test-hooks")]
fn take_matching_room_operation_test_control(
    control: &mut Option<RoomOperationTestControl>,
    kind: RoomOperationKind,
) -> Option<RoomOperationTestControl> {
    if control.as_ref().is_some_and(|control| control.kind == kind) {
        control.take()
    } else {
        None
    }
}

#[cfg(feature = "test-hooks")]
type RoomOperationTestControlSlot = Arc<Mutex<Option<RoomOperationTestControl>>>;

#[cfg(all(test, feature = "test-hooks"))]
#[test]
fn room_operation_test_control_matches_and_consumes_once() {
    let (reached, _reached_rx) = oneshot::channel();
    let (_completion, completion_rx) = oneshot::channel();
    let mut control = Some(RoomOperationTestControl {
        kind: RoomOperationKind::LeaveRoom,
        reached,
        completion: completion_rx,
    });

    assert!(
        take_matching_room_operation_test_control(&mut control, RoomOperationKind::JoinRoom,)
            .is_none()
    );
    assert!(control.is_some());
    assert!(
        take_matching_room_operation_test_control(&mut control, RoomOperationKind::LeaveRoom,)
            .is_some()
    );
    assert!(
        take_matching_room_operation_test_control(&mut control, RoomOperationKind::LeaveRoom,)
            .is_none()
    );
}

#[derive(Clone)]
struct TimelineResidencyBinding {
    session: Arc<MatrixClientSession>,
    handle: TimelineSubscriptionResidencyHandle,
}

/// Handle to the RoomActor background task (owned by AccountActor).
pub struct RoomActorHandle {
    pub(crate) tx: mpsc::Sender<RoomMessage>,
    timeline_residency: watch::Sender<Option<TimelineResidencyBinding>>,
    session: watch::Sender<Option<Arc<MatrixClientSession>>>,
    #[cfg(feature = "test-hooks")]
    room_operation_test_control: RoomOperationTestControlSlot,
    #[cfg(feature = "test-hooks")]
    room_operation_test_reached_count: Arc<AtomicUsize>,
    #[cfg(feature = "test-hooks")]
    encryption_debug_test_control: EncryptionDebugTestControlSlot,
    task: Option<executor::JoinHandle<()>>,
}

impl RoomActorHandle {
    pub(crate) fn bind_timeline_residency(
        &self,
        session: Arc<MatrixClientSession>,
        handle: TimelineSubscriptionResidencyHandle,
    ) {
        self.timeline_residency
            .send_replace(Some(TimelineResidencyBinding { session, handle }));
    }

    pub(crate) fn clear_timeline_residency(&self) {
        self.timeline_residency.send_replace(None);
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn operation_test_reached_count(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.room_operation_test_reached_count)
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn install_room_operation_test_control(
        &self,
        control: RoomOperationTestControl,
    ) -> bool {
        let mut slot = self
            .room_operation_test_control
            .lock()
            .expect("room operation test control lock");
        if slot.is_some() {
            return false;
        }
        *slot = Some(control);
        true
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn install_encryption_debug_test_control(
        &self,
        control: EncryptionDebugTestControl,
    ) -> bool {
        let mut slot = self
            .encryption_debug_test_control
            .lock()
            .expect("encryption-debug test control lock");
        if slot.is_some() {
            return false;
        }
        *slot = Some(control);
        true
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) async fn install_known_rooms_for_test(&self, room_ids: BTreeSet<String>) -> bool {
        let (forwarded_tx, forwarded_rx) = oneshot::channel();
        if !self
            .send(RoomMessage::TestKnownRooms {
                room_ids,
                forwarded: forwarded_tx,
            })
            .await
        {
            return false;
        }
        forwarded_rx.await.is_ok()
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn timeline_residency_snapshot(
        &self,
    ) -> Option<(
        Arc<MatrixClientSession>,
        TimelineSubscriptionResidencyHandle,
    )> {
        self.timeline_residency
            .borrow()
            .as_ref()
            .map(|binding| (binding.session.clone(), binding.handle.clone()))
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn session_snapshot(&self) -> Option<Arc<MatrixClientSession>> {
        self.session.borrow().clone()
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) async fn wait_for_session(&self, expected: &Arc<MatrixClientSession>) -> bool {
        let mut session = self.session.subscribe();
        session
            .wait_for(|current| {
                current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, expected))
            })
            .await
            .is_ok()
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) async fn room_subscription_residency_test_observe_visible(
        &self,
        core_generation: u64,
        reconciliation_is_complete: bool,
        room_ids: Vec<VisibleRoomObservation>,
    ) -> bool {
        let (forwarded_tx, forwarded_rx) = oneshot::channel();
        if !self
            .send(RoomMessage::TestVisibleRoomsObserved {
                core_generation,
                reconciliation_is_complete,
                room_ids,
                forwarded: forwarded_tx,
            })
            .await
        {
            return false;
        }
        forwarded_rx.await.unwrap_or(false)
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) async fn room_subscription_residency_test_observe_membership(
        &self,
        core_generation: u64,
        transitions: Vec<RoomMembershipTransition>,
    ) -> bool {
        let (forwarded_tx, forwarded_rx) = oneshot::channel();
        if !self
            .send(RoomMessage::TestMembershipObserved {
                core_generation,
                transitions,
                forwarded: forwarded_tx,
            })
            .await
        {
            return false;
        }
        forwarded_rx.await.unwrap_or(false)
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn clear_room_operation_test_control(&self) {
        self.room_operation_test_control
            .lock()
            .expect("room operation test control lock")
            .take();
    }

    pub async fn send(&self, msg: RoomMessage) -> bool {
        self.tx.send(msg).await.is_ok()
    }

    /// Wait for the actor task to complete (used in ordered shutdown).
    pub async fn shutdown(&mut self) -> bool {
        self.shutdown_with_timeouts(
            ROOM_ACTOR_SHUTDOWN_SEND_TIMEOUT,
            ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT,
        )
        .await
    }

    async fn shutdown_with_timeouts(
        &mut self,
        send_timeout: Duration,
        join_timeout: Duration,
    ) -> bool {
        let sent = matches!(
            executor::timeout(send_timeout, self.tx.send(RoomMessage::Shutdown)).await,
            Ok(Ok(()))
        );
        let Some(mut task) = self.task.take() else {
            return sent;
        };
        if sent && executor::timeout(join_timeout, &mut task).await.is_ok() {
            return true;
        }
        task.abort();
        let _ = task.await;
        false
    }

    pub async fn join(mut self) {
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

/// Handle on the spawned room-list observation loop: oneshot stop signal plus
/// the task handle so teardown can await completion. Operation-triggered
/// refreshes are always sent to the observation loop so command handling never
    session: Option<Arc<MatrixClientSession>>,
    timeline_residency: watch::Receiver<Option<TimelineResidencyBinding>>,
    session_slot: watch::Sender<Option<Arc<MatrixClientSession>>>,
    #[cfg(feature = "test-hooks")]
    room_operation_test_control: RoomOperationTestControlSlot,
    #[cfg(feature = "test-hooks")]
    room_operation_test_reached_count: Arc<AtomicUsize>,
    #[cfg(feature = "test-hooks")]
    encryption_debug_test_control: EncryptionDebugTestControlSlot,
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
    /// In-flight temporary dangerous encryption-debug operations (issue
    /// #538): at most one per room, keyed by room id and fenced by request
    /// id. A start is rejected only when that same room already has an
    /// in-flight operation.
    encryption_debug_fences: std::collections::HashMap<String, EncryptionDebugFence>,
    /// Reliable nonblocking completion ingress for the encryption-debug
    /// operation task (issue #538). Unbounded so the join during teardown
    /// cannot deadlock on a full mailbox, and lossless so the reducer never
    /// stays pending.
    encryption_debug_completion_rx: mpsc::UnboundedReceiver<EncryptionDebugCompletion>,
    encryption_debug_completion_tx: mpsc::UnboundedSender<EncryptionDebugCompletion>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    sliding_sync_diagnostics: crate::SlidingSyncDiagnostics,
    self_tx: mpsc::Sender<RoomMessage>,
    command_rx: mpsc::Receiver<RoomMessage>,
    account_work: AccountWorkScheduler,
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

/// Fence for the in-flight temporary dangerous encryption-debug operation
/// (issue #538). Holds the cancellation sender (so logout/leave can stop
/// the SDK executor's wire effects), the actor session snapshot (post-check
/// fails closed if the session changed), and the spawned task handle for
/// bounded join on teardown.
struct EncryptionDebugFence {
    request_id: RequestId,
    room_id: String,
    kind: EncryptionDebugOperationKind,
    session: Arc<koushi_sdk::MatrixClientSession>,
    cancel: broadcast::Sender<()>,
    /// Actor-owned lifecycle flag: set on logout/leave so the spawned task's
    /// validator fails closed before further wire effects.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    join: executor::JoinHandle<()>,
}

/// Reliable completion result of the encryption-debug operation task.
struct EncryptionDebugCompletion {
    room_id: String,
    request_id: RequestId,
    kind: EncryptionDebugOperationKind,
    outcome: CoreEncryptionDebugOutcome,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct SpaceMemberRefreshFence {
    request_id: RequestId,
    session_generation: u64,
    demand_generation: u64,
    refresh_generation: u64,
}

struct AdmittedRoomOperation {
    session: Arc<MatrixClientSession>,
    residency: TimelineSubscriptionResidencyHandle,
    permit: TimelineSubscriptionResidencyPermit,
}

impl AdmittedRoomOperation {
    async fn room_left(&self, room_id: &str, cause: RoomRemovalCause) -> bool {
        let Ok(room_id) = room_id.parse() else {
            return false;
        };
        self.residency.room_left(&self.permit, room_id, cause).await
    }

    async fn room_rejoined(&self, room_id: &str) -> bool {
        let Ok(room_id) = room_id.parse() else {
            return false;
        };
        self.residency.room_rejoined(&self.permit, room_id).await
    }
}

impl RoomActor {

impl RoomActor {
    pub fn spawn(
        action_tx: mpsc::Sender<Vec<AppAction>>,
        event_tx: broadcast::Sender<CoreEvent>,
        sliding_sync_diagnostics: crate::SlidingSyncDiagnostics,
    ) -> RoomActorHandle {
        Self::spawn_with_account_work(
            action_tx,
            event_tx,
            sliding_sync_diagnostics,
            AccountWorkScheduler::default(),
        )
    }
}

impl RoomActor {
    pub(crate) fn spawn_with_account_work(
        action_tx: mpsc::Sender<Vec<AppAction>>,
        event_tx: broadcast::Sender<CoreEvent>,
        sliding_sync_diagnostics: crate::SlidingSyncDiagnostics,
        account_work: AccountWorkScheduler,
    ) -> RoomActorHandle {
        let (tx, command_rx) = mpsc::channel(crate::runtime::ACTOR_MESSAGE_QUEUE_CAPACITY);
        let (encryption_debug_completion_tx, encryption_debug_completion_rx) =
            mpsc::unbounded_channel::<EncryptionDebugCompletion>();
        let (timeline_residency, timeline_residency_rx) = watch::channel(None);
        let (session_slot, _session_rx) = watch::channel(None);
        #[cfg(feature = "test-hooks")]
        let room_operation_test_control = Arc::new(Mutex::new(None));
        #[cfg(feature = "test-hooks")]
        let room_operation_test_reached_count = Arc::new(AtomicUsize::new(0));
        #[cfg(feature = "test-hooks")]
        let encryption_debug_test_control = Arc::new(Mutex::new(None));
        let actor = RoomActor {
            session: None,
            timeline_residency: timeline_residency_rx,
            session_slot: session_slot.clone(),
            #[cfg(feature = "test-hooks")]
            room_operation_test_control: room_operation_test_control.clone(),
            #[cfg(feature = "test-hooks")]
            room_operation_test_reached_count: room_operation_test_reached_count.clone(),
            #[cfg(feature = "test-hooks")]
            encryption_debug_test_control: encryption_debug_test_control.clone(),
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
            encryption_debug_fences: std::collections::HashMap::new(),
            encryption_debug_completion_rx,
            encryption_debug_completion_tx: encryption_debug_completion_tx.clone(),
            action_tx,
            event_tx,
            sliding_sync_diagnostics,
            self_tx: tx.clone(),
            command_rx,
            account_work,
        };
        let task = executor::spawn(actor.run());
        RoomActorHandle {
            tx,
            timeline_residency,
            session: session_slot,
            #[cfg(feature = "test-hooks")]
            room_operation_test_control,
            #[cfg(feature = "test-hooks")]
            room_operation_test_reached_count,
            #[cfg(feature = "test-hooks")]
            encryption_debug_test_control,
            task: Some(task),
        }
    }
}

impl RoomActor {
    async fn run(mut self) {
        loop {
            let msg = tokio::select! {
                msg = self.command_rx.recv() => match msg {
                    Some(msg) => msg,
                    None => break,
                },
                completion = self.encryption_debug_completion_rx.recv() => {
                    let Some(completion) = completion else { continue };
                    self.handle_encryption_debug_completion(completion).await;
                    continue;
                }
            };
            match msg {
                RoomMessage::Shutdown => {
                    // Cancel and join every in-flight encryption-debug
                    // operation to completion (no abort) before the actor
                    // exits (issue #538), then settle CancelledStale and
                    // reset the state machine so no reducer entry is left
                    // pending.
                    let room_ids = self
                        .encryption_debug_fences
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    self.cancel_encryption_debug_for_rooms(&room_ids).await;
                    self.stop_observation().await;
                    break;
                }
                RoomMessage::Command(command) => {
                    self.handle_command(command).await;
                }
                #[cfg(feature = "test-hooks")]
                RoomMessage::TestCommand { command, processed } => {
                    self.handle_command(command).await;
                    let _ = processed.send(());
                }
                RoomMessage::SessionEstablished { session } => {
                    // Room operations become available; observation starts
                    // later on SyncStarted (backend then known).
                    self.reset_space_member_session();
                    self.session = Some(session.clone());
                    self.session_slot.send_replace(Some(session));
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
                    self.session_slot.send_replace(Some(session.clone()));
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
                    // Its first diff batch (Reset with the current entries)
                    // provides the initial snapshot, so no separate initial
                    // refresh is needed. Cached rows remain available until
                    // that generation settles.
                    let timeline_residency = self
                        .timeline_residency
                        .borrow()
                        .as_ref()
                        .filter(|binding| Arc::ptr_eq(&binding.session, &session))
                        .map(|binding| binding.handle.clone());
                    self.start_live_observation(
                        session,
                        room_list_service,
                        self.room_list_generation,
                        source,
                        timeline_residency,
                    );
                }
                RoomMessage::ReconcileCommittedRange {
                    source,
                    backend_generation,
                    response_sequence,
                    ack,
                } => {
                    if self.room_list_source == Some(source)
                        && self.room_list_backend_generation == Some(backend_generation)
                        && let Some(observation) = &self.observation
                        && observation.source == source
                        && observation.generation == self.room_list_generation
                    {
                        let _ = observation
                            .command_tx
                            .send(RoomListObservationCommand::Reconcile {
                                backend_generation,
                                response_sequence,
                                ack,
                            })
                            .await;
                    }
                }
                RoomMessage::RefreshCommittedProjection {
                    source,
                    backend_generation,
                } => {
                    if self.room_list_source == Some(source)
                        && self.room_list_backend_generation == Some(backend_generation)
                        && let Some(observation) = &self.observation
                        && observation.source == source
                        && observation.generation == self.room_list_generation
                    {
                        // This is a single post-commit wake. Operation
                        // refreshes use bounded retries because the local
                        // mutation may settle asynchronously; a sync response
                        // must not spawn retry work for every response.
                        let _ = observation
                            .command_tx
                            .try_send(RoomListObservationCommand::Refresh);
                    }
                }
                RoomMessage::RefreshMembershipProjection {
                    source,
                    room_generation,
                } => {
                    if self.room_list_source == Some(source)
                        && self.room_list_generation == room_generation
                    {
                        // Membership updates are sparse and can precede the
                        // SDK room-store publication. Use the existing
                        // bounded refresh path so the single live service is
                        // re-read after that publication window closes.
                        self.refresh_room_list();
                    }
                }
                RoomMessage::AuthoritativeRoomsRemoved { room_ids } => {
                    self.cancel_encryption_debug_for_rooms(&room_ids).await;
                }
                RoomMessage::StopSyncObservation {
                    backend_generation,
                    ack,
                } => {
                    if room_stop_matches_generation(
                        self.room_list_backend_generation,
                        backend_generation,
                    ) {
                        self.stop_observation().await;
                        self.reset_space_member_session();
                        self.clear_known_rooms();
                        self.clear_space_child_repair_attempts();
                        self.room_list_source = None;
                        self.room_list_backend_generation = None;
                    }
                    let _ = ack.send(());
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
                RoomMessage::SessionCleared { ack } => {
                    // Cancel and join every in-flight encryption-debug
                    // operation to completion before clearing the session
                    // (issue #538): the SDK executor stops at the next
                    // wire-effect boundary and runs cleanup before the task
                    // returns; we never detach it (the operation is bounded
                    // by its monotonic deadline and the completion lane is
                    // nonblocking, so the join cannot deadlock).
                    let room_ids = self
                        .encryption_debug_fences
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    // Settle CancelledStale before clearing the session; the
                    // helper also resets each room's reducer state.
                    self.cancel_encryption_debug_for_rooms(&room_ids).await;
                    self.stop_observation().await;
                    self.reset_space_member_session();
                    self.session = None;
                    self.session_slot.send_replace(None);
                    self.clear_known_rooms();
                    self.clear_space_child_repair_attempts();
                    self.clear_mention_candidates();
                    // Acknowledge the teardown so the account actor can
                    // proceed with session teardown only after the
                    // encryption-debug operation was cancelled and settled.
                    let _ = ack.send(());
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
                #[cfg(feature = "test-hooks")]
                RoomMessage::TestVisibleRoomsObserved {
                    core_generation,
                    reconciliation_is_complete,
                    room_ids,
                    forwarded,
                } => {
                    self.handle_test_visible_rooms_observed(
                        core_generation,
                        reconciliation_is_complete,
                        room_ids,
                        forwarded,
                    )
                    .await;
                }
                #[cfg(feature = "test-hooks")]
                RoomMessage::TestMembershipObserved {
                    core_generation,
                    transitions,
                    forwarded,
                } => {
                    self.handle_test_membership_observed(core_generation, transitions, forwarded)
                        .await;
                }
                #[cfg(feature = "test-hooks")]
                RoomMessage::TestKnownRooms {
                    room_ids,
                    forwarded,
                } => {
                    *self.known_room_ids.write().expect("known room ids lock") = room_ids;
                    let _ = forwarded.send(());
                }
                #[cfg(test)]
                RoomMessage::InspectObservationGeneration { response } => {
                    let _ = response.send(self.room_list_backend_generation);
                }
                _ => {}
            }
        }
    }
}

impl RoomActor {
    async fn handle_encryption_debug_completion(&mut self, completion: EncryptionDebugCompletion) {
        let EncryptionDebugCompletion {
            room_id,
            request_id,
            kind,
            outcome,
        } = completion;
        let Some(fence) = self.encryption_debug_fences.get(&room_id) else {
            return;
        };
        if fence.request_id != request_id || fence.room_id != room_id || fence.kind != kind {
            return;
        }
        let fence = self
            .encryption_debug_fences
            .remove(&room_id)
            .expect("matched fence");
        let joined = match koushi_sdk::room_is_joined(&fence.session, &room_id).await {
            Ok(joined) => joined,
            Err(_) => false,
        };
        let outcome = if joined
            && self
                .session
                .as_ref()
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, &fence.session))
        {
            outcome
        } else {
            // The session changed or the user left the room while the
            // operation ran; fail closed rather than apply the result.
            CoreEncryptionDebugOutcome::CancelledStale
        };
        self.emit_encryption_debug_outcome(request_id, room_id, kind, outcome)
            .await;
    }
}

impl RoomActor {
    fn placeholder_never_called() {}
}

impl RoomActor {
    async fn handle_test_visible_rooms_observed(
        &mut self,
        core_generation: u64,
        reconciliation_is_complete: bool,
        room_ids: Vec<VisibleRoomObservation>,
        forwarded: oneshot::Sender<bool>,
    ) {
        let entries_count = room_ids.len();
        let distinct_identity_count = room_ids
            .iter()
            .map(|observation| observation.room_id.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        let current_session = self.session.as_ref();
        let timeline_residency = self
            .timeline_residency
            .borrow()
            .as_ref()
            .filter(|binding| {
                current_session.is_some_and(|session| Arc::ptr_eq(&binding.session, session))
            })
            .map(|binding| binding.handle.clone());
        let forwarded_result = forward_visible_rooms_if_authoritative(
            timeline_residency.as_ref(),
            core_generation,
            reconciliation_is_complete,
            entries_count,
            distinct_identity_count,
            room_ids,
        )
        .await;
        let _ = forwarded.send(forwarded_result);
    }
}

impl RoomActor {
    async fn handle_test_membership_observed(
        &mut self,
        core_generation: u64,
        transitions: Vec<RoomMembershipTransition>,
        forwarded: oneshot::Sender<bool>,
    ) {
        let current_session = self.session.as_ref();
        let timeline_residency = self
            .timeline_residency
            .borrow()
            .as_ref()
            .filter(|binding| {
                current_session.is_some_and(|session| Arc::ptr_eq(&binding.session, session))
            })
            .map(|binding| binding.handle.clone());
        let forwarded_result =
            forward_membership_batches(timeline_residency.as_ref(), core_generation, [transitions])
                .await;
        let _ = forwarded.send(forwarded_result);
    }
}

impl RoomActor {
    fn start_live_observation(
        &mut self,
        session: Arc<MatrixClientSession>,
        service: Arc<matrix_sdk_ui::room_list_service::RoomListService>,
        generation: u64,
        source: RoomListSource,
        timeline_residency: Option<TimelineSubscriptionResidencyHandle>,
    ) {
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let (command_tx, command_rx) = mpsc::channel::<RoomListObservationCommand>(8);
        let authoritative = Arc::new(AtomicBool::new(false));
        let task = executor::spawn(run_live_room_list_observation(
            session,
            service,
            self.known_room_ids.clone(),
            self.self_tx.clone(),
            self.action_tx.clone(),
            self.event_tx.clone(),
            command_rx,
            stop_rx,
            generation,
            source,
            authoritative.clone(),
            self.sliding_sync_diagnostics.clone(),
            timeline_residency,
        ));
        self.observation = Some(RoomListObservation {
            stop_tx,
            task,
            command_tx,
            generation,
            source,
        });
    }
}

impl RoomActor {
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
}

impl RoomActor {
    async fn cancel_encryption_debug_for_rooms(&mut self, room_ids: &BTreeSet<String>) {
        let fences = room_ids
            .iter()
            .filter_map(|room_id| {
                self.encryption_debug_fences
                    .remove(room_id)
                    .map(|fence| (room_id.clone(), fence))
            })
            .collect::<Vec<_>>();
        for (room_id, mut fence) in fences {
            fence
                .cancelled
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = fence.cancel.send(());
            if tokio::time::timeout(ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT, &mut fence.join)
                .await
                .is_err()
            {
                fence.join.abort();
                let _ = fence.join.await;
            }
            self.emit_encryption_debug_outcome(
                fence.request_id,
                room_id.clone(),
                fence.kind,
                CoreEncryptionDebugOutcome::CancelledStale,
            )
            .await;
            self.reduce_reliable(vec![AppAction::EncryptionDebugOperationReset { room_id }])
                .await;
        }
    }
}

impl RoomActor {
    async fn stop_observation(&mut self) {
        if let Some(mut observation) = self.observation.take() {
            let _ = observation.stop_tx.send(());
            if executor::timeout(
                ROOM_OBSERVATION_SHUTDOWN_JOIN_TIMEOUT,
                &mut observation.task,
            )
            .await
            .is_err()
            {
                observation.task.abort();
                let _ = observation.task.await;
            }
        }
    }
}

impl RoomActor {
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
            RoomCommand::ForceNewOutboundSession {
                request_id,
                room_id,
            } => {
                self.handle_force_new_outbound_session(request_id, room_id)
                    .await;
            }
            RoomCommand::ShareIndex0RoomKey {
                request_id,
                room_id,
            } => {
                self.handle_share_index0_room_key(request_id, room_id).await;
            }
            RoomCommand::ResendIndex0RoomKey {
                request_id,
                room_id,
            } => {
                self.handle_resend_index0_room_key(request_id, room_id)
                    .await;
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
}

impl RoomActor {
    fn refresh_room_list(&self) {
        self.refresh_room_list_with_command(RoomListObservationCommand::Refresh);
    }
}

impl RoomActor {
    fn refresh_room_list_with_command(&self, command: RoomListObservationCommand) {
        if let Some(observation) = &self.observation {
            let retry_room_id = match &command {
                RoomListObservationCommand::Refresh => None,
                RoomListObservationCommand::RefreshRoom { room_id } => Some(room_id.clone()),
                RoomListObservationCommand::Reconcile { .. } => return,
            };
            let command_tx = observation.command_tx.clone();
            let _ = command_tx.try_send(command);
            // A successful local mutation can update the SDK room store just
            // after the immediate refresh observes the live list. Keep the
            // same live-service projection authoritative by retrying a few
            // bounded wakes; this never creates another service or network
            // sync loop.
            let _ = executor::spawn(async move {
                for delay in [
                    Duration::from_millis(100),
                    Duration::from_millis(300),
                    Duration::from_millis(1_000),
                ] {
                    executor::sleep(delay).await;
                    let retry_command = match retry_room_id.as_deref() {
                        Some(room_id) => RoomListObservationCommand::RefreshRoom {
                            room_id: room_id.to_owned(),
                        },
                        None => RoomListObservationCommand::Refresh,
                    };
                    if command_tx.send(retry_command).await.is_err() {
                        break;
                    }
                }
            });
        }
    }
}

impl RoomActor {
    fn emit(&self, event: CoreEvent) {
        let _ = self.event_tx.send(event);
    }
}

impl RoomActor {
    fn emit_failure(&self, request_id: RequestId, failure: CoreFailure) {
        self.emit(CoreEvent::OperationFailed {
            request_id,
            failure,
        });
    }
}

impl RoomActor {
    async fn reduce_reliable(&self, actions: Vec<AppAction>) {
        let _ = self.action_tx.send(actions).await;
    }
}
