use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tauri::Emitter;

use crate::dto::FrontendDesktopSnapshotDelta;
use koushi_core::{
    CoreCommand, CoreCommandHandle, CoreConnection, CoreEvent, EventStreamLag, SearchEvent,
    TimelineCommand, TimelineEvent, event::AppStateSnapshot,
};
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};

/// Tauri event for serialized CoreEvent payloads (discrete events + diff batches).
pub(crate) const CORE_EVENT_NAME: &str = "koushi-desktop://event";
/// Tauri event for serialized AppStateSnapshot payloads (latest-wins).
const STATE_EVENT_NAME: &str = "koushi-desktop://state";
const CORE_FORWARDER_TIMELINE_REPLAY_TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForwarderLagDisposition {
    ResyncAndReplay,
    ResyncAndStop,
}
pub(super) struct CoreEventForwarderTask(tauri::async_runtime::JoinHandle<()>);
impl Drop for CoreEventForwarderTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct ForwardedWebviewEvent {
    event_name: &'static str,
    payload: serde_json::Value,
}
/// Spawn the CoreEvent forwarding task. This task owns a dedicated connection
/// (second `attach()`) so it can loop on `recv_event` without blocking command
/// dispatch.
///
/// On any `CoreEvent`: emit `koushi-desktop://event` with a serialized DTO.
/// On `EventStreamLag`: emit the latest snapshot (resync) + a
/// `ResyncMarker` event so the frontend resets its timeline stores.
fn forwarder_lag_disposition(lag: EventStreamLag) -> ForwarderLagDisposition {
    if lag.skipped == 0 {
        ForwarderLagDisposition::ResyncAndStop
    } else {
        ForwarderLagDisposition::ResyncAndReplay
    }
}
pub(super) fn spawn_core_event_forwarder(
    app: tauri::AppHandle,
    mut event_conn: CoreConnection,
    timeline_items_count: Arc<AtomicUsize>,
) -> CoreEventForwarderTask {
    CoreEventForwarderTask(tauri::async_runtime::spawn(async move {
        loop {
            match event_conn.recv_event().await {
                Ok(event) => {
                    emit_forwarded_webview_events(
                        &app,
                        forwarded_webview_events_for_core_event(&event, &timeline_items_count),
                    );
                }
                Err(lag) => {
                    // Consumer fell behind or the stream closed. Emit the
                    // latest snapshot and marker once before replay or exit.
                    let snapshot = event_conn.snapshot();
                    emit_forwarded_webview_events(
                        &app,
                        forwarded_webview_events_for_lag_resync(&snapshot),
                    );
                    match forwarder_lag_disposition(lag) {
                        ForwarderLagDisposition::ResyncAndReplay => {
                            let command_handle = event_conn.command_handle();
                            let request_id = event_conn.next_request_id();
                            submit_timeline_replay_after_forwarder_lag(command_handle, request_id)
                                .await;
                        }
                        ForwarderLagDisposition::ResyncAndStop => break,
                    }
                }
            }
        }
    }))
}
async fn submit_timeline_replay_after_forwarder_lag(
    command_handle: CoreCommandHandle,
    request_id: koushi_core::RequestId,
) {
    let command = CoreCommand::Timeline(TimelineCommand::ReplaySubscribed { request_id });
    let _ = tokio::time::timeout(
        CORE_FORWARDER_TIMELINE_REPLAY_TIMEOUT,
        command_handle.command(command),
    )
    .await;
}
fn forwarded_webview_events_for_core_event(
    event: &CoreEvent,
    timeline_items_count: &AtomicUsize,
) -> Vec<ForwardedWebviewEvent> {
    let mut forwarded = Vec::new();

    // Track timeline item count for QA window title.
    match event {
        CoreEvent::Timeline(TimelineEvent::InitialItems { items, .. }) => {
            timeline_items_count.store(items.len(), Ordering::Relaxed);
        }
        CoreEvent::Timeline(TimelineEvent::ItemsUpdated { diffs, .. }) => {
            // Apply diff count delta (approximate; exact count tracked by React store)
            let current = timeline_items_count.load(Ordering::Relaxed);
            let delta = diffs_net_count_change(diffs);
            let new_count = (current as i64 + delta).max(0) as usize;
            timeline_items_count.store(new_count, Ordering::Relaxed);
        }
        _ => {}
    }

    if let CoreEvent::StateDelta(delta) = event {
        let requires_snapshot_refresh = delta.changed.session.is_some();
        forwarded.push(ForwardedWebviewEvent {
            event_name: CORE_EVENT_NAME,
            payload: serde_json::json!({
                "kind": "StateDelta",
                "generation": delta.generation,
                "changed": FrontendDesktopSnapshotDelta::from(delta.clone()).changed,
            }),
        });
        if requires_snapshot_refresh {
            forwarded.push(ForwardedWebviewEvent {
                event_name: STATE_EVENT_NAME,
                payload: serde_json::Value::String("stateChanged".to_owned()),
            });
        }
    }

    if let Some(payload) = serialize_core_event(event) {
        forwarded.push(ForwardedWebviewEvent {
            event_name: CORE_EVENT_NAME,
            payload,
        });
    }

    forwarded
}
fn diffs_net_count_change(diffs: &[koushi_core::TimelineDiff]) -> i64 {
    diffs
        .iter()
        .map(|diff| match diff {
            koushi_core::TimelineDiff::PushFront { .. }
            | koushi_core::TimelineDiff::PushBack { .. }
            | koushi_core::TimelineDiff::Insert { .. } => 1_i64,
            koushi_core::TimelineDiff::Remove { .. } => -1_i64,
            koushi_core::TimelineDiff::Truncate { .. }
            | koushi_core::TimelineDiff::Clear
            | koushi_core::TimelineDiff::Reset { .. }
            | koushi_core::TimelineDiff::Set { .. } => 0_i64,
        })
        .sum()
}
fn forwarded_webview_events_for_state_changed(
    _snapshot: &AppStateSnapshot,
) -> Vec<ForwardedWebviewEvent> {
    vec![ForwardedWebviewEvent {
        event_name: STATE_EVENT_NAME,
        payload: serde_json::Value::String("stateChanged".to_owned()),
    }]
}
fn forwarded_webview_events_for_lag_resync(
    snapshot: &AppStateSnapshot,
) -> Vec<ForwardedWebviewEvent> {
    let mut forwarded = forwarded_webview_events_for_state_changed(snapshot);
    forwarded.push(ForwardedWebviewEvent {
        event_name: CORE_EVENT_NAME,
        payload: serde_json::json!({ "kind": "ResyncMarker" }),
    });
    forwarded
}
fn emit_forwarded_webview_events(
    app: &tauri::AppHandle,
    forwarded_events: Vec<ForwardedWebviewEvent>,
) {
    let mut failed = 0_u64;
    for forwarded_event in forwarded_events {
        if app
            .emit(forwarded_event.event_name, forwarded_event.payload)
            .is_err()
        {
            failed = failed.saturating_add(1);
        }
    }
    if failed > 0 {
        record(
            DiagnosticEvent::new(
                DiagnosticLevel::Warn,
                "tauri.transport",
                "webview_emit_failed",
            )
            .field(DiagnosticField::count("events", failed)),
        );
    }
}
/// Serialize a `CoreEvent` to a JSON value for IPC.
///
/// Security: message bodies flow in `Timeline` events. These are visible
/// content (not secret), but we never trace IPC payloads in release.
/// The serialization produces structured JSON only — no raw SDK errors.
fn serialize_core_event(event: &CoreEvent) -> Option<serde_json::Value> {
    Some(match event {
        CoreEvent::StateDelta(_) => {
            return None;
        }
        CoreEvent::Account(e) => serde_json::json!({ "kind": "Account", "event": e }),
        CoreEvent::Sync(e) => serde_json::json!({ "kind": "Sync", "event": e }),
        CoreEvent::Room(e) => serde_json::json!({ "kind": "Room", "event": e }),
        CoreEvent::Timeline(e) => serde_json::json!({ "kind": "Timeline", "event": e }),
        CoreEvent::LiveSignals(e) => serde_json::json!({ "kind": "LiveSignals", "event": e }),
        CoreEvent::Search(SearchEvent::IndexUpdated { .. }) => {
            // Internal indexer wake-up signal. Forwarding one WebView IPC event
            // per indexed message competes with input and scroll rendering.
            return None;
        }
        CoreEvent::Search(e) => serde_json::json!({ "kind": "Search", "event": e }),
        CoreEvent::E2eeTrust(e) => serde_json::json!({ "kind": "E2eeTrust", "event": e }),
        CoreEvent::Activity(e) => serde_json::json!({ "kind": "Activity", "event": e }),
        CoreEvent::LocalEncryption(e) => {
            serde_json::json!({ "kind": "LocalEncryption", "event": e })
        }
        CoreEvent::NativeAttention(e) => {
            serde_json::json!({ "kind": "NativeAttention", "event": e })
        }
        CoreEvent::CjkTextPolicy(e) => serde_json::json!({ "kind": "CjkTextPolicy", "event": e }),
        CoreEvent::ThreadsList(e) => serde_json::json!({ "kind": "ThreadsList", "event": e }),
        CoreEvent::OperationFailed {
            request_id,
            failure,
        } => {
            serde_json::json!({
                "kind": "OperationFailed",
                "request_id": request_id,
                "failure": failure,
            })
        }
        // Telemetry-lane event: emitted after reduce, never mixed with
        // StateDelta never drives product state from telemetry in React.
        CoreEvent::IntentLifecycle {
            request_id,
            outcome,
            published_generation,
        } => {
            serde_json::json!({
                "kind": "IntentLifecycle",
                "request_id": request_id,
                "outcome": outcome,
                "published_generation": published_generation,
            })
        }
    })
}

#[cfg(test)]
mod tests;
