//! Filesystem port for history export folders.
//!
//! Core owns the export layout and ordering; the port only performs
//! operations. Errors are classified without paths or OS text, so nothing
//! private crosses this boundary.

use std::fmt;
use std::io::{ErrorKind, Write as _};
use std::path::Path;

/// Private-safe classification of a filesystem failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HistoryExportFsError {
    #[error("export path was not found")]
    NotFound,
    #[error("export disk is full")]
    NoSpace,
    #[error("export path permission was denied")]
    PermissionDenied,
    #[error("export filesystem operation failed")]
    Io,
}

impl From<std::io::Error> for HistoryExportFsError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            ErrorKind::NotFound => Self::NotFound,
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => Self::NoSpace,
            ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => Self::PermissionDenied,
            _ => Self::Io,
        }
    }
}

/// A file whose bytes reach its destination only on [`StagedFile::commit`].
/// Dropping it without committing removes the staged bytes.
pub trait StagedFile: Send {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), HistoryExportFsError>;
    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError>;
}

pub trait HistoryExportFilesystem: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError>;
    fn read(&self, path: &Path) -> Result<Vec<u8>, HistoryExportFsError>;
    fn create_file(&self, path: &Path) -> Result<Box<dyn StagedFile>, HistoryExportFsError>;
    /// Write through a temporary file, fsync it, and rename it over `path`.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), HistoryExportFsError>;
    fn rename(&self, from: &Path, to: &Path) -> Result<(), HistoryExportFsError>;
    fn remove_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError>;
    /// Entry names directly inside `path`.
    fn list_dir(&self, path: &Path) -> Result<Vec<String>, HistoryExportFsError>;
}

#[derive(Default)]
pub struct NativeHistoryExportFilesystem;

impl fmt::Debug for NativeHistoryExportFilesystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeHistoryExportFilesystem")
    }
}

struct NativeStagedFile {
    staged: std::io::BufWriter<tempfile::NamedTempFile>,
    destination: std::path::PathBuf,
}

fn staging_file(path: &Path) -> Result<tempfile::NamedTempFile, HistoryExportFsError> {
    let parent = path.parent().ok_or(HistoryExportFsError::Io)?;
    Ok(tempfile::Builder::new()
        .prefix(".koushi-export-")
        .suffix(".partial")
        .tempfile_in(parent)?)
}

fn persist(
    staged: tempfile::NamedTempFile,
    destination: &Path,
) -> Result<(), HistoryExportFsError> {
    staged.as_file().sync_all()?;
    staged
        .persist(destination)
        .map(|_| ())
        .map_err(|error| error.error.into())
}

impl StagedFile for NativeStagedFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        Ok(self.staged.write_all(bytes)?)
    }

    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError> {
        let Self {
            staged,
            destination,
        } = *self;
        let staged = staged.into_inner().map_err(|error| error.into_error())?;
        persist(staged, &destination)
    }
}

impl HistoryExportFilesystem for NativeHistoryExportFilesystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError> {
        Ok(std::fs::create_dir_all(path)?)
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, HistoryExportFsError> {
        Ok(std::fs::read(path)?)
    }

    fn create_file(&self, path: &Path) -> Result<Box<dyn StagedFile>, HistoryExportFsError> {
        Ok(Box::new(NativeStagedFile {
            staged: std::io::BufWriter::new(staging_file(path)?),
            destination: path.to_path_buf(),
        }))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        let mut staged = staging_file(path)?;
        staged.write_all(bytes)?;
        persist(staged, path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HistoryExportFsError> {
        Ok(std::fs::rename(from, to)?)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError> {
        match std::fs::remove_dir_all(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            result => Ok(result?),
        }
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<String>, HistoryExportFsError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        Ok(names)
    }
}
