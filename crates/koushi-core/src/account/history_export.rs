//! AccountActor ownership of the history export task.
//!
//! The actor admits one export at a time and spawns it so the mailbox keeps
//! serving other commands. Stop is cooperative: the task finishes its current
//! step, removes the interrupted room's `.partial` folder, and settles as
//! stopped. Session teardown aborts the task instead; the folder's manifest
//! still lets a later export resume it. The resolved export folder stays in
//! the actor after settlement so a retry can resume it without the path ever
//! leaving Core.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_util::FutureExt;

use koushi_protocol::command::HistoryExportRequest;
use koushi_protocol::failure::{CoreFailure, RoomFailureKind};
use koushi_protocol::ids::RequestId;
use koushi_state::{
    AppAction, HistoryExportFailureKind, HistoryExportRoom, HistoryExportRoomCounts,
    HistoryExportRoomFailureKind, HistoryExportRoomPhase, HistoryExportScope,
};

use super::actor::AccountActor;
use crate::executor;
use crate::native_artifact::NativeArtifactKind;
use crate::room_history_export::{
    ArchiveOutcome, ArchiveReporter, ArchiveRequest, HistoryExportFilesystem, ManifestScope,
    SdkArchiveSource, SdkAttachmentFetcher, StopFlag, run_archive,
};

/// The export currently owned by the actor.
pub(super) struct ActiveHistoryExport {
    request_id: RequestId,
    stop: StopFlag,
    /// The task's terminal action, recorded before it is sent, so an abort
    /// that lands during that send settles with the real outcome.
    settlement: Arc<Mutex<Option<AppAction>>>,
    task: executor::JoinHandle<()>,
}

/// The latest export, kept after it settles so it can be retried.
pub(super) struct LastHistoryExport {
    request_id: RequestId,
    export_dir: Arc<Mutex<Option<PathBuf>>>,
    request: HistoryExportRequest,
    locale: koushi_state::CatalogLocale,
}

struct ToReducer {
    request_id: u64,
    action_tx: tokio::sync::mpsc::Sender<Vec<AppAction>>,
}

impl ArchiveReporter for ToReducer {
    async fn prepared(&mut self, rooms: Vec<HistoryExportRoom>) {
        let _ = self
            .action_tx
            .send(vec![AppAction::HistoryExportPrepared {
                request_id: self.request_id,
                rooms,
            }])
            .await;
    }

    fn progressed(
        &mut self,
        room_id: &str,
        phase: HistoryExportRoomPhase,
        counts: HistoryExportRoomCounts,
    ) {
        // Progress is superseded by the next update or the settlement, so a
        // full channel drops it instead of stalling the export.
        let _ = self
            .action_tx
            .try_send(vec![AppAction::HistoryExportRoomProgressed {
                request_id: self.request_id,
                room_id: room_id.to_owned(),
                phase,
                counts,
            }]);
    }

    async fn settled(
        &mut self,
        room_id: &str,
        phase: HistoryExportRoomPhase,
        counts: HistoryExportRoomCounts,
        failure_kind: Option<HistoryExportRoomFailureKind>,
    ) {
        let _ = self
            .action_tx
            .send(vec![AppAction::HistoryExportRoomSettled {
                request_id: self.request_id,
                room_id: room_id.to_owned(),
                phase,
                counts,
                failure_kind,
            }])
            .await;
    }
}

fn failure_event(kind: HistoryExportFailureKind) -> CoreFailure {
    CoreFailure::RoomOperationFailed {
        kind: match kind {
            HistoryExportFailureKind::RoomNotFound | HistoryExportFailureKind::SpaceNotFound => {
                RoomFailureKind::NotFound
            }
            HistoryExportFailureKind::Network => RoomFailureKind::Network,
            HistoryExportFailureKind::InvalidRange
            | HistoryExportFailureKind::DestinationUnavailable
            | HistoryExportFailureKind::ManifestMismatch
            | HistoryExportFailureKind::Write
            | HistoryExportFailureKind::NoSpace
            | HistoryExportFailureKind::Sdk => RoomFailureKind::Sdk,
        },
    }
}

fn manifest_scope(scope: &HistoryExportScope) -> ManifestScope {
    match scope {
        HistoryExportScope::Room { room_id } => ManifestScope::Room {
            id: room_id.clone(),
        },
        HistoryExportScope::Space { space_id } => ManifestScope::Space {
            id: space_id.clone(),
        },
    }
}

/// `YYYY-MM-DD` of `now_ms` shifted by the platform's UTC offset.
fn local_civil_date(now_ms: u64, utc_offset_minutes: i32) -> String {
    let shifted = i64::try_from(now_ms)
        .unwrap_or_default()
        .saturating_add(i64::from(utc_offset_minutes) * 60_000);
    jiff::Timestamp::from_millisecond(shifted)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string()
}

impl AccountActor {
    async fn fail_history_export(&self, request_id: RequestId, kind: HistoryExportFailureKind) {
        self.send_actions(vec![AppAction::HistoryExportFailed {
            request_id: request_id.sequence,
            kind,
        }])
        .await;
        self.emit_failure(request_id, failure_event(kind));
    }

    pub(super) async fn handle_export_history(
        &mut self,
        request_id: RequestId,
        request: HistoryExportRequest,
        locale: koushi_state::CatalogLocale,
    ) {
        let chosen = self
            .native_artifacts
            .take(request_id, NativeArtifactKind::HistoryExportDirectory);
        if self.session.is_none() {
            self.send_actions(vec![AppAction::HistoryExportFailed {
                request_id: request_id.sequence,
                kind: HistoryExportFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        }
        let Ok(chosen) = chosen else {
            self.fail_history_export(request_id, HistoryExportFailureKind::DestinationUnavailable)
                .await;
            return;
        };
        if !request.range.is_valid() {
            self.fail_history_export(request_id, HistoryExportFailureKind::InvalidRange)
                .await;
            return;
        }
        self.spawn_history_export(request_id, chosen, request, locale)
            .await;
    }

    async fn spawn_history_export(
        &mut self,
        request_id: RequestId,
        chosen_dir: PathBuf,
        request: HistoryExportRequest,
        locale: koushi_state::CatalogLocale,
    ) {
        let Some(session) = self.session.clone() else {
            self.fail_history_export(request_id, HistoryExportFailureKind::Sdk)
                .await;
            return;
        };
        // The reducer admits one export at a time; a finished handle may
        // still be retained and is released here.
        if let Some(previous) = self.history_export.take() {
            previous.task.abort();
            let _ = previous.task.await;
        }

        let stop = StopFlag::default();
        let settlement = Arc::new(Mutex::new(None));
        let export_dir = Arc::new(Mutex::new(None));
        let action_tx = self.action_tx.clone();
        let event_tx = self.event_tx.clone();
        let account_work = self.account_work.clone();
        let fs = Arc::clone(&self.history_export_fs);
        let task_stop = stop.clone();
        let task_settlement = Arc::clone(&settlement);
        let task_export_dir = Arc::clone(&export_dir);
        let task_request = request.clone();
        let task = executor::spawn(async move {
            let now_ms = crate::time::current_epoch_ms();
            let archive_request = ArchiveRequest {
                scope: manifest_scope(&task_request.scope),
                range: task_request.range.clone(),
                chosen_dir,
                folder_name_stem: task_request.folder_name_stem.clone(),
                folder_date: local_civil_date(
                    now_ms,
                    task_request.export_date_utc_offset_minutes,
                ),
                time_zone: task_request.display_time_zone.clone(),
                now_ms,
                labels: task_request.labels.clone(),
            };
            let export = async {
                let mut source = SdkArchiveSource::new(
                    Arc::clone(&session),
                    account_work.clone(),
                    now_ms,
                    task_request.export_date_utc_offset_minutes,
                    locale,
                );
                let fetcher =
                    SdkAttachmentFetcher::new(session.client().clone(), account_work.clone());
                let mut reporter = ToReducer {
                    request_id: request_id.sequence,
                    action_tx: action_tx.clone(),
                };
                let mut resolved = None;
                let result = run_archive(
                    fs.as_ref(),
                    &mut source,
                    &fetcher,
                    &mut reporter,
                    &task_stop,
                    &archive_request,
                    &mut resolved,
                )
                .await;
                if let (Some(dir), Ok(mut slot)) = (resolved, task_export_dir.lock()) {
                    *slot = Some(dir);
                }
                result
            };
            // A panic inside the export must still settle the reducer.
            let result = std::panic::AssertUnwindSafe(export)
                .catch_unwind()
                .await
                .unwrap_or(Err(HistoryExportFailureKind::Sdk));
            let action = match result {
                Ok(ArchiveOutcome::Completed) => AppAction::HistoryExportCompleted {
                    request_id: request_id.sequence,
                },
                Ok(ArchiveOutcome::Stopped) => AppAction::HistoryExportStopped {
                    request_id: request_id.sequence,
                },
                Err(kind) => {
                    let _ = event_tx.send(koushi_protocol::event::CoreEvent::OperationFailed {
                        request_id,
                        failure: failure_event(kind),
                    });
                    AppAction::HistoryExportFailed {
                        request_id: request_id.sequence,
                        kind,
                    }
                }
            };
            if let Ok(mut slot) = task_settlement.lock() {
                *slot = Some(action.clone());
            }
            let _ = action_tx.send(vec![action]).await;
        });
        self.history_export = Some(ActiveHistoryExport {
            request_id,
            stop,
            settlement,
            task,
        });
        self.last_history_export = Some(LastHistoryExport {
            request_id,
            export_dir,
            request,
            locale,
        });
    }

    /// Ask the export started by `target_request_id` to stop after its
    /// current step. An export that already settled is left as it is.
    pub(super) async fn handle_stop_history_export(
        &mut self,
        request_id: RequestId,
        target_request_id: RequestId,
    ) {
        match &self.history_export {
            Some(active) if active.request_id == target_request_id => active.stop.set(),
            _ => self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::NotFound,
                },
            ),
        }
    }

    /// Resume the folder of the settled export `target_request_id`.
    pub(super) async fn handle_retry_history_export(
        &mut self,
        request_id: RequestId,
        target_request_id: RequestId,
    ) {
        let still_running = self
            .history_export
            .as_ref()
            .is_some_and(|active| !active.task.is_finished());
        let retry = self
            .last_history_export
            .as_ref()
            .filter(|last| last.request_id == target_request_id && !still_running)
            .map(|last| {
                let dir = last.export_dir.lock().ok().and_then(|slot| slot.clone());
                (dir, last.request.clone(), last.locale)
            });
        match retry {
            Some((Some(dir), request, locale)) => {
                self.spawn_history_export(request_id, dir, request, locale)
                    .await;
            }
            Some((None, _, _)) => {
                self.fail_history_export(
                    request_id,
                    HistoryExportFailureKind::DestinationUnavailable,
                )
                .await;
            }
            None => {
                // The reducer admitted the retry into Preparing; settle it.
                self.fail_history_export(request_id, HistoryExportFailureKind::Sdk)
                    .await;
            }
        }
    }

    /// Abort and await the export task, then settle an unfinished export as
    /// stopped. Used by session teardown; the retry target is forgotten
    /// because it belongs to this session.
    pub(super) async fn stop_history_export(&mut self) {
        self.last_history_export = None;
        let Some(active) = self.history_export.take() else {
            return;
        };
        active.task.abort();
        if active.task.await.is_ok() {
            // The task ran to completion and sent its own settlement.
            return;
        }
        let recorded = active
            .settlement
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let action = recorded.unwrap_or(AppAction::HistoryExportStopped {
            request_id: active.request_id.sequence,
        });
        self.send_actions(vec![action]).await;
    }
}

/// Default filesystem for exports.
pub(super) fn default_history_export_fs() -> Arc<dyn HistoryExportFilesystem> {
    Arc::new(crate::room_history_export::NativeHistoryExportFilesystem)
}

#[cfg(test)]
mod tests {
    use super::local_civil_date;

    #[test]
    fn folder_date_uses_the_platform_offset() {
        // 2025-09-25T15:30:00Z is already 2025-09-26 in UTC+09:00.
        let now = 1_758_814_200_000;
        assert_eq!(local_civil_date(now, 0), "2025-09-25");
        assert_eq!(local_civil_date(now, 540), "2025-09-26");
        assert_eq!(local_civil_date(now, -600), "2025-09-25");
    }
}
