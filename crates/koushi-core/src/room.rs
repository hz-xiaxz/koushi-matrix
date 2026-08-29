//! RoomActor: room list normalization and room operations.
//!
//! Ownership and behavioral contracts are documented in `actor` and the
//! feature modules; this file preserves the existing flat module API.

mod actor;
mod directory;
mod encryption_debug;
mod list_observer;
mod management;
mod mentions;
mod normalization;
mod operations;
mod pins;
mod space_members;
#[cfg(test)]
mod test_source;

pub use actor::{
    MissingSpaceChildLink, RoomActor, RoomActorHandle, RoomListReconcileAck, RoomMessage,
};
pub use normalization::assign_dm_space_ids;

pub(crate) use operations::RoomOperationKind;
#[cfg(feature = "test-hooks")]
pub(crate) use operations::RoomOperationTestControl;
