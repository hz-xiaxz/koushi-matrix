//! #1060: selecting a known DM, room, or Space is purely local navigation.
//! AppActor must admit, reduce, publish, and settle it without a round trip
//! through the AccountActor/RoomActor network-operation mailboxes. Every test
//! holds the AccountActor mailbox full for its whole duration; the 250 ms
//! deadline is a regression budget, not an end-to-end latency claim. All
//! identifiers are synthetic.

use super::*;
use koushi_protocol::command::RoomCommand;

const USER: &str = "@synthetic:example.invalid";
const ROOM_A: &str = "!navigation-network-a:example.invalid";
const ROOM_B: &str = "!navigation-network-b:example.invalid";
const ROOM_C: &str = "!navigation-network-c:example.invalid";
const DM: &str = "!navigation-network-dm:example.invalid";
const SPACE: &str = "!navigation-network-space:example.invalid";
const SPACE_ROOM: &str = "!navigation-network-space-room:example.invalid";
const EMPTY_SPACE: &str = "!navigation-network-empty-space:example.invalid";
const EVENT: &str = "$navigation-network-event:example.invalid";
const DEADLINE: Duration = Duration::from_millis(250);

fn request(sequence: u64) -> RequestId {
    RequestId {
        connection_id: RuntimeConnectionId(1060),
        sequence,
    }
}

fn dm_room(room_id: &str) -> RoomSummary {
    RoomSummary {
        is_dm: true,
        dm_user_ids: vec!["@peer:example.invalid".to_owned()],
        ..unread_diagnostic_room(room_id)
    }
}

/// Ready session on Home with an ordinary room selected, a DM, and one Space
/// whose remembered room is `SPACE_ROOM`.
fn navigation_state() -> AppState {
    let mut space_room = unread_diagnostic_room(SPACE_ROOM);
    space_room.parent_space_ids = vec![SPACE.to_owned()];
    let mut state = AppState {
        session: SessionState::Ready(SessionInfo {
            homeserver: "https://example.invalid".to_owned(),
            user_id: USER.to_owned(),
            device_id: "SYNTHETIC".to_owned(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        }),
        rooms: vec![
            unread_diagnostic_room(ROOM_A),
            unread_diagnostic_room(ROOM_B),
            unread_diagnostic_room(ROOM_C),
            dm_room(DM),
            space_room,
        ],
        spaces: vec![
            koushi_state::SpaceSummary {
                space_id: SPACE.to_owned(),
                raw_name: None,
                display_name: "Synthetic space".to_owned(),
                avatar: None,
                join_rule: None,
                child_room_ids: vec![SPACE_ROOM.to_owned()],
            },
            koushi_state::SpaceSummary {
                space_id: EMPTY_SPACE.to_owned(),
                raw_name: None,
                display_name: "Synthetic empty space".to_owned(),
                avatar: None,
                join_rule: None,
                child_room_ids: Vec::new(),
            },
        ],
        ..AppState::default()
    };
    reduce(
        &mut state,
        AppAction::SelectRoom {
            room_id: ROOM_A.to_owned(),
        },
    );
    assert_eq!(state.navigation.active_room_id.as_deref(), Some(ROOM_A));
    state
}

struct BlockedMailbox {
    command_tx: mpsc::Sender<CoreCommandEnvelope>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_rx: broadcast::Receiver<CoreEvent>,
    snapshot_rx: watch::Receiver<VersionedAppStateSnapshot>,
    navigation_projection_rx: watch::Receiver<crate::timeline::NavigationProjectionDemand>,
    account_rx: mpsc::Receiver<AccountMessage>,
    account_actor: AccountActorHandle,
    initial: AppState,
    actor_task: executor::JoinHandle<()>,
    _data_dir: tempfile::TempDir,
    _event_navigation_prepared_tx: mpsc::UnboundedSender<EventNavigationPrepared>,
    _focused_projection_tx: mpsc::UnboundedSender<FocusedProjectionCommitted>,
}

impl BlockedMailbox {
    /// Start an AppActor whose one-slot AccountActor mailbox is already full
    /// and is never drained by the test until [`Self::finish`].
    async fn start(state: AppState) -> Self {
        Self::start_with(state, |_| {}).await
    }

    /// Like [`Self::start`], with white-box AppActor ownership installed
    /// before the actor runs (for an event navigation still in flight).
    async fn start_with(state: AppState, prepare: impl FnOnce(&mut AppActor)) -> Self {
        let data_dir = tempfile::tempdir().expect("runtime data directory");
        let (
            actor,
            command_tx,
            action_tx,
            account_rx,
            event_rx,
            snapshot_rx,
            navigation_projection_rx,
            event_navigation_prepared_tx,
            focused_projection_tx,
        ) = app_actor_fixture_with_account_capacity(data_dir.path(), state.clone(), 1);
        let mut actor = actor;
        prepare(&mut actor);
        assert!(
            actor
                .account_actor
                .send(AccountMessage::CancelActivityResolution)
                .await,
            "fill the AccountActor mailbox"
        );
        let account_actor = actor.account_actor.clone();
        let actor_task = executor::spawn(async move {
            let _ = actor.run().await;
        });
        Self {
            command_tx,
            action_tx,
            event_rx,
            snapshot_rx,
            navigation_projection_rx,
            account_rx,
            account_actor,
            initial: state,
            actor_task,
            _data_dir: data_dir,
            _event_navigation_prepared_tx: event_navigation_prepared_tx,
            _focused_projection_tx: focused_projection_tx,
        }
    }

    async fn submit(&self, command: CoreCommand) -> oneshot::Receiver<CoreCommandAdmission> {
        let (admission, admitted) = oneshot::channel();
        executor::timeout(
            DEADLINE,
            self.command_tx.send(CoreCommandEnvelope::Public {
                command,
                composer_permit: None,
                admission: Some(admission),
            }),
        )
        .await
        .expect("AppActor command ingress must not wait for the AccountActor")
        .expect("AppActor command ingress remains open");
        admitted
    }

    async fn select_room(&self, request_id: RequestId, room_id: &str) {
        let _admitted = self
            .submit(CoreCommand::Room(RoomCommand::SelectRoom {
                request_id,
                room_id: room_id.to_owned(),
            }))
            .await;
    }

    /// Collect the terminal outcome for every request, asserting each arrives
    /// exactly once and only after the state generation it names was
    /// published. The returned state is the navigation/timeline the WebView
    /// had received when that terminal arrived, rebuilt from ordered deltas.
    async fn terminals(&mut self, requests: &[RequestId]) -> Vec<(IntentOutcome, AppState)> {
        let mut received = self.initial.clone();
        let mut received_generation = 0;
        let mut outcomes: HashMap<RequestId, (IntentOutcome, AppState)> = HashMap::new();
        executor::timeout(DEADLINE, async {
            while outcomes.len() < requests.len() {
                match self
                    .event_rx
                    .recv()
                    .await
                    .expect("event stream remains open")
                {
                    CoreEvent::StateDelta(delta) => {
                        assert!(delta.generation > received_generation);
                        received_generation = delta.generation;
                        if let Some(navigation) = delta.changed.navigation {
                            received.navigation = navigation;
                        }
                        if let Some(timeline) = delta.changed.timeline {
                            received.timeline = timeline;
                        }
                    }
                    CoreEvent::IntentLifecycle {
                        request_id,
                        outcome,
                        published_generation,
                    } if requests.contains(&request_id) => {
                        assert!(
                            published_generation <= received_generation,
                            "terminal for {request_id:?} preceded its state publication"
                        );
                        assert!(
                            outcomes
                                .insert(request_id, (outcome, received.clone()))
                                .is_none(),
                            "request {request_id:?} settled twice"
                        );
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("room selection must settle while the AccountActor mailbox is full");
        requests
            .iter()
            .map(|request_id| outcomes.remove(request_id).expect("settled request"))
            .collect()
    }

    async fn wait_for_snapshot(&mut self, predicate: impl Fn(&AppState) -> bool) -> AppState {
        executor::timeout(DEADLINE, async {
            loop {
                let state = self.snapshot_rx.borrow_and_update().state.clone();
                if predicate(&state) {
                    break state;
                }
                self.snapshot_rx
                    .changed()
                    .await
                    .expect("snapshot channel remains open");
            }
        })
        .await
        .expect("navigation must publish while the AccountActor mailbox is full")
    }

    /// The retained post-commit enrichment demand, as (room, space).
    fn enrichment(&self) -> Option<(u64, Option<String>, Option<String>)> {
        self.account_actor
            .latest_navigation_enrichment()
            .map(|demand| {
                assert_eq!(
                    demand.session_key.user_id, USER,
                    "enrichment is fenced to the committing session"
                );
                (
                    demand.generation,
                    demand.active_room_id,
                    demand.active_space_id,
                )
            })
    }

    fn retained_projection_key(&mut self) -> Option<TimelineKey> {
        self.navigation_projection_rx
            .borrow_and_update()
            .room
            .as_ref()
            .map(|intent| intent.key.clone())
    }

    /// Wait until every previously sent action batch has been fully reduced:
    /// the one-slot action channel only accepts a second empty batch after the
    /// loop has taken the first, which follows the complete earlier batch.
    async fn drain_action_batches(&self) {
        for _ in 0..2 {
            executor::timeout(DEADLINE, self.action_tx.send(Vec::new()))
                .await
                .expect("action ingress must not wait for the AccountActor")
                .expect("action ingress remains open");
        }
    }

    /// The fill message is still queued: nothing drained the mailbox, so the
    /// assertions above really ran while it was full.
    fn finish(mut self) {
        self.actor_task.abort();
        assert!(
            matches!(
                self.account_rx.try_recv(),
                Ok(AccountMessage::CancelActivityResolution)
            ),
            "the AccountActor mailbox remained full throughout navigation"
        );
    }
}

fn room_key(room_id: &str) -> TimelineKey {
    TimelineKey::room(AccountKey(USER.to_owned()), room_id)
}

#[tokio::test]
async fn navigation_network_dm_selection_commits_while_account_mailbox_is_full() {
    let mut harness = BlockedMailbox::start(navigation_state()).await;
    harness.select_room(request(1), DM).await;

    let [(outcome, state)] = harness
        .terminals(&[request(1)])
        .await
        .try_into()
        .ok()
        .unwrap();
    assert_eq!(outcome, IntentOutcome::Committed);
    assert_eq!(state.navigation.active_room_id.as_deref(), Some(DM));
    assert_eq!(state.timeline.room_id.as_deref(), Some(DM));
    assert_eq!(harness.retained_projection_key(), Some(room_key(DM)));
    // Pinned-event refresh is scheduled after the commit, not awaited by it.
    assert_eq!(harness.enrichment(), Some((1, Some(DM.to_owned()), None)));
    harness.finish();
}

#[tokio::test]
async fn navigation_network_room_selection_commits_while_account_mailbox_is_full() {
    let mut harness = BlockedMailbox::start(navigation_state()).await;
    harness.select_room(request(1), ROOM_B).await;

    let [(outcome, state)] = harness
        .terminals(&[request(1)])
        .await
        .try_into()
        .ok()
        .unwrap();
    assert_eq!(outcome, IntentOutcome::Committed);
    assert_eq!(state.navigation.active_room_id.as_deref(), Some(ROOM_B));
    assert_eq!(state.timeline.room_id.as_deref(), Some(ROOM_B));
    assert_eq!(harness.retained_projection_key(), Some(room_key(ROOM_B)));
    harness.finish();
}

#[tokio::test]
async fn navigation_network_space_selection_commits_while_account_mailbox_is_full() {
    let mut harness = BlockedMailbox::start(navigation_state()).await;
    let admitted = harness
        .submit(CoreCommand::Room(RoomCommand::SelectSpace {
            request_id: request(1),
            space_id: Some(SPACE.to_owned()),
        }))
        .await;

    let state = harness
        .wait_for_snapshot(|state| {
            state.navigation.active_space_id.as_deref() == Some(SPACE)
                && state.navigation.active_room_id.as_deref() == Some(SPACE_ROOM)
        })
        .await;
    assert_eq!(state.timeline.room_id.as_deref(), Some(SPACE_ROOM));
    let admission = executor::timeout(DEADLINE, admitted)
        .await
        .expect("Space selection admission must not wait for the AccountActor")
        .expect("admission sender retained");
    assert!(admission.admitted_generation > 0);
    assert_eq!(
        harness.retained_projection_key(),
        Some(room_key(SPACE_ROOM))
    );
    // Member hydration and the restored room's pins follow the commit.
    assert_eq!(
        harness.enrichment(),
        Some((1, Some(SPACE_ROOM.to_owned()), Some(SPACE.to_owned())))
    );

    // Returning Home clears the Space-restored room without member hydration.
    let _admitted = harness
        .submit(CoreCommand::Room(RoomCommand::SelectSpace {
            request_id: request(2),
            space_id: None,
        }))
        .await;
    harness
        .wait_for_snapshot(|state| {
            state.navigation.active_space_id.is_none() && state.navigation.active_room_id.is_none()
        })
        .await;
    // Home is retained as current demand too, superseding the Space.
    assert_eq!(harness.enrichment(), Some((2, None, None)));
    harness.finish();
}

#[tokio::test]
async fn navigation_network_rapid_selection_leaves_last_room_authoritative() {
    let mut harness = BlockedMailbox::start(navigation_state()).await;
    harness.select_room(request(1), ROOM_B).await;
    harness.select_room(request(2), ROOM_C).await;
    harness.select_room(request(3), DM).await;

    let terminals = harness
        .terminals(&[request(1), request(2), request(3)])
        .await;
    for ((outcome, state), room_id) in terminals.iter().zip([ROOM_B, ROOM_C, DM]) {
        // Each terminal follows the publication of its own committed room,
        // or reports that a later selection in the same batch replaced it.
        match outcome {
            IntentOutcome::Committed => {
                assert_eq!(state.navigation.active_room_id.as_deref(), Some(room_id));
            }
            IntentOutcome::FailedNoOp(IntentNoOpReason::Superseded) => {
                assert_ne!(room_id, DM, "the last selection cannot be superseded");
            }
            outcome => panic!("unexpected selection outcome {outcome:?}"),
        }
    }
    assert_eq!(terminals[2].0, IntentOutcome::Committed);

    // A late actor projection of an earlier selection has no request owner
    // left and must not restore the older room.
    executor::timeout(
        DEADLINE,
        harness.action_tx.send(vec![AppAction::SelectRoom {
            room_id: ROOM_B.to_owned(),
        }]),
    )
    .await
    .expect("action ingress")
    .expect("action ingress remains open");
    harness.drain_action_batches().await;
    let state = harness.snapshot_rx.borrow().state.clone();
    assert_eq!(state.navigation.active_room_id.as_deref(), Some(DM));
    assert_eq!(state.timeline.room_id.as_deref(), Some(DM));
    assert_eq!(harness.retained_projection_key(), Some(room_key(DM)));
    // Only the latest selection remains as enrichment demand.
    assert_eq!(harness.enrichment(), Some((3, Some(DM.to_owned()), None)));
    harness.finish();
}

#[tokio::test]
async fn navigation_network_selection_from_home_commits_while_account_mailbox_is_full() {
    let mut state = navigation_state();
    reduce(&mut state, AppAction::SelectSpace { space_id: None });
    state.navigation.active_room_id = None;
    state.timeline.room_id = None;
    let mut harness = BlockedMailbox::start(state).await;
    harness.select_room(request(1), SPACE_ROOM).await;

    let [(outcome, state)] = harness
        .terminals(&[request(1)])
        .await
        .try_into()
        .ok()
        .unwrap();
    assert_eq!(outcome, IntentOutcome::Committed);
    assert_eq!(state.navigation.active_room_id.as_deref(), Some(SPACE_ROOM));
    // Selecting a Space child from Home moves navigation into its Space.
    assert_eq!(state.navigation.active_space_id.as_deref(), Some(SPACE));
    harness.finish();
}

#[tokio::test]
async fn navigation_network_selection_from_activity_commits_while_account_mailbox_is_full() {
    let mut state = navigation_state();
    reduce(&mut state, AppAction::ActivityOpened { request_id: 1 });
    assert!(!matches!(state.activity, ActivityState::Closed { .. }));
    let mut harness = BlockedMailbox::start(state).await;
    harness.select_room(request(1), ROOM_B).await;
    // A second selection proves that leaving Activity did not leave the
    // AppActor loop waiting on AccountActor cleanup admission.
    harness.select_room(request(2), ROOM_C).await;

    let terminals = harness.terminals(&[request(1), request(2)]).await;
    assert_eq!(terminals[1].0, IntentOutcome::Committed);
    assert_eq!(
        terminals[1].1.navigation.active_room_id.as_deref(),
        Some(ROOM_C)
    );
    harness.finish();
}

#[tokio::test]
async fn navigation_network_already_active_and_unknown_rooms_keep_their_outcomes() {
    let mut harness = BlockedMailbox::start(navigation_state()).await;
    harness.select_room(request(1), ROOM_A).await;
    harness
        .select_room(request(2), "!navigation-network-unknown:example.invalid")
        .await;

    let terminals = harness.terminals(&[request(1), request(2)]).await;
    assert_eq!(
        terminals[0].0,
        IntentOutcome::BenignNoOp(IntentNoOpReason::AlreadyActive)
    );
    assert_eq!(
        terminals[1].0,
        IntentOutcome::FailedNoOp(IntentNoOpReason::RoomNotInState)
    );
    assert_eq!(
        harness
            .snapshot_rx
            .borrow()
            .state
            .navigation
            .active_room_id
            .as_deref(),
        Some(ROOM_A)
    );
    // Re-selecting the active room still refreshes its pins; an unknown room
    // schedules nothing.
    assert_eq!(
        harness.enrichment(),
        Some((1, Some(ROOM_A.to_owned()), None))
    );
    harness.finish();
}

fn focused_key(room_id: &str) -> TimelineKey {
    TimelineKey {
        account_key: AccountKey(USER.to_owned()),
        kind: TimelineKind::Focused {
            room_id: room_id.to_owned(),
            event_id: EVENT.to_owned(),
        },
    }
}

/// `ROOM_A` anchored at `EVENT`: the focused timeline is already open.
fn anchored_state() -> AppState {
    let mut state = navigation_state();
    reduce(
        &mut state,
        AppAction::OpenFocusedContext {
            room_id: ROOM_A.to_owned(),
            event_id: EVENT.to_owned(),
        },
    );
    reduce(
        &mut state,
        AppAction::EnterAnchoredTimeline {
            room_id: ROOM_A.to_owned(),
            event_id: EVENT.to_owned(),
        },
    );
    assert!(state.navigation.main_timeline_anchor.is_some());
    state
}

/// Start from a focused context whose timeline the manager currently owns.
async fn start_focused(state: AppState, prepare: impl FnOnce(&mut AppActor)) -> BlockedMailbox {
    BlockedMailbox::start_with(state, |actor| {
        actor
            .account_actor
            .admit_focused_foreground(Some(focused_key(ROOM_A)));
        prepare(actor);
    })
    .await
}

/// Two selections in a row: the second proves no post-commit focused cleanup
/// left the AppActor loop waiting on the full AccountActor mailbox. The old
/// focused owner is retired through the retained desired foreground.
async fn assert_two_selections_commit(harness: &mut BlockedMailbox) {
    harness.select_room(request(11), ROOM_B).await;
    harness.select_room(request(12), ROOM_C).await;
    let terminals = harness.terminals(&[request(11), request(12)]).await;
    assert_eq!(terminals[1].0, IntentOutcome::Committed);
    assert_eq!(
        terminals[1].1.navigation.active_room_id.as_deref(),
        Some(ROOM_C)
    );
    assert_eq!(terminals[1].1.timeline.room_id.as_deref(), Some(ROOM_C));
    assert_eq!(harness.navigation_projection_rx.borrow().focused, None);
    assert_eq!(harness.retained_projection_key(), Some(room_key(ROOM_C)));
}

#[tokio::test]
async fn navigation_network_selection_from_opening_focused_context_commits() {
    let generation = 7;
    let mut state = navigation_state();
    state.focused_context = koushi_state::FocusedContextState::Open {
        room_id: ROOM_A.to_owned(),
        event_id: EVENT.to_owned(),
        is_subscribed: true,
    };
    state.navigation.event_navigation = koushi_state::EventNavigationState::Opening {
        generation,
        source: koushi_state::EventNavigationSource::Activity,
    };
    let mut harness = start_focused(state, |actor| {
        actor.pending_event_navigation = Some(PendingEventNavigation {
            request_id: request(1),
            select_request_id: request(2),
            room_id: ROOM_A.to_owned(),
            event_id: EVENT.to_owned(),
            source: koushi_state::EventNavigationSource::Activity,
            generation,
        });
        actor.pending_focused_navigation = Some(PendingFocusedNavigation {
            projection_request_id: request(1),
            key: focused_key(ROOM_A),
            room_id: ROOM_A.to_owned(),
            event_id: EVENT.to_owned(),
            allow_live_fallback: true,
            generation: Some(TimelineGeneration(generation)),
        });
    })
    .await;

    assert_two_selections_commit(&mut harness).await;
    harness.finish();
}

#[tokio::test]
async fn navigation_network_selection_from_anchored_focused_context_commits() {
    let mut harness = start_focused(anchored_state(), |_| {}).await;
    assert_two_selections_commit(&mut harness).await;
    harness.finish();
}

#[tokio::test]
async fn navigation_network_home_from_anchored_focused_context_commits() {
    let mut harness = start_focused(anchored_state(), |_| {}).await;
    let _admitted = harness
        .submit(CoreCommand::Room(RoomCommand::SelectSpace {
            request_id: request(10),
            space_id: None,
        }))
        .await;
    assert_two_selections_commit(&mut harness).await;
    harness.finish();
}

#[tokio::test]
async fn navigation_network_empty_space_from_anchored_focused_context_commits() {
    let mut harness = start_focused(anchored_state(), |_| {}).await;
    let _admitted = harness
        .submit(CoreCommand::Room(RoomCommand::SelectSpace {
            request_id: request(10),
            space_id: Some(EMPTY_SPACE.to_owned()),
        }))
        .await;
    harness
        .wait_for_snapshot(|state| {
            state.navigation.active_space_id.as_deref() == Some(EMPTY_SPACE)
                && state.navigation.active_room_id.is_none()
        })
        .await;
    assert_two_selections_commit(&mut harness).await;
    harness.finish();
}
