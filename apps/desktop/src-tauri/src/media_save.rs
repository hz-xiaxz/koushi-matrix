use std::path::{Path, PathBuf};

use koushi_core::{MediaSaveFilesystem, MediaSaveIoError};

/// Native filesystem port for Core's media-save policy.
///
/// This adapter performs syscalls only; Core decides which operations are
/// admitted and in what order.
pub(crate) struct NativeMediaSaveFilesystem;

impl MediaSaveFilesystem for NativeMediaSaveFilesystem {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, MediaSaveIoError> {
        std::fs::canonicalize(path).map_err(classify_io_error)
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), MediaSaveIoError> {
        std::fs::create_dir_all(path).map_err(classify_io_error)
    }

    fn copy(&self, source: &Path, destination: &Path) -> Result<(), MediaSaveIoError> {
        std::fs::copy(source, destination)
            .map(|_| ())
            .map_err(classify_io_error)
    }
}

fn classify_io_error(error: std::io::Error) -> MediaSaveIoError {
    match error.kind() {
        std::io::ErrorKind::NotFound => MediaSaveIoError::NotFound,
        std::io::ErrorKind::PermissionDenied => MediaSaveIoError::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => MediaSaveIoError::AlreadyExists,
        std::io::ErrorKind::InvalidInput => MediaSaveIoError::InvalidInput,
        _ => MediaSaveIoError::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::classify_io_error;
    use koushi_core::MediaSaveIoError;

    #[test]
    fn native_io_mapping_is_private_safe_and_classified() {
        assert_eq!(
            classify_io_error(std::io::Error::from(std::io::ErrorKind::NotFound)),
            MediaSaveIoError::NotFound
        );
        assert_eq!(
            classify_io_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            MediaSaveIoError::PermissionDenied
        );
        assert_eq!(
            classify_io_error(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
            MediaSaveIoError::AlreadyExists
        );
        assert_eq!(
            classify_io_error(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
            MediaSaveIoError::InvalidInput
        );
        let error = classify_io_error(std::io::Error::new(
            std::io::ErrorKind::Other,
            "/private/path should not escape",
        ));
        assert_eq!(error, MediaSaveIoError::Other);
        assert!(!format!("{error:?}").contains("private"));
        assert!(!error.to_string().contains("private"));
    }
}
