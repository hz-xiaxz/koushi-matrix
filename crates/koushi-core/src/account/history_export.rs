//! AccountActor ownership of the room-history export task (#59).
//!
//! The actor admits one export at a time, spawns it so the mailbox keeps
//! serving other commands, and owns the task handle: a cancel command or
//! session teardown aborts the task and awaits it before settling the reducer.
//! Dropping the uncommitted export file removes its staged bytes.

use std::sync::Arc;

use futures_util::FutureExt;

use koushi_protocol::command::RoomHistoryExportRequest;
use koushi_protocol::failure::{CoreFailure, RoomFailureKind};
use koushi_protocol::ids::RequestId;
use koushi_state::{AppAction, RoomHistoryExportFailureKind, RoomHistoryExportProgress};

use super::actor::AccountActor;
use crate::executor;
use crate::native_artifact::NativeArtifactKind;
use crate::room_history_export::{
    AsyncProgress, ExportCounters, MatrixRoomHistorySource, RoomHistoryExportSink,
    room_export_header, run_export,
};

/// The export currently owned by the actor.
pub(super) struct ActiveRoomHistoryExport {
    request_id: RequestId,
    counters: Arc<ExportCounters>,
    /// The task's terminal action, recorded before it is sent. An abort that
    /// lands while that send is pending must settle with this outcome: the
    /// destination may already have been committed.
    settlement: Arc<std::sync::Mutex<Option<AppAction>>>,
    task: executor::JoinHandle<()>,
}

struct ProgressToReducer {
    request_id: u64,
    action_tx: tokio::sync::mpsc::Sender<Vec<AppAction>>,
}

impl AsyncProgress for ProgressToReducer {
    async fn page_completed(&mut self, progress: RoomHistoryExportProgress) {
        let _ = self
            .action_tx
            .send(vec![AppAction::RoomHistoryExportProgressed {
                request_id: self.request_id,
                progress,
            }])
            .await;
    }
}

fn failure_event(kind: RoomHistoryExportFailureKind) -> CoreFailure {
    CoreFailure::RoomOperationFailed {
        kind: match kind {
            RoomHistoryExportFailureKind::RoomNotFound => RoomFailureKind::NotFound,
            RoomHistoryExportFailureKind::Network => RoomFailureKind::Network,
            RoomHistoryExportFailureKind::InvalidRange
            | RoomHistoryExportFailureKind::DestinationUnavailable
            | RoomHistoryExportFailureKind::Write
            | RoomHistoryExportFailureKind::Sdk => RoomFailureKind::Sdk,
        },
    }
}

impl AccountActor {
    async fn fail_room_history_export(
        &self,
        request_id: RequestId,
        kind: RoomHistoryExportFailureKind,
    ) {
        self.send_actions(vec![AppAction::RoomHistoryExportFailed {
            request_id: request_id.sequence,
            kind,
            progress: RoomHistoryExportProgress::default(),
        }])
        .await;
        self.emit_failure(request_id, failure_event(kind));
    }

    pub(super) async fn handle_export_room_history(
        &mut self,
        request_id: RequestId,
        request: RoomHistoryExportRequest,
        locale: koushi_state::CatalogLocale,
    ) {
        let destination = self
            .native_artifacts
            .take(request_id, NativeArtifactKind::RoomHistoryExportDestination);
        let Some(session) = self.session.clone() else {
            self.send_actions(vec![AppAction::RoomHistoryExportFailed {
                request_id: request_id.sequence,
                kind: RoomHistoryExportFailureKind::Sdk,
                progress: RoomHistoryExportProgress::default(),
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        let Ok(destination) = destination else {
            self.fail_room_history_export(
                request_id,
                RoomHistoryExportFailureKind::DestinationUnavailable,
            )
            .await;
            return;
        };
        let RoomHistoryExportRequest {
            room_id,
            range,
            export_date_utc_offset_minutes,
        } = request;
        if !range.is_valid() {
            self.fail_room_history_export(request_id, RoomHistoryExportFailureKind::InvalidRange)
                .await;
            return;
        }
        let room = room_id
            .parse::<matrix_sdk::ruma::OwnedRoomId>()
            .ok()
            .and_then(|room_id| session.client().get_room(&room_id));
        let Some(room) = room else {
            self.fail_room_history_export(request_id, RoomHistoryExportFailureKind::RoomNotFound)
                .await;
            return;
        };
        let file = match self.room_history_export_sink.create(&destination) {
            Ok(file) => file,
            Err(_) => {
                self.fail_room_history_export(request_id, RoomHistoryExportFailureKind::Write)
                    .await;
                return;
            }
        };

        // The reducer admits one export at a time; a finished handle may
        // still be retained and is released here.
        if let Some(previous) = self.room_history_export.take() {
            previous.task.abort();
            let _ = previous.task.await;
        }

        let counters = Arc::new(ExportCounters::default());
        let action_tx = self.action_tx.clone();
        let event_tx = self.event_tx.clone();
        let account_work = self.account_work.clone();
        let own_user_id = session.info.user_id.clone();
        let task_counters = Arc::clone(&counters);
        let settlement = Arc::new(std::sync::Mutex::new(None));
        let task_settlement = Arc::clone(&settlement);
        let task = executor::spawn(async move {
            let export = async {
                let header = room_export_header(
                    &room,
                    crate::time::current_epoch_ms(),
                    export_date_utc_offset_minutes,
                    locale,
                )
                .await;
                let mut source = MatrixRoomHistorySource::new(room, account_work);
                let progress = ProgressToReducer {
                    request_id: request_id.sequence,
                    action_tx: action_tx.clone(),
                };
                run_export(
                    &mut source,
                    file,
                    &header,
                    &range,
                    &own_user_id,
                    &task_counters,
                    progress,
                )
                .await
            };
            // A panic inside the export must still settle the reducer.
            let result = std::panic::AssertUnwindSafe(export)
                .catch_unwind()
                .await
                .unwrap_or(Err(RoomHistoryExportFailureKind::Sdk));
            let progress = task_counters.snapshot();
            let action = match result {
                Ok(()) => AppAction::RoomHistoryExportCompleted {
                    request_id: request_id.sequence,
                    progress,
                },
                Err(kind) => {
                    let _ = event_tx.send(koushi_protocol::event::CoreEvent::OperationFailed {
                        request_id,
                        failure: failure_event(kind),
                    });
                    AppAction::RoomHistoryExportFailed {
                        request_id: request_id.sequence,
                        kind,
                        progress,
                    }
                }
            };
            if let Ok(mut slot) = task_settlement.lock() {
                *slot = Some(action.clone());
            }
            let _ = action_tx.send(vec![action]).await;
        });
        self.room_history_export = Some(ActiveRoomHistoryExport {
            request_id,
            counters,
            settlement,
            task,
        });
    }

    /// Cancel the export started by `target_request_id`. An export that has
    /// already settled is left as it is.
    pub(super) async fn handle_cancel_room_history_export(
        &mut self,
        request_id: RequestId,
        target_request_id: RequestId,
    ) {
        let is_target = self
            .room_history_export
            .as_ref()
            .is_some_and(|active| active.request_id == target_request_id);
        if !is_target {
            self.emit_failure(
                request_id,
                CoreFailure::RoomOperationFailed {
                    kind: RoomFailureKind::NotFound,
                },
            );
            return;
        }
        self.stop_room_history_export().await;
    }

    /// Abort and await the export task, then settle an unfinished export as
    /// cancelled. Used by cancel and by session teardown.
    pub(super) async fn stop_room_history_export(&mut self) {
        let Some(active) = self.room_history_export.take() else {
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
        let action = recorded.unwrap_or(AppAction::RoomHistoryExportCancelled {
            request_id: active.request_id.sequence,
            progress: active.counters.snapshot(),
        });
        self.send_actions(vec![action]).await;
    }
}

/// Default destination sink for the actor.
pub(super) fn default_room_history_export_sink() -> Arc<dyn RoomHistoryExportSink> {
    Arc::new(crate::room_history_export::NativeRoomHistoryExportSink)
}
