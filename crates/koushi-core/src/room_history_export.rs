//! History export: a room, or every joined non-DM room of a Space, written to
//! an archive folder (HTML pages, Element-compatible `messages.json`, the
//! lossless `events.jsonl`, and attachments).
//!
//! Core owns paging, range inclusion, decryption classification, event
//! selection, room selection, resume, attachment download, and page
//! rendering. The directory is a platform-registered native artifact and
//! bytes reach it only through [`HistoryExportFilesystem`].

pub(crate) mod archive;
pub(crate) mod attachments;
mod driver;
pub(crate) mod fs;
pub(crate) mod html;
#[cfg(test)]
pub(crate) mod fs_fake;
mod element;
mod katex_assets;
pub(crate) mod layout;
pub(crate) mod manifest;
pub(crate) mod records;
pub(crate) mod space_selection;
mod sdk_source;

#[cfg(test)]
mod archive_tests;
#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod fs_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod manifest_tests;
#[cfg(test)]
mod sdk_source_tests;
#[cfg(test)]
mod space_selection_tests;
#[cfg(test)]
mod tests;

pub(crate) use archive::{ArchiveOutcome, ArchiveReporter, ArchiveRequest, run_archive};
pub(crate) use attachments::{SdkAttachmentFetcher, StopFlag};
pub use fs::{HistoryExportFilesystem, HistoryExportFsError, NativeHistoryExportFilesystem};
pub(crate) use manifest::ManifestScope;
pub(crate) use sdk_source::SdkArchiveSource;
