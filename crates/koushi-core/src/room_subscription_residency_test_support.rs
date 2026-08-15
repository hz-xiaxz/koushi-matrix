//! Test-only support for issue #532 room-subscription residency checks.
//!
//! This module is intentionally absent from default builds.  The harness is
//! filled in with production actor probes as each RED check is admitted; it
//! never contains a second residency policy.

#![cfg(feature = "test-hooks")]

use std::sync::Arc;

use crate::timeline::TimelineManagerActor;
use crate::{CoreConnection, CoreRuntime, TimelineKey};

use matrix_sdk_ui::room_list_service::RoomListService;

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

/// Test-only wrapper for the real core runtime and its actor tree.
///
/// The wrapper deliberately has no policy or fake state.  Its public surface
/// is the eventual set of probes/barriers over the production actors.
pub struct RoomSubscriptionResidencyHarness {
    runtime: CoreRuntime,
    connection: CoreConnection,
    manager: Option<TimelineManagerActor>,
}

impl RoomSubscriptionResidencyHarness {
    /// Construct the real AccountActor/RoomActor/TimelineManagerActor tree.
    pub fn new() -> Self {
        let runtime = CoreRuntime::start_with_event_capacity(crate::EVENT_QUEUE_CAPACITY);
        let connection = runtime.attach();
        Self {
            runtime,
            connection,
            manager: None,
        }
    }

    /// Wrap a real manager around the caller's live RoomListService.
    pub fn with_room_list_service(room_list_service: Arc<RoomListService>) -> Self {
        let runtime = CoreRuntime::start_with_event_capacity(crate::EVENT_QUEUE_CAPACITY);
        let connection = runtime.attach();
        let manager =
            TimelineManagerActor::room_subscription_residency_test_manager(room_list_service);
        Self {
            runtime,
            connection,
            manager: Some(manager),
        }
    }

    /// Compile-only scaffold probe.  It has no production side effect.
    pub fn compile_probe(&self) {
        let _ = (&self.runtime, &self.connection, &self.manager);
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

    pub fn snapshot(&self) -> RoomSubscriptionResidencySnapshot {
        let (desired_rooms, active_rooms, actor_count, lease_count, sdk_generation) = self
            .manager
            .as_ref()
            .expect("residency manager")
            .room_subscription_residency_test_snapshot();
        RoomSubscriptionResidencySnapshot {
            desired_rooms,
            active_rooms,
            actor_count,
            lease_count,
            sdk_generation,
            ..RoomSubscriptionResidencySnapshot::default()
        }
    }
}

impl Default for RoomSubscriptionResidencyHarness {
    fn default() -> Self {
        Self::new()
    }
}
