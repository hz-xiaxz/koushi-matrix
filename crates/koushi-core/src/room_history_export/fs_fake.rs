//! In-memory [`HistoryExportFilesystem`] for tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::fs::{HistoryExportFilesystem, HistoryExportFsError, StagedFile};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Entry {
    Dir,
    File(Vec<u8>),
}

#[derive(Default)]
struct Inner {
    entries: BTreeMap<PathBuf, Entry>,
    write_failure: Option<HistoryExportFsError>,
    /// Number of file writes, used to inject a failure after N writes.
    writes: u64,
    fail_after_writes: Option<(u64, HistoryExportFsError)>,
    /// Writes whose path contains this text fail with this error.
    fail_paths_containing: Option<(String, HistoryExportFsError)>,
}

#[derive(Clone, Default)]
pub(crate) struct MemoryFilesystem {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryFilesystem {
    /// Make every later file write fail with `failure`.
    pub(crate) fn fail_writes_with(&self, failure: Option<HistoryExportFsError>) {
        self.inner.lock().unwrap().write_failure = failure;
    }

    /// Let `count` more file writes succeed, then fail with `failure`.
    pub(crate) fn fail_after_writes(&self, count: u64, failure: HistoryExportFsError) {
        let mut inner = self.inner.lock().unwrap();
        let writes = inner.writes;
        inner.fail_after_writes = Some((writes + count, failure));
    }

    /// Make writes of paths containing `needle` fail with `failure`.
    pub(crate) fn fail_paths_containing(&self, needle: &str, failure: HistoryExportFsError) {
        self.inner.lock().unwrap().fail_paths_containing = Some((needle.to_owned(), failure));
    }

    /// Every file path below `root`, relative to it, sorted.
    pub(crate) fn files_below(&self, root: &Path) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .entries
            .iter()
            .filter(|(path, entry)| matches!(entry, Entry::File(_)) && path.starts_with(root))
            .map(|(path, _)| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn check_write(inner: &mut Inner) -> Result<(), HistoryExportFsError> {
        if let Some(failure) = inner.write_failure {
            return Err(failure);
        }
        if let Some((limit, failure)) = inner.fail_after_writes
            && inner.writes >= limit
        {
            return Err(failure);
        }
        inner.writes += 1;
        Ok(())
    }

    fn put_file(&self, path: &Path, bytes: Vec<u8>) -> Result<(), HistoryExportFsError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some((needle, failure)) = &inner.fail_paths_containing
            && path.to_string_lossy().contains(needle.as_str())
        {
            return Err(*failure);
        }
        Self::check_write(&mut inner)?;
        let parent_is_dir = path
            .parent()
            .is_some_and(|parent| inner.entries.get(parent) == Some(&Entry::Dir));
        if !parent_is_dir {
            return Err(HistoryExportFsError::NotFound);
        }
        inner.entries.insert(path.to_path_buf(), Entry::File(bytes));
        Ok(())
    }
}

struct MemoryStagedFile {
    fs: MemoryFilesystem,
    path: PathBuf,
    bytes: Vec<u8>,
}

impl StagedFile for MemoryStagedFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError> {
        let Self { fs, path, bytes } = *self;
        fs.put_file(&path, bytes)
    }
}

impl HistoryExportFilesystem for MemoryFilesystem {
    fn exists(&self, path: &Path) -> bool {
        self.inner.lock().unwrap().entries.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.inner.lock().unwrap().entries.get(path) == Some(&Entry::Dir)
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError> {
        let mut inner = self.inner.lock().unwrap();
        for ancestor in path.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            if let Some(Entry::File(_)) = inner.entries.get(ancestor) {
                return Err(HistoryExportFsError::Io);
            }
            inner.entries.insert(ancestor.to_path_buf(), Entry::Dir);
        }
        Ok(())
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, HistoryExportFsError> {
        match self.inner.lock().unwrap().entries.get(path) {
            Some(Entry::File(bytes)) => Ok(bytes.clone()),
            Some(Entry::Dir) => Err(HistoryExportFsError::Io),
            None => Err(HistoryExportFsError::NotFound),
        }
    }

    fn create_file(&self, path: &Path) -> Result<Box<dyn StagedFile>, HistoryExportFsError> {
        Ok(Box::new(MemoryStagedFile {
            fs: self.clone(),
            path: path.to_path_buf(),
            bytes: Vec::new(),
        }))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        self.put_file(path, bytes.to_vec())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HistoryExportFsError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.entries.contains_key(from) {
            return Err(HistoryExportFsError::NotFound);
        }
        let moved: Vec<(PathBuf, Entry)> = inner
            .entries
            .iter()
            .filter(|(path, _)| path.starts_with(from))
            .map(|(path, entry)| (path.clone(), entry.clone()))
            .collect();
        for (path, entry) in moved {
            inner.entries.remove(&path);
            let relative = path.strip_prefix(from).unwrap();
            let target = if relative.as_os_str().is_empty() {
                to.to_path_buf()
            } else {
                to.join(relative)
            };
            inner.entries.insert(target, entry);
        }
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError> {
        self.inner
            .lock()
            .unwrap()
            .entries
            .retain(|entry_path, _| !entry_path.starts_with(path));
        Ok(())
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<String>, HistoryExportFsError> {
        let inner = self.inner.lock().unwrap();
        if inner.entries.get(path) != Some(&Entry::Dir) {
            return Err(HistoryExportFsError::NotFound);
        }
        Ok(inner
            .entries
            .keys()
            .filter(|entry_path| entry_path.parent() == Some(path))
            .filter_map(|entry_path| entry_path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect())
    }
}
