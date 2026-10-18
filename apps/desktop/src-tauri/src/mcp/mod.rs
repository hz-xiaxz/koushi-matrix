//! Desktop ownership of the MCP bridge. The bridge itself lives in
//! `koushi-mcp-host`, so the headless QA harness can host the same code.

use std::path::PathBuf;
use std::sync::Arc;

use koushi_core::account_runtime_manager::AccountRuntimeManager;
use koushi_mcp_host::BridgeHandle;

/// Tauri-managed slot for the running bridge. Stopping it removes the endpoint
/// file and the socket before the process exits.
pub(crate) struct McpBridge(pub(crate) std::sync::Mutex<Option<BridgeHandle>>);

/// Starts the bridge for this app process.
///
/// Failure is logged and non-fatal: a desktop client that cannot bind a socket
/// must still run.
pub(crate) fn start(
    runtime: Arc<AccountRuntimeManager>,
    data_dir: PathBuf,
) -> Option<BridgeHandle> {
    match koushi_mcp_host::start(runtime, data_dir) {
        Ok(handle) => Some(handle),
        Err(reason) => {
            koushi_diagnostics::record(
                koushi_diagnostics::DiagnosticEvent::new(
                    koushi_diagnostics::DiagnosticLevel::Warn,
                    "mcp.bridge",
                    "start_failed",
                )
                .field(koushi_diagnostics::DiagnosticField::token("reason", reason)),
            );
            None
        }
    }
}
