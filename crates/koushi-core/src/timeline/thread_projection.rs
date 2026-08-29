use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use koushi_sdk::MatrixClientSession;
use koushi_state::{AppAction, AvatarImage, AvatarThumbnailState, OperationFailureKind};

use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::events::sticker::StickerEventContent;
use matrix_sdk_ui::timeline::{
    EventItemOrigin, EventTimelineItem, TimelineItem as SdkTimelineItem,
    resolve_thread_relation_aggregate,
};
use tokio::sync::mpsc;

use crate::causal_projection::CausalProjectionId;
use crate::event::{
    ReactionGroup, ReactionSender, ThreadSummaryDto, TimelineItem, TimelineItemId,
    TimelineUnableToDecrypt, TimelineUnableToDecryptReason, message_actions_for_timeline_item,
};
use crate::executor;
use crate::ids::{TimelineKey, TimelineKind};
use crate::threads_list::{
    AggregateRefresh, AggregateRefreshCause, ThreadRootProjectionActivity,
    ThreadRootProjectionDecision, ThreadRootProjectionRecord, ThreadRootProjectionRefreshResult,
    ThreadRootProjectionService, activity_is_newer, authoritative_thread_aggregate_from_sdk,
    classify_thread_list_error,
};

// BEGIN GENERATED SIBLING IMPORTS
use super::actor::{ThreadSummaryProjectionWake, TimelineActor};
use super::item_projection::{
    MessageProjection, eligible_activity_preview, is_attention_eligible_event,
    link_ranges_for_message_projection, message_projection_from_msgtype,
    non_user_content_projection, sticker_projection_from_body, timeline_content_is_renderable,
    timeline_item_event_id,
};
use super::manager::{TimelineManagerActor, TimelineMessage};
use super::navigation::TimelineActorGenerationGate;
use super::outbound_send::{
    matching_remote_thread_reply_event_id, matching_thread_reply_event_id, thread_attention_action,
};
// END GENERATED SIBLING IMPORTS

/// Manager-owned tasks for bounded root hydration and aggregate refresh.
/// Removing a task before a queued completion is handled makes the completion
/// stale by construction. The optional revision distinguishes an aggregate
/// worker from a root-hydration worker and fences an old aggregate completion
/// from removing a newer worker for the same root.
#[derive(Default)]
pub(super) struct ThreadRootProjectionFetchRegistry {
    tasks: HashMap<(String, String), (u64, Option<u64>, executor::JoinHandle<()>)>,
}

impl ThreadRootProjectionFetchRegistry {
    fn contains_hydration(
        &self,
        room_id: &str,
        root_event_id: &str,
        actor_generation: u64,
    ) -> bool {
        self.tasks
            .get(&(room_id.to_owned(), root_event_id.to_owned()))
            .is_some_and(|(generation, revision, _)| {
                *generation == actor_generation && revision.is_none()
            })
    }

    fn contains_aggregate(
        &self,
        room_id: &str,
        root_event_id: &str,
        actor_generation: u64,
        summary_revision: u64,
    ) -> bool {
        self.tasks
            .get(&(room_id.to_owned(), root_event_id.to_owned()))
            .is_some_and(|(generation, revision, _)| {
                *generation == actor_generation && *revision == Some(summary_revision)
            })
    }

    fn insert(
        &mut self,
        room_id: String,
        root_event_id: String,
        actor_generation: u64,
        summary_revision: Option<u64>,
        task: executor::JoinHandle<()>,
    ) {
        if let Some((_, _, previous)) = self.tasks.insert(
            (room_id, root_event_id),
            (actor_generation, summary_revision, task),
        ) {
            previous.abort();
        }
    }

    /// Returns false when unsubscribe, replacement, or a newer refresh already
    /// cancelled this worker; callers must ignore its late terminal message.
    fn take_completion(
        &mut self,
        room_id: &str,
        root_event_id: &str,
        actor_generation: u64,
        summary_revision: Option<u64>,
    ) -> bool {
        let key = (room_id.to_owned(), root_event_id.to_owned());
        if self
            .tasks
            .get(&key)
            .is_some_and(|(generation, revision, _)| {
                *generation == actor_generation && *revision == summary_revision
            })
        {
            self.tasks.remove(&key);
            true
        } else {
            false
        }
    }

    async fn abort_room(&mut self, room_id: &str) -> usize {
        let keys = self
            .tasks
            .keys()
            .filter(|(entry_room_id, _)| entry_room_id == room_id)
            .cloned()
            .collect::<Vec<_>>();
        let tasks = keys
            .into_iter()
            .filter_map(|key| self.tasks.remove(&key).map(|(_, _, task)| task))
            .collect::<Vec<_>>();
        let count = tasks.len();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
        count
    }

    pub(super) async fn abort_all(&mut self) {
        let tasks = self
            .tasks
            .drain()
            .map(|(_, (_, _, task))| task)
            .collect::<Vec<_>>();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
    }
}

/// Lifecycle registry for ready root snapshots copied from an actor's own
/// navigation cache during a bounded replay. This is separate from the
/// fetch-backed projection service: no SDK fetch was started for these roots,
/// but unsubscribe and shutdown still must emit a matching frontend clear.
impl TimelineManagerActor {
    pub(super) async fn handle_thread_root_projection_fetch_finished(
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
            None,
        ) || !self.timelines.contains_key(&key)
        {
            return;
        }
        let Ok(action_permit) = self.action_tx.clone().reserve_owned().await else {
            return;
        };
        let Some(_lease) = self
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
        let pending_refresh = record.pending_refresh();
        let wake = ThreadSummaryProjectionWake::Updated {
            root_event_id: record.activity.root_event_id.clone(),
            activity_revision: record.activity_revision,
            summary_revision: record.summary_revision,
        };
        action_permit.send(vec![thread_root_projection_action_from_record(&record)]);
        drop(service);
        self.publish_thread_summary_projection_wake(&key, wake);
        if let Some(refresh) = pending_refresh {
            self.start_aggregate_worker(
                &key,
                actor_generation,
                self.session
                    .as_ref()
                    .and_then(|session| session.client().user_id().map(ToOwned::to_owned)),
                refresh,
            );
        }
    }

    pub(super) async fn handle_aggregate_refresh_start(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
        refreshes: Vec<AggregateRefresh>,
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
        for refresh in refreshes {
            if refresh.hydrate_root {
                if self.thread_root_projection_fetches.contains_aggregate(
                    &refresh.activity.room_id,
                    &refresh.activity.root_event_id,
                    actor_generation,
                    refresh.summary_revision,
                ) {
                    continue;
                }
                if self.thread_root_projection_fetches.contains_hydration(
                    &refresh.activity.room_id,
                    &refresh.activity.root_event_id,
                    actor_generation,
                ) {
                    continue;
                }
                let should_start = self
                    .thread_root_projection_service
                    .lock()
                    .expect("thread-root projection service lock must not be poisoned")
                    .has_pending_attempt(&refresh.activity);
                if !should_start {
                    continue;
                }
                let task = spawn_thread_root_projection_fetch(
                    session.clone(),
                    key.clone(),
                    actor_generation,
                    own_user_id.clone(),
                    self.msg_tx.clone(),
                    refresh.activity.clone(),
                );
                self.thread_root_projection_fetches.insert(
                    refresh.activity.room_id.clone(),
                    refresh.activity.root_event_id.clone(),
                    actor_generation,
                    None,
                    task,
                );
            } else {
                self.start_aggregate_worker(&key, actor_generation, own_user_id.clone(), refresh);
            }
        }
    }

    pub(super) fn handle_thread_summary_activity_observed(
        &mut self,
        source_key: TimelineKey,
        actor_generation: u64,
        observation: ThreadSummaryActivityObservation,
    ) {
        let TimelineKind::Thread {
            room_id,
            root_event_id,
        } = &source_key.kind
        else {
            return;
        };
        if self
            .timeline_actor_generations
            .current_generation(&source_key)
            != Some(actor_generation)
            || !self.timelines.contains_key(&source_key)
        {
            return;
        }
        let room_key = TimelineKey::room(source_key.account_key.clone(), room_id.clone());
        let Some(room_actor_generation) = self
            .timeline_actor_generations
            .current_generation(&room_key)
        else {
            return;
        };
        if !self.timelines.contains_key(&room_key) {
            return;
        }
        let mut refreshes = Vec::new();
        {
            let mut service = self
                .thread_root_projection_service
                .lock()
                .expect("thread-root projection service lock must not be poisoned");
            match observation {
                ThreadSummaryActivityObservation::Activity(activity)
                    if activity.root_event_id == *root_event_id =>
                {
                    let decision = service.observe_live_activity(activity.clone());
                    let should_refresh = matches!(
                        decision,
                        ThreadRootProjectionDecision::StartFetch(_)
                            | ThreadRootProjectionDecision::ActivityUpdated(_)
                    );
                    if should_refresh {
                        let activity_active = service.activity_active(room_id, root_event_id);
                        let canonical_root_active =
                            service.canonical_root_active(room_id, root_event_id);
                        if let Some(refresh) = service
                            .schedule_aggregate_refresh_with_canonical_root(
                                &activity,
                                AggregateRefreshCause::SelectedActivity,
                                activity_active,
                                canonical_root_active,
                                false,
                            )
                        {
                            refreshes.push(refresh);
                        }
                    }
                }
                ThreadSummaryActivityObservation::Invalidated {
                    root_event_id: invalidated_root_event_id,
                    activity_event_id,
                } if invalidated_root_event_id == *root_event_id => {
                    let invalidated = service.invalidate_live_activity(
                        room_id,
                        root_event_id,
                        &activity_event_id,
                    );
                    if invalidated
                        && let Some(activity) = service.activity_for_root(room_id, root_event_id)
                    {
                        let activity_active = service.activity_active(room_id, root_event_id);
                        let canonical_root_active =
                            service.canonical_root_active(room_id, root_event_id);
                        if let Some(refresh) = service
                            .schedule_aggregate_refresh_with_canonical_root(
                                &activity,
                                AggregateRefreshCause::Removal,
                                activity_active,
                                canonical_root_active,
                                false,
                            )
                        {
                            refreshes.push(refresh);
                        }
                    }
                }
                _ => return,
            }
        }
        let own_user_id = self
            .session
            .as_ref()
            .and_then(|session| session.client().user_id().map(ToOwned::to_owned));
        // Application travels through the Room actor's watch sender, never
        // through `TimelineActorHandle::send`.
        for refresh in refreshes {
            self.start_aggregate_worker(
                &room_key,
                room_actor_generation,
                own_user_id.clone(),
                refresh,
            );
        }
    }

    fn start_aggregate_worker(
        &mut self,
        key: &TimelineKey,
        actor_generation: u64,
        own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
        refresh: AggregateRefresh,
    ) {
        if !self.timelines.contains_key(key)
            || self.thread_root_projection_fetches.contains_hydration(
                &refresh.activity.room_id,
                &refresh.activity.root_event_id,
                actor_generation,
            )
            || self.thread_root_projection_fetches.contains_aggregate(
                &refresh.activity.room_id,
                &refresh.activity.root_event_id,
                actor_generation,
                refresh.summary_revision,
            )
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        let task = spawn_aggregate_refresh(
            session,
            key.clone(),
            actor_generation,
            own_user_id,
            self.msg_tx.clone(),
            refresh.clone(),
        );
        self.thread_root_projection_fetches.insert(
            refresh.activity.room_id.clone(),
            refresh.activity.root_event_id.clone(),
            actor_generation,
            Some(refresh.summary_revision),
            task,
        );
    }

    pub(super) async fn handle_aggregate_refresh_finished(
        &mut self,
        key: TimelineKey,
        actor_generation: u64,
        refresh: AggregateRefresh,
        result: Result<ThreadRootProjectionRefreshResult, OperationFailureKind>,
    ) {
        if !self.thread_root_projection_fetches.take_completion(
            &refresh.activity.room_id,
            &refresh.activity.root_event_id,
            actor_generation,
            Some(refresh.summary_revision),
        ) || !self.timelines.contains_key(&key)
        {
            return;
        }
        let Some(_lease) = self
            .timeline_actor_generations
            .try_acquire(&key, actor_generation)
        else {
            return;
        };
        let completion = self
            .thread_root_projection_service
            .lock()
            .expect("thread-root projection service lock must not be poisoned")
            .complete_refresh(&refresh, result);
        match completion {
            crate::threads_list::ThreadRootProjectionCompletion::Updated(record) => {
                let wake = ThreadSummaryProjectionWake::Updated {
                    root_event_id: record.activity.root_event_id.clone(),
                    activity_revision: record.activity_revision,
                    summary_revision: record.summary_revision,
                };
                if !refresh.canonical_root_active {
                    let Ok(action_permit) = self.action_tx.clone().reserve_owned().await else {
                        return;
                    };
                    action_permit.send(vec![thread_root_projection_action_from_record(&record)]);
                }
                self.publish_thread_summary_projection_wake(&key, wake);
            }
            crate::threads_list::ThreadRootProjectionCompletion::Cleared(activity) => {
                let Ok(action_permit) = self.action_tx.clone().reserve_owned().await else {
                    return;
                };
                action_permit.send(vec![AppAction::ThreadRootProjectionCleared {
                    room_id: activity.room_id.clone(),
                    root_event_id: activity.root_event_id.clone(),
                }]);
                self.publish_thread_summary_projection_wake(
                    &key,
                    ThreadSummaryProjectionWake::Cleared {
                        root_event_id: activity.root_event_id,
                        activity_revision: refresh.activity_revision,
                        summary_revision: refresh.summary_revision,
                    },
                );
            }
            crate::threads_list::ThreadRootProjectionCompletion::Ignored => {}
        }
    }

    fn publish_thread_summary_projection_wake(
        &self,
        key: &TimelineKey,
        wake: ThreadSummaryProjectionWake,
    ) {
        for (timeline_key, actor) in &self.timelines {
            if timeline_key.account_key == key.account_key
                && timeline_key.room_id() == key.room_id()
                && matches!(timeline_key.kind, TimelineKind::Room { .. })
            {
                actor.thread_summary_projection().publish(wake.clone());
            }
        }
    }

    pub(super) async fn clear_thread_root_projections_for_room(&mut self, key: &TimelineKey) {
        if !matches!(key.kind, TimelineKind::Room { .. }) {
            return;
        }
        self.timeline_actor_generations
            .invalidate_and_quiesce(key)
            .await;
        let room_id = key.room_id();
        self.thread_root_projection_fetches
            .abort_room(room_id)
            .await;
        self.thread_root_projection_service
            .lock()
            .expect("thread-root projection service lock must not be poisoned")
            .clear_room(room_id);
        let _ = self
            .emit_action_reliable(AppAction::ThreadRootProjectionsCleared {
                room_id: room_id.to_owned(),
            })
            .await;
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

fn spawn_aggregate_refresh(
    session: Arc<MatrixClientSession>,
    key: TimelineKey,
    actor_generation: u64,
    own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
    manager_tx: mpsc::Sender<TimelineMessage>,
    refresh: AggregateRefresh,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        let result =
            resolve_aggregate_refresh(&session, &key, own_user_id.as_deref(), &refresh).await;
        let _ = manager_tx
            .send(TimelineMessage::AggregateRefreshFinished {
                key,
                actor_generation,
                refresh,
                result,
            })
            .await;
    })
}

async fn resolve_aggregate_refresh(
    session: &MatrixClientSession,
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    refresh: &AggregateRefresh,
) -> Result<ThreadRootProjectionRefreshResult, OperationFailureKind> {
    let room_id = matrix_sdk::ruma::RoomId::parse(refresh.activity.room_id.as_str())
        .map_err(|_| OperationFailureKind::Invalid)?;
    let root_event_id = matrix_sdk::ruma::EventId::parse(refresh.activity.root_event_id.as_str())
        .map_err(|_| OperationFailureKind::Invalid)?;
    let room = session
        .client()
        .get_room(&room_id)
        .ok_or(OperationFailureKind::NotFound)?;
    let item = if refresh.hydrate_root {
        Some(
            load_thread_root_projection_item_from_room(&room, key, own_user_id, &refresh.activity)
                .await?,
        )
    } else {
        None
    };
    let sdk_aggregate = resolve_thread_relation_aggregate(&room, &root_event_id)
        .await
        .map_err(|error| classify_thread_list_error(&error))?;
    let aggregate = authoritative_thread_aggregate_from_sdk(&sdk_aggregate);
    Ok(match item {
        Some(item) => ThreadRootProjectionRefreshResult::Hydrated { item, aggregate },
        None => ThreadRootProjectionRefreshResult::Aggregate(aggregate),
    })
}

struct PreparedThreadRootHydration {
    activities_by_root: HashMap<String, ThreadRootProjectionActivity>,
    missing_activities: Vec<ThreadRootProjectionActivity>,
    canonical_root_event_ids: HashSet<String>,
    redacted_activity_event_ids: HashSet<String>,
    /// `None` refreshes the full bounded initial/reprojection window. A live
    /// batch supplies only roots whose canonical root/reply items changed.
    refresh_root_event_ids: Option<HashSet<String>>,
}

#[allow(clippy::too_many_arguments)]
async fn commit_prepared_thread_root_hydration_for_generation(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    generations: &Arc<TimelineActorGenerationGate>,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    manager_tx: &mpsc::Sender<TimelineMessage>,
    key: &TimelineKey,
    actor_generation: u64,
    own_user_id: Option<matrix_sdk::ruma::OwnedUserId>,
    prepared: PreparedThreadRootHydration,
) -> bool {
    let current_missing_activities = prepared
        .missing_activities
        .into_iter()
        .map(|activity| (activity.root_event_id.clone(), activity))
        .collect::<HashMap<_, _>>();
    let current_activities = prepared.activities_by_root;
    let canonical_root_event_ids = prepared.canonical_root_event_ids;
    let redacted_activity_event_ids = prepared.redacted_activity_event_ids;
    let refresh_root_event_ids = prepared.refresh_root_event_ids;
    let previous_tracked_activities = service
        .lock()
        .expect("thread-root projection service lock must not be poisoned")
        .active_activities(key.room_id());
    let manager_capacity_needed = !current_activities.is_empty()
        || !current_missing_activities.is_empty()
        || !previous_tracked_activities.is_empty();
    let refresh_permit = if manager_capacity_needed {
        let Ok(permit) = manager_tx.clone().reserve_owned().await else {
            return false;
        };
        Some(permit)
    } else {
        None
    };
    // Manager capacity is reserved first. The reducer permit is the final
    // await, so hydration/aggregate work can never hold reducer capacity while
    // a manager message that needs that same reducer is ahead of it in the
    // mailbox.
    let Ok(action_permit) = action_tx.clone().reserve_owned().await else {
        return false;
    };
    let Some(_lease) = generations.try_acquire(key, actor_generation) else {
        return false;
    };
    let mut actions = Vec::new();
    let mut refreshes = Vec::new();
    let mut service_guard = service
        .lock()
        .expect("thread-root projection service lock must not be poisoned");
    service_guard.set_canonical_root_event_ids(key.room_id(), &canonical_root_event_ids);
    for (root_event_id, previous_activity) in &previous_tracked_activities {
        if redacted_activity_event_ids.contains(&previous_activity.activity_event_id) {
            service_guard.invalidate_live_activity(
                key.room_id(),
                root_event_id,
                &previous_activity.activity_event_id,
            );
        }
    }
    let changed_root_event_ids =
        service_guard.reconcile_room_activities(key.room_id(), &current_activities);
    for activity in current_activities.values() {
        let was_tracked = previous_tracked_activities.contains_key(&activity.root_event_id);
        let decision = service_guard.observe(activity.clone());
        match decision {
            ThreadRootProjectionDecision::StartFetch(activity) => {
                if !canonical_root_event_ids.contains(&activity.root_event_id) {
                    actions.push(AppAction::ThreadRootProjectionObserved {
                        room_id: activity.room_id.clone(),
                        root_event_id: activity.root_event_id.clone(),
                        activity_event_id: activity.activity_event_id.clone(),
                        activity_timestamp_ms: activity.activity_timestamp_ms,
                    });
                }
            }
            ThreadRootProjectionDecision::ActivityUpdated(record)
            | ThreadRootProjectionDecision::Existing(record) => {
                if !canonical_root_event_ids.contains(&activity.root_event_id) {
                    actions.push(thread_root_projection_action_from_record(&record));
                }
            }
            ThreadRootProjectionDecision::Retired => continue,
        }
        let cause = if !was_tracked {
            AggregateRefreshCause::InitialHydration
        } else if changed_root_event_ids.contains(&activity.root_event_id) {
            AggregateRefreshCause::SelectedActivity
        } else {
            AggregateRefreshCause::CanonicalBatch
        };
        let canonical_root_active = canonical_root_event_ids.contains(&activity.root_event_id);
        let should_refresh = !was_tracked
            || refresh_root_event_ids
                .as_ref()
                .is_none_or(|roots| roots.contains(&activity.root_event_id));
        if should_refresh
            && let Some(refresh) = service_guard.schedule_aggregate_refresh_with_canonical_root(
                activity,
                cause,
                true,
                canonical_root_active,
                false,
            )
        {
            refreshes.push(refresh);
        }
    }
    for (root_event_id, activity) in &previous_tracked_activities {
        if current_activities.contains_key(root_event_id)
            || refresh_root_event_ids
                .as_ref()
                .is_some_and(|roots| !roots.contains(root_event_id))
        {
            continue;
        }
        let canonical_root_active = canonical_root_event_ids.contains(root_event_id);
        let cause = if canonical_root_active {
            AggregateRefreshCause::CanonicalBatch
        } else {
            AggregateRefreshCause::Removal
        };
        if let Some(refresh) = service_guard.schedule_aggregate_refresh_with_canonical_root(
            activity,
            cause,
            false,
            canonical_root_active,
            false,
        ) {
            refreshes.push(refresh);
        }
    }
    action_permit.send(actions);
    drop(service_guard);
    if let Some(permit) = refresh_permit
        && !refreshes.is_empty()
    {
        permit.send(TimelineMessage::StartAggregateRefresh {
            key: key.clone(),
            actor_generation,
            own_user_id,
            refreshes,
        });
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

pub(super) fn seed_thread_summary_item(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    key: &TimelineKey,
    item: &TimelineItem,
) {
    if !matches!(key.kind, TimelineKind::Room { .. }) {
        return;
    }
    if item.thread_root.is_some() || item.thread_summary.is_none() {
        return;
    }
    service
        .lock()
        .expect("thread-root projection service lock must not be poisoned")
        .seed_canonical_root(key.room_id(), item);
}

pub(super) fn seed_thread_summary_diff(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    key: &TimelineKey,
    diff: &crate::event::TimelineDiff,
) {
    match diff {
        crate::event::TimelineDiff::PushFront { item }
        | crate::event::TimelineDiff::PushBack { item }
        | crate::event::TimelineDiff::Insert { item, .. }
        | crate::event::TimelineDiff::Set { item, .. } => {
            seed_thread_summary_item(service, key, item);
        }
        crate::event::TimelineDiff::Reset { items } => {
            for item in items {
                seed_thread_summary_item(service, key, item);
            }
        }
        crate::event::TimelineDiff::Remove { .. }
        | crate::event::TimelineDiff::Truncate { .. }
        | crate::event::TimelineDiff::Clear => {}
    }
}

pub(super) fn overlay_thread_summary_item(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    key: &TimelineKey,
    item: &TimelineItem,
) -> TimelineItem {
    let TimelineItemId::Event { event_id } = &item.id else {
        return item.clone();
    };
    if item.thread_root.is_some() {
        return item.clone();
    }
    let Some(aggregate) = service
        .lock()
        .expect("thread-root projection service lock must not be poisoned")
        .current_aggregate(key.room_id(), event_id)
    else {
        return item.clone();
    };
    thread_root_item_with_authoritative_aggregate(item, &aggregate)
}

pub(super) fn overlay_thread_summary_diff(
    service: &Arc<Mutex<ThreadRootProjectionService>>,
    key: &TimelineKey,
    diff: &mut crate::event::TimelineDiff,
) {
    match diff {
        crate::event::TimelineDiff::PushFront { item }
        | crate::event::TimelineDiff::PushBack { item }
        | crate::event::TimelineDiff::Insert { item, .. }
        | crate::event::TimelineDiff::Set { item, .. } => {
            *item = overlay_thread_summary_item(service, key, item);
        }
        crate::event::TimelineDiff::Reset { items } => {
            for item in items {
                *item = overlay_thread_summary_item(service, key, item);
            }
        }
        crate::event::TimelineDiff::Remove { .. }
        | crate::event::TimelineDiff::Truncate { .. }
        | crate::event::TimelineDiff::Clear => {}
    }
}

pub(super) fn thread_root_item_with_authoritative_aggregate(
    item: &TimelineItem,
    aggregate: &crate::threads_list::AuthoritativeThreadAggregate,
) -> TimelineItem {
    let mut item = item.clone();
    let summary = item.thread_summary.get_or_insert(ThreadSummaryDto {
        reply_count: 0,
        latest_event_id: None,
        latest_sender: None,
        latest_sender_label: None,
        latest_body_preview: None,
        latest_timestamp_ms: None,
    });
    summary.reply_count = aggregate.reply_count;
    summary.latest_event_id = aggregate.latest_event_id.clone();
    summary.latest_sender = aggregate.latest_sender.clone();
    summary.latest_sender_label = aggregate.latest_sender_label.clone();
    summary.latest_body_preview = aggregate.latest_body_preview.clone();
    summary.latest_timestamp_ms = aggregate.latest_timestamp_ms;
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
    let room = session
        .client()
        .get_room(&room_id)
        .ok_or(OperationFailureKind::NotFound)?;
    load_thread_root_projection_item_from_room(&room, key, own_user_id, activity).await
}

async fn load_thread_root_projection_item_from_room(
    room: &matrix_sdk::Room,
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    activity: &ThreadRootProjectionActivity,
) -> Result<TimelineItem, OperationFailureKind> {
    let root_event_id = matrix_sdk::ruma::EventId::parse(activity.root_event_id.as_str())
        .map_err(|_| OperationFailureKind::Invalid)?;
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
    if !is_attention_eligible_event(item) {
        return None;
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ThreadSummaryActivityObservation {
    Activity(ThreadRootProjectionActivity),
    Invalidated {
        root_event_id: String,
        activity_event_id: String,
    },
}

pub(super) fn thread_summary_affected_root_event_ids(
    key: &TimelineKey,
    before: &[TimelineItem],
    after: &[TimelineItem],
) -> HashSet<String> {
    if !matches!(key.kind, TimelineKind::Room { .. }) {
        return HashSet::new();
    }
    let collect = |items: &[TimelineItem]| {
        items
            .iter()
            .filter_map(|item| {
                let event_id = timeline_item_event_id(item)?.to_owned();
                let root_event_id = item
                    .thread_root
                    .as_deref()
                    .filter(|root| !root.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| item.thread_summary.as_ref().map(|_| event_id.clone()))?;
                Some((event_id, (root_event_id, item.clone())))
            })
            .collect::<HashMap<_, _>>()
    };
    let before_by_event = collect(before);
    let after_by_event = collect(after);
    let mut affected = HashSet::new();
    for event_id in before_by_event.keys().chain(after_by_event.keys()) {
        let before = before_by_event.get(event_id);
        let after = after_by_event.get(event_id);
        if before == after {
            continue;
        }
        if let Some((root_event_id, _)) = before {
            affected.insert(root_event_id.clone());
        }
        if let Some((root_event_id, _)) = after {
            affected.insert(root_event_id.clone());
        }
    }
    affected
}

pub(super) fn thread_summary_observations_for_windows(
    key: &TimelineKey,
    before: &[TimelineItem],
    after: &[TimelineItem],
) -> Vec<ThreadSummaryActivityObservation> {
    let TimelineKind::Thread { root_event_id, .. } = &key.kind else {
        return Vec::new();
    };
    let collect = |items: &[TimelineItem]| {
        items
            .iter()
            .filter_map(|item| thread_root_projection_activity_from_item(key.room_id(), item))
            .filter(|activity| activity.root_event_id == *root_event_id)
            .map(|activity| (activity.activity_event_id.clone(), activity))
            .collect::<HashMap<_, _>>()
    };
    let before_by_event = collect(before);
    let after_by_event = collect(after);
    let after_items_by_event = after
        .iter()
        .filter_map(|item| Some((timeline_item_event_id(item)?.to_owned(), item)))
        .collect::<HashMap<_, _>>();
    let mut observations = Vec::new();
    for activity in after_by_event.values() {
        if before_by_event
            .get(&activity.activity_event_id)
            .is_none_or(|previous| previous != activity)
        {
            observations.push(ThreadSummaryActivityObservation::Activity(activity.clone()));
        }
    }
    for activity in before_by_event.values() {
        if !after_by_event.contains_key(&activity.activity_event_id)
            && after_items_by_event
                .get(&activity.activity_event_id)
                .is_some_and(|item| item.is_redacted)
        {
            observations.push(ThreadSummaryActivityObservation::Invalidated {
                root_event_id: activity.root_event_id.clone(),
                activity_event_id: activity.activity_event_id.clone(),
            });
        }
    }
    observations.sort_by(|left, right| {
        let left_key = match left {
            ThreadSummaryActivityObservation::Activity(activity) => (
                1u8,
                activity.root_event_id.as_str(),
                activity.activity_event_id.as_str(),
            ),
            ThreadSummaryActivityObservation::Invalidated {
                root_event_id,
                activity_event_id,
            } => (0u8, root_event_id.as_str(), activity_event_id.as_str()),
        };
        let right_key = match right {
            ThreadSummaryActivityObservation::Activity(activity) => (
                1u8,
                activity.root_event_id.as_str(),
                activity.activity_event_id.as_str(),
            ),
            ThreadSummaryActivityObservation::Invalidated {
                root_event_id,
                activity_event_id,
            } => (0u8, root_event_id.as_str(), activity_event_id.as_str()),
        };
        left_key.cmp(&right_key)
    });
    observations
}

/// The exact Room items currently represented by the bounded display replay.
///
/// `navigation_items` deliberately has a wider lifetime than the UI's replay
/// window. It may therefore contain a latest reply that was not rendered. A
/// replay-known root must be reconciled against this context, never the whole
/// navigation cache, or an unrelated cached reply can clear the visible root.
fn thread_root_activity_preview(item: &TimelineItem) -> Option<String> {
    eligible_activity_preview(item)
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
    let thread_summary = thread_summary_from_loaded_root_raw(&raw);

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
        display_metadata: None,
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
pub(super) struct ThreadAttentionCounters {
    pub(super) notification_count: u64,
    pub(super) highlight_count: u64,
    pub(super) live_event_marker_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ThreadAttentionObservation {
    Live,
    Backfill,
    Replay,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ThreadAttentionBatchProvenance {
    event_observations: HashMap<String, ThreadAttentionObservation>,
}

pub(super) fn gap_repair_projections_from_sdk_diffs(
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
    pub(super) fn from_sdk_diffs(diffs: &[eyeball_im::VectorDiff<Arc<SdkTimelineItem>>]) -> Self {
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

    pub(super) fn observation_for(&self, event_id: &str) -> Option<ThreadAttentionObservation> {
        self.event_observations.get(event_id).copied()
    }
}

#[derive(Debug, Default)]
pub(super) struct ThreadAttentionTracker {
    pub(super) receipt_event_id: Option<String>,
    pub(super) observed_reply_event_ids: HashSet<String>,
    pub(super) attention_event_ids: HashSet<String>,
    pub(super) counts: ThreadAttentionCounters,
}

impl ThreadAttentionTracker {
    pub(super) fn hydrate(
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

    pub(super) fn reconcile(
        &mut self,
        key: &TimelineKey,
        items: &[TimelineItem],
        own_user_id: Option<&str>,
        observation: ThreadAttentionObservation,
    ) -> Option<AppAction> {
        let provenance = ThreadAttentionBatchProvenance::from_timeline_items(items, observation);
        self.reconcile_batch(key, items, own_user_id, &provenance)
    }

    pub(super) fn reconcile_batch(
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
        let eligible_reply_event_ids = items
            .iter()
            .filter(|item| is_attention_eligible_event(item))
            .filter_map(|item| matching_thread_reply_event_id(item, root_event_id))
            .collect::<HashSet<_>>();
        self.attention_event_ids
            .retain(|event_id| eligible_reply_event_ids.contains(event_id.as_str()));
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
            if !is_attention_eligible_event(item) {
                continue;
            }
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
            self.observed_reply_event_ids
                .insert(stable_event_id.to_owned());
            self.attention_event_ids.insert(stable_event_id.to_owned());
        }

        self.refresh_counts();
        (self.counts != previous)
            .then(|| thread_attention_action(self.counts, key))
            .flatten()
    }

    pub(super) fn acknowledge(
        &mut self,
        key: &TimelineKey,
        items: &[TimelineItem],
        event_id: String,
    ) -> Option<AppAction> {
        let TimelineKind::Thread { root_event_id, .. } = &key.kind else {
            return None;
        };
        let eligible_reply_event_ids = items
            .iter()
            .filter(|item| is_attention_eligible_event(item))
            .filter_map(|item| matching_thread_reply_event_id(item, root_event_id))
            .collect::<HashSet<_>>();
        self.attention_event_ids.retain(|attention_event_id| {
            eligible_reply_event_ids.contains(attention_event_id.as_str())
        });
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
        self.observed_reply_event_ids.extend(
            items
                .iter()
                .filter(|item| is_attention_eligible_event(item))
                .filter_map(|item| {
                    matching_thread_reply_event_id(item, root_event_id).map(str::to_owned)
                }),
        );
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
    pub(super) async fn maybe_hydrate_missing_thread_roots(
        &mut self,
        refresh_root_event_ids: Option<HashSet<String>>,
    ) {
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
        let canonical_root_event_ids = self
            .navigation_items
            .iter()
            .filter(|item| item.thread_root.is_none() && item.thread_summary.is_some())
            .filter_map(timeline_item_event_id)
            .map(ToOwned::to_owned)
            .collect();
        let redacted_activity_event_ids = self
            .navigation_items
            .iter()
            .filter(|item| item.is_redacted)
            .filter_map(timeline_item_event_id)
            .map(ToOwned::to_owned)
            .collect();
        let committed = commit_prepared_thread_root_hydration_for_generation(
            &self.thread_root_projection_service,
            &self.timeline_actor_generations,
            &self.action_tx,
            &self.manager_tx,
            &self.key,
            self.actor_generation,
            self.own_user_id.clone(),
            PreparedThreadRootHydration {
                activities_by_root,
                missing_activities,
                canonical_root_event_ids,
                redacted_activity_event_ids,
                refresh_root_event_ids,
            },
        )
        .await;
        if committed {
            let display_diffs = self.reproject_display_items();
            if !display_diffs.is_empty() {
                let batch_id = self.next_batch_id;
                if super::navigation::emit_items_updated_for_generation(
                    &self.event_tx,
                    &self.timeline_actor_generations,
                    &self.key,
                    self.actor_generation,
                    self.generation,
                    batch_id,
                    display_diffs,
                ) {
                    self.next_batch_id = crate::ids::TimelineBatchId(batch_id.0 + 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_source::item_body;

    use std::collections::{HashMap, HashSet};

    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use koushi_state::{AppAction, OperationFailureKind};

    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_ui::timeline::{
        EventItemOrigin, TimelineDetails, TimelineEventItemId, TimelineItemContent,
    };
    use tokio::sync::{broadcast, mpsc};

    use crate::event::{
        CoreEvent, ThreadSummaryDto, TimelineDiff, TimelineEvent, TimelineItem, TimelineItemId,
        TimelineMediaKind, TimelineMessageActions,
    };
    use crate::executor;

    use crate::ids::{TimelineBatchId, TimelineGeneration, TimelineKey, TimelineKind};
    use crate::threads_list::{
        AggregateRefreshCause, ThreadRootProjectionActivity, ThreadRootProjectionDecision,
        ThreadRootProjectionService,
    };

    use std::future::poll_fn;

    use matrix_sdk::ruma::{OwnedUserId, uint};

    use super::super::actor::{
        ThreadSummaryProjectionIngress, TimelineActorHandle, TimelineActorMessage,
        emit_app_action_reliable,
    };
    use super::super::display_projection::apply_timeline_diffs_to_items;
    use super::super::item_projection::{
        megolm_session_fingerprint, thread_root_from_original_json, thread_summary_from_sdk,
        timeline_item_event_id, timeline_item_should_be_hidden_for_key,
    };
    use super::super::navigation::{
        TimelineActorGenerationGate, emit_timeline_events_for_generation,
    };
    use super::super::outbound_send::{
        newest_provable_receipt_event_id, thread_activity_observed_action,
        thread_activity_observed_action_for_batch,
    };
    use super::super::test_support::{
        focused_key, live_tail_test_manager, room_key, test_timeline_actor_handle, thread_key,
        timeline_item,
    };
    use crate::threads_list::AuthoritativeThreadAggregate;

    use super::{
        ThreadAttentionBatchProvenance, ThreadAttentionCounters, ThreadAttentionObservation,
        ThreadAttentionTracker, ThreadRootProjectionFetchRegistry, overlay_thread_summary_diff,
        reaction_groups_from_cached_relation_events,
        thread_attention_observation_from_event_origin,
        thread_root_item_with_authoritative_aggregate, thread_root_projection_activity_from_item,
        thread_root_projection_item_from_raw, thread_summary_affected_root_event_ids,
    };

    #[test]
    fn thread_activity_promotion_requires_a_matching_event_backed_reply() {
        let key = thread_key();
        let matching = thread_reply_item("$reply:test", "@b:test", "$root:test");
        assert_eq!(
            thread_activity_observed_action(&key, std::slice::from_ref(&matching)),
            Some(AppAction::ThreadActivityObserved {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
            })
        );
        let live_batch = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&matching),
            ThreadAttentionObservation::Live,
        );
        assert_eq!(
            thread_activity_observed_action_for_batch(
                &key,
                std::slice::from_ref(&matching),
                &live_batch,
            ),
            Some(AppAction::ThreadActivityObserved {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
            })
        );
        assert_eq!(
            thread_activity_observed_action_for_batch(
                &key,
                std::slice::from_ref(&matching),
                &ThreadAttentionBatchProvenance::default(),
            ),
            None
        );

        let mut local_echo = matching;
        local_echo.id = TimelineItemId::Transaction {
            transaction_id: "txn".to_owned(),
        };
        assert_eq!(thread_activity_observed_action(&key, &[local_echo]), None);
        assert_eq!(
            thread_activity_observed_action(
                &key,
                &[thread_reply_item(
                    "$other:test",
                    "@b:test",
                    "$other-root:test",
                )],
            ),
            None
        );
        assert_eq!(
            thread_activity_observed_action(
                &room_key(),
                &[thread_reply_item("$reply:test", "@b:test", "$root:test",)]
            ),
            None
        );
    }

    #[test]
    fn thread_and_focused_items_do_not_claim_room_canonical_summary_ownership() {
        let service = Arc::new(Mutex::new(ThreadRootProjectionService::default()));
        let mut root = timeline_item("$root:test", Some("root"), "@root:test", false);
        root.thread_summary = Some(ThreadSummaryDto {
            reply_count: 1,
            latest_event_id: Some("$reply:test".to_owned()),
            latest_sender: None,
            latest_sender_label: None,
            latest_body_preview: Some("reply".to_owned()),
            latest_timestamp_ms: Some(100),
        });
        super::seed_thread_summary_item(&service, &thread_key(), &root);
        assert!(
            service
                .lock()
                .expect("service lock")
                .current_aggregate("!r:test", "$root:test")
                .is_none()
        );
    }

    #[test]
    fn newer_sdk_summary_is_detected_before_overlay_and_repaired_by_exact_aggregate() {
        let key = room_key();
        let service = Arc::new(Mutex::new(ThreadRootProjectionService::default()));
        let mut root_a = timeline_item("$root:test", Some("root"), "@root:test", false);
        root_a.thread_summary = Some(ThreadSummaryDto {
            reply_count: 1,
            latest_event_id: Some("$reply-a:test".to_owned()),
            latest_sender: Some("@a:test".to_owned()),
            latest_sender_label: Some("A".to_owned()),
            latest_body_preview: Some("A".to_owned()),
            latest_timestamp_ms: Some(100),
        });
        super::seed_thread_summary_item(&service, &key, &root_a);

        let mut root_b = root_a.clone();
        root_b.thread_summary = Some(ThreadSummaryDto {
            reply_count: 2,
            latest_event_id: Some("$reply-b:test".to_owned()),
            latest_sender: Some("@b:test".to_owned()),
            latest_sender_label: Some("B".to_owned()),
            latest_body_preview: Some("B".to_owned()),
            latest_timestamp_ms: Some(200),
        });
        let raw_diff = TimelineDiff::Set {
            index: 0,
            item: root_b.clone(),
        };
        let mut raw_after = vec![root_a.clone()];
        apply_timeline_diffs_to_items(&mut raw_after, std::slice::from_ref(&raw_diff));
        assert_eq!(
            thread_summary_affected_root_event_ids(&key, &[root_a.clone()], &raw_after),
            HashSet::from(["$root:test".to_owned()])
        );

        // The bundled identity is provisional (it may be an edit event), so
        // overlay retains A until the exact event-cache aggregate validates B.
        super::seed_thread_summary_diff(&service, &key, &raw_diff);
        let mut overlaid_diff = raw_diff;
        overlay_thread_summary_diff(&service, &key, &mut overlaid_diff);
        let TimelineDiff::Set { item, .. } = &overlaid_diff else {
            panic!("expected root Set")
        };
        assert_eq!(
            item.thread_summary
                .as_ref()
                .and_then(|summary| summary.latest_event_id.as_deref()),
            Some("$reply-a:test")
        );

        let activity = service
            .lock()
            .expect("service lock")
            .activity_for_root(key.room_id(), "$root:test")
            .expect("tracked root");
        let refresh = service
            .lock()
            .expect("service lock")
            .schedule_aggregate_refresh_with_canonical_root(
                &activity,
                AggregateRefreshCause::CanonicalBatch,
                true,
                true,
                false,
            )
            .expect("aggregate refresh");
        assert!(matches!(
            service.lock().expect("service lock").complete_refresh(
                &refresh,
                Ok(
                    crate::threads_list::ThreadRootProjectionRefreshResult::Aggregate(
                        AuthoritativeThreadAggregate {
                            reply_count: 2,
                            latest_event_id: Some("$reply-b:test".to_owned()),
                            latest_sender: Some("@b:test".to_owned()),
                            latest_sender_label: Some("B".to_owned()),
                            latest_body_preview: Some("B".to_owned()),
                            latest_timestamp_ms: Some(200),
                        },
                    )
                ),
            ),
            crate::threads_list::ThreadRootProjectionCompletion::Updated(_)
        ));
        let mut validated_diff = TimelineDiff::Set {
            index: 0,
            item: root_b,
        };
        overlay_thread_summary_diff(&service, &key, &mut validated_diff);
        let TimelineDiff::Set { item, .. } = validated_diff else {
            panic!("expected validated root Set")
        };
        assert_eq!(
            item.thread_summary
                .as_ref()
                .and_then(|summary| summary.latest_event_id.as_deref()),
            Some("$reply-b:test")
        );
        assert_eq!(
            item.thread_summary
                .as_ref()
                .map(|summary| summary.reply_count),
            Some(2)
        );
    }

    #[tokio::test]
    async fn canonical_completion_bypasses_a_full_room_mailbox_via_projection_watch() {
        let key = room_key();
        let (actor_tx, _actor_rx) = mpsc::channel(1);
        actor_tx
            .try_send(TimelineActorMessage::OwnReadReceiptChanged)
            .expect("fill ordinary Room actor mailbox");
        let (projection, projection_rx) = ThreadSummaryProjectionIngress::channel();
        let mut manager = live_tail_test_manager(HashMap::from([(
            key.clone(),
            TimelineActorHandle {
                tx: actor_tx,
                control_tx: None,
                thread_summary_projection: projection,
                position_rx: None,
                task: None,
                auxiliary_tasks: Vec::new(),
                subscription_generation: None,
                enqueue_context: None,
            },
        )]));
        let actor_generation = manager
            .timeline_actor_generations
            .activate_after_quiescence(&key)
            .await
            .generation;
        let activity = ThreadRootProjectionActivity {
            room_id: key.room_id().to_owned(),
            root_event_id: "$root:test".to_owned(),
            activity_event_id: "$reply-b:test".to_owned(),
            activity_timestamp_ms: Some(200),
            activity_sender: Some("@b:test".to_owned()),
            activity_sender_label: Some("B".to_owned()),
            activity_body_preview: Some("B".to_owned()),
        };
        let refresh = {
            let mut service = manager
                .thread_root_projection_service
                .lock()
                .expect("service lock");
            assert!(matches!(
                service.observe(activity.clone()),
                ThreadRootProjectionDecision::StartFetch(_)
            ));
            service.set_canonical_root_event_ids(
                key.room_id(),
                &HashSet::from([activity.root_event_id.clone()]),
            );
            service
                .schedule_aggregate_refresh_with_canonical_root(
                    &activity,
                    AggregateRefreshCause::SelectedActivity,
                    true,
                    true,
                    false,
                )
                .expect("canonical refresh")
        };
        manager.thread_root_projection_fetches.insert(
            activity.room_id.clone(),
            activity.root_event_id.clone(),
            actor_generation,
            Some(refresh.summary_revision),
            executor::spawn(async { std::future::pending::<()>().await }),
        );

        executor::timeout(
            Duration::from_millis(100),
            manager.handle_aggregate_refresh_finished(
                key,
                actor_generation,
                refresh,
                Ok(
                    crate::threads_list::ThreadRootProjectionRefreshResult::Aggregate(
                        AuthoritativeThreadAggregate {
                            reply_count: 2,
                            latest_event_id: Some(activity.activity_event_id.clone()),
                            latest_sender: activity.activity_sender.clone(),
                            latest_sender_label: activity.activity_sender_label.clone(),
                            latest_body_preview: activity.activity_body_preview.clone(),
                            latest_timestamp_ms: activity.activity_timestamp_ms,
                        },
                    ),
                ),
            ),
        )
        .await
        .expect("manager must not wait for ordinary Room actor capacity");
        let pending = projection_rx.borrow();
        let wake = pending
            .get(&activity.root_event_id)
            .expect("accepted canonical completion wake");
        assert!(matches!(
            wake,
            super::ThreadSummaryProjectionWake::Updated {
                activity_revision: 1,
                summary_revision: 1,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn actor_owner_generation_remains_monotonic_across_manager_gate_recreation() {
        let key = focused_key();
        let first_gate = TimelineActorGenerationGate::default();
        let first = first_gate.activate_after_quiescence(&key).await.generation;
        drop(first_gate);

        let replacement_gate = TimelineActorGenerationGate::default();
        let replacement = replacement_gate
            .activate_after_quiescence(&key)
            .await
            .generation;
        assert!(replacement > first);
    }

    #[tokio::test]
    async fn stale_actor_generation_cannot_emit_any_timeline_event_after_replacement() {
        let key = room_key();
        let actor_generations = Arc::new(TimelineActorGenerationGate::default());
        let old_generation = actor_generations
            .activate_after_quiescence(&key)
            .await
            .generation;
        let old_lease = actor_generations
            .try_acquire(&key, old_generation)
            .expect("old actor lease");
        let replacement_gate = actor_generations.clone();
        let replacement_key = key.clone();
        let replacement = tokio::spawn(async move {
            replacement_gate
                .activate_after_quiescence(&replacement_key)
                .await
        });
        for _ in 0..10 {
            if actor_generations
                .try_acquire(&key, old_generation)
                .is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            actor_generations
                .try_acquire(&key, old_generation)
                .is_none()
        );
        drop(old_lease);
        let new_generation = replacement.await.expect("replacement task").generation;

        let (event_tx, mut event_rx) = broadcast::channel(8);
        assert!(!emit_timeline_events_for_generation(
            &event_tx,
            &actor_generations,
            &key,
            old_generation,
            vec![TimelineEvent::ItemsUpdated {
                key: key.clone(),
                generation: TimelineGeneration(0),
                batch_id: TimelineBatchId(1),
                diffs: vec![TimelineDiff::PushBack {
                    item: timeline_item("$old-diff:test", Some("old"), "@a:test", false),
                }],
            }],
        ));
        assert!(!emit_timeline_events_for_generation(
            &event_tx,
            &actor_generations,
            &key,
            old_generation,
            vec![TimelineEvent::InitialItems {
                request_id: None,
                cause_request_id: None,
                key: key.clone(),
                actor_generation: old_generation,
                generation: TimelineGeneration(0),
                items: vec![timeline_item(
                    "$old-initial:test",
                    Some("old"),
                    "@a:test",
                    false
                )],
            }],
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        assert!(emit_timeline_events_for_generation(
            &event_tx,
            &actor_generations,
            &key,
            new_generation,
            vec![TimelineEvent::InitialItems {
                request_id: None,
                cause_request_id: None,
                key: key.clone(),
                actor_generation: new_generation,
                generation: TimelineGeneration(0),
                items: vec![timeline_item(
                    "$new-initial:test",
                    Some("new"),
                    "@a:test",
                    false
                )],
            }],
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(CoreEvent::Timeline(TimelineEvent::InitialItems { items, .. }))
                if items.iter().any(|item| timeline_item_event_id(item) == Some("$new-initial:test"))
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    fn timeline_message_item(event_id: &str, sender: &str) -> TimelineItem {
        TimelineItem {
            request_state: None,
            id: TimelineItemId::Event {
                event_id: event_id.to_owned(),
            },
            sender: Some(sender.to_owned()),
            sender_label: None,
            sender_avatar: None,
            body: Some("body".to_owned()),
            notice_i18n: None,
            message_kind: Default::default(),
            spoiler_spans: Vec::new(),
            timestamp_ms: Some(1),
            in_reply_to_event_id: None,
            formatted: None,
            reply_quote: None,
            thread_root: None,
            thread_summary: None,
            media: None,
            link_previews: None,
            link_ranges: Vec::new(),
            reactions: Vec::new(),
            can_react: true,
            is_redacted: false,
            is_hidden: false,
            can_redact: false,
            is_edited: false,
            can_edit: false,
            actions: TimelineMessageActions::default(),
            send_state: None,
            unable_to_decrypt: None,
            display_metadata: None,
        }
    }

    fn thread_reply_item(event_id: &str, sender: &str, root_event_id: &str) -> TimelineItem {
        TimelineItem {
            thread_root: Some(root_event_id.to_owned()),
            ..timeline_message_item(event_id, sender)
        }
    }

    #[test]
    fn old_root_reply_reaches_bounded_room_projection_hydration_without_pagination() {
        let mut reply = timeline_item(
            "$latest-reply:test",
            Some("new reply"),
            "@alice:test",
            false,
        );
        reply.timestamp_ms = Some(1_700_000_100_000);
        reply.thread_root = Some("$old-root:test".to_owned());

        let activity = thread_root_projection_activity_from_item("!room:test", &reply)
            .expect("a canonical Room reply must be observable for root hydration");
        assert_eq!(activity.root_event_id, "$old-root:test");
        assert_eq!(activity.activity_event_id, "$latest-reply:test");
        assert_eq!(activity.activity_timestamp_ms, Some(1_700_000_100_000));
    }

    #[tokio::test]
    async fn root_projection_actions_wait_for_reducer_capacity_instead_of_dropping() {
        let (action_tx, mut action_rx) = mpsc::channel(1);
        action_tx
            .try_send(vec![AppAction::ThreadRootProjectionsCleared {
                room_id: "!already-buffered:test".to_owned(),
            }])
            .expect("fill the reducer channel");

        let reliable_tx = action_tx.clone();
        let delivery = tokio::spawn(async move {
            emit_app_action_reliable(
                &reliable_tx,
                AppAction::ThreadRootProjectionsCleared {
                    room_id: "!must-arrive:test".to_owned(),
                },
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(
            !delivery.is_finished(),
            "the reliable sender must wait behind a full channel, not discard the projection transition"
        );
        let _ = action_rx.recv().await.expect("drain buffered action");
        assert!(delivery.await.expect("delivery task"));
        assert!(matches!(
            action_rx.recv().await,
            Some(actions) if matches!(
                actions.as_slice(),
                [AppAction::ThreadRootProjectionsCleared { room_id }]
                    if room_id == "!must-arrive:test"
            )
        ));
    }

    #[tokio::test]
    async fn root_projection_fetch_registry_aborts_room_workers_and_rejects_late_completion() {
        struct CancellationProbe(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for CancellationProbe {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task = executor::spawn(async move {
            let _probe = CancellationProbe(Some(cancelled_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        let mut registry = ThreadRootProjectionFetchRegistry::default();
        registry.insert(
            "!room:test".to_owned(),
            "$root:test".to_owned(),
            7,
            None,
            task,
        );
        started_rx
            .await
            .expect("worker must be in flight before cancellation");

        assert_eq!(registry.abort_room("!room:test").await, 1);
        tokio::time::timeout(Duration::from_secs(1), cancelled_rx)
            .await
            .expect("abort must end the in-flight hydration worker")
            .expect("worker cancellation probe should be delivered");
        assert!(
            !registry.take_completion("!room:test", "$root:test", 7, None),
            "a completion queued before unsubscribe must not publish a stale terminal state"
        );
    }

    #[tokio::test]
    async fn aggregate_start_preserves_fetch_finished_worker_and_failed_hydration_terminal() {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let session = Arc::new(koushi_sdk::MatrixClientSession::from_client_for_testing(
            client.clone(),
            koushi_state::SessionInfo {
                homeserver: server.server().uri(),
                user_id: client.user_id().expect("synthetic user id").to_string(),
                device_id: client.device_id().expect("synthetic device id").to_string(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        ));
        let key = room_key();
        let mut manager =
            live_tail_test_manager(HashMap::from([(key.clone(), test_timeline_actor_handle())]));
        manager.session = Some(session);
        let actor_generation = manager
            .timeline_actor_generations
            .activate_after_quiescence(&key)
            .await
            .generation;
        let activity = ThreadRootProjectionActivity {
            room_id: key.room_id().to_owned(),
            root_event_id: "$failed-root:test".to_owned(),
            activity_event_id: "$reply:test".to_owned(),
            activity_timestamp_ms: Some(100),
            activity_sender: None,
            activity_sender_label: None,
            activity_body_preview: None,
        };
        let refresh = {
            let mut service = manager
                .thread_root_projection_service
                .lock()
                .expect("service lock");
            assert!(matches!(
                service.observe(activity.clone()),
                ThreadRootProjectionDecision::StartFetch(_)
            ));
            let refresh = service
                .schedule_aggregate_refresh(
                    &activity,
                    AggregateRefreshCause::InitialHydration,
                    true,
                    false,
                )
                .expect("initial aggregate refresh");
            service.mark_failed(&activity, OperationFailureKind::NotFound);
            refresh
        };

        // FetchFinished has removed hydration and started this exact aggregate
        // worker before the original StartAggregateRefresh reaches the FIFO.
        manager.thread_root_projection_fetches.insert(
            activity.room_id.clone(),
            activity.root_event_id.clone(),
            actor_generation,
            None,
            executor::spawn(async { std::future::pending::<()>().await }),
        );
        assert!(manager.thread_root_projection_fetches.take_completion(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
            None,
        ));
        manager.thread_root_projection_fetches.insert(
            activity.room_id.clone(),
            activity.root_event_id.clone(),
            actor_generation,
            Some(refresh.summary_revision),
            executor::spawn(async { std::future::pending::<()>().await }),
        );
        assert!(manager.thread_root_projection_fetches.contains_aggregate(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
            refresh.summary_revision,
        ));

        manager
            .handle_aggregate_refresh_start(
                key.clone(),
                actor_generation,
                None,
                vec![refresh.clone()],
            )
            .await;
        assert!(manager.thread_root_projection_fetches.contains_aggregate(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
            refresh.summary_revision,
        ));
        assert!(!manager.thread_root_projection_fetches.contains_hydration(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
        ));

        assert!(manager.thread_root_projection_fetches.take_completion(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
            Some(refresh.summary_revision),
        ));
        assert!(matches!(
            manager
                .thread_root_projection_service
                .lock()
                .expect("service lock")
                .complete_refresh(&refresh, Err(OperationFailureKind::Network)),
            crate::threads_list::ThreadRootProjectionCompletion::Updated(record)
                if record.failure_kind() == Some(OperationFailureKind::Network)
        ));
        let service = manager
            .thread_root_projection_service
            .lock()
            .expect("service lock");
        assert!(!service.has_pending_attempt(&activity));
        drop(service);
        manager
            .handle_aggregate_refresh_start(key, actor_generation, None, vec![refresh])
            .await;
        assert!(!manager.thread_root_projection_fetches.contains_hydration(
            &activity.room_id,
            &activity.root_event_id,
            actor_generation,
        ));
    }

    #[test]
    fn loaded_old_root_raw_event_projects_renderable_snapshot_with_latest_activity_identity() {
        let activity = ThreadRootProjectionActivity {
            room_id: "!room:test".to_owned(),
            root_event_id: "$old-root:test".to_owned(),
            activity_event_id: "$latest-reply:test".to_owned(),
            activity_timestamp_ms: Some(1_700_000_100_000),
            activity_sender: Some("@latest:test".to_owned()),
            activity_sender_label: Some("Latest".to_owned()),
            activity_body_preview: Some("live reply preview".to_owned()),
        };
        let raw = serde_json::json!({
            "type": "m.room.message",
            "event_id": "$old-root:test",
            "sender": "@alice:test",
            "origin_server_ts": 1_700_000_000_000_u64,
            "content": { "msgtype": "m.text", "body": "old root body" },
            "unsigned": {
                "m.relations": {
                    "m.thread": {
                        "count": 3,
                        "latest_event": {
                            "event_id": "$stale-latest:test",
                            "sender": "@bob:test",
                            "origin_server_ts": 1_700_000_050_000_u64,
                            "content": { "body": "stale preview" }
                        }
                    }
                }
            }
        });

        let item = thread_root_projection_item_from_raw(&room_key(), None, &activity, raw)
            .expect("valid loaded root must yield a renderable snapshot");
        assert_eq!(timeline_item_event_id(&item), Some("$old-root:test"));
        assert_eq!(item.body.as_deref(), Some("old root body"));
        assert_eq!(item.timestamp_ms, Some(1_700_000_000_000));
        assert_eq!(item.thread_root, None);
        assert_eq!(
            item.thread_summary
                .as_ref()
                .and_then(|summary| summary.latest_event_id.as_deref()),
            Some("$stale-latest:test"),
            "raw bundled relation data is only provisional before Task A resolution"
        );
        assert_eq!(
            item.thread_summary
                .as_ref()
                .map(|summary| summary.reply_count),
            Some(3)
        );

        let authoritative = thread_root_item_with_authoritative_aggregate(
            &item,
            &AuthoritativeThreadAggregate {
                reply_count: 4,
                latest_event_id: Some(activity.activity_event_id.clone()),
                latest_sender: activity.activity_sender.clone(),
                latest_sender_label: activity.activity_sender_label.clone(),
                latest_body_preview: activity.activity_body_preview.clone(),
                latest_timestamp_ms: activity.activity_timestamp_ms,
            },
        );
        assert_eq!(
            authoritative
                .thread_summary
                .as_ref()
                .and_then(|summary| summary.latest_event_id.as_deref()),
            Some("$latest-reply:test")
        );
        assert_eq!(
            authoritative
                .thread_summary
                .as_ref()
                .map(|summary| summary.reply_count),
            Some(4)
        );
    }

    #[test]
    fn loaded_old_root_reuses_message_projection_for_formatted_spoiler_and_media_content() {
        let activity = ThreadRootProjectionActivity {
            room_id: "!room:test".to_owned(),
            root_event_id: "$old-root:test".to_owned(),
            activity_event_id: "$latest-reply:test".to_owned(),
            activity_timestamp_ms: Some(1_700_000_100_000),
            activity_sender: Some("@latest:test".to_owned()),
            activity_sender_label: Some("Latest".to_owned()),
            activity_body_preview: Some("live reply preview".to_owned()),
        };
        let raw = serde_json::json!({
            "event_id": "$old-root:test",
            "sender": "@alice:test",
            "origin_server_ts": 1_700_000_000_000u64,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "caption ||secret||",
                "filename": "image.png",
                "format": "org.matrix.custom.html",
                "formatted_body": "<strong>caption</strong> <span data-mx-spoiler=\"reason\">secret</span>",
                "url": "mxc://test/media",
                "info": {
                    "mimetype": "image/png",
                    "size": 42,
                    "w": 640,
                    "h": 480
                }
            }
        });

        let item = thread_root_projection_item_from_raw(&room_key(), None, &activity, raw)
            .expect("loaded image root must keep normal render fields");

        assert_eq!(
            item.formatted
                .as_ref()
                .map(|formatted| formatted.plain_text.as_str()),
            Some("caption secret")
        );
        assert!(
            item.spoiler_spans
                .iter()
                .any(|span| span.reason.as_deref() == Some("reason"))
        );
        let media = item
            .media
            .expect("image root must retain media renderer data");
        assert_eq!(media.kind, TimelineMediaKind::Image);
        assert_eq!(media.source.mxc_uri, "mxc://test/media");
        assert_eq!(media.width, Some(640));
        assert_eq!(media.height, Some(480));
    }

    #[test]
    fn loaded_old_root_reuses_message_projection_for_file_audio_and_sticker_content() {
        let activity = ThreadRootProjectionActivity {
            room_id: "!room:test".to_owned(),
            root_event_id: "$old-root:test".to_owned(),
            activity_event_id: "$latest-reply:test".to_owned(),
            activity_timestamp_ms: Some(1_700_000_100_000),
            activity_sender: Some("@latest:test".to_owned()),
            activity_sender_label: Some("Latest".to_owned()),
            activity_body_preview: Some("live reply preview".to_owned()),
        };

        let file = thread_root_projection_item_from_raw(
            &room_key(),
            None,
            &activity,
            serde_json::json!({
                "event_id": "$old-root:test",
                "sender": "@alice:test",
                "origin_server_ts": 1_700_000_000_000u64,
                "type": "m.room.message",
                "content": {
                    "msgtype": "m.file", "body": "report.pdf", "url": "mxc://test/file",
                    "filename": "report.pdf", "info": { "mimetype": "application/pdf", "size": 4 }
                }
            }),
        )
        .expect("loaded file root should use the standard file projection");
        assert_eq!(
            file.media.as_ref().map(|media| media.kind),
            Some(TimelineMediaKind::File)
        );
        assert_eq!(
            file.media.as_ref().map(|media| media.filename.as_str()),
            Some("report.pdf")
        );

        let audio = thread_root_projection_item_from_raw(
            &room_key(),
            None,
            &activity,
            serde_json::json!({
                "event_id": "$old-root:test",
                "sender": "@alice:test",
                "origin_server_ts": 1_700_000_000_000u64,
                "type": "m.room.message",
                "content": {
                    "msgtype": "m.audio", "body": "voice.ogg", "url": "mxc://test/audio",
                    "info": { "mimetype": "audio/ogg", "size": 4 }
                }
            }),
        )
        .expect("loaded audio root should use the standard audio projection");
        assert_eq!(
            audio.media.as_ref().map(|media| media.kind),
            Some(TimelineMediaKind::Audio)
        );

        let sticker = thread_root_projection_item_from_raw(
            &room_key(),
            None,
            &activity,
            serde_json::json!({
                "event_id": "$old-root:test",
                "sender": "@alice:test",
                "origin_server_ts": 1_700_000_000_000u64,
                "type": "m.sticker",
                "content": {
                    "body": "party", "url": "mxc://test/sticker",
                    "info": { "mimetype": "image/png" }
                }
            }),
        )
        .expect("loaded sticker root should use the standard sticker projection");
        assert_eq!(sticker.body.as_deref(), Some("party"));
    }

    #[test]
    fn cached_root_relations_project_reactions_without_network_or_unrelated_targets() {
        let relations = vec![
            serde_json::json!({
                "event_id": "$reaction-a:test", "sender": "@alice:test", "type": "m.reaction",
                "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$old-root:test", "key": "👍" } }
            }),
            serde_json::json!({
                "event_id": "$reaction-b:test", "sender": "@me:test", "type": "m.reaction",
                "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$old-root:test", "key": "👍" } }
            }),
            serde_json::json!({
                "event_id": "$different-target:test", "sender": "@eve:test", "type": "m.reaction",
                "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$other-root:test", "key": "👍" } }
            }),
        ];
        let own_user_id = matrix_sdk::ruma::UserId::parse("@me:test").expect("valid own user");

        let reactions = reaction_groups_from_cached_relation_events(
            relations,
            "$old-root:test",
            Some(own_user_id.as_ref()),
        );

        assert_eq!(reactions.len(), 1);
        assert_eq!(reactions[0].key, "👍");
        assert_eq!(reactions[0].count, 2);
        assert!(reactions[0].reacted_by_me);
        assert_eq!(
            reactions[0].my_reaction_event_id.as_deref(),
            Some("$reaction-b:test")
        );
    }

    #[test]
    fn thread_summary_projection_preserves_ready_latest_event_id() {
        use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, OwnedEventId};
        use matrix_sdk_ui::timeline::{EmbeddedEvent, MsgLikeContent, ThreadSummary};

        let latest_event_id =
            OwnedEventId::try_from("$latest-thread-reply:test").expect("event id");
        let summary = ThreadSummary {
            latest_event: TimelineDetails::Ready(Box::new(EmbeddedEvent {
                content: TimelineItemContent::MsgLike(MsgLikeContent::redacted()),
                sender: OwnedUserId::try_from("@latest:test").expect("user id"),
                sender_profile: TimelineDetails::Unavailable,
                timestamp: MilliSecondsSinceUnixEpoch(uint!(42)),
                identifier: TimelineEventItemId::EventId(latest_event_id.clone()),
            })),
            num_replies: 1,
            public_read_receipt_event_id: None,
            private_read_receipt_event_id: None,
        };

        let dto = thread_summary_from_sdk(summary);

        assert_eq!(
            dto.latest_event_id.as_deref(),
            Some(latest_event_id.as_str())
        );
    }

    #[test]
    fn encrypted_thread_reply_relation_is_recovered_from_original_json() {
        let original_json = serde_json::json!({
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "ciphertext": "ciphertext",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$thread-root:test",
                    "m.in_reply_to": {
                        "event_id": "$reply-target:test"
                    },
                    "is_falling_back": true
                },
                "session_id": "session"
            },
            "event_id": "$thread-reply:test",
            "type": "m.room.encrypted"
        });

        assert_eq!(
            thread_root_from_original_json(&original_json).as_deref(),
            Some("$thread-root:test")
        );
    }

    #[test]
    fn megolm_session_fingerprint_is_stable_compact_and_distinguishes_rotation() {
        let first = megolm_session_fingerprint("AbCdEfGhIjKlMnOpQrStUvWxYz0123456789");
        let same = megolm_session_fingerprint("AbCdEfGhIjKlMnOpQrStUvWxYz0123456789");
        let rotated = megolm_session_fingerprint("ZyXwVuTsRqPoNmLkJiHgFeDcBa9876543210");

        assert_eq!(first, "AbCdEfGhIjKl");
        assert_eq!(first, same);
        assert_ne!(first, rotated);
    }

    #[test]
    fn room_timeline_keeps_renderable_thread_messages_visible() {
        let key = room_key();

        assert!(!timeline_item_should_be_hidden_for_key(
            &key,
            true,
            false,
            Some("$thread-root:test")
        ));
    }

    #[test]
    fn thread_root_activity_requires_shared_attention_eligibility() {
        let mut item = timeline_item("$reply:test", Some("reply"), "@alice:test", false);
        item.thread_root = Some("$root:test".to_owned());
        item.is_redacted = true;
        assert!(thread_root_projection_activity_from_item("!r:test", &item).is_none());

        item.is_redacted = false;
        item.is_hidden = true;
        assert!(thread_root_projection_activity_from_item("!r:test", &item).is_none());
    }

    #[test]
    fn thread_attention_does_not_count_root_or_hydrated_history_pushed_back() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let items = vec![
            timeline_message_item("$root:test", "@alice:test"),
            thread_reply_item("$historical:test", "@bob:test", "$root:test"),
        ];
        let tracker = ThreadAttentionTracker::hydrate(
            &key,
            &items,
            Some(own_user_id),
            Some("$historical:test".to_owned()),
        );

        assert_eq!(tracker.counts, ThreadAttentionCounters::default());
    }

    #[test]
    fn thread_attention_hydration_uses_visible_authoritative_receipt_baseline() {
        let key = thread_key();
        let items = vec![
            thread_reply_item("$read:test", "@alice:test", "$root:test"),
            thread_reply_item("$unread:test", "@bob:test", "$root:test"),
        ];

        let tracker = ThreadAttentionTracker::hydrate(
            &key,
            &items,
            Some("@me:test"),
            Some("$read:test".to_owned()),
        );

        assert_eq!(tracker.counts.notification_count, 1);
        assert_eq!(tracker.counts.live_event_marker_count, 1);
    }

    #[test]
    fn thread_attention_prunes_redacted_reply_before_replay() {
        let key = thread_key();
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some("@me:test"), None);
        let live = thread_reply_item("$live-redaction:test", "@bob:test", "$root:test");
        assert!(
            tracker
                .reconcile(
                    &key,
                    std::slice::from_ref(&live),
                    Some("@me:test"),
                    ThreadAttentionObservation::Live,
                )
                .is_some()
        );
        assert_eq!(tracker.counts.notification_count, 1);

        let mut redacted = live.clone();
        redacted.is_redacted = true;
        let provenance = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&redacted),
            ThreadAttentionObservation::Replay,
        );
        assert_eq!(
            tracker.reconcile_batch(
                &key,
                std::slice::from_ref(&redacted),
                Some("@me:test"),
                &provenance,
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 0,
                highlight_count: 0,
                live_event_marker_count: 0,
            })
        );
        assert_eq!(tracker.counts.notification_count, 0);
        assert_eq!(
            tracker.reconcile(
                &key,
                std::slice::from_ref(&redacted),
                Some("@me:test"),
                ThreadAttentionObservation::Replay,
            ),
            None
        );
    }

    #[test]
    fn thread_attention_acknowledge_prunes_hidden_reply_without_reconcile() {
        let key = thread_key();
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some("@me:test"), None);
        let live = thread_reply_item("$live-hidden:test", "@bob:test", "$root:test");
        assert!(
            tracker
                .reconcile(
                    &key,
                    std::slice::from_ref(&live),
                    Some("@me:test"),
                    ThreadAttentionObservation::Live,
                )
                .is_some()
        );
        let mut hidden = live;
        hidden.is_hidden = true;

        assert_eq!(
            tracker.acknowledge(
                &key,
                std::slice::from_ref(&hidden),
                "$outside:test".to_owned()
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 0,
                highlight_count: 0,
                live_event_marker_count: 0,
            })
        );
    }

    #[test]
    fn thread_attention_counts_one_live_remote_reply_and_deduplicates_replay() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let mut items = vec![thread_reply_item(
            "$baseline:test",
            "@alice:test",
            "$root:test",
        )];
        let mut tracker = ThreadAttentionTracker::hydrate(
            &key,
            &items,
            Some(own_user_id),
            Some("$baseline:test".to_owned()),
        );

        let mut local_echo = thread_reply_item("$unused:test", own_user_id, "$root:test");
        local_echo.id = TimelineItemId::Transaction {
            transaction_id: "txn-own".to_owned(),
        };
        items.extend([
            local_echo,
            thread_reply_item("$own-remote:test", own_user_id, "$root:test"),
            thread_reply_item("$live:test", "@bob:test", "$root:test"),
        ]);

        assert_eq!(
            tracker.reconcile(
                &key,
                &items,
                Some(own_user_id),
                ThreadAttentionObservation::Live,
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 1,
                highlight_count: 0,
                live_event_marker_count: 1,
            })
        );
        assert_eq!(
            tracker.reconcile(
                &key,
                &items,
                Some(own_user_id),
                ThreadAttentionObservation::Replay,
            ),
            None,
            "the same stable event must not increment after reconnect/replay"
        );
        assert_eq!(tracker.counts.notification_count, 1);
    }

    #[test]
    fn live_encrypted_reply_counts_when_a_later_set_becomes_renderable() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let mut unavailable = thread_reply_item("$encrypted-live:test", "@bob:test", "$root:test");
        unavailable.body = None;
        unavailable.media = None;
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some(own_user_id), None);

        let unavailable_provenance = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&unavailable),
            ThreadAttentionObservation::Live,
        );
        assert_eq!(
            tracker.reconcile_batch(
                &key,
                std::slice::from_ref(&unavailable),
                Some(own_user_id),
                &unavailable_provenance,
            ),
            None
        );

        let unrelated = thread_reply_item("$unrelated:test", "@alice:test", "$other-root:test");
        let unrelated_provenance = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&unrelated),
            ThreadAttentionObservation::Live,
        );
        assert_eq!(
            tracker.reconcile_batch(
                &key,
                &[unavailable, unrelated],
                Some(own_user_id),
                &unrelated_provenance,
            ),
            None,
            "an unrelated batch must not absorb the pending live encrypted event"
        );

        let renderable = thread_reply_item("$encrypted-live:test", "@bob:test", "$root:test");
        let renderable_provenance = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&renderable),
            ThreadAttentionObservation::Live,
        );
        assert_eq!(
            tracker.reconcile_batch(
                &key,
                &[renderable],
                Some(own_user_id),
                &renderable_provenance,
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 1,
                highlight_count: 0,
                live_event_marker_count: 1,
            })
        );
    }

    #[test]
    fn thread_attention_backfill_reset_and_other_roots_do_not_increment() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some(own_user_id), None);
        let other_root = thread_reply_item("$other:test", "@alice:test", "$other-root:test");
        let historical = thread_reply_item("$old:test", "@bob:test", "$root:test");

        assert_eq!(
            tracker.reconcile(
                &key,
                std::slice::from_ref(&historical),
                Some(own_user_id),
                ThreadAttentionObservation::Backfill,
            ),
            None
        );
        assert_eq!(
            tracker.reconcile(
                &key,
                &[historical, other_root],
                Some(own_user_id),
                ThreadAttentionObservation::Replay,
            ),
            None
        );
        assert_eq!(tracker.counts, ThreadAttentionCounters::default());

        let receipt = thread_reply_item("$visible-read:test", own_user_id, "$root:test");
        let after_receipt = thread_reply_item("$historical-after:test", "@bob:test", "$root:test");
        let mut tracker = ThreadAttentionTracker::hydrate(
            &key,
            std::slice::from_ref(&receipt),
            Some(own_user_id),
            Some("$visible-read:test".to_owned()),
        );
        assert_eq!(
            tracker.reconcile(
                &key,
                &[receipt, after_receipt],
                Some(own_user_id),
                ThreadAttentionObservation::Backfill,
            ),
            None,
            "ordinary pagination never manufactures attention"
        );
        assert_eq!(tracker.counts, ThreadAttentionCounters::default());
    }

    #[test]
    fn delayed_pagination_batch_does_not_become_live_after_task_completion() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let historical = thread_reply_item("$old-delayed:test", "@bob:test", "$root:test");
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some(own_user_id), None);

        // Reproduce the actor race reported by independent review: the SDK
        // pagination call has completed and cleared ambient task state before
        // its separately relayed PushBack batch reaches the actor.
        let delayed_pagination_provenance = ThreadAttentionBatchProvenance::from_timeline_items(
            std::slice::from_ref(&historical),
            ThreadAttentionObservation::Backfill,
        );

        assert_eq!(
            tracker.reconcile_batch(
                &key,
                std::slice::from_ref(&historical),
                Some(own_user_id),
                &delayed_pagination_provenance,
            ),
            None,
            "pagination provenance must travel with the delayed batch"
        );
        assert_eq!(tracker.counts, ThreadAttentionCounters::default());
    }

    #[test]
    fn sdk_event_origin_is_the_relay_batch_attention_provenance() {
        assert_eq!(
            thread_attention_observation_from_event_origin(Some(EventItemOrigin::Sync)),
            ThreadAttentionObservation::Live
        );
        assert_eq!(
            thread_attention_observation_from_event_origin(Some(EventItemOrigin::Pagination)),
            ThreadAttentionObservation::Backfill
        );
        assert_eq!(
            thread_attention_observation_from_event_origin(Some(EventItemOrigin::Cache)),
            ThreadAttentionObservation::Replay
        );
        assert_eq!(
            thread_attention_observation_from_event_origin(None),
            ThreadAttentionObservation::Replay,
            "unknown and delayed hydration must be conservative"
        );
    }

    #[test]
    fn thread_attention_trackers_do_not_contaminate_different_threads() {
        let first_key = thread_key();
        let second_key = TimelineKey {
            account_key: first_key.account_key.clone(),
            kind: TimelineKind::Thread {
                room_id: "!r:test".to_owned(),
                root_event_id: "$second-root:test".to_owned(),
            },
        };
        let first_live = thread_reply_item("$first-live:test", "@alice:test", "$root:test");
        let mut first = ThreadAttentionTracker::hydrate(&first_key, &[], Some("@me:test"), None);
        let mut second = ThreadAttentionTracker::hydrate(&second_key, &[], Some("@me:test"), None);

        assert!(
            first
                .reconcile(
                    &first_key,
                    std::slice::from_ref(&first_live),
                    Some("@me:test"),
                    ThreadAttentionObservation::Live,
                )
                .is_some()
        );
        assert_eq!(
            second.reconcile(
                &second_key,
                &[first_live],
                Some("@me:test"),
                ThreadAttentionObservation::Live,
            ),
            None
        );
        assert_eq!(first.counts.notification_count, 1);
        assert_eq!(second.counts.notification_count, 0);
    }

    #[test]
    fn thread_attention_acknowledgement_clears_without_changing_total_reply_count() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let mut root = timeline_message_item("$root:test", "@alice:test");
        root.thread_summary = Some(ThreadSummaryDto {
            reply_count: 2,
            latest_event_id: Some("$live:test".to_owned()),
            latest_sender: Some("@bob:test".to_owned()),
            latest_sender_label: Some("Bob".to_owned()),
            latest_body_preview: Some("preview".to_owned()),
            latest_timestamp_ms: Some(2),
        });
        let items = vec![
            root,
            thread_reply_item("$baseline:test", "@alice:test", "$root:test"),
            thread_reply_item("$live:test", "@bob:test", "$root:test"),
        ];
        let mut tracker = ThreadAttentionTracker::hydrate(
            &key,
            &items[..2],
            Some(own_user_id),
            Some("$baseline:test".to_owned()),
        );
        let _ = tracker.reconcile(
            &key,
            &items,
            Some(own_user_id),
            ThreadAttentionObservation::Live,
        );

        assert_eq!(tracker.counts.notification_count, 1);
        assert_eq!(items[0].thread_summary.as_ref().unwrap().reply_count, 2);
        assert_eq!(
            tracker.acknowledge(&key, &items, "$outside-window:test".to_owned()),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 1,
                highlight_count: 0,
                live_event_marker_count: 1,
            }),
            "an out-of-window receipt must not guess the relative ordering"
        );
        assert_eq!(
            tracker.acknowledge(&key, &items, "$live:test".to_owned()),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 0,
                highlight_count: 0,
                live_event_marker_count: 0,
            })
        );
        assert_eq!(items[0].thread_summary.as_ref().unwrap().reply_count, 2);
    }

    #[test]
    fn visible_receipt_prunes_attention_preserved_while_it_was_outside_the_window() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let live = thread_reply_item("$live-before-receipt:test", "@bob:test", "$root:test");
        let mut tracker = ThreadAttentionTracker::hydrate(&key, &[], Some(own_user_id), None);
        let _ = tracker.reconcile(
            &key,
            std::slice::from_ref(&live),
            Some(own_user_id),
            ThreadAttentionObservation::Live,
        );
        assert_eq!(tracker.counts.notification_count, 1);
        let _ = tracker.acknowledge(
            &key,
            std::slice::from_ref(&live),
            "$later-receipt:test".to_owned(),
        );
        assert_eq!(tracker.counts.notification_count, 1);

        let receipt = thread_reply_item("$later-receipt:test", own_user_id, "$root:test");
        let expanded = vec![live, receipt];
        assert_eq!(
            tracker.reconcile(
                &key,
                &expanded,
                Some(own_user_id),
                ThreadAttentionObservation::Backfill,
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 0,
                highlight_count: 0,
                live_event_marker_count: 0,
            })
        );
    }

    #[test]
    fn recovery_counts_first_seen_unread_reply_after_visible_receipt() {
        let key = thread_key();
        let own_user_id = "@me:test";
        let receipt = thread_reply_item("$read-before-overflow:test", own_user_id, "$root:test");
        let unread = thread_reply_item("$missed-during-overflow:test", "@bob:test", "$root:test");
        let mut tracker = ThreadAttentionTracker::hydrate(
            &key,
            std::slice::from_ref(&receipt),
            Some(own_user_id),
            Some("$read-before-overflow:test".to_owned()),
        );

        assert_eq!(
            tracker.reconcile(
                &key,
                &[receipt, unread],
                Some(own_user_id),
                ThreadAttentionObservation::Replay,
            ),
            Some(AppAction::ThreadAttentionUpdated {
                room_id: "!r:test".to_owned(),
                root_event_id: "$root:test".to_owned(),
                notification_count: 1,
                highlight_count: 0,
                live_event_marker_count: 1,
            })
        );
    }

    #[test]
    fn successful_receipt_uses_newest_provable_canonical_boundary() {
        let items = vec![
            thread_reply_item("$old-read:test", "@me:test", "$root:test"),
            thread_reply_item("$requested-read:test", "@me:test", "$root:test"),
            thread_reply_item("$newer-device-read:test", "@me:test", "$root:test"),
        ];

        let requested = "$requested-read:test";
        let selected = newest_provable_receipt_event_id(
            &items,
            requested,
            Some("$old-read:test".to_owned()),
            Some("$old-read:test"),
        );
        assert_eq!(
            selected, requested,
            "a stale SDK query must not delay the successful newer request"
        );

        assert_eq!(
            newest_provable_receipt_event_id(
                &items,
                "$requested-read:test",
                Some("$old-read:test".to_owned()),
                Some("$newer-device-read:test"),
            ),
            "$newer-device-read:test",
            "a stale request must not regress a newer multi-device boundary"
        );

        assert_eq!(
            newest_provable_receipt_event_id(
                &items[1..2],
                "$requested-read:test",
                Some("$queried-outside-window:test".to_owned()),
                Some("$current-outside-window:test"),
            ),
            "$requested-read:test",
            "unknown out-of-window IDs cannot override a visible successful request"
        );
    }
}
