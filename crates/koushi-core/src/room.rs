//! RoomActor: room list normalization and room operations.
//!
//! Ownership and behavioral contracts are documented in `actor` and the
//! feature modules; this file preserves the existing flat module API.

mod actor;
mod directory;
mod list_observer;
mod management;
mod mentions;
mod navigation_enrichment;
mod normalization;
mod operations;
mod pins;
mod space_children;
mod space_members;

pub(crate) use actor::RoomMessage;
pub use actor::{MissingSpaceChildLink, RoomActor, RoomActorHandle, RoomListReconcileAck};
#[cfg(test)]
pub(crate) use navigation_enrichment::NavigationEnrichmentDemand;
pub(crate) use navigation_enrichment::NavigationEnrichmentIngress;
pub use normalization::assign_dm_space_ids;

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use operations::RoomOperationKind;
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use operations::RoomOperationTestControl;
