#![forbid(unsafe_code)]

//! Transport-neutral public command/event identity and state-update DTOs.

pub mod event;
pub mod failure;
pub mod ids;
pub mod state_update;

pub use event::*;
pub use failure::*;
pub use ids::*;
pub use state_update::{
    AppStateSnapshot, CoreCommandAdmission, StateDelta, StateDeltaChangedSlices,
    VersionedAppStateSnapshot,
};
