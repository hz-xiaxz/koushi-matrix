//! Native file drops for the composer.
//!
//! WebKitGTK does not expose files dragged from a file manager to the page, so
//! Linux enables Tauri's native drag/drop and this adapter hands the dropped
//! files to the existing renderer ingestion path.
//!
//! The webview never names a path. The ledger records only what the windowing
//! system reported for the latest drop, the renderer claims it once, and each
//! claimed file can be read once by an opaque token. Paths, names, and bytes
//! are user data and are never logged or recorded in diagnostics.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use koushi_core::media_staging::{MAX_MEDIA_STAGING_BATCH_BYTES, MAX_MEDIA_STAGING_BATCH_SIZE};
use serde::Serialize;
use tauri::{State, ipc::Response};

#[derive(Default)]
pub struct DroppedFileLedger {
    inner: Mutex<LedgerState>,
}

#[derive(Default)]
struct LedgerState {
    dropped: Vec<PathBuf>,
    claimed: HashMap<u64, PathBuf>,
    next_token: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedDroppedFile {
    token: u64,
    filename: String,
    mime_type: String,
}

impl DroppedFileLedger {
    /// Replaces the pending drop; an unclaimed earlier drop is forgotten.
    pub fn record_drop(&self, paths: &[PathBuf]) {
        let mut state = self.lock();
        state.dropped = paths.to_vec();
        state.claimed.clear();
    }

    /// Moves the pending drop into single-read tokens, in drop order. Anything
    /// that is not a regular file within the staging limits is skipped.
    fn claim(&self) -> Vec<ClaimedDroppedFile> {
        let mut state = self.lock();
        let dropped = std::mem::take(&mut state.dropped);
        let mut total_bytes = 0_u64;
        let mut claimed = Vec::new();
        for path in dropped {
            if claimed.len() == MAX_MEDIA_STAGING_BATCH_SIZE {
                break;
            }
            let Some(byte_count) = regular_file_len(&path) else {
                continue;
            };
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            total_bytes = total_bytes.saturating_add(byte_count);
            if total_bytes > MAX_MEDIA_STAGING_BATCH_BYTES as u64 {
                break;
            }
            let token = state.next_token;
            state.next_token += 1;
            claimed.push(ClaimedDroppedFile {
                token,
                filename: filename.to_owned(),
                mime_type: mime_type_for(&path),
            });
            state.claimed.insert(token, path);
        }
        claimed
    }

    fn take_claimed(&self, token: u64) -> Option<PathBuf> {
        self.lock().claimed.remove(&token)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LedgerState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn regular_file_len(path: &Path) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;
    metadata.is_file().then(|| metadata.len())
}

/// The shared-mime-info type the desktop itself would report for the file,
/// matching what a browser puts in `File.type`.
#[cfg(target_os = "linux")]
fn mime_type_for(path: &Path) -> String {
    use std::io::Read;

    // Name plus leading bytes, exactly as the desktop file manager decides.
    let mut head = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(4096).read_to_end(&mut head);
    }
    let (content_type, _uncertain) = gtk::gio::content_type_guess(Some(path), &head);
    gtk::gio::content_type_get_mime_type(&content_type)
        .map(|mime| mime.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

#[cfg(not(target_os = "linux"))]
fn mime_type_for(_path: &Path) -> String {
    "application/octet-stream".to_owned()
}

#[tauri::command]
pub async fn claim_dropped_files(
    ledger: State<'_, DroppedFileLedger>,
) -> Result<Vec<ClaimedDroppedFile>, String> {
    Ok(ledger.claim())
}

/// Answers with the file bytes as a raw IPC body, or an empty body when the
/// token is unknown, already read, or the file is no longer readable.
#[tauri::command]
pub async fn read_dropped_file(
    token: u64,
    ledger: State<'_, DroppedFileLedger>,
) -> Result<Response, String> {
    let Some(path) = ledger.take_claimed(token) else {
        return Ok(Response::new(Vec::new()));
    };
    let bytes = tauri::async_runtime::spawn_blocking(move || read_within_limit(&path))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    Ok(Response::new(bytes))
}

fn read_within_limit(path: &Path) -> Option<Vec<u8>> {
    // The file can change between claim and read; re-check before loading it.
    let byte_count = regular_file_len(path)?;
    if byte_count > MAX_MEDIA_STAGING_BATCH_BYTES as u64 {
        return None;
    }
    std::fs::read(path).ok()
}

#[cfg(test)]
mod tests;
