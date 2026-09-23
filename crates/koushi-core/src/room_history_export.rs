//! Room-history export (#59): Element-compatible chat-export JSON.
//!
//! Core owns paging, range inclusion, decryption classification, event
//! selection, and JSON generation. The destination is a platform-registered
//! native artifact and bytes reach it only through [`RoomHistoryExportSink`].

mod driver;
mod element;
mod sdk_source;
mod sink;

#[cfg(test)]
mod sdk_source_tests;
#[cfg(test)]
mod tests;

pub(crate) use driver::{AsyncProgress, ExportCounters, run_export};
pub(crate) use sdk_source::{MatrixRoomHistorySource, room_export_header};
pub use sink::{
    NativeRoomHistoryExportSink, RoomHistoryExportFile, RoomHistoryExportSink,
    RoomHistoryExportSinkError,
};
