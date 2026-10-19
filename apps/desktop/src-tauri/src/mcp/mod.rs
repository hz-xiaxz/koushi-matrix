//! Desktop ownership of the MCP bridge. The bridge and its consent broker live
//! in `koushi-mcp-host`, so the headless QA harness can host the same code;
//! this module only starts and stops the bridge from the `mcp.enabled` setting,
//! forwards the broker's state to the WebView, and asks for attention when an
//! approval is waiting.

mod launcher;

use std::path::PathBuf;
use std::sync::Arc;

use koushi_core::account_runtime_manager::AccountRuntimeManager;
use koushi_mcp_host::BridgeHandle;
use koushi_mcp_host::consent::{BridgeStatus, ConsentBroker, ConsentMode, McpAssistantsState};
use tauri::{AppHandle, Emitter, Manager};

pub const MCP_EVENT_NAME: &str = "koushi-desktop://mcp";

/// Tauri-managed owner of the broker and the running bridge.
pub(crate) struct McpController {
    pub(crate) broker: Arc<ConsentBroker>,
    bridge: tokio::sync::Mutex<Option<BridgeHandle>>,
}

impl McpController {
    /// Stops the bridge on process exit without waiting: removes the
    /// endpoint file and socket and releases waiting calls.
    pub(crate) fn stop_now(&self) {
        if let Ok(mut slot) = self.bridge.try_lock()
            && let Some(handle) = slot.take()
        {
            handle.stop();
        }
    }
}

/// Creates the broker and starts following the `mcp.enabled` setting.
pub(crate) fn install(app: &AppHandle, runtime: Arc<AccountRuntimeManager>, data_dir: PathBuf) {
    let broker = ConsentBroker::new(data_dir.clone(), ConsentMode::Interactive);
    broker.set_launcher(launcher::resolve(&data_dir));
    app.manage(McpController {
        broker: Arc::clone(&broker),
        bridge: tokio::sync::Mutex::new(None),
    });
    spawn_forwarder(app.clone(), Arc::clone(&broker));
    spawn_switch(app.clone(), runtime, data_dir);
}

/// Starts or stops the bridge whenever the setting changes. The first decision
/// waits for the settings file: an unreadable file keeps the bridge off rather
/// than falling back to the default.
fn spawn_switch(app: AppHandle, runtime: Arc<AccountRuntimeManager>, data_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let loader = Arc::clone(&runtime);
        let initial = tauri::async_runtime::spawn_blocking(move || loader.load_app_settings())
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|values| values.mcp.enabled);
        let mut updates = runtime.subscribe_app_settings_updates();
        updates.borrow_and_update();
        let mut enabled = initial;
        loop {
            apply(&app, &runtime, &data_dir, enabled).await;
            if updates.changed().await.is_err() {
                return;
            }
            enabled = updates.borrow_and_update().mcp.enabled;
        }
    });
}

async fn apply(
    app: &AppHandle,
    runtime: &Arc<AccountRuntimeManager>,
    data_dir: &std::path::Path,
    enabled: bool,
) {
    let Some(controller) = app.try_state::<McpController>() else {
        return;
    };
    let mut slot = controller.bridge.lock().await;
    match (enabled, slot.is_some()) {
        (true, false) => {
            let started = koushi_mcp_host::start(
                Arc::clone(runtime) as Arc<dyn koushi_mcp_host::BridgeAccounts>,
                data_dir.to_path_buf(),
                Arc::clone(&controller.broker),
            );
            match started {
                Ok(handle) => {
                    *slot = Some(handle);
                    controller.broker.set_bridge_status(BridgeStatus::Running);
                }
                Err(reason) => {
                    koushi_diagnostics::record(
                        koushi_diagnostics::DiagnosticEvent::new(
                            koushi_diagnostics::DiagnosticLevel::Warn,
                            "mcp.bridge",
                            "start_failed",
                        )
                        .field(koushi_diagnostics::DiagnosticField::token("reason", reason)),
                    );
                    controller
                        .broker
                        .set_bridge_status(BridgeStatus::StartFailed);
                }
            }
        }
        (false, true) => {
            if let Some(handle) = slot.take() {
                handle.shutdown().await;
            }
            controller.broker.set_bridge_status(BridgeStatus::Disabled);
        }
        (false, false) => controller.broker.set_bridge_status(BridgeStatus::Disabled),
        (true, true) => {}
    }
}

/// Forwards every state change to the WebView and draws attention to a newly
/// waiting approval. Attention never forces focus: an assistant must not be
/// able to pull keyboard input into the dialog.
fn spawn_forwarder(app: AppHandle, broker: Arc<ConsentBroker>) {
    let mut state = broker.subscribe();
    tauri::async_runtime::spawn(async move {
        let mut newest_request = 0;
        loop {
            let current: McpAssistantsState = state.borrow_and_update().clone();
            let newest = current
                .pending
                .iter()
                .map(|pending| pending.request_id)
                .max()
                .unwrap_or(0);
            if newest > newest_request {
                newest_request = newest;
                request_attention(&app);
            }
            let _ = app.emit(MCP_EVENT_NAME, &current);
            if state.changed().await.is_err() {
                return;
            }
        }
    });
}

fn request_attention(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if !window.is_visible().unwrap_or(true) {
        let _ = window.show();
    }
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
    }
    let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
}
