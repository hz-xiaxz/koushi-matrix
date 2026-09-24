//! Orchestrates one history export: resolve the folder, read or start the
//! manifest, list the rooms, and run each room's pipeline (fetch →
//! attachments → page) in its own `.partial` folder, renaming it into place
//! only when every stage succeeded.
//!
//! The manifest is rewritten after every room settles, so an interrupted
//! export resumes with the rooms that did not complete. A stop request is
//! honoured between pages and attachments; the interrupted room's `.partial`
//! folder is removed and the table of contents is rewritten.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use koushi_protocol::HistoryExportLabels;
use koushi_state::{
    HistoryExportFailureKind, HistoryExportRange, HistoryExportRoom, HistoryExportRoomCounts,
    HistoryExportRoomFailureKind, HistoryExportRoomPhase, HistoryExportRoomSkipReason,
};

use super::attachments::{ArchiveStepError, AttachmentFetcher, StopFlag, download_attachments};
use super::driver::{
    AsyncProgress, ExportCounters, FetchFailure, FetchOutputs, HistoryPageSource, run_fetch,
};
use super::element::ExportHeader;
use super::fs::{HistoryExportFilesystem, HistoryExportFsError};
use super::html::MATH_BOOTSTRAP;
use super::html::index_page::render_index_page;
use super::html::room_page::render_room_page;
use super::katex_assets::katex_assets;
use super::layout::{export_folder_name, partial_folder_name, room_folder_name};
use super::manifest::{
    ExportManifest, MANIFEST_FILE_NAME, ManifestMatch, ManifestRoomStatus, ManifestScope,
    match_manifest,
};
use super::records::RoomMeta;
use super::space_selection::{SelectedRoom, SelectionError};

/// Attachment progress is reported at most this often, or every
/// [`PROGRESS_EVERY_FILES`] files, and always for the last one.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const PROGRESS_EVERY_FILES: u64 = 10;

/// Everything the orchestrator reads from the Matrix session.
pub(crate) trait ArchiveSource: Send {
    type Pages: HistoryPageSource;

    fn own_user_id(&self) -> &str;
    /// The room or Space name; `None` when this session does not know it.
    fn scope_title(&mut self, scope: &ManifestScope)
    -> impl Future<Output = Option<String>> + Send;
    fn select_rooms(
        &mut self,
        scope: &ManifestScope,
    ) -> impl Future<Output = Result<Vec<SelectedRoom>, SelectionError>> + Send;
    /// Element's header and a history reader; `None` when the room is gone.
    fn open_room(
        &mut self,
        room_id: &str,
    ) -> impl Future<Output = Option<(ExportHeader, Self::Pages)>> + Send;
    /// Display names for the senders of a room.
    fn sender_names(
        &mut self,
        room_id: &str,
        senders: &BTreeSet<String>,
    ) -> impl Future<Output = BTreeMap<String, String>> + Send;
}

/// Where progress goes. `progressed` is best effort and may drop updates;
/// `prepared` and `settled` are delivered.
pub(crate) trait ArchiveReporter: Send {
    fn prepared(&mut self, rooms: Vec<HistoryExportRoom>) -> impl Future<Output = ()> + Send;
    fn progressed(
        &mut self,
        room_id: &str,
        phase: HistoryExportRoomPhase,
        counts: HistoryExportRoomCounts,
    );
    fn settled(
        &mut self,
        room_id: &str,
        phase: HistoryExportRoomPhase,
        counts: HistoryExportRoomCounts,
        failure_kind: Option<HistoryExportRoomFailureKind>,
    ) -> impl Future<Output = ()> + Send;
}

pub(crate) struct ArchiveRequest {
    pub(crate) scope: ManifestScope,
    pub(crate) range: HistoryExportRange,
    /// The directory the user chose: an export folder to resume, or the
    /// parent to create a new one in.
    pub(crate) chosen_dir: PathBuf,
    pub(crate) folder_name_stem: String,
    /// Local civil date of the start, `YYYY-MM-DD`.
    pub(crate) folder_date: String,
    /// IANA zone pages show times in.
    pub(crate) time_zone: String,
    pub(crate) now_ms: u64,
    pub(crate) labels: HistoryExportLabels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveOutcome {
    Completed,
    Stopped,
}

enum RoomError {
    Failed(HistoryExportRoomFailureKind),
    Stopped,
    Write(HistoryExportFsError),
}

fn export_failure(error: HistoryExportFsError) -> HistoryExportFailureKind {
    match error {
        HistoryExportFsError::NoSpace => HistoryExportFailureKind::NoSpace,
        _ => HistoryExportFailureKind::Write,
    }
}

/// Run one export. `resolved_dir` is set as soon as the export folder is
/// known, so the caller can resume it later even after a failure.
pub(crate) async fn run_archive<S, F, R>(
    fs: &dyn HistoryExportFilesystem,
    source: &mut S,
    fetcher: &F,
    reporter: &mut R,
    stop: &StopFlag,
    request: &ArchiveRequest,
    resolved_dir: &mut Option<PathBuf>,
) -> Result<ArchiveOutcome, HistoryExportFailureKind>
where
    S: ArchiveSource,
    F: AttachmentFetcher,
    R: ArchiveReporter,
{
    let Some(title) = source.scope_title(&request.scope).await else {
        return Err(match request.scope {
            ManifestScope::Room { .. } => HistoryExportFailureKind::RoomNotFound,
            ManifestScope::Space { .. } => HistoryExportFailureKind::SpaceNotFound,
        });
    };
    let dir = resolve_export_dir(fs, request).map_err(export_failure)?;
    *resolved_dir = Some(dir.clone());
    let existing = fs.read(&dir.join(MANIFEST_FILE_NAME)).ok();
    let mut manifest = match match_manifest(existing.as_deref(), &request.scope, &request.range) {
        ManifestMatch::Fresh => {
            ExportManifest::new(request.scope.clone(), request.range.clone(), title)
        }
        ManifestMatch::Resume(manifest) => manifest,
        ManifestMatch::Mismatch => return Err(HistoryExportFailureKind::ManifestMismatch),
    };

    let selected = source
        .select_rooms(&request.scope)
        .await
        .map_err(|_| HistoryExportFailureKind::Sdk)?;
    for room in &selected {
        let previous = manifest.room(&room.room_id);
        let folder = previous
            .map(|previous| previous.folder.clone())
            .unwrap_or_else(|| room_folder_name(&room.display_name, &room.room_id));
        let (status, counts) = match previous {
            Some(previous) if previous.status == ManifestRoomStatus::Completed => {
                (ManifestRoomStatus::Completed, previous.counts)
            }
            _ if !room.target => (
                ManifestRoomStatus::Skipped,
                HistoryExportRoomCounts::default(),
            ),
            _ => (
                ManifestRoomStatus::Pending,
                HistoryExportRoomCounts::default(),
            ),
        };
        manifest.upsert_room(&room.room_id, &room.display_name, &folder, status, counts);
    }

    let rooms_dir = dir.join("rooms");
    fs.create_dir_all(&rooms_dir).map_err(export_failure)?;
    write_assets(fs, &dir).map_err(export_failure)?;
    remove_partial_folders(fs, &rooms_dir);
    write_manifest(fs, &dir, &manifest).map_err(export_failure)?;
    reporter.prepared(initial_rooms(&selected, &manifest)).await;

    let pending: Vec<SelectedRoom> = selected
        .into_iter()
        .filter(|room| {
            manifest
                .room(&room.room_id)
                .is_some_and(|entry| entry.status == ManifestRoomStatus::Pending)
        })
        .collect();
    let mut outcome = Ok(ArchiveOutcome::Completed);
    for room in pending {
        if stop.is_set() {
            outcome = Ok(ArchiveOutcome::Stopped);
            break;
        }
        let Some(folder) = manifest
            .room(&room.room_id)
            .map(|entry| entry.folder.clone())
        else {
            continue;
        };
        let partial = rooms_dir.join(partial_folder_name(&folder));
        let final_dir = rooms_dir.join(&folder);
        let result = export_room(
            fs, source, fetcher, reporter, stop, request, &room, &partial,
        )
        .await;
        let result = match result {
            Ok(counts) => commit_room(fs, &partial, &final_dir).map(|()| counts),
            Err(error) => Err(error),
        };
        match result {
            Ok(counts) => {
                manifest.upsert_room(
                    &room.room_id,
                    &room.display_name,
                    &folder,
                    ManifestRoomStatus::Completed,
                    counts,
                );
                if let Err(error) = write_manifest(fs, &dir, &manifest) {
                    outcome = Err(export_failure(error));
                    break;
                }
                reporter
                    .settled(
                        &room.room_id,
                        HistoryExportRoomPhase::Completed,
                        counts,
                        None,
                    )
                    .await;
            }
            Err(RoomError::Failed(kind)) => {
                let _ = fs.remove_dir_all(&partial);
                manifest.upsert_room(
                    &room.room_id,
                    &room.display_name,
                    &folder,
                    ManifestRoomStatus::Failed,
                    HistoryExportRoomCounts::default(),
                );
                if let Err(error) = write_manifest(fs, &dir, &manifest) {
                    outcome = Err(export_failure(error));
                    break;
                }
                reporter
                    .settled(
                        &room.room_id,
                        HistoryExportRoomPhase::Failed,
                        HistoryExportRoomCounts::default(),
                        Some(kind),
                    )
                    .await;
            }
            Err(RoomError::Stopped) => {
                let _ = fs.remove_dir_all(&partial);
                outcome = Ok(ArchiveOutcome::Stopped);
                break;
            }
            Err(RoomError::Write(error)) => {
                let _ = fs.remove_dir_all(&partial);
                outcome = Err(export_failure(error));
                break;
            }
        }
    }

    // Best effort on failure: the disk may be full.
    let manifest_written = write_manifest(fs, &dir, &manifest);
    let index = render_index_page(
        &manifest,
        &request.labels,
        request.now_ms,
        &request.time_zone,
    );
    let index_written = fs.write_atomic(&dir.join("index.html"), &index);
    if outcome.is_ok() {
        manifest_written.map_err(export_failure)?;
        index_written.map_err(export_failure)?;
    }
    outcome
}

/// The chosen directory is the export folder when it holds a manifest;
/// otherwise a folder named from the stem and date is used inside it. An
/// existing folder of that name without a manifest is never reused.
fn resolve_export_dir(
    fs: &dyn HistoryExportFilesystem,
    request: &ArchiveRequest,
) -> Result<PathBuf, HistoryExportFsError> {
    let chosen = &request.chosen_dir;
    if fs.exists(&chosen.join(MANIFEST_FILE_NAME)) {
        return Ok(chosen.clone());
    }
    let base = export_folder_name(&request.folder_name_stem, &request.folder_date);
    for attempt in 1_u32.. {
        let name = if attempt == 1 {
            base.clone()
        } else {
            format!("{base} ({attempt})")
        };
        let candidate = chosen.join(name);
        if !fs.exists(&candidate) || fs.exists(&candidate.join(MANIFEST_FILE_NAME)) {
            fs.create_dir_all(&candidate)?;
            return Ok(candidate);
        }
    }
    Err(HistoryExportFsError::Io)
}

fn write_assets(fs: &dyn HistoryExportFilesystem, dir: &Path) -> Result<(), HistoryExportFsError> {
    let assets = dir.join("assets");
    fs.create_dir_all(&assets.join("katex").join("fonts"))?;
    let bootstrap = assets.join("koushi-math.js");
    if !fs.exists(&bootstrap) {
        fs.write_atomic(&bootstrap, MATH_BOOTSTRAP)?;
    }
    for asset in katex_assets() {
        let path = assets.join("katex").join(asset.path);
        if !fs.exists(&path) {
            fs.write_atomic(&path, asset.bytes)?;
        }
    }
    Ok(())
}

fn remove_partial_folders(fs: &dyn HistoryExportFilesystem, rooms_dir: &Path) {
    for name in fs.list_dir(rooms_dir).unwrap_or_default() {
        if name.starts_with('.') && name.ends_with(".partial") {
            let _ = fs.remove_dir_all(&rooms_dir.join(name));
        }
    }
}

fn write_manifest(
    fs: &dyn HistoryExportFilesystem,
    dir: &Path,
    manifest: &ExportManifest,
) -> Result<(), HistoryExportFsError> {
    fs.write_atomic(&dir.join(MANIFEST_FILE_NAME), &manifest.to_bytes())
}

fn commit_room(
    fs: &dyn HistoryExportFilesystem,
    partial: &Path,
    final_dir: &Path,
) -> Result<(), RoomError> {
    // A folder left by an earlier failed or superseded run is replaced.
    fs.remove_dir_all(final_dir).map_err(RoomError::Write)?;
    fs.rename(partial, final_dir).map_err(RoomError::Write)
}

fn initial_rooms(selected: &[SelectedRoom], manifest: &ExportManifest) -> Vec<HistoryExportRoom> {
    selected
        .iter()
        .filter_map(|room| {
            let entry = manifest.room(&room.room_id)?;
            let phase = match entry.status {
                ManifestRoomStatus::Completed => HistoryExportRoomPhase::Completed,
                ManifestRoomStatus::Skipped => HistoryExportRoomPhase::Skipped,
                ManifestRoomStatus::Failed | ManifestRoomStatus::Pending => {
                    HistoryExportRoomPhase::Pending
                }
            };
            Some(HistoryExportRoom {
                room_id: room.room_id.clone(),
                display_name: room.display_name.clone(),
                phase,
                counts: entry.counts,
                skip_reason: (phase == HistoryExportRoomPhase::Skipped)
                    .then_some(HistoryExportRoomSkipReason::NotJoined),
                failure_kind: None,
            })
        })
        .collect()
}

struct FetchProgress<'a, R> {
    reporter: &'a mut R,
    room_id: &'a str,
}

impl<R: ArchiveReporter> AsyncProgress for FetchProgress<'_, R> {
    async fn page_completed(&mut self, counts: HistoryExportRoomCounts) {
        self.reporter
            .progressed(self.room_id, HistoryExportRoomPhase::Fetching, counts);
    }
}

fn step_error(error: ArchiveStepError) -> RoomError {
    match error {
        ArchiveStepError::Stopped => RoomError::Stopped,
        ArchiveStepError::Write(error) => RoomError::Write(error),
    }
}

#[allow(clippy::too_many_arguments)]
async fn export_room<S, F, R>(
    fs: &dyn HistoryExportFilesystem,
    source: &mut S,
    fetcher: &F,
    reporter: &mut R,
    stop: &StopFlag,
    request: &ArchiveRequest,
    room: &SelectedRoom,
    partial: &Path,
) -> Result<HistoryExportRoomCounts, RoomError>
where
    S: ArchiveSource,
    F: AttachmentFetcher,
    R: ArchiveReporter,
{
    let room_id = room.room_id.as_str();
    fs.create_dir_all(partial).map_err(RoomError::Write)?;
    let Some((header, mut pages)) = source.open_room(room_id).await else {
        return Err(RoomError::Failed(HistoryExportRoomFailureKind::Sdk));
    };
    reporter.progressed(
        room_id,
        HistoryExportRoomPhase::Fetching,
        HistoryExportRoomCounts::default(),
    );
    let counters = Arc::new(ExportCounters::default());
    let outputs = FetchOutputs {
        messages: fs
            .create_file(&partial.join("messages.json"))
            .map_err(RoomError::Write)?,
        events: fs
            .create_file(&partial.join("events.jsonl"))
            .map_err(RoomError::Write)?,
    };
    let own_user_id = source.own_user_id().to_owned();
    let fetched = run_fetch(
        &mut pages,
        outputs,
        &header,
        &request.range,
        &own_user_id,
        &counters,
        stop,
        FetchProgress {
            reporter: &mut *reporter,
            room_id,
        },
    )
    .await
    .map_err(|failure| match failure {
        FetchFailure::Network => RoomError::Failed(HistoryExportRoomFailureKind::Network),
        FetchFailure::Sdk => RoomError::Failed(HistoryExportRoomFailureKind::Sdk),
        FetchFailure::Write(error) => RoomError::Write(error),
        FetchFailure::Stopped => RoomError::Stopped,
    })?;

    let meta = RoomMeta {
        room_id: room_id.to_owned(),
        name: header.room_name.clone(),
        topic: header.topic.clone(),
        senders: source.sender_names(room_id, &fetched.senders).await,
        exported_at_ms: request.now_ms,
        time_zone: request.time_zone.clone(),
    };
    write_json(fs, &partial.join("room.json"), &meta)?;

    let mut counts = counters.snapshot();
    counts.attachments_total = fetched.attachments.len() as u64;
    reporter.progressed(room_id, HistoryExportRoomPhase::Attachments, counts);
    let mut last_report = Instant::now();
    let attachments = download_attachments(
        fetcher,
        fs,
        partial,
        fetched.attachments,
        stop,
        |done, total, failed| {
            counts.attachments_done = done;
            counts.attachments_failed = failed;
            let due = done == total
                || done % PROGRESS_EVERY_FILES == 0
                || last_report.elapsed() >= PROGRESS_INTERVAL;
            if due {
                last_report = Instant::now();
                reporter.progressed(room_id, HistoryExportRoomPhase::Attachments, counts);
            }
        },
    )
    .await
    .map_err(step_error)?;
    write_json(fs, &partial.join("attachments.json"), &attachments)?;

    if stop.is_set() {
        return Err(RoomError::Stopped);
    }
    reporter.progressed(room_id, HistoryExportRoomPhase::Rendering, counts);
    let events = fs
        .read(&partial.join("events.jsonl"))
        .map_err(RoomError::Write)?;
    let page = render_room_page(&events, &meta, &attachments, &request.labels);
    fs.write_atomic(&partial.join("index.html"), &page)
        .map_err(RoomError::Write)?;
    Ok(counts)
}

fn write_json<T: serde::Serialize>(
    fs: &dyn HistoryExportFilesystem,
    path: &Path,
    value: &T,
) -> Result<(), RoomError> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap_or_default();
    bytes.push(b'\n');
    fs.write_atomic(path, &bytes).map_err(RoomError::Write)
}
