//! Room-history export (#59): Element-compatible chat-export JSON.
//!
//! Core owns paging, range inclusion, decryption classification, event
//! selection, and JSON generation. The destination is a platform-registered
//! native artifact and bytes reach it only through [`RoomHistoryExportSink`].

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
pub(crate) mod thumbnail;
mod sdk_source;
mod sink;

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
mod thumbnail_tests;
#[cfg(test)]
mod tests;

pub(crate) use driver::{AsyncProgress, ExportCounters, run_export};
pub(crate) use sdk_source::{MatrixRoomHistorySource, room_export_header};
pub use sink::{
    NativeRoomHistoryExportSink, RoomHistoryExportFile, RoomHistoryExportSink,
    RoomHistoryExportSinkError,
};
