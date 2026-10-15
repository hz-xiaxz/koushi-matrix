//! Reconcile the latest Rust-owned demand into the existing avatar downloader.

use koushi_state::{AppAction, AvatarThumbnailState};

use super::{
    actor::AccountActor,
    profile::{AVATAR_DOWNLOAD_CONCURRENCY, AVATAR_DOWNLOAD_QUEUE_CAPACITY},
};

impl AccountActor {
    pub(super) async fn accept_avatar_demand(&mut self) {
        let next = self.avatar_demand_rx.borrow_and_update().clone();
        if let Some(demand) = &next {
            let context = demand.context();
            if context.session_generation
                != self
                    .avatar_session_generation
                    .load(std::sync::atomic::Ordering::Acquire)
                || self
                    .session
                    .as_ref()
                    .is_none_or(|session| session.info.user_id != context.account_id)
            {
                return;
            }
        }
        let publish_cached = match (&self.avatar_demand, &next) {
            (Some(previous), Some(next)) => !next.same_resource_demand(previous),
            _ => true,
        };
        self.avatar_demand = next;
        if publish_cached {
            // #1433: a changed working set can fit the cache now, so the
            // sources a larger working set could not hold get one more attempt.
            self.avatar_capacity_evicted.clear();
            let capacity_failures: Vec<String> = self
                .avatar_cache
                .iter()
                .filter(|(_, thumbnail)| {
                    matches!(
                        thumbnail,
                        AvatarThumbnailState::Failed {
                            kind: koushi_state::AvatarThumbnailFailureKind::Capacity,
                            ..
                        }
                    )
                })
                .map(|(mxc_uri, _)| mxc_uri.clone())
                .collect();
            for mxc_uri in capacity_failures {
                self.avatar_cache.remove(&mxc_uri);
            }
        }
        self.reconcile_avatar_demand(true, publish_cached).await;
    }

    pub(super) async fn reconcile_avatar_demand(
        &mut self,
        revalidate_ready: bool,
        publish_cached: bool,
    ) {
        // The Arc is a bounded resolved-demand snapshot, not a clone of AppState.
        let demand = self.avatar_demand.clone();
        let resources = demand
            .as_ref()
            .map(|demand| demand.resources_by_priority())
            .unwrap_or_default();
        // #1432: leaving the scoped demand does not prove the source is unused.
        // A DM row, header, own profile, or timeline copy demands the same MXC
        // through its own path, so the bounded cache and the session lifecycle
        // own the release policy.
        // #1433: `Ready` never lies about bytes that are gone, whether or not
        // the scoped demand still references the source: a demanded source's
        // renderer must see the downgrade at the reconcile that follows its
        // eviction, not at its next observation. What bounds re-requesting is
        // the capacity terminal below, not a stale projection.
        let invalidated: Vec<String> = self
            .avatar_cache
            .iter()
            .filter(|(_, thumbnail)| {
                matches!(thumbnail, AvatarThumbnailState::Ready { source_ref, .. }
                    if !crate::renderable_thumbnail::contains_renderable_thumbnail(source_ref))
            })
            .map(|(mxc_uri, _)| mxc_uri.clone())
            .collect();
        if !invalidated.is_empty() {
            let mut actions = Vec::with_capacity(invalidated.len());
            for mxc_uri in invalidated {
                self.avatar_cache.remove(&mxc_uri);
                // #1433: a source whose bytes disappeared while the demand
                // persisted cannot be satisfied by refetching in a loop, so it
                // is capacity-terminal until the demanded resource set changes.
                // A changed set is exactly the case that must retry instead:
                // the source is being demanded afresh, not re-requested.
                if !publish_cached
                    && demand
                        .as_ref()
                        .is_some_and(|demand| demand.contains_resource(&mxc_uri))
                {
                    self.avatar_capacity_evicted.insert(mxc_uri.clone());
                }
                actions.push(AppAction::AvatarThumbnailUpdated {
                    mxc_uri,
                    thumbnail: AvatarThumbnailState::NotRequested,
                });
            }
            self.send_actions(actions).await;
        }
        let obsolete: Vec<_> = self
            .avatar_inflight
            .keys()
            .filter(|uri| {
                !demand
                    .as_ref()
                    .is_some_and(|demand| demand.contains_resource(uri))
            })
            .cloned()
            .collect();
        for uri in obsolete {
            self.cancel_unwanted_avatar(&uri);
        }

        // Rebuild only queued scoped work in current priority order. Keep active
        // work and ordinary command waiters; neither gets restarted or lost.
        for uri in std::mem::take(&mut self.avatar_pending) {
            if self.avatar_inflight.get(&uri).is_some_and(Vec::is_empty) {
                self.avatar_inflight.remove(&uri);
            } else {
                self.avatar_pending.push_back(uri);
            }
        }
        for uri in resources {
            // Revalidate Ready bytes when an observation is published, not on
            // every completion: a working set larger than the renderable LRU
            // must not drive an endless eviction/refill loop.
            let cached = if revalidate_ready {
                self.cached_avatar_thumbnail(uri).await
            } else {
                self.avatar_cache.get(uri).cloned()
            };
            if let Some(cached) = cached {
                // Revision-only observations follow model updates; echoing
                // cached state again would continuously invalidate those models.
                if publish_cached {
                    self.send_actions(vec![AppAction::AvatarThumbnailUpdated {
                        mxc_uri: uri.to_owned(),
                        thumbnail: cached.clone(),
                    }])
                    .await;
                }
                continue;
            }
            if self.avatar_capacity_evicted.contains(uri) {
                // #1433: another source's store evicted this one while it was
                // still demanded, so the working set does not fit the cache.
                // The renderer's re-request is answered with a terminal
                // Capacity failure; only a changed demand set retries it.
                continue;
            }
            if self.avatar_inflight.contains_key(uri) || self.session.is_none() {
                continue;
            }
            if self.avatar_active_fetches < AVATAR_DOWNLOAD_CONCURRENCY {
                self.avatar_inflight.insert(uri.to_owned(), Vec::new());
                self.spawn_avatar_fetch(uri.to_owned(), 0);
            } else if self.avatar_pending.len() < AVATAR_DOWNLOAD_QUEUE_CAPACITY {
                self.avatar_inflight.insert(uri.to_owned(), Vec::new());
                self.avatar_pending.push_back(uri.to_owned());
            }
            // Excess demand remains in the bounded state snapshot. Completion
            // reconciles it again; Capacity is not cached as a terminal failure.
        }
        self.start_pending_avatar_fetches();
    }
}
