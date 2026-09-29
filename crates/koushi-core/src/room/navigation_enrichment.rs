//! Post-commit navigation enrichment (#1060).
//!
//! AppActor admits and reduces room/Space selection locally. Supplementary
//! network work for the selection — pinned-event bodies and Space-member
//! hydration — is owned by RoomActor and must never gate that commit. AppActor
//! therefore publishes the reducer-accepted navigation as one retained,
//! latest-wins value; RoomActor consumes it whenever its serial SDK work allows.
//! Enrichment never emits a selection action.

use koushi_protocol::SessionKeyId;
use koushi_sdk::MatrixClientSession;
use tokio::sync::watch;

use super::RoomActor;
use super::list_observer::RoomListObservationCommand;
use crate::executor;

/// Reducer-accepted navigation after a committed selection. `generation` is
/// assigned at admission and strictly increasing, so re-selecting the active
/// room still wakes RoomActor for a pinned refresh.
/// Deliberately not `Debug`: it carries account and room identifiers.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct NavigationEnrichmentDemand {
    pub(crate) generation: u64,
    pub(crate) session_key: SessionKeyId,
    pub(crate) active_room_id: Option<String>,
    pub(crate) active_space_id: Option<String>,
    /// An explicit Space/Home selection (re)requests member hydration even
    /// when the active Space is unchanged; a room selection only hydrates
    /// when it moved navigation into another Space.
    pub(crate) space_selected: bool,
}

/// One-slot latest-wins ingress. Replacing the value never waits for, or
/// fills, the RoomActor mailbox. AppActor is its only writer.
#[derive(Clone)]
pub(crate) struct NavigationEnrichmentIngress {
    tx: watch::Sender<Option<NavigationEnrichmentDemand>>,
}

impl NavigationEnrichmentIngress {
    pub(crate) fn channel() -> (Self, watch::Receiver<Option<NavigationEnrichmentDemand>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }

    pub(crate) fn admit(
        &self,
        session_key: SessionKeyId,
        active_room_id: Option<String>,
        active_space_id: Option<String>,
        space_selected: bool,
    ) {
        // `send_modify` retains the value even while no receiver exists.
        self.tx.send_modify(|current| {
            let generation = current
                .as_ref()
                .map_or(1, |current| current.generation.wrapping_add(1).max(1));
            *current = Some(NavigationEnrichmentDemand {
                generation,
                session_key,
                active_room_id,
                active_space_id,
                space_selected,
            });
        });
    }

    #[cfg(test)]
    pub(crate) fn latest(&self) -> Option<NavigationEnrichmentDemand> {
        self.tx.borrow().clone()
    }
}

/// Which enrichment generation each consumer has already applied. Reset
/// whenever the session or the room-list observation is replaced (session
/// establishment, sync start, session clear), so the retained demand is
/// replayed for the new owner.
#[derive(Default)]
pub(super) struct NavigationEnrichmentApplied {
    pinned: Option<u64>,
    space_members: Option<u64>,
    /// The Space whose hydration was last requested for this observation.
    /// Only reset with the observation, so an unchanged Space is not
    /// re-hydrated by every room selection inside it.
    hydrated_space: Option<Option<String>>,
}

impl NavigationEnrichmentApplied {
    pub(super) fn reset_session(&mut self) {
        *self = Self::default();
    }
}

pub(super) async fn receive_navigation_enrichment(
    receiver: &mut Option<watch::Receiver<Option<NavigationEnrichmentDemand>>>,
) {
    let Some(active) = receiver.as_mut() else {
        return futures_util::future::pending().await;
    };
    if active.changed().await.is_err() {
        // The ingress lives as long as the actor handle; a closed sender only
        // happens during teardown and leaves the retained value in place.
        *receiver = None;
    }
}

fn session_matches(session: &MatrixClientSession, session_key: &SessionKeyId) -> bool {
    crate::store::session_key_id_from_info(&session.info) == *session_key
}

impl RoomActor {
    /// Apply the retained demand for the current session and observation.
    /// Demand recorded for another session is ignored; stale pinned results
    /// remain fenced by the pinned generation that session replacement bumps.
    pub(super) async fn apply_navigation_enrichment(&mut self) {
        let Some(demand) = self
            .navigation_enrichment_rx
            .as_mut()
            .and_then(|receiver| receiver.borrow_and_update().clone())
        else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        if !session_matches(&session, &demand.session_key) {
            return;
        }
        if self.navigation_enrichment_applied.pinned != Some(demand.generation) {
            self.navigation_enrichment_applied.pinned = Some(demand.generation);
            if let Some(room_id) = demand.active_room_id.clone() {
                self.start_pinned_refresh(room_id, None);
            }
        }
        if self.observation.is_some()
            && self.navigation_enrichment_applied.space_members != Some(demand.generation)
        {
            self.navigation_enrichment_applied.space_members = Some(demand.generation);
            let space = Some(demand.active_space_id.clone());
            if demand.space_selected || self.navigation_enrichment_applied.hydrated_space != space {
                self.navigation_enrichment_applied.hydrated_space = space;
                self.enqueue_space_member_hydration(demand.active_space_id)
                    .await;
            }
        }
    }

    /// Keep only the current Space's hydration request. Home and a cleared
    /// Space cancel an older, still-unadmitted request.
    async fn enqueue_space_member_hydration(&mut self, space_id: Option<String>) {
        if let Some(task) = self.space_hydration_enqueue_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(space_id) = space_id
            && let Some(observation) = &self.observation
        {
            let command_tx = observation.command_tx.clone();
            self.space_hydration_enqueue_task = Some(executor::spawn(async move {
                let _ = command_tx
                    .send(RoomListObservationCommand::HydrateSpaceMembers { space_id })
                    .await;
            }));
        }
    }
}
