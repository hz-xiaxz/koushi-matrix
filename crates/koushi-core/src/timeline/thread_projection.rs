//! Exact AST extraction draft from immutable timeline baseline.

use super::*;

/// A bounded Room replay may surface summary-only roots that the SDK omitted
/// from its item stream. Keep this much smaller than the base window so a
/// historical root-heavy room cannot multiply initial render work.
const ROOM_REPLAY_KNOWN_THREAD_ROOT_PROJECTIONS_MAX: usize = 32;

/// `epoch` crosses the JSON/JavaScript IPC boundary as a number. It must stay
/// within JavaScript's exact integer range so a source-scoped Clear can never
/// be rounded into another replay owner's epoch.
const JAVASCRIPT_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;

/// Manager-owned tasks for bounded root hydration. Removing a task before a
/// queued completion is handled makes the completion stale by construction.
#[derive(Default)]
struct ThreadRootProjectionFetchRegistry {
    tasks: HashMap<(String, String), (u64, executor::JoinHandle<()>)>,
}

impl ThreadRootProjectionFetchRegistry {
    fn contains(&self, room_id: &str, root_event_id: &str, actor_generation: u64) -> bool {
        self.tasks
            .get(&(room_id.to_owned(), root_event_id.to_owned()))
            .is_some_and(|(generation, _)| *generation == actor_generation)
    }

    fn insert(
        &mut self,
        room_id: String,
        root_event_id: String,
        actor_generation: u64,
        task: executor::JoinHandle<()>,
    ) {
        if let Some((previous_generation, previous)) = self
            .tasks
            .insert((room_id, root_event_id), (actor_generation, task))
        {
            // This is defensive: start handling gates duplicates, but a future
            // caller must never leak the prior worker if it violates that
            // invariant.
            previous.abort();
            debug_assert_ne!(
                previous_generation, actor_generation,
                "a root hydration worker must be unique within one actor generation"
            );
        }
    }

    /// Returns false when unsubscribe/shutdown already cancelled this worker;
    /// callers must then ignore its late terminal message.
    fn take_completion(
        &mut self,
        room_id: &str,
        root_event_id: &str,
        actor_generation: u64,
    ) -> bool {
        let key = (room_id.to_owned(), root_event_id.to_owned());
        if self
            .tasks
            .get(&key)
            .is_some_and(|(generation, _)| *generation == actor_generation)
        {
            self.tasks.remove(&key);
            true
        } else {
            false
        }
    }

    fn abort_room(&mut self, room_id: &str) -> usize {
        let keys = self
            .tasks
            .keys()
            .filter(|(entry_room_id, _)| entry_room_id == room_id)
            .cloned()
            .collect::<Vec<_>>();
        let count = keys.len();
        for key in keys {
            if let Some((_, task)) = self.tasks.remove(&key) {
                task.abort();
            }
        }
        count
    }

    fn abort_all(&mut self) {
        for (_, (_, task)) in self.tasks.drain() {
            task.abort();
        }
    }
}

/// Lifecycle registry for ready root snapshots copied from an actor's own
/// navigation cache during a bounded replay. This is separate from the
/// fetch-backed projection service: no SDK fetch was started for these roots,
/// but unsubscribe and shutdown still must emit a matching frontend clear.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReplayKnownThreadRootProjection {
    root_event_id: String,
    activity_event_id: String,
    activity_timestamp_ms: Option<u64>,
    /// The full renderable Ready payload. Activity identity alone is not a
    /// revision: edits, redactions, reactions, and action affordances can
    /// change while the latest thread reply remains the same.
    item: TimelineItem,
    source: ThreadRootProjectionSourceDto,
}

#[derive(Default)]
struct ReplayKnownThreadRootProjectionRegistry {
    entries: HashMap<TimelineKey, HashMap<String, ReplayKnownThreadRootProjection>>,
    /// Hydration terminal results that arrived while a replay-known Ready
    /// owned the root. The marker is consumed when that owner clears.
    suppressed_hydration_terminals: HashMap<TimelineKey, HashSet<String>>,
    /// Hydration terminal results that were actually broadcast while no replay
    /// owner existed. A later replay Ready can overwrite that source in the
    /// desktop store, so its scoped Clear must reassert this terminal. Merely
    /// retaining a terminal in the service is not sufficient: it may never
    /// have been visible to the store.
    emitted_hydration_terminals: HashMap<TimelineKey, HashSet<String>>,
    next_epoch: u64,
}

#[derive(Default)]
struct ReplayKnownThreadRootProjectionUpdate {
    ready: Vec<ThreadRootProjectionDto>,
    stale: Vec<ReplayKnownThreadRootProjection>,
}

impl TimelineManagerActor {
    async fn handle_thread_root_projection_fetch_start(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
        activities: Vec<ThreadRootProjectionActivity>,
    ) {
        let Some(_lease) = self
            .timeline_actor_generations
            .try_acquire(&key, actor_generation)
        else {
            return;
        };
        if !matches!(key.kind, TimelineKind::Room { .. }) || !self.timelines.contains_key(&key) {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        for activity in activities {
            if self.thread_root_projection_fetches.contains(
                &activity.room_id,
                &activity.root_event_id,
                actor_generation,
            ) {
                continue;
            }
            let should_start = self
                .thread_root_projection_service
                .lock()
                .expect("thread-root projection service lock must not be poisoned")
                .has_pending_attempt(&activity);
            if !should_start {
                continue;
            }
            let task = spawn_thread_root_projection_fetch(
                session.clone(),
                key.clone(),
                actor_generation,
                own_user_id.clone(),
                self.msg_tx.clone(),
                activity.clone(),
            );
            self.thread_root_projection_fetches.insert(
                activity.room_id,
                activity.root_event_id,
                actor_generation,
                task,
            );
        }
    }

    async fn handle_thread_root_projection_fetch_finished(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        activity: ThreadRootProjectionActivity,
        result: Result<TimelineItem, OperationFailureKind>,
    ) {
        if !self.thread_root_projection_fetches.take_completion(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
        ) || !self.timelines.contains_key(&key)
        {
            return;
        }
        let Ok(action_permit) = self.action_tx.clone().reserve_owned().await else {
            return;
        };
        let Some(lease) = self
            .timeline_actor_generations
            .try_acquire(&key, actor_generation)
        else {
            return;
        };
        let mut service = self
            .thread_root_projection_service
            .lock()
            .expect("thread-root projection service lock must not be poisoned");
        let record = match result {
            Ok(item) => service.mark_ready(&activity, item),
            Err(failure_kind) => service.mark_failed(&activity, failure_kind),
        };
        let Some(record) = record else {
            return;
        };
        action_permit.send(vec![thread_root_projection_action_from_record(&record)]);
        drop(service);
        // A bounded replay may have acquired display ownership while this
        // manager-owned cache/network lookup was in flight. The shared
        // registry mutex covers both this decision and the synchronous
        // broadcast: actor replay publication uses the same boundary, so no
        // replay Ready can land between a no-owner check and hydration's
        // terminal event. The terminal remains in the service/reducer state
        // either way and is handed back when replay ownership later ends.
        let _ = emit_hydration_terminal_unless_replay_owned(
            &self.event_tx,
            &self.replay_known_thread_root_projections,
            &key,
            thread_root_projection_dto_from_record(&record),
        );
        drop(lease);
    }

    async fn clear_thread_root_projections_for_room(&mut self, key: &TimelineKey) {
        if !matches!(key.kind, TimelineKind::Room { .. }) {
            return;
        }
        // Stop an old actor from acquiring a replay-known lease, then wait
        // only for its already-synchronous registry/Core emission section.
        // This releases the gate mutex before awaiting the watch notification.
        self.timeline_actor_generations
            .invalidate_and_quiesce(key)
            .await;
        let room_id = key.room_id();
        self.thread_root_projection_fetches.abort_room(room_id);
        let records = self
            .thread_root_projection_service
            .lock()
            .expect("thread-root projection service lock must not be poisoned")
            .clear_room(room_id);
        let replay_known = self
            .replay_known_thread_root_projections
            .lock()
            .expect("replay-known root registry lock must not be poisoned")
            .clear(key);
        let _ = self
            .emit_action_reliable(AppAction::ThreadRootProjectionsCleared {
                room_id: room_id.to_owned(),
            })
            .await;
        for record in records {
            self.emit(CoreEvent::Timeline(TimelineEvent::ThreadRootProjection {
                key: key.clone(),
                projection: ThreadRootProjectionDto {
                    root_event_id: record.activity.root_event_id,
                    activity_event_id: record.activity.activity_event_id,
                    activity_timestamp_ms: record.activity.activity_timestamp_ms,
                    retain_without_reply: false,
                    source: ThreadRootProjectionSourceDto::Hydration,
                    state: ThreadRootProjectionStateDto::Cleared,
                },
            }));
        }
        for projection in replay_known {
            self.emit(CoreEvent::Timeline(TimelineEvent::ThreadRootProjection {
                key: key.clone(),
                projection: replay_known_clear_projection(projection),
            }));
        }
    }
}

/// Starts the only allowed old-root hydration operation. It performs one
/// cache-first `load_or_fetch_event` call and reports a typed terminal outcome
/// back to the owning manager. It intentionally has no access to the SDK
/// `Timeline`, so backward pagination and anchor materialization are not
/// possible from this path.
fn spawn_thread_root_projection_fetch(
    session: Arc<MatrixClientSession>,
    key: TimelineKey,
    actor_generation: u64,
    own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
    manager_tx: mpsc::Sender<TimelineMessage>,
    activity: ThreadRootProjectionActivity,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        let result =
            load_thread_root_projection_item(&session, &key, own_user_id.as_deref(), &activity)
                .await;
        let _ = manager_tx
            .send(TimelineMessage::ThreadRootProjectionFetchFinished {
                key,
                actor_generation,
                activity,
                result,
            })
            .await;
    })
}

fn thread_root_projection_dto_from_record(
    record: &ThreadRootProjectionRecord,
) -> ThreadRootProjectionDto {
    let state = if record.is_pending() {
        ThreadRootProjectionStateDto::Pending
    } else if let Some(item) = record.item() {
        ThreadRootProjectionStateDto::Ready {
            item: thread_root_item_with_latest_activity_summary(item, &record.activity),
        }
    } else if let Some(failure_kind) = record.failure_kind() {
        ThreadRootProjectionStateDto::Failed { failure_kind }
    } else {
        ThreadRootProjectionStateDto::Pending
    };
    ThreadRootProjectionDto {
        root_event_id: record.activity.root_event_id.clone(),
        activity_event_id: record.activity.activity_event_id.clone(),
        activity_timestamp_ms: record.activity.activity_timestamp_ms,
        retain_without_reply: false,
        source: ThreadRootProjectionSourceDto::Hydration,
        state,
    }
}

fn thread_root_projection_pending_dto(
    activity: &ThreadRootProjectionActivity,
) -> ThreadRootProjectionDto {
    ThreadRootProjectionDto {
        root_event_id: activity.root_event_id.clone(),
        activity_event_id: activity.activity_event_id.clone(),
        activity_timestamp_ms: activity.activity_timestamp_ms,
        retain_without_reply: false,
        source: ThreadRootProjectionSourceDto::Hydration,
        state: ThreadRootProjectionStateDto::Pending,
    }
}

fn hydration_projection_event(
    key: &TimelineKey,
    projection: ThreadRootProjectionDto,
) -> TimelineEvent {
    TimelineEvent::ThreadRootProjection {
        key: key.clone(),
        projection,
    }
}

struct PreparedThreadRootHydration {
    activities_by_root: HashMap<String, ThreadRootProjectionActivity>,
    missing_activities: Vec<ThreadRootProjectionActivity>,
}

#[allow(clippy::too_many_arguments)]
async fn commit_prepared_thread_root_hydration_for_generation(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    replay_registry: &Arc<Mutex<ReplayKnownThreadRootProjectionRegistry>>,
    generations: &Arc<TimelineActorGenerationGate>,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    manager_tx: &mpsc::Sender<TimelineMessage>,
    event_tx: &broadcast::Sender<CoreEvent>,
    key: &TimelineKey,
    actor_generation: u64,
    own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
    prepared: PreparedThreadRootHydration,
) -> bool {
    let fetch_permit = if prepared.missing_activities.is_empty() {
        None
    } else {
        let Ok(permit) = manager_tx.clone().reserve_owned().await else {
            return false;
        };
        Some(permit)
    };
    // Manager capacity is reserved first. The reducer permit is the final
    // await, so hydration can never hold reducer capacity while a manager
    // message that needs that same reducer is ahead of it in the mailbox.
    let Ok(action_permit) = action_tx.clone().reserve_owned().await else {
        return false;
    };
    let Some(lease) = generations.try_acquire(key, actor_generation) else {
        return false;
    };
    let mut actions = vec![AppAction::ThreadRootProjectionsReconciled {
        room_id: key.room_id().to_owned(),
        activities: prepared
            .activities_by_root
            .values()
            .map(|activity| ThreadRootProjectionActivityState {
                root_event_id: activity.root_event_id.clone(),
                activity_event_id: activity.activity_event_id.clone(),
                activity_timestamp_ms: activity.activity_timestamp_ms,
            })
            .collect(),
    }];
    let mut events = Vec::new();
    let mut terminal_projections = Vec::new();
    let mut fetches = Vec::new();
    let mut service_guard = service
        .lock()
        .expect("thread-root projection service lock must not be poisoned");
    service_guard.reconcile_room_activities(key.room_id(), &prepared.activities_by_root);
    for activity in prepared.missing_activities {
        let decision = service_guard.observe(activity);
        match decision {
            ThreadRootProjectionDecision::StartFetch(activity) => {
                actions.push(AppAction::ThreadRootProjectionObserved {
                    room_id: activity.room_id.clone(),
                    root_event_id: activity.root_event_id.clone(),
                    activity_event_id: activity.activity_event_id.clone(),
                    activity_timestamp_ms: activity.activity_timestamp_ms,
                });
                events.push(hydration_projection_event(
                    key,
                    thread_root_projection_pending_dto(&activity),
                ));
                fetches.push(activity);
            }
            ThreadRootProjectionDecision::ActivityUpdated(record)
            | ThreadRootProjectionDecision::Existing(record) => {
                actions.push(thread_root_projection_action_from_record(&record));
                if record.is_pending() {
                    events.push(hydration_projection_event(
                        key,
                        thread_root_projection_dto_from_record(&record),
                    ));
                    fetches.push(record.activity.clone());
                } else {
                    terminal_projections.push(thread_root_projection_dto_from_record(&record));
                }
            }
        }
    }
    action_permit.send(actions);
    emit_timeline_events_with_lease(event_tx, &lease, events);
    drop(service_guard);
    for projection in terminal_projections {
        let _ =
            emit_hydration_terminal_unless_replay_owned(event_tx, replay_registry, key, projection);
    }
    if let Some(permit) = fetch_permit {
        if !fetches.is_empty() {
            permit.send(TimelineMessage::StartThreadRootProjectionFetch {
                key: key.clone(),
                actor_generation,
                own_user_id,
                activities: fetches,
            });
        }
    }
    true
}

fn thread_root_projection_action_from_record(record: &ThreadRootProjectionRecord) -> AppAction {
    if let Some(failure_kind) = record.failure_kind() {
        AppAction::ThreadRootProjectionFailed {
            room_id: record.activity.room_id.clone(),
            root_event_id: record.activity.root_event_id.clone(),
            activity_event_id: record.activity.activity_event_id.clone(),
            activity_timestamp_ms: record.activity.activity_timestamp_ms,
            failure_kind,
        }
    } else if record.item().is_some() {
        AppAction::ThreadRootProjectionReady {
            room_id: record.activity.room_id.clone(),
            root_event_id: record.activity.root_event_id.clone(),
            activity_event_id: record.activity.activity_event_id.clone(),
            activity_timestamp_ms: record.activity.activity_timestamp_ms,
        }
    } else {
        AppAction::ThreadRootProjectionObserved {
            room_id: record.activity.room_id.clone(),
            root_event_id: record.activity.root_event_id.clone(),
            activity_event_id: record.activity.activity_event_id.clone(),
            activity_timestamp_ms: record.activity.activity_timestamp_ms,
        }
    }
}

fn thread_root_item_with_latest_activity_summary(
    item: &TimelineItem,
    activity: &ThreadRootProjectionActivity,
) -> TimelineItem {
    let mut item = item.clone();
    let summary = item.thread_summary.get_or_insert(ThreadSummaryDto {
        reply_count: 1,
        latest_event_id: None,
        latest_sender: None,
        latest_sender_label: None,
        latest_body_preview: None,
        latest_timestamp_ms: None,
    });
    summary.reply_count = summary.reply_count.max(1);
    summary.latest_event_id = Some(activity.activity_event_id.clone());
    summary.latest_sender = activity.activity_sender.clone();
    summary.latest_sender_label = activity.activity_sender_label.clone();
    summary.latest_body_preview = activity.activity_body_preview.clone();
    summary.latest_timestamp_ms = activity.activity_timestamp_ms;
    item
}

async fn load_thread_root_projection_item(
    session: &MatrixClientSession,
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    activity: &ThreadRootProjectionActivity,
) -> Result<TimelineItem, OperationFailureKind> {
    let room_id = matrix_sdk::ruma::RoomId::parse(activity.room_id.as_str())
        .map_err(|_| OperationFailureKind::Invalid)?;
    let root_event_id = matrix_sdk::ruma::EventId::parse(activity.root_event_id.as_str())
        .map_err(|_| OperationFailureKind::Invalid)?;
    let room = session
        .client()
        .get_room(&room_id)
        .ok_or(OperationFailureKind::NotFound)?;
    let loaded = room
        .load_or_fetch_event(&root_event_id, None)
        .await
        .map_err(|_| OperationFailureKind::Network)?;
    let raw: serde_json::Value =
        serde_json::from_str(loaded.raw().json().get()).map_err(|_| OperationFailureKind::Sdk)?;
    let sender_id = raw
        .get("sender")
        .and_then(serde_json::Value::as_str)
        .and_then(|sender| matrix_sdk::ruma::UserId::parse(sender).ok());
    let sender_profile = match sender_id {
        Some(sender_id) => room
            .get_member_no_sync(sender_id.as_ref())
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let sender_label = sender_profile
        .as_ref()
        .and_then(|member| member.display_name())
        .map(str::to_owned);
    let sender_avatar = sender_profile
        .as_ref()
        .and_then(|member| member.avatar_url())
        .map(|avatar_url| AvatarImage {
            mxc_uri: avatar_url.to_string(),
            thumbnail: AvatarThumbnailState::NotRequested,
        });
    let relation_events = match room.event_cache().await {
        Ok((cache, _drop_handles)) => cache
            .find_event_relations(&root_event_id, None)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|event| serde_json::from_str(event.raw().json().get()).ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    let context = ThreadRootProjectionRenderContext {
        sender_label,
        sender_avatar,
        reactions: reaction_groups_from_cached_relation_events(
            relation_events,
            root_event_id.as_str(),
            own_user_id,
        ),
    };
    thread_root_projection_item_from_raw_with_context(key, own_user_id, activity, raw, context)
        .ok_or(OperationFailureKind::Sdk)
}

fn thread_root_projection_activity_from_item(
    room_id: &str,
    item: &TimelineItem,
) -> Option<ThreadRootProjectionActivity> {
    let TimelineItemId::Event { event_id } = &item.id else {
        return None;
    };
    let root_event_id = item.thread_root.as_ref()?.trim();
    (!root_event_id.is_empty()).then(|| ThreadRootProjectionActivity {
        room_id: room_id.to_owned(),
        root_event_id: root_event_id.to_owned(),
        activity_event_id: event_id.clone(),
        activity_timestamp_ms: item.timestamp_ms,
        activity_sender: item.sender.clone(),
        activity_sender_label: item.sender_label.clone(),
        activity_body_preview: thread_root_activity_preview(item),
    })
}

/// The exact Room items currently represented by the bounded display replay.
///
/// `navigation_items` deliberately has a wider lifetime than the UI's replay
/// window. It may therefore contain a latest reply that was not rendered. A
/// replay-known root must be reconciled against this context, never the whole
/// navigation cache, or an unrelated cached reply can clear the visible root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ReplayKnownDisplayContext {
    event_ids: HashSet<String>,
    exact_thread_reply_pairs: HashSet<(String, String)>,
    activity_range: Option<(u64, u64)>,
}

impl ReplayKnownDisplayContext {
    fn from_display_items(display_items: &[TimelineItem]) -> Self {
        let event_ids = display_items
            .iter()
            .filter_map(timeline_item_event_id)
            .map(ToOwned::to_owned)
            .collect::<HashSet<_>>();
        let exact_thread_reply_pairs = display_items
            .iter()
            .filter_map(|item| {
                let root_event_id = item.thread_root.as_deref()?.trim();
                let reply_event_id = timeline_item_event_id(item)?.trim();
                (!root_event_id.is_empty() && !reply_event_id.is_empty())
                    .then(|| (root_event_id.to_owned(), reply_event_id.to_owned()))
            })
            .collect::<HashSet<_>>();
        Self {
            event_ids,
            exact_thread_reply_pairs,
            activity_range: replay_activity_timestamp_range(display_items),
        }
    }
}

/// Returns root snapshots already known to the actor but absent from the
/// bounded display context. This is not hydration: copying a root from
/// `navigation_items` must never call the SDK, paginate, or materialize a
/// viewport anchor.
#[cfg(test)]
fn known_thread_root_projections_for_replay(
    navigation_items: &[TimelineItem],
    replay_items: &[TimelineItem],
) -> Vec<ThreadRootProjectionDto> {
    known_thread_root_projections_for_display_context(
        navigation_items,
        &ReplayKnownDisplayContext::from_display_items(replay_items),
    )
}

fn known_thread_root_projections_for_display_context(
    navigation_items: &[TimelineItem],
    display_context: &ReplayKnownDisplayContext,
) -> Vec<ThreadRootProjectionDto> {
    let Some((range_start_ms, range_end_ms)) = display_context.activity_range else {
        return Vec::new();
    };
    let mut emitted_root_event_ids = HashSet::new();
    let mut projections = navigation_items
        .iter()
        .filter_map(|item| {
            let root_event_id = timeline_item_event_id(item)?;
            if item.thread_root.is_some() || display_context.event_ids.contains(root_event_id) {
                return None;
            }
            let summary = item.thread_summary.as_ref()?;
            let activity_event_id = summary.latest_event_id.as_ref()?.trim();
            if activity_event_id.is_empty() {
                return None;
            }
            if display_context
                .exact_thread_reply_pairs
                .contains(&(root_event_id.to_owned(), activity_event_id.to_owned()))
            {
                return None;
            }
            let activity_timestamp_ms = summary.latest_timestamp_ms?;
            // The replay display range is inclusive: a summary on either
            // boundary belongs to the same visual window, never outside it.
            if activity_timestamp_ms < range_start_ms || activity_timestamp_ms > range_end_ms {
                return None;
            }
            if !emitted_root_event_ids.insert(root_event_id.to_owned()) {
                return None;
            }
            Some(ThreadRootProjectionDto {
                root_event_id: root_event_id.to_owned(),
                activity_event_id: activity_event_id.to_owned(),
                activity_timestamp_ms: Some(activity_timestamp_ms),
                retain_without_reply: true,
                source: ThreadRootProjectionSourceDto::Hydration,
                state: ThreadRootProjectionStateDto::Ready { item: item.clone() },
            })
        })
        .collect::<Vec<_>>();
    projections.sort_by(|left, right| {
        left.activity_timestamp_ms
            .cmp(&right.activity_timestamp_ms)
            .then_with(|| left.root_event_id.cmp(&right.root_event_id))
    });
    projections.truncate(ROOM_REPLAY_KNOWN_THREAD_ROOT_PROJECTIONS_MAX);
    projections
}

/// Returns the inclusive activity bounds represented by event-backed replay
/// rows. A replay with no timestamped event rows cannot place summary-only
/// roots chronologically, so it deliberately emits none.
fn replay_activity_timestamp_range(replay_items: &[TimelineItem]) -> Option<(u64, u64)> {
    replay_items
        .iter()
        .filter(|item| timeline_item_event_id(item).is_some())
        .filter_map(|item| item.timestamp_ms)
        .fold(None, |range, timestamp_ms| match range {
            Some((start, end)) => Some((start.min(timestamp_ms), end.max(timestamp_ms))),
            None => Some((timestamp_ms, timestamp_ms)),
        })
}

/// Derives the bounded replay candidates before entering an ownership group.
/// Only Room timelines have this out-of-band root snapshot behaviour.
fn replay_known_candidates_for_display_items(
    key: &TimelineKey,
    navigation_items: &[TimelineItem],
    display_items: &[TimelineItem],
) -> Vec<ThreadRootProjectionDto> {
    if !matches!(key.kind, TimelineKind::Room { .. }) {
        return Vec::new();
    }
    known_thread_root_projections_for_display_context(
        navigation_items,
        &ReplayKnownDisplayContext::from_display_items(display_items),
    )
}

#[cfg(test)]
fn refresh_replay_known_root_projections(
    registry: &Arc<Mutex<ReplayKnownThreadRootProjectionRegistry>>,
    key: &TimelineKey,
    navigation_items: &[TimelineItem],
    display_items: &[TimelineItem],
) -> ReplayKnownThreadRootProjectionUpdate {
    refresh_replay_known_root_projections_with_display_context(
        registry,
        key,
        navigation_items,
        &ReplayKnownDisplayContext::from_display_items(display_items),
    )
}

#[cfg(test)]
fn refresh_replay_known_root_projections_with_display_context(
    registry: &Arc<Mutex<ReplayKnownThreadRootProjectionRegistry>>,
    key: &TimelineKey,
    navigation_items: &[TimelineItem],
    display_context: &ReplayKnownDisplayContext,
) -> ReplayKnownThreadRootProjectionUpdate {
    let candidates = if matches!(key.kind, TimelineKind::Room { .. }) {
        known_thread_root_projections_for_display_context(navigation_items, display_context)
    } else {
        Vec::new()
    };
    registry
        .lock()
        .expect("replay-known root registry lock must not be poisoned")
        .replace(key, candidates)
}

#[cfg(test)]
fn reconcile_replay_known_root_projections_after_navigation_update(
    registry: &Arc<Mutex<ReplayKnownThreadRootProjectionRegistry>>,
    key: &TimelineKey,
    navigation_items: &[TimelineItem],
    display_context: &ReplayKnownDisplayContext,
) -> ReplayKnownThreadRootProjectionUpdate {
    registry
        .lock()
        .expect("replay-known root registry lock must not be poisoned")
        .reconcile_navigation(key, navigation_items, display_context)
}

fn replay_known_clear_projection(
    projection: ReplayKnownThreadRootProjection,
) -> ThreadRootProjectionDto {
    ThreadRootProjectionDto {
        root_event_id: projection.root_event_id,
        activity_event_id: projection.activity_event_id,
        activity_timestamp_ms: projection.activity_timestamp_ms,
        retain_without_reply: false,
        source: projection.source,
        state: ThreadRootProjectionStateDto::Cleared,
    }
}

#[cfg(test)]
fn emit_replay_known_root_projection_update(
    event_tx: &broadcast::Sender<CoreEvent>,
    key: &TimelineKey,
    update: ReplayKnownThreadRootProjectionUpdate,
) {
    for event in replay_known_timeline_events(key, update) {
        let _ = event_tx.send(CoreEvent::Timeline(event));
    }
}

#[cfg(test)]
fn replay_known_timeline_events(
    key: &TimelineKey,
    update: ReplayKnownThreadRootProjectionUpdate,
) -> Vec<TimelineEvent> {
    let mut events = Vec::with_capacity(update.stale.len() + update.ready.len());
    for projection in update.stale {
        events.push(TimelineEvent::ThreadRootProjection {
            key: key.clone(),
            projection: replay_known_clear_projection(projection),
        });
    }
    for projection in update.ready {
        events.push(TimelineEvent::ThreadRootProjection {
            key: key.clone(),
            projection,
        });
    }
    events
}

/// Builds a replay-known transition while the caller still owns the registry
/// mutex. When a root loses replay ownership, hand the retained terminal
/// hydration snapshot back after its source-scoped Clear so an exact canonical
/// reply slot continues to represent the complete root block. No lookup is
/// started here; the service is consulted read-only and this function never
/// awaits.
fn replay_known_timeline_events_with_hydration_handoffs(
    key: &TimelineKey,
    registry: &mut ReplayKnownThreadRootProjectionRegistry,
    thread_root_projection_service: &Arc<Mutex<ThreadRootProjectionService>>,
    update: ReplayKnownThreadRootProjectionUpdate,
) -> Vec<TimelineEvent> {
    let mut events = Vec::with_capacity(update.stale.len() + update.ready.len() * 2);
    for projection in update.stale {
        let root_event_id = projection.root_event_id.clone();
        events.push(TimelineEvent::ThreadRootProjection {
            key: key.clone(),
            projection: replay_known_clear_projection(projection),
        });
        if registry.owns_root(key, &root_event_id) {
            continue;
        }
        // Reassert only a terminal that the frontend had already observed, or
        // one deliberately withheld while replay ownership was current. A
        // retained service terminal that was never emitted is not a UI source
        // and must remain silent after the replay Clear.
        let was_suppressed = registry.take_suppressed_hydration_terminal(key, &root_event_id);
        let was_emitted = registry.take_emitted_hydration_terminal(key, &root_event_id);
        if !was_suppressed && !was_emitted {
            continue;
        }
        let terminal_hydration = thread_root_projection_service
            .lock()
            .expect("thread-root projection service lock must not be poisoned")
            .terminal_record(key.room_id(), &root_event_id)
            .map(|record| thread_root_projection_dto_from_record(&record));
        if let Some(projection) = terminal_hydration {
            registry.mark_hydration_terminal_emitted(key, root_event_id);
            events.push(TimelineEvent::ThreadRootProjection {
                key: key.clone(),
                projection,
            });
        }
    }
    for projection in update.ready {
        events.push(TimelineEvent::ThreadRootProjection {
            key: key.clone(),
            projection,
        });
    }
    events
}

/// Delivers one hydration terminal only if a replay-owned snapshot has not
/// already won the same root. The replay registry lock covers both the
/// ownership decision and synchronous Core broadcast, so a replay Ready can
/// never appear between them and be overwritten by this hydration DTO.
///
/// The caller must finish reducer delivery before calling this helper. It does
/// no I/O and never awaits while the registry mutex is held.
fn emit_hydration_terminal_unless_replay_owned(
    event_tx: &broadcast::Sender<CoreEvent>,
    registry: &Arc<Mutex<ReplayKnownThreadRootProjectionRegistry>>,
    key: &TimelineKey,
    projection: ThreadRootProjectionDto,
) -> bool {
    let mut registry = registry
        .lock()
        .expect("replay-known root registry lock must not be poisoned");
    if registry.owns_root(key, &projection.root_event_id) {
        registry.mark_hydration_terminal_suppressed(key, projection.root_event_id.clone());
        return false;
    }
    registry.mark_hydration_terminal_emitted(key, projection.root_event_id.clone());
    let _ = event_tx.send(CoreEvent::Timeline(TimelineEvent::ThreadRootProjection {
        key: key.clone(),
        projection,
    }));
    true
}

fn thread_root_activity_preview(item: &TimelineItem) -> Option<String> {
    let source = item
        .formatted
        .as_ref()
        .map(|formatted| formatted.plain_text.as_str())
        .or(item.body.as_deref())
        .or_else(|| item.media.as_ref().map(|media| media.filename.as_str()))?;
    collapsed_preview(source, REPLY_QUOTE_PREVIEW_MAX_CHARS)
}

/// Deserializes the public cache/network event just far enough to use the
/// same content-to-rendering functions as a canonical SDK timeline item. The
/// SDK's `EventTimelineItem` constructor is private, so deliberately do not
/// construct a second timeline merely for this projection.
fn message_projection_from_loaded_root_raw(raw: &serde_json::Value) -> Option<MessageProjection> {
    let content = raw.get("content")?.clone();
    match raw.get("type").and_then(serde_json::Value::as_str) {
        Some("m.room.message") => {
            let message = serde_json::from_value::<RoomMessageEventContent>(content).ok()?;
            Some(message_projection_from_msgtype(
                &message.msgtype,
                message.body(),
            ))
        }
        Some("m.sticker") => {
            let sticker = serde_json::from_value::<StickerEventContent>(content).ok()?;
            Some(sticker_projection_from_body(&sticker.body))
        }
        Some("m.room.encrypted") => Some(non_user_content_projection("Unable to decrypt message")),
        _ => None,
    }
}

/// Builds the normal reaction DTO from relation events already resident in the
/// event cache. This intentionally accepts only cached records and performs
/// no relation lookup over the network.
fn reaction_groups_from_cached_relation_events(
    events: Vec<serde_json::Value>,
    target_event_id: &str,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
) -> Vec<ReactionGroup> {
    let mut groups: BTreeMap<String, BTreeMap<String, Option<String>>> = BTreeMap::new();

    for event in events {
        if event.get("type").and_then(serde_json::Value::as_str) != Some("m.reaction") {
            continue;
        }
        let Some(sender) = event
            .get("sender")
            .and_then(serde_json::Value::as_str)
            .filter(|sender| !sender.is_empty())
        else {
            continue;
        };
        let Some(relates_to) = event.pointer("/content/m.relates_to") else {
            continue;
        };
        if relates_to
            .get("rel_type")
            .and_then(serde_json::Value::as_str)
            != Some("m.annotation")
            || relates_to
                .get("event_id")
                .and_then(serde_json::Value::as_str)
                != Some(target_event_id)
        {
            continue;
        }
        let Some(key) = relates_to
            .get("key")
            .and_then(serde_json::Value::as_str)
            .filter(|key| !key.is_empty())
        else {
            continue;
        };
        let reaction_event_id = event
            .get("event_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        groups
            .entry(key.to_owned())
            .or_default()
            .entry(sender.to_owned())
            .or_insert(reaction_event_id);
    }

    groups
        .into_iter()
        .map(|(key, senders)| {
            let own_sender = own_user_id.map(matrix_sdk::ruma::UserId::as_str);
            ReactionGroup {
                key,
                count: senders.len().min(u32::MAX as usize) as u32,
                reacted_by_me: own_sender.is_some_and(|own| senders.contains_key(own)),
                my_reaction_event_id: own_sender
                    .and_then(|own| senders.get(own))
                    .cloned()
                    .flatten(),
                sender_preview: senders
                    .keys()
                    .take(3)
                    .cloned()
                    .map(|user_id| ReactionSender {
                        user_id,
                        display_label: None,
                    })
                    .collect(),
            }
        })
        .collect()
}

#[derive(Default)]
struct ThreadRootProjectionRenderContext {
    sender_label: Option<String>,
    sender_avatar: Option<AvatarImage>,
    reactions: Vec<ReactionGroup>,
}

/// Convert the cache/network event payload into a self-contained root DTO
/// without inserting it into the SDK timeline. `load_or_fetch_event` exposes a
/// public decrypted raw event, not the SDK-private `EventTimelineItem`; this
/// path therefore reuses the same message/media/formatted-body helpers as the
/// canonical conversion and augments it with cache-only profile/reaction data.
#[cfg(test)]
fn thread_root_projection_item_from_raw(
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    activity: &ThreadRootProjectionActivity,
    raw: serde_json::Value,
) -> Option<TimelineItem> {
    thread_root_projection_item_from_raw_with_context(
        key,
        own_user_id,
        activity,
        raw,
        ThreadRootProjectionRenderContext::default(),
    )
}

fn thread_root_projection_item_from_raw_with_context(
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    activity: &ThreadRootProjectionActivity,
    raw: serde_json::Value,
    context: ThreadRootProjectionRenderContext,
) -> Option<TimelineItem> {
    let event_id = raw.get("event_id")?.as_str()?.to_owned();
    if event_id != activity.root_event_id {
        return None;
    }
    let sender = raw
        .get("sender")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let timestamp_ms = raw
        .get("origin_server_ts")
        .and_then(serde_json::Value::as_u64);
    let content = raw.get("content").unwrap_or(&serde_json::Value::Null);
    let is_redacted = raw
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("redacted_because"))
        .is_some();
    let message_projection = message_projection_from_loaded_root_raw(&raw);
    let body = message_projection
        .as_ref()
        .and_then(|projection| projection.body.clone())
        .or_else(|| {
            (raw.get("type").and_then(serde_json::Value::as_str) == Some("m.room.encrypted"))
                .then(|| "Unable to decrypt message".to_owned())
        });
    let notice_i18n = message_projection
        .as_ref()
        .and_then(|projection| projection.notice_i18n.clone());
    let message_kind = message_projection
        .as_ref()
        .map(|projection| projection.message_kind)
        .unwrap_or_default();
    let spoiler_spans = message_projection
        .as_ref()
        .map(|projection| projection.spoiler_spans.clone())
        .unwrap_or_default();
    let media = message_projection
        .as_ref()
        .and_then(|projection| projection.media.clone());
    let formatted = message_projection
        .as_ref()
        .and_then(|projection| projection.formatted.clone());
    let actionable_body = (!is_redacted)
        .then(|| {
            message_projection
                .as_ref()
                .filter(|projection| projection.body_is_user_content)
                .and_then(|projection| projection.body.as_deref())
        })
        .flatten();
    let id = TimelineItemId::Event {
        event_id: event_id.clone(),
    };
    let mut thread_summary = thread_summary_from_loaded_root_raw(&raw);
    let summary = thread_summary.get_or_insert(ThreadSummaryDto {
        reply_count: 1,
        latest_event_id: None,
        latest_sender: None,
        latest_sender_label: None,
        latest_body_preview: None,
        latest_timestamp_ms: None,
    });
    summary.reply_count = summary.reply_count.max(1);
    summary.latest_event_id = Some(activity.activity_event_id.clone());
    summary.latest_sender = activity.activity_sender.clone();
    summary.latest_sender_label = activity.activity_sender_label.clone();
    summary.latest_body_preview = activity.activity_body_preview.clone();
    summary.latest_timestamp_ms = activity.activity_timestamp_ms;

    Some(TimelineItem {
        id,
        sender: sender.clone(),
        sender_label: context.sender_label,
        sender_avatar: context.sender_avatar,
        body: body.clone(),
        notice_i18n,
        message_kind,
        spoiler_spans,
        timestamp_ms,
        in_reply_to_event_id: content
            .get("m.relates_to")
            .and_then(|relation| relation.get("m.in_reply_to"))
            .and_then(|reply| reply.get("event_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        formatted: formatted.clone(),
        reply_quote: None,
        thread_root: None,
        thread_summary,
        media: media.clone(),
        link_previews: None,
        link_ranges: link_ranges_for_message_projection(body.as_deref(), formatted.as_ref()),
        reactions: context.reactions,
        can_react: !is_redacted
            && timeline_content_is_renderable(body.as_deref(), media.as_ref(), formatted.as_ref()),
        is_redacted,
        // A loaded old root is deliberately visible even if it is a
        // non-message event; the terminal state must be observable rather
        // than triggering another history fetch.
        is_hidden: false,
        can_redact: !is_redacted
            && timeline_content_is_renderable(body.as_deref(), media.as_ref(), formatted.as_ref())
            && own_user_id
                .zip(sender.as_deref())
                .is_some_and(|(own, event_sender)| own.as_str() == event_sender),
        is_edited: false,
        can_edit: !is_redacted
            && actionable_body.is_some()
            && own_user_id
                .zip(sender.as_deref())
                .is_some_and(|(own, event_sender)| own.as_str() == event_sender),
        unable_to_decrypt: (raw.get("type").and_then(serde_json::Value::as_str)
            == Some("m.room.encrypted"))
        .then_some(TimelineUnableToDecrypt {
            session_id: None,
            reason: TimelineUnableToDecryptReason::Unknown,
            can_request_keys: false,
            recovery_stage: None,
            recovery_guidance: None,
        }),
        request_state: None,
        actions: message_actions_for_timeline_item(
            key.room_id(),
            &TimelineItemId::Event { event_id },
            actionable_body,
            media.is_some(),
            is_redacted,
        ),
        send_state: None,
    })
}

fn thread_summary_from_loaded_root_raw(raw: &serde_json::Value) -> Option<ThreadSummaryDto> {
    let summary = raw.get("unsigned")?.get("m.relations")?.get("m.thread")?;
    let latest = summary.get("latest_event");
    Some(ThreadSummaryDto {
        reply_count: summary
            .get("count")
            .and_then(serde_json::Value::as_u64)
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or(0),
        latest_event_id: latest
            .and_then(|event| event.get("event_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        latest_sender: latest
            .and_then(|event| event.get("sender"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        latest_sender_label: None,
        latest_body_preview: latest
            .and_then(|event| event.get("content"))
            .and_then(|content| content.get("body"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        latest_timestamp_ms: latest
            .and_then(|event| event.get("origin_server_ts"))
            .and_then(serde_json::Value::as_u64),
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ThreadAttentionCounters {
    notification_count: u64,
    highlight_count: u64,
    live_event_marker_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThreadAttentionObservation {
    Live,
    Backfill,
    Replay,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ThreadAttentionBatchProvenance {
    event_observations: HashMap<String, ThreadAttentionObservation>,
}

fn gap_repair_projections_from_sdk_diffs(
    diffs: &[eyeball_im::VectorDiff<Arc<SdkTimelineItem>>],
) -> BTreeSet<CausalProjectionId> {
    let mut projections = BTreeSet::new();
    let mut observe = |item: &Arc<SdkTimelineItem>| {
        if let Some(projection) = item
            .as_event()
            .and_then(EventTimelineItem::gap_repair_projection)
        {
            projections.insert(CausalProjectionId::decode_transport(projection));
        }
    };
    for diff in diffs {
        match diff {
            eyeball_im::VectorDiff::PushFront { value }
            | eyeball_im::VectorDiff::PushBack { value }
            | eyeball_im::VectorDiff::Insert { value, .. }
            | eyeball_im::VectorDiff::Set { value, .. } => observe(value),
            eyeball_im::VectorDiff::Reset { values }
            | eyeball_im::VectorDiff::Append { values } => {
                for value in values {
                    observe(value);
                }
            }
            eyeball_im::VectorDiff::Remove { .. }
            | eyeball_im::VectorDiff::Truncate { .. }
            | eyeball_im::VectorDiff::Clear
            | eyeball_im::VectorDiff::PopFront
            | eyeball_im::VectorDiff::PopBack => {}
        }
    }
    projections
}

fn thread_attention_observation_from_event_origin(
    origin: Option<EventItemOrigin>,
) -> ThreadAttentionObservation {
    match origin {
        Some(EventItemOrigin::Sync) => ThreadAttentionObservation::Live,
        Some(EventItemOrigin::Pagination) => ThreadAttentionObservation::Backfill,
        Some(EventItemOrigin::Cache) | Some(EventItemOrigin::Local) | None => {
            ThreadAttentionObservation::Replay
        }
    }
}

impl ThreadAttentionBatchProvenance {
    fn from_sdk_diffs(diffs: &[eyeball_im::VectorDiff<Arc<SdkTimelineItem>>]) -> Self {
        let mut provenance = Self::default();
        for diff in diffs {
            match diff {
                eyeball_im::VectorDiff::PushFront { value }
                | eyeball_im::VectorDiff::PushBack { value }
                | eyeball_im::VectorDiff::Insert { value, .. }
                | eyeball_im::VectorDiff::Set { value, .. } => {
                    provenance.observe_sdk_item(value, None);
                }
                // Reset and Append are replay/full-window shapes. Even if an
                // individual SDK item retains Sync origin, this delivery is
                // not evidence that it first arrived live in this actor.
                eyeball_im::VectorDiff::Reset { values }
                | eyeball_im::VectorDiff::Append { values } => {
                    for value in values {
                        provenance
                            .observe_sdk_item(value, Some(ThreadAttentionObservation::Replay));
                    }
                }
                eyeball_im::VectorDiff::Remove { .. }
                | eyeball_im::VectorDiff::Truncate { .. }
                | eyeball_im::VectorDiff::Clear
                | eyeball_im::VectorDiff::PopFront
                | eyeball_im::VectorDiff::PopBack => {}
            }
        }
        provenance
    }

    fn from_timeline_items(
        items: &[TimelineItem],
        observation: ThreadAttentionObservation,
    ) -> Self {
        let event_observations = items
            .iter()
            .filter_map(|item| match &item.id {
                TimelineItemId::Event { event_id } => Some((event_id.clone(), observation)),
                TimelineItemId::Transaction { .. } | TimelineItemId::Synthetic { .. } => None,
            })
            .collect();
        Self { event_observations }
    }

    fn observe_sdk_item(
        &mut self,
        item: &Arc<SdkTimelineItem>,
        forced: Option<ThreadAttentionObservation>,
    ) {
        let Some(event) = item.as_event() else {
            return;
        };
        let Some(event_id) = event.event_id() else {
            return;
        };
        let observation = forced
            .unwrap_or_else(|| thread_attention_observation_from_event_origin(event.origin()));
        self.event_observations
            .entry(event_id.to_string())
            .and_modify(|existing| {
                if *existing != observation {
                    *existing = ThreadAttentionObservation::Replay;
                }
            })
            .or_insert(observation);
    }

    fn observation_for(&self, event_id: &str) -> Option<ThreadAttentionObservation> {
        self.event_observations.get(event_id).copied()
    }
}

#[derive(Debug, Default)]
struct ThreadAttentionTracker {
    receipt_event_id: Option<String>,
    observed_reply_event_ids: HashSet<String>,
    attention_event_ids: HashSet<String>,
    counts: ThreadAttentionCounters,
}

impl ThreadAttentionTracker {
    fn hydrate(
        key: &TimelineKey,
        items: &[TimelineItem],
        own_user_id: Option<&str>,
        receipt_event_id: Option<String>,
    ) -> Self {
        let mut tracker = Self {
            receipt_event_id,
            ..Self::default()
        };
        tracker.observe_without_increment(key, items);
        if let (TimelineKind::Thread { root_event_id, .. }, Some(receipt_event_id)) =
            (&key.kind, tracker.receipt_event_id.as_deref())
        {
            if let Some(receipt_position) = items.iter().position(|item| {
                matches!(
                    &item.id,
                    TimelineItemId::Event { event_id } if event_id == receipt_event_id
                )
            }) {
                tracker.attention_event_ids.extend(
                    items
                        .iter()
                        .skip(receipt_position.saturating_add(1))
                        .filter_map(|item| {
                            matching_remote_thread_reply_event_id(item, root_event_id, own_user_id)
                                .map(str::to_owned)
                        }),
                );
                tracker.refresh_counts();
            }
        }
        tracker
    }

    fn reconcile(
        &mut self,
        key: &TimelineKey,
        items: &[TimelineItem],
        own_user_id: Option<&str>,
        observation: ThreadAttentionObservation,
    ) -> Option<AppAction> {
        let provenance = ThreadAttentionBatchProvenance::from_timeline_items(items, observation);
        self.reconcile_batch(key, items, own_user_id, &provenance)
    }

    fn reconcile_batch(
        &mut self,
        key: &TimelineKey,
        items: &[TimelineItem],
        own_user_id: Option<&str>,
        provenance: &ThreadAttentionBatchProvenance,
    ) -> Option<AppAction> {
        let TimelineKind::Thread { root_event_id, .. } = &key.kind else {
            return None;
        };
        let previous = self.counts;
        let event_positions = items
            .iter()
            .enumerate()
            .filter_map(|(position, item)| match &item.id {
                TimelineItemId::Event { event_id } => Some((event_id.as_str(), position)),
                TimelineItemId::Transaction { .. } | TimelineItemId::Synthetic { .. } => None,
            })
            .collect::<HashMap<_, _>>();
        let receipt_position = self
            .receipt_event_id
            .as_deref()
            .and_then(|receipt_event_id| event_positions.get(receipt_event_id).copied());
        if let Some(receipt_position) = receipt_position {
            self.attention_event_ids.retain(|event_id| {
                event_positions
                    .get(event_id.as_str())
                    .is_none_or(|position| *position > receipt_position)
            });
        }

        for (position, item) in items.iter().enumerate() {
            let Some(stable_event_id) = matching_thread_reply_event_id(item, root_event_id) else {
                continue;
            };
            let Some(observation) = provenance.observation_for(stable_event_id) else {
                continue;
            };
            let is_authoritatively_unread =
                receipt_position.is_some_and(|receipt_position| position > receipt_position);
            let may_add_attention = observation == ThreadAttentionObservation::Live
                || (observation == ThreadAttentionObservation::Replay && is_authoritatively_unread);
            if !may_add_attention {
                self.observed_reply_event_ids
                    .insert(stable_event_id.to_owned());
                continue;
            }
            if self.observed_reply_event_ids.contains(stable_event_id) {
                continue;
            }
            if own_user_id.is_some_and(|own_user_id| item.sender.as_deref() == Some(own_user_id)) {
                self.observed_reply_event_ids
                    .insert(stable_event_id.to_owned());
                continue;
            }
            if receipt_position.is_some_and(|receipt_position| position <= receipt_position) {
                self.observed_reply_event_ids
                    .insert(stable_event_id.to_owned());
                continue;
            }
            if item.body.is_none() && item.media.is_none() {
                // A live encrypted reply can first arrive without renderable
                // content. Keep it eligible for the SDK's later decrypted Set.
                continue;
            }
            self.observed_reply_event_ids
                .insert(stable_event_id.to_owned());
            self.attention_event_ids.insert(stable_event_id.to_owned());
        }

        self.refresh_counts();
        (self.counts != previous)
            .then(|| thread_attention_action(self.counts, key))
            .flatten()
    }

    fn acknowledge(
        &mut self,
        key: &TimelineKey,
        items: &[TimelineItem],
        event_id: String,
    ) -> Option<AppAction> {
        self.receipt_event_id = Some(event_id.clone());
        let positions = items
            .iter()
            .enumerate()
            .filter_map(|(position, item)| match &item.id {
                TimelineItemId::Event { event_id } => Some((event_id.as_str(), position)),
                TimelineItemId::Transaction { .. } | TimelineItemId::Synthetic { .. } => None,
            })
            .collect::<HashMap<_, _>>();
        let receipt_position = positions.get(event_id.as_str()).copied();
        self.attention_event_ids.retain(|attention_event_id| {
            match (
                receipt_position,
                positions.get(attention_event_id.as_str()).copied(),
            ) {
                (Some(receipt_position), Some(attention_position)) => {
                    attention_position > receipt_position
                }
                // A receipt outside the retained window is authoritative as a
                // future baseline, but its ordering relative to retained
                // attention is unknown. Preserve the count until the SDK gives
                // us a correlatable canonical position.
                (None, _) => true,
                (Some(_), None) => false,
            }
        });
        self.refresh_counts();
        thread_attention_action(self.counts, key)
    }

    fn observe_without_increment(&mut self, key: &TimelineKey, items: &[TimelineItem]) {
        let TimelineKind::Thread { root_event_id, .. } = &key.kind else {
            return;
        };
        self.observed_reply_event_ids
            .extend(items.iter().filter_map(|item| {
                matching_thread_reply_event_id(item, root_event_id).map(str::to_owned)
            }));
    }

    fn refresh_counts(&mut self) {
        let count = self.attention_event_ids.len() as u64;
        self.counts.notification_count = count;
        self.counts.live_event_marker_count = count;
    }
}

impl TimelineActor {
    /// Detect Room thread replies whose root is not present in the canonical
    /// SDK item window. The projection service is deliberately out-of-band:
    /// this method never creates a VectorDiff, calls Room pagination, or
    /// asks the viewport/anchor path to materialize an event.
    async fn maybe_hydrate_missing_thread_roots(&mut self) {
        if !matches!(self.key.kind, TimelineKind::Room { .. }) {
            return;
        }

        let activities_by_root = self
            .navigation_items
            .iter()
            .filter_map(|item| thread_root_projection_activity_from_item(self.key.room_id(), item))
            .fold(HashMap::new(), |mut selected, activity| {
                let should_replace = selected
                    .get(&activity.root_event_id)
                    .is_none_or(|existing| activity_is_newer(&activity, existing));
                if should_replace {
                    selected.insert(activity.root_event_id.clone(), activity);
                }
                selected
            });
        let missing_activities = activities_by_root
            .values()
            .filter(|activity| !self.timeline_contains_event_id(&activity.root_event_id))
            .cloned()
            .collect();
        let _ = commit_prepared_thread_root_hydration_for_generation(
            &self.thread_root_projection_service,
            &self.replay_known_thread_root_projections,
            &self.timeline_actor_generations,
            &self.action_tx,
            &self.manager_tx,
            &self.event_tx,
            &self.key,
            self.actor_generation,
            self.own_user_id.clone(),
            PreparedThreadRootHydration {
                activities_by_root,
                missing_activities,
            },
        )
        .await;
    }
}

