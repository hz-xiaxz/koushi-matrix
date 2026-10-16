//! Async macOS notification activation via UNUserNotificationCenter.

use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;

use super::*;

pub(super) const MAX_PENDING_MACOS_ACTIVATIONS: usize = 64;
const NOTIFICATION_PREFIX: &str = "koushi-attention-";
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static CLEAR_GENERATION: AtomicU64 = AtomicU64::new(0);
static REGISTRY: OnceLock<Mutex<MacActivationRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<MacActivationRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(MacActivationRegistry::default()))
}

#[derive(Default)]
pub(super) struct MacActivationRegistry {
    pending: VecDeque<(String, oneshot::Sender<()>)>,
}

impl MacActivationRegistry {
    pub(super) fn len(&self) -> usize {
        self.pending.len()
    }

    pub(super) fn insert(
        &mut self,
        id: String,
        cancel: oneshot::Sender<()>,
    ) -> Option<(String, oneshot::Sender<()>)> {
        let evicted = if self.pending.len() >= MAX_PENDING_MACOS_ACTIVATIONS {
            self.pending.pop_front()
        } else {
            None
        };
        self.pending.push_back((id, cancel));
        evicted
    }

    fn remove(&mut self, id: &str) {
        self.pending.retain(|(pending_id, _)| pending_id != id);
    }

    pub(super) fn clear(&mut self) -> Vec<oneshot::Sender<()>> {
        self.pending.drain(..).map(|(_, cancel)| cancel).collect()
    }
}

pub(super) fn pending_count() -> usize {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len()
}

pub(super) fn dispatch(
    app: &AppHandle,
    payload: &NativeNotificationPayload,
    fence: ActivationFence,
) -> NativeNotificationOutcome {
    // UNUserNotificationCenter requires a real application bundle. The normal
    // desktop release has one; a `cargo test` process does not.
    if mac_usernotifications::check_bundle().is_err() {
        record_show_failed();
        return NativeNotificationOutcome::Failed;
    }

    let id = format!(
        "{NOTIFICATION_PREFIX}{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    let title = payload.title.clone();
    let body = payload.body.clone();
    let target = payload.target.clone();
    let app = app.clone();
    let generation = CLEAR_GENERATION.load(Ordering::Acquire);
    tauri::async_runtime::spawn(async move {
        if generation != CLEAR_GENERATION.load(Ordering::Acquire) {
            return;
        }
        let authorized = mac_usernotifications::get_notification_settings()
            .await
            .is_ok_and(|settings| {
                matches!(
                    settings.authorization_status,
                    mac_usernotifications::AuthorizationStatus::Authorized
                        | mac_usernotifications::AuthorizationStatus::Provisional
                        | mac_usernotifications::AuthorizationStatus::Ephemeral
                )
            });
        if !authorized {
            record_show_failed();
            return;
        }
        let notification = mac_usernotifications::Notification::new()
            .id(&id)
            .title(title)
            .message(body);
        let handle = match notification.send().await {
            Ok(handle) => handle,
            Err(_) => {
                record_show_failed();
                return;
            }
        };
        if generation != CLEAR_GENERATION.load(Ordering::Acquire) {
            mac_usernotifications::cancel_pending(&id).await;
            mac_usernotifications::close_delivered(&id).await;
            return;
        }
        let (cancel, cancelled) = oneshot::channel();
        let (stale, evicted) = {
            let mut pending = registry()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if generation != CLEAR_GENERATION.load(Ordering::Acquire) {
                (true, None)
            } else {
                (false, pending.insert(id.clone(), cancel))
            }
        };
        if stale {
            mac_usernotifications::cancel_pending(&id).await;
            mac_usernotifications::close_delivered(&id).await;
            return;
        }
        if let Some((old_id, old_cancel)) = evicted {
            let _ = old_cancel.send(());
            mac_usernotifications::close_delivered(&old_id).await;
        }
        let response = tokio::select! {
            response = tokio::time::timeout(Duration::from_secs(24 * 60 * 60), handle.response()) => response.ok().and_then(Result::ok),
            _ = cancelled => None,
        };
        if response.is_none() {
            mac_usernotifications::cancel_pending(&id).await;
            mac_usernotifications::close_delivered(&id).await;
        }
        registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id);
        if generation == CLEAR_GENERATION.load(Ordering::Acquire)
            && response.is_some_and(|response| response.is_default_action())
        {
            activate_notification(&app, &fence, target);
        }
    });
    NativeNotificationOutcome::Delivered
}

pub(super) async fn clear() {
    let senders = {
        let mut pending = registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CLEAR_GENERATION.fetch_add(1, Ordering::AcqRel);
        pending.clear()
    };
    for cancel in senders {
        let _ = cancel.send(());
    }
    // Also close notifications delivered by an earlier process lifetime.
    for id in mac_usernotifications::get_delivered_notification_ids().await {
        if id.starts_with(NOTIFICATION_PREFIX) {
            mac_usernotifications::close_delivered(&id).await;
        }
    }
    for id in mac_usernotifications::get_pending_notification_ids().await {
        if id.starts_with(NOTIFICATION_PREFIX) {
            mac_usernotifications::cancel_pending(&id).await;
        }
    }
}
