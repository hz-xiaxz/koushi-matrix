use std::{fs, io, io::Write, path::Path};

pub(crate) fn atomic_replace_file(
    path: &Path,
    payload: &[u8],
    fail_before_persist: bool,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "replacement path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(payload)?;
    temporary.as_file().sync_all()?;
    if fail_before_persist {
        return Err(io::Error::other("atomic replacement failed before persist"));
    }
    temporary.persist(path).map_err(|error| error.error)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}
