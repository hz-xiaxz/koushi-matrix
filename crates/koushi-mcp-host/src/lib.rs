//! Local MCP bridge: a stdio launcher (`koushi-mcp`) talks to this crate over a
//! private local socket.
//!
//! The crate is a transport adapter over an `AccountRuntimeManager`. It
//! publishes `endpoint.json`, verifies the per-launch token, and dispatches tool
//! calls to the account runtimes it was handed. It never opens a crypto store
//! and never reads Matrix credentials, so the desktop app and the headless QA
//! harness host the same bridge.

mod endpoint;
mod events;
mod grants;
mod handlers;
mod server;

pub use server::{start, BridgeHandle};
