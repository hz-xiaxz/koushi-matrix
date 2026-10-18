//! Local MCP bridge: a stdio launcher (`koushi-mcp`) talks to this module over
//! a private local socket.
//!
//! The module is a transport adapter. It publishes `endpoint.json`, verifies
//! the per-launch token, and dispatches tool calls to the account runtime the
//! app already owns. It never opens a crypto store and never reads Matrix
//! credentials.

pub(crate) mod endpoint;
mod events;
mod grants;
mod handlers;
mod server;

use std::path::PathBuf;

use tauri::AppHandle;

pub(crate) use server::BridgeHandle;

/// Tauri-managed slot for the running bridge. Stopping it removes the endpoint
/// file and the socket before the process exits.
pub(crate) struct McpBridge(pub(crate) std::sync::Mutex<Option<BridgeHandle>>);

/// Starts the bridge for this app process.
///
/// Failure is logged and non-fatal: a desktop client that cannot bind a socket
/// must still run.
pub(crate) fn start(app: AppHandle, data_dir: PathBuf) -> Option<BridgeHandle> {
    match server::start(app, data_dir) {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("mcp bridge unavailable: {error}");
            None
        }
    }
}
