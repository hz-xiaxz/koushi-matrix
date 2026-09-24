//! Attachment stage of a room export: find each attachment in the fetched
//! events, download it (decrypting encrypted media), write it under
//! `files/`, and write a thumbnail under `thumbs/` for images.
//!
//! Files are fetched one at a time and held in memory only while being
//! written, because the SDK returns media as a single buffer.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use matrix_sdk::media::{MediaFormat, MediaRequestParameters};
use matrix_sdk::ruma::OwnedMxcUri;
use matrix_sdk::ruma::events::room::{EncryptedFile, MediaSource};
use serde_json::Value;

use super::fs::{HistoryExportFilesystem, HistoryExportFsError};
use super::layout::{attachment_file_name, thumbnail_file_name};
use super::records::{AttachmentIndex, AttachmentKind, AttachmentRecord, AttachmentStatus};
use super::thumbnail::{THUMB_MAX_EDGE, thumbnail_jpeg};
use crate::account_work::{AccountWorkKind, AccountWorkScheduler};
use crate::executor;

/// Attempts per attachment for transient failures.
pub(crate) const FETCH_ATTEMPTS: u32 = 3;
/// Waits before the second and third attempts.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
/// Upper bound for one download; generous because attachments can be videos.
const ATTACHMENT_FETCH_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Cooperative stop request shared by an export task and its owner.
#[derive(Clone)]
pub(crate) struct StopFlag(Arc<tokio::sync::watch::Sender<bool>>);

impl Default for StopFlag {
    fn default() -> Self {
        Self(Arc::new(tokio::sync::watch::Sender::new(false)))
    }
}

impl StopFlag {
    pub(crate) fn set(&self) {
        self.0.send_replace(true);
    }

    pub(crate) fn is_set(&self) -> bool {
        *self.0.borrow()
    }

    /// Resolves once [`StopFlag::set`] has been called.
    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|stopped| *stopped).await;
    }
}

/// Why a step of a room export did not finish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveStepError {
    Stopped,
    Write(HistoryExportFsError),
}

impl From<HistoryExportFsError> for ArchiveStepError {
    fn from(error: HistoryExportFsError) -> Self {
        Self::Write(error)
    }
}

/// One attachment found in an event.
#[derive(Clone, Debug)]
pub(crate) struct AttachmentRef {
    pub(crate) event_id: String,
    pub(crate) kind: AttachmentKind,
    pub(crate) name: String,
    pub(crate) size: Option<u64>,
    pub(crate) mimetype: Option<String>,
    pub(crate) source: MediaSource,
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The attachment an `m.room.message` (image, file, video, audio) or
/// `m.sticker` event carries, from its original content.
pub(crate) fn attachment_ref(event: &Value) -> Option<AttachmentRef> {
    let content = event.get("content")?;
    let kind = match (str_at(event, "type")?, str_at(content, "msgtype")) {
        ("m.room.message", Some("m.image")) => AttachmentKind::Image,
        ("m.room.message", Some("m.file")) => AttachmentKind::File,
        ("m.room.message", Some("m.video")) => AttachmentKind::Video,
        ("m.room.message", Some("m.audio")) => AttachmentKind::Audio,
        ("m.sticker", _) => AttachmentKind::Sticker,
        _ => return None,
    };
    let source = match content.get("file") {
        Some(file) => MediaSource::Encrypted(Box::new(
            serde_json::from_value::<EncryptedFile>(file.clone()).ok()?,
        )),
        None => MediaSource::Plain(OwnedMxcUri::from(str_at(content, "url")?)),
    };
    let info = content.get("info");
    Some(AttachmentRef {
        event_id: str_at(event, "event_id")?.to_owned(),
        kind,
        name: str_at(content, "filename")
            .or_else(|| str_at(content, "body"))
            .unwrap_or_default()
            .to_owned(),
        size: info.and_then(|info| info.get("size")).and_then(Value::as_u64),
        mimetype: info
            .and_then(|info| str_at(info, "mimetype"))
            .map(str::to_owned),
        source,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FetchError {
    /// Worth retrying: transport failures and timeouts.
    Transient,
    /// Not retried: the server refused, the media is gone, or decryption failed.
    Permanent,
}

pub(crate) trait AttachmentFetcher: Send + Sync {
    fn fetch(
        &self,
        source: &MediaSource,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}

/// Downloads through the SDK media API without writing the SDK media cache.
pub(crate) struct SdkAttachmentFetcher {
    client: matrix_sdk::Client,
    account_work: AccountWorkScheduler,
}

impl SdkAttachmentFetcher {
    pub(crate) fn new(client: matrix_sdk::Client, account_work: AccountWorkScheduler) -> Self {
        Self {
            client,
            account_work,
        }
    }
}

impl AttachmentFetcher for SdkAttachmentFetcher {
    async fn fetch(&self, source: &MediaSource) -> Result<Vec<u8>, FetchError> {
        // Wait for background admission, but do not restart a long download
        // when visible work later preempts the permit.
        let _permit = self.account_work.acquire(AccountWorkKind::SearchCrawl).await;
        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::File,
        };
        match executor::timeout(
            ATTACHMENT_FETCH_TIMEOUT,
            self.client.media().get_media_content(&request, false),
        )
        .await
        {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(matrix_sdk::Error::Http(error))) if error.as_client_api_error().is_some() => {
                Err(FetchError::Permanent)
            }
            Ok(Err(matrix_sdk::Error::Http(_))) | Err(_) => Err(FetchError::Transient),
            Ok(Err(_)) => Err(FetchError::Permanent),
        }
    }
}

async fn fetch_with_retries<F: AttachmentFetcher>(
    fetcher: &F,
    source: &MediaSource,
    stop: &StopFlag,
) -> Result<Result<Vec<u8>, FetchError>, ArchiveStepError> {
    let mut attempt = 0;
    loop {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(ArchiveStepError::Stopped),
            result = fetcher.fetch(source) => result,
        };
        attempt += 1;
        match result {
            Err(FetchError::Transient) if attempt < FETCH_ATTEMPTS => {
                let delay = RETRY_DELAYS[(attempt - 1) as usize];
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(ArchiveStepError::Stopped),
                    _ = executor::sleep(delay) => {}
                }
            }
            result => return Ok(result),
        }
    }
}

/// Download every attachment into `room_dir`. `on_progress` receives
/// `(done, total, failed)` after each attachment. A failed download is
/// recorded and skipped; a write failure or a stop ends the step.
pub(crate) async fn download_attachments<F: AttachmentFetcher>(
    fetcher: &F,
    fs: &dyn HistoryExportFilesystem,
    room_dir: &Path,
    refs: Vec<AttachmentRef>,
    stop: &StopFlag,
    mut on_progress: impl FnMut(u64, u64, u64),
) -> Result<AttachmentIndex, ArchiveStepError> {
    let total = refs.len() as u64;
    let mut index = AttachmentIndex::default();
    let mut failed = 0;
    if !refs.is_empty() {
        fs.create_dir_all(&room_dir.join("files"))?;
        fs.create_dir_all(&room_dir.join("thumbs"))?;
    }
    for (position, attachment) in refs.into_iter().enumerate() {
        if stop.is_set() {
            return Err(ArchiveStepError::Stopped);
        }
        let sequence = u32::try_from(position + 1).unwrap_or(u32::MAX);
        let mut record = AttachmentRecord {
            event_id: attachment.event_id,
            kind: attachment.kind,
            name: attachment.name,
            size: attachment.size,
            mimetype: attachment.mimetype,
            file: None,
            thumb: None,
            status: AttachmentStatus::Failed,
        };
        match fetch_with_retries(fetcher, &attachment.source, stop).await? {
            Ok(bytes) => {
                let file = format!(
                    "files/{}",
                    attachment_file_name(sequence, Some(&record.name), record.mimetype.as_deref())
                );
                fs.write_atomic(&room_dir.join(&file), &bytes)?;
                if matches!(record.kind, AttachmentKind::Image | AttachmentKind::Sticker)
                    && let Some(thumb_bytes) = thumbnail_jpeg(&bytes, THUMB_MAX_EDGE)
                {
                    let thumb = format!("thumbs/{}", thumbnail_file_name(sequence));
                    fs.write_atomic(&room_dir.join(&thumb), &thumb_bytes)?;
                    record.thumb = Some(thumb);
                }
                record.size = Some(bytes.len() as u64);
                record.file = Some(file);
                record.status = AttachmentStatus::Retrieved;
            }
            Err(_) => failed += 1,
        }
        index.attachments.push(record);
        on_progress(position as u64 + 1, total, failed);
    }
    Ok(index)
}
