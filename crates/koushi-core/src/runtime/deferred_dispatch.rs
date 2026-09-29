//! #1060: AppActor → AccountActor dispatches produced while committing an
//! action batch must never wait for AccountActor mailbox capacity. An awaited
//! send there holds the AppActor loop, so every later command, including a
//! purely local room selection, queues behind a full mailbox.
//!
//! Only latest-wins or generation-guarded dispatches are deferred here, where
//! a late or coalesced delivery cannot change the outcome:
//!
//! - `ResolveActivity`: its `Resolving` state is published before dispatch,
//!   so live updates never restart it, and every settlement is fenced by
//!   generation. A newer generation supersedes a deferred older one.
//! - `NotifySearchCrawlerRoomsAvailable`: the AccountActor already stores it
//!   as a latest-wins pending notification; a newer one replaces the deferred
//!   payload.
//!
//! The run loop waits for one mailbox slot only while something is deferred,
//! so free capacity can never spin it.

use koushi_state::{ActivityResolutionState, ActivityState, AppAction, OperationFailureKind};
use tokio::sync::mpsc;

use super::{ActionBatchOrigin, AppActor};
use crate::account::AccountMessage;
use crate::activity_resolution::ActivityResolutionRequest;

/// A started resolution generation whose `ResolveActivity` waits for capacity.
pub(super) struct DeferredActivityResolution {
    generation: u64,
    unresolved_room_count: u32,
    requests: Vec<ActivityResolutionRequest>,
}

/// The latest search-crawler room availability waiting for capacity.
pub(super) struct DeferredSearchCrawlerRooms {
    room_ids: Vec<String>,
    latest_event_ids: std::collections::BTreeMap<String, String>,
    settings: koushi_state::SearchCrawlerSettings,
}

/// At most one deferred dispatch per kind.
#[derive(Default)]
pub(super) struct DeferredAccountDispatch {
    activity_resolution: Option<DeferredActivityResolution>,
    search_crawler_rooms: Option<DeferredSearchCrawlerRooms>,
}

impl DeferredAccountDispatch {
    pub(super) fn is_pending(&self) -> bool {
        self.activity_resolution.is_some() || self.search_crawler_rooms.is_some()
    }
}

/// Whether `ResolveActivity` reached the mailbox (now or deferred), or the
/// AccountActor is gone and the generation must be settled as failed.
pub(super) enum ActivityResolutionDispatch {
    SentOrDeferred,
    Closed,
}

impl AppActor {
    pub(super) fn dispatch_activity_resolution(
        &mut self,
        generation: u64,
        unresolved_room_count: u32,
        requests: Vec<ActivityResolutionRequest>,
    ) -> ActivityResolutionDispatch {
        // A newer generation supersedes any still-deferred older dispatch; it
        // must not reach the AccountActor after this one.
        self.deferred_account_dispatch.activity_resolution = None;
        let Err(unsent) = self
            .account_actor
            .try_send(AccountMessage::ResolveActivity {
                generation,
                requests,
            })
        else {
            return ActivityResolutionDispatch::SentOrDeferred;
        };
        match *unsent {
            mpsc::error::TrySendError::Full(AccountMessage::ResolveActivity {
                requests, ..
            }) => {
                self.deferred_account_dispatch.activity_resolution =
                    Some(DeferredActivityResolution {
                        generation,
                        unresolved_room_count,
                        requests,
                    });
                ActivityResolutionDispatch::SentOrDeferred
            }
            mpsc::error::TrySendError::Full(_) | mpsc::error::TrySendError::Closed(_) => {
                ActivityResolutionDispatch::Closed
            }
        }
    }

    pub(super) fn dispatch_search_crawler_rooms(
        &mut self,
        room_ids: Vec<String>,
        latest_event_ids: std::collections::BTreeMap<String, String>,
        settings: koushi_state::SearchCrawlerSettings,
    ) {
        // Latest wins: an older deferred payload must not overwrite this one.
        self.deferred_account_dispatch.search_crawler_rooms = None;
        if let Err(unsent) =
            self.account_actor
                .try_send(AccountMessage::NotifySearchCrawlerRoomsAvailable {
                    room_ids,
                    latest_event_ids,
                    settings,
                })
            && let mpsc::error::TrySendError::Full(
                AccountMessage::NotifySearchCrawlerRoomsAvailable {
                    room_ids,
                    latest_event_ids,
                    settings,
                },
            ) = *unsent
        {
            self.deferred_account_dispatch.search_crawler_rooms =
                Some(DeferredSearchCrawlerRooms {
                    room_ids,
                    latest_event_ids,
                    settings,
                });
        }
        // A closed mailbox has no crawler to notify; there is nothing to settle.
    }

    /// Whether open Activity still waits on this resolution generation.
    fn activity_resolution_is_current(&self, generation: u64) -> bool {
        matches!(
            &self.state.activity,
            ActivityState::Open { unread, .. }
                if matches!(
                    unread.resolution,
                    ActivityResolutionState::Resolving { generation: current, .. }
                        if current == generation
                )
        )
    }

    /// Spend one reserved mailbox slot on the most important deferred
    /// dispatch, or settle what can no longer be delivered.
    pub(super) async fn deliver_deferred_account_dispatch(
        &mut self,
        permit: Result<mpsc::OwnedPermit<AccountMessage>, mpsc::error::SendError<()>>,
    ) {
        let Ok(permit) = permit else {
            // The AccountActor is gone. A crawler notification needs no
            // settlement; a started resolution becomes failed and retryable,
            // exactly as an AccountActor without a session reports it.
            self.deferred_account_dispatch.search_crawler_rooms = None;
            if let Some(deferred) = self.deferred_account_dispatch.activity_resolution.take() {
                Box::pin(self.commit_action_batch(
                    vec![AppAction::ActivityResolutionFailed {
                        generation: deferred.generation,
                        unresolved_room_count: deferred.unresolved_room_count,
                        kind: OperationFailureKind::Sdk,
                    }],
                    ActionBatchOrigin::Actor,
                ))
                .await;
            }
            return;
        };
        // A superseded generation, or one whose Activity closed, is dropped.
        if let Some(deferred) = self.deferred_account_dispatch.activity_resolution.take()
            && self.activity_resolution_is_current(deferred.generation)
        {
            permit.send(AccountMessage::ResolveActivity {
                generation: deferred.generation,
                requests: deferred.requests,
            });
        } else if let Some(deferred) = self.deferred_account_dispatch.search_crawler_rooms.take() {
            permit.send(AccountMessage::NotifySearchCrawlerRoomsAvailable {
                room_ids: deferred.room_ids,
                latest_event_ids: deferred.latest_event_ids,
                settings: deferred.settings,
            });
        }
    }
}
