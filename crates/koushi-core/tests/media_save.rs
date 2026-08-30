use std::path::{Path, PathBuf};
use std::sync::Mutex;

use koushi_core::{
    MediaSaveError, MediaSaveFilesystem, MediaSaveIoError, default_media_save_path,
    safe_media_save_filename, save_downloaded_media,
};

struct FakeFilesystem {
    canonical: Mutex<Vec<Result<PathBuf, MediaSaveIoError>>>,
    operations: Mutex<Vec<&'static str>>,
    parent_result: Result<(), MediaSaveIoError>,
    copy_result: Result<(), MediaSaveIoError>,
}

impl Default for FakeFilesystem {
    fn default() -> Self {
        Self {
            canonical: Mutex::new(Vec::new()),
            operations: Mutex::new(Vec::new()),
            parent_result: Ok(()),
            copy_result: Ok(()),
        }
    }
}

impl FakeFilesystem {
    fn with_canonical(canonical: Vec<Result<PathBuf, MediaSaveIoError>>) -> Self {
        Self {
            canonical: Mutex::new(canonical),
            parent_result: Ok(()),
            copy_result: Ok(()),
            ..Default::default()
        }
    }

    fn with_failures(
        canonical: Vec<Result<PathBuf, MediaSaveIoError>>,
        parent_result: Result<(), MediaSaveIoError>,
        copy_result: Result<(), MediaSaveIoError>,
    ) -> Self {
        Self {
            canonical: Mutex::new(canonical),
            parent_result,
            copy_result,
            ..Default::default()
        }
    }

    fn operations(&self) -> Vec<&'static str> {
        self.operations.lock().unwrap().clone()
    }
}

impl MediaSaveFilesystem for FakeFilesystem {
    fn canonicalize(&self, _: &Path) -> Result<PathBuf, MediaSaveIoError> {
        self.operations.lock().unwrap().push("canonicalize");
        self.canonical.lock().unwrap().remove(0)
    }

    fn create_dir_all(&self, _: &Path) -> Result<(), MediaSaveIoError> {
        self.operations.lock().unwrap().push("create_dir_all");
        self.parent_result
    }

    fn copy(&self, _: &Path, _: &Path) -> Result<(), MediaSaveIoError> {
        self.operations.lock().unwrap().push("copy");
        self.copy_result
    }
}

#[test]
fn rejects_empty_relative_and_url_sources_before_io() {
    let fake = FakeFilesystem::with_canonical(Vec::new());

    assert_eq!(
        save_downloaded_media(&fake, Path::new("/cache"), Path::new(""), Path::new("/out")),
        Err(MediaSaveError::SourceEmpty)
    );
    assert_eq!(
        save_downloaded_media(
            &fake,
            Path::new("/cache"),
            Path::new("cache/file"),
            Path::new("/out"),
        ),
        Err(MediaSaveError::SourceRelative)
    );
    assert_eq!(
        save_downloaded_media(
            &fake,
            Path::new("/cache"),
            Path::new("https://example.invalid/file"),
            Path::new("/out"),
        ),
        Err(MediaSaveError::SourceUrl)
    );
    assert!(fake.operations().is_empty());
}

#[test]
fn rejects_empty_and_relative_destinations_before_io() {
    let fake = FakeFilesystem::with_canonical(Vec::new());

    assert_eq!(
        save_downloaded_media(
            &fake,
            Path::new("/cache"),
            Path::new("/cache/file"),
            Path::new(""),
        ),
        Err(MediaSaveError::DestinationEmpty)
    );
    assert_eq!(
        save_downloaded_media(
            &fake,
            Path::new("/cache"),
            Path::new("/cache/file"),
            Path::new("out/file"),
        ),
        Err(MediaSaveError::DestinationRelative)
    );
    assert!(fake.operations().is_empty());
}

#[test]
fn classifies_missing_cache_and_source_failures_without_paths() {
    let missing_cache =
        FakeFilesystem::with_failures(vec![Err(MediaSaveIoError::NotFound)], Ok(()), Ok(()));
    let error = save_downloaded_media(
        &missing_cache,
        Path::new("/private/cache"),
        Path::new("/private/cache/file"),
        Path::new("/private/out/file"),
    )
    .unwrap_err();
    assert_eq!(
        error,
        MediaSaveError::CacheCanonicalize(MediaSaveIoError::NotFound)
    );
    assert!(!format!("{error:?}").contains("private"));
    assert!(!error.to_string().contains("private"));

    let source_failure = FakeFilesystem::with_failures(
        vec![
            Ok(PathBuf::from("/cache")),
            Err(MediaSaveIoError::PermissionDenied),
        ],
        Ok(()),
        Ok(()),
    );
    assert_eq!(
        save_downloaded_media(
            &source_failure,
            Path::new("/private/cache"),
            Path::new("/private/cache/file"),
            Path::new("/private/out/file"),
        ),
        Err(MediaSaveError::SourceCanonicalize(
            MediaSaveIoError::PermissionDenied
        ))
    );
}

#[test]
fn rejects_outside_sibling_and_symlink_escape_after_canonicalization() {
    let sibling = FakeFilesystem::with_canonical(vec![
        Ok(PathBuf::from("/cache/root")),
        Ok(PathBuf::from("/cache/root-sibling/file")),
    ]);
    assert_eq!(
        save_downloaded_media(
            &sibling,
            Path::new("/private/cache"),
            Path::new("/private/cache/root-sibling/file"),
            Path::new("/out/file"),
        ),
        Err(MediaSaveError::SourceOutsideCache)
    );

    let symlink = FakeFilesystem::with_canonical(vec![
        Ok(PathBuf::from("/cache/root")),
        Ok(PathBuf::from("/outside/file")),
    ]);
    assert_eq!(
        save_downloaded_media(
            &symlink,
            Path::new("/private/cache"),
            Path::new("/private/cache/link/file"),
            Path::new("/out/file"),
        ),
        Err(MediaSaveError::SourceOutsideCache)
    );
}

#[test]
fn creates_selected_parent_then_copies_successfully() {
    let fake = FakeFilesystem::with_canonical(vec![
        Ok(PathBuf::from("/cache/root")),
        Ok(PathBuf::from("/cache/root/file")),
    ]);

    save_downloaded_media(
        &fake,
        Path::new("/private/cache"),
        Path::new("/private/cache/file"),
        Path::new("/private/selected/nested/file"),
    )
    .unwrap();
    assert_eq!(
        fake.operations(),
        vec!["canonicalize", "canonicalize", "create_dir_all", "copy"]
    );
}

#[test]
fn classifies_parent_and_copy_failures() {
    let parent_failure = FakeFilesystem::with_failures(
        vec![
            Ok(PathBuf::from("/cache")),
            Ok(PathBuf::from("/cache/file")),
        ],
        Err(MediaSaveIoError::PermissionDenied),
        Ok(()),
    );
    assert_eq!(
        save_downloaded_media(
            &parent_failure,
            Path::new("/cache"),
            Path::new("/cache/file"),
            Path::new("/out/nested/file"),
        ),
        Err(MediaSaveError::DestinationParent(
            MediaSaveIoError::PermissionDenied
        ))
    );

    let copy_failure = FakeFilesystem::with_failures(
        vec![
            Ok(PathBuf::from("/cache")),
            Ok(PathBuf::from("/cache/file")),
        ],
        Ok(()),
        Err(MediaSaveIoError::Other),
    );
    assert_eq!(
        save_downloaded_media(
            &copy_failure,
            Path::new("/cache"),
            Path::new("/cache/file"),
            Path::new("/out/file"),
        ),
        Err(MediaSaveError::Copy(MediaSaveIoError::Other))
    );
}

#[test]
fn filename_policy_replaces_forbidden_characters_and_falls_back() {
    assert_eq!(
        safe_media_save_filename(r#" report:name?/\\*\\\"<>|.png "#),
        "report_name____________.png"
    );
    assert_eq!(safe_media_save_filename("   "), "download");
    assert_eq!(
        default_media_save_path(" report:name?.png ", Some(Path::new("/downloads"))),
        PathBuf::from("/downloads/report_name_.png")
    );
    assert_eq!(
        default_media_save_path("   ", None),
        PathBuf::from("download")
    );
}

#[test]
fn real_filesystem_port_copies_bytes_after_admission() {
    use std::fs;

    struct RealFilesystem;
    impl MediaSaveFilesystem for RealFilesystem {
        fn canonicalize(&self, path: &Path) -> Result<PathBuf, MediaSaveIoError> {
            fs::canonicalize(path).map_err(|_| MediaSaveIoError::Other)
        }

        fn create_dir_all(&self, path: &Path) -> Result<(), MediaSaveIoError> {
            fs::create_dir_all(path).map_err(|_| MediaSaveIoError::Other)
        }

        fn copy(&self, source: &Path, destination: &Path) -> Result<(), MediaSaveIoError> {
            fs::copy(source, destination)
                .map(|_| ())
                .map_err(|_| MediaSaveIoError::Other)
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let source = cache.join("file");
    let destination = temp.path().join("selected/nested/file");
    fs::create_dir_all(&cache).unwrap();
    fs::write(&source, b"synthetic media").unwrap();

    save_downloaded_media(&RealFilesystem, &cache, &source, &destination).unwrap();
    assert_eq!(fs::read(destination).unwrap(), b"synthetic media");
}

#[cfg(unix)]
#[test]
fn real_symlink_escape_is_rejected_by_canonical_results() {
    use std::fs;

    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("file"), b"private").unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("link")).unwrap();

    struct RealCanonical;
    impl MediaSaveFilesystem for RealCanonical {
        fn canonicalize(&self, path: &Path) -> Result<PathBuf, MediaSaveIoError> {
            fs::canonicalize(path).map_err(|_| MediaSaveIoError::Other)
        }

        fn create_dir_all(&self, _: &Path) -> Result<(), MediaSaveIoError> {
            Ok(())
        }

        fn copy(&self, _: &Path, _: &Path) -> Result<(), MediaSaveIoError> {
            panic!("copy must not be admitted for an escaped source")
        }
    }

    assert_eq!(
        save_downloaded_media(
            &RealCanonical,
            &cache,
            &cache.join("link/file"),
            &temp.path().join("destination"),
        ),
        Err(MediaSaveError::SourceOutsideCache)
    );
}
