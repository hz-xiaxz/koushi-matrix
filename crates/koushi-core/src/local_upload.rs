//! Local-file uploads for out-of-band senders such as the MCP bridge (#1533).
//!
//! The caller names absolute local paths; Core checks them, reads them off the
//! async runtime, and runs the same media preparation the composer's staged
//! uploads use, in a detached registry. The result is one
//! [`UploadMediaRequest`] per file for `TimelineCommand::UploadAndSendMedia`.
//! No composer target, staged-upload state, or `AppAction` is touched, so the
//! user's composer never shows an assistant's files.
//!
//! Paths, file names, and bytes are user data: failures carry only a closed
//! kind, and nothing here logs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use koushi_protocol::command::{UploadMediaKind, UploadMediaRequest};
use koushi_state::{
    ComposerDocument, ComposerFormattingOptions, ComposerTarget, ImageUploadCompressionPolicy,
    StagedUploadPreparation,
};

use crate::media_preparation::{MediaPreparationRegistry, StageUploadBytesInput};
use crate::native_image_decoder::NativeStillImageDecoder;

/// Most files one call may send.
pub const MAX_LOCAL_UPLOAD_FILES: usize = 10;
/// Largest single file, in bytes.
pub const MAX_LOCAL_UPLOAD_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// Largest total of one call, in bytes.
pub const MAX_LOCAL_UPLOAD_TOTAL_BYTES: u64 = 100 * 1024 * 1024;

/// Why a set of local files cannot be sent. Carries no path or name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalUploadFailure {
    #[error("no files were named")]
    NoFiles,
    #[error("too many files")]
    TooManyFiles,
    #[error("a path is not absolute")]
    NotAbsolute,
    #[error("a path is not a regular file")]
    NotARegularFile,
    #[error("a file could not be read")]
    Unreadable,
    #[error("a file or the batch exceeds the size limit")]
    TooLarge,
    #[error("a file is empty")]
    Empty,
    #[error("a file could not be prepared for upload")]
    PreparationFailed,
}

/// Cheap checks that need no file contents: count, absolute paths, regular
/// files, and size limits from metadata. Run before asking for consent, so a
/// call that can never succeed does not raise an approval.
pub fn check_local_paths(paths: &[PathBuf]) -> Result<(), LocalUploadFailure> {
    if paths.is_empty() {
        return Err(LocalUploadFailure::NoFiles);
    }
    if paths.len() > MAX_LOCAL_UPLOAD_FILES {
        return Err(LocalUploadFailure::TooManyFiles);
    }
    let mut total = 0_u64;
    for path in paths {
        let len = regular_file_len(path)?;
        if len > MAX_LOCAL_UPLOAD_FILE_BYTES {
            return Err(LocalUploadFailure::TooLarge);
        }
        total = total.saturating_add(len);
        if total > MAX_LOCAL_UPLOAD_TOTAL_BYTES {
            return Err(LocalUploadFailure::TooLarge);
        }
    }
    Ok(())
}

/// Reads and prepares every file, or none. `caption` goes on the first file
/// only. The bytes uploaded are the untouched source (the preparation's
/// default output), as an Original selection in the composer would send.
pub async fn prepare_local_uploads(
    paths: Vec<PathBuf>,
    caption: Option<ComposerDocument>,
    formatting_options: ComposerFormattingOptions,
    policy: ImageUploadCompressionPolicy,
    native_image_decoder: Option<Arc<dyn NativeStillImageDecoder>>,
) -> Result<Vec<UploadMediaRequest>, LocalUploadFailure> {
    check_local_paths(&paths)?;
    crate::executor::spawn_blocking(move || {
        prepare_blocking(
            &paths,
            caption.as_ref(),
            formatting_options,
            policy,
            native_image_decoder,
        )
    })
    .await
    .map_err(|_| LocalUploadFailure::PreparationFailed)?
}

fn prepare_blocking(
    paths: &[PathBuf],
    caption: Option<&ComposerDocument>,
    formatting_options: ComposerFormattingOptions,
    policy: ImageUploadCompressionPolicy,
    native_image_decoder: Option<Arc<dyn NativeStillImageDecoder>>,
) -> Result<Vec<UploadMediaRequest>, LocalUploadFailure> {
    let mut inputs = Vec::with_capacity(paths.len());
    let mut total = 0_u64;
    for (index, path) in paths.iter().enumerate() {
        let bytes = read_within_limit(path)?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_LOCAL_UPLOAD_TOTAL_BYTES {
            return Err(LocalUploadFailure::TooLarge);
        }
        if bytes.is_empty() {
            return Err(LocalUploadFailure::Empty);
        }
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("attachment")
            .to_owned();
        let mime_type = sniff_mime_type(&bytes, &filename).to_owned();
        inputs.push(StageUploadBytesInput {
            staged_id: format!("local-{index}"),
            position: index as u64 + 1,
            filename,
            mime_type,
            bytes,
        });
    }

    // A detached registry keyed by a target no composer ever uses: nothing is
    // merged into the shared preparation service or published to state.
    let target = ComposerTarget::Main {
        room_id: String::new(),
    };
    let mut registry = MediaPreparationRegistry::with_native_image_decoder(native_image_decoder);
    let items = registry.prepare_items(&target, inputs, policy);
    let caption = caption
        .map(|document| (document, document.plain_body()))
        .filter(|(_, plain_body)| !plain_body.trim().is_empty())
        .map(
            |(document, plain_body)| koushi_state::FormattedMessageDraft {
                plain_body,
                formatted_body: document.formatted_body_with_options(formatting_options),
                mentions: document.mention_intent(),
            },
        );

    let mut requests = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if !matches!(item.preparation, StagedUploadPreparation::Ready { .. }) {
            return Err(LocalUploadFailure::PreparationFailed);
        }
        let prepared = registry
            .selected_upload(&target, &item.staged_id)
            .ok_or(LocalUploadFailure::PreparationFailed)?;
        let descriptor = prepared.descriptor;
        let kind = if descriptor.mime_type.starts_with("image/") {
            UploadMediaKind::Image {
                width: descriptor.width,
                height: descriptor.height,
            }
        } else {
            UploadMediaKind::File
        };
        requests.push(UploadMediaRequest {
            filename: descriptor.filename,
            mime_type: descriptor.mime_type,
            bytes: prepared.bytes,
            kind,
            compression: None,
            thumbnail: None,
            caption: if index == 0 { caption.clone() } else { None },
        });
    }
    Ok(requests)
}

fn regular_file_len(path: &Path) -> Result<u64, LocalUploadFailure> {
    if !path.is_absolute() {
        return Err(LocalUploadFailure::NotAbsolute);
    }
    let metadata = std::fs::metadata(path).map_err(|_| LocalUploadFailure::Unreadable)?;
    if !metadata.is_file() {
        return Err(LocalUploadFailure::NotARegularFile);
    }
    Ok(metadata.len())
}

/// Reads at most one byte past the limit, so a file that grew after the
/// metadata check is refused instead of read whole.
fn read_within_limit(path: &Path) -> Result<Vec<u8>, LocalUploadFailure> {
    regular_file_len(path)?;
    let file = std::fs::File::open(path).map_err(|_| LocalUploadFailure::Unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_LOCAL_UPLOAD_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LocalUploadFailure::Unreadable)?;
    if bytes.len() as u64 > MAX_LOCAL_UPLOAD_FILE_BYTES {
        return Err(LocalUploadFailure::TooLarge);
    }
    Ok(bytes)
}

/// The file's type from its leading bytes, then its extension. HEIF is
/// detected by media preparation itself.
fn sniff_mime_type(bytes: &[u8], filename: &str) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    let extension = Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}
