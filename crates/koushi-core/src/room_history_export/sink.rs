//! Destination port for room-history exports.
//!
//! An export is written to a staging file and only replaces the destination
//! when the whole history has been written. A staging file that is dropped
//! without [`RoomHistoryExportFile::commit`] is removed, so a cancelled,
//! failed, or torn-down export never leaves a file that looks complete.

use std::fmt;
use std::path::Path;

/// Private-safe classification of a destination failure. No path or OS error
/// text crosses this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RoomHistoryExportSinkError {
    #[error("export destination is not an absolute file path")]
    InvalidDestination,
    #[error("export destination could not be created")]
    Create,
    #[error("export destination could not be written")]
    Write,
    #[error("export destination could not be finalized")]
    Commit,
}

/// One in-progress export file.
pub trait RoomHistoryExportFile: Send {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), RoomHistoryExportSinkError>;
    /// Flush and atomically move the staged bytes to the destination.
    fn commit(self: Box<Self>) -> Result<(), RoomHistoryExportSinkError>;
}

/// Opens export files. Implementations must discard uncommitted bytes when
/// the returned file is dropped.
pub trait RoomHistoryExportSink: Send + Sync {
    fn create(
        &self,
        destination: &Path,
    ) -> Result<Box<dyn RoomHistoryExportFile>, RoomHistoryExportSinkError>;
}

/// Native filesystem sink: a hidden staging file next to the destination,
/// renamed over it on commit.
#[derive(Default)]
pub struct NativeRoomHistoryExportSink;

impl fmt::Debug for NativeRoomHistoryExportSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeRoomHistoryExportSink")
    }
}

struct NativeRoomHistoryExportFile {
    staged: std::io::BufWriter<tempfile::NamedTempFile>,
    destination: std::path::PathBuf,
}

impl RoomHistoryExportSink for NativeRoomHistoryExportSink {
    fn create(
        &self,
        destination: &Path,
    ) -> Result<Box<dyn RoomHistoryExportFile>, RoomHistoryExportSinkError> {
        if !destination.is_absolute() || destination.file_name().is_none() {
            return Err(RoomHistoryExportSinkError::InvalidDestination);
        }
        let parent = destination
            .parent()
            .ok_or(RoomHistoryExportSinkError::InvalidDestination)?;
        let staged = tempfile::Builder::new()
            .prefix(".koushi-export-")
            .suffix(".partial")
            .tempfile_in(parent)
            .map_err(|_| RoomHistoryExportSinkError::Create)?;
        Ok(Box::new(NativeRoomHistoryExportFile {
            staged: std::io::BufWriter::new(staged),
            destination: destination.to_path_buf(),
        }))
    }
}

impl RoomHistoryExportFile for NativeRoomHistoryExportFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), RoomHistoryExportSinkError> {
        std::io::Write::write_all(&mut self.staged, bytes)
            .map_err(|_| RoomHistoryExportSinkError::Write)
    }

    fn commit(self: Box<Self>) -> Result<(), RoomHistoryExportSinkError> {
        let Self {
            staged,
            destination,
        } = *self;
        let staged = staged
            .into_inner()
            .map_err(|_| RoomHistoryExportSinkError::Write)?;
        staged
            .as_file()
            .sync_all()
            .map_err(|_| RoomHistoryExportSinkError::Commit)?;
        // `persist` replaces an existing destination (the save dialog already
        // confirmed the overwrite) and leaves the staged file for cleanup on
        // failure, which the returned error drops.
        staged
            .persist(&destination)
            .map(|_| ())
            .map_err(|_| RoomHistoryExportSinkError::Commit)
    }
}
