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

pub use server::{BridgeHandle, start};

use std::path::PathBuf;

use koushi_core::CoreConnection;
use koushi_core::account_runtime_manager::AccountRuntimeManager;

/// One account the bridge may talk to. The desktop app builds these from its
/// runtime manager; the headless QA harness builds a single one.
pub struct BridgeAccount {
    pub id: String,
    pub homeserver: Option<String>,
    pub connection: CoreConnection,
    pub media_cache_dir: PathBuf,
}

/// The accounts a host exposes to MCP clients.
pub trait BridgeAccounts: Send + Sync + 'static {
    fn accounts(&self) -> Vec<BridgeAccount>;
}

impl BridgeAccounts for AccountRuntimeManager {
    fn accounts(&self) -> Vec<BridgeAccount> {
        self.tab_descriptors()
            .into_iter()
            .filter(|tab| tab.id.is_account())
            .filter_map(|tab| {
                let connection = self.tab_connection(&tab.id)?;
                let media_cache_dir = self.media_cache_dir_for_tab(&tab.id)?;
                Some(BridgeAccount {
                    id: tab.id.as_str().to_owned(),
                    homeserver: tab.homeserver.clone(),
                    connection,
                    media_cache_dir,
                })
            })
            .collect()
    }
}
