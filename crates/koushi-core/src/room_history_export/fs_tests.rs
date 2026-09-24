//! Port contract, run against both the native filesystem and the fake.

use std::path::Path;

use super::fs::*;
use super::fs_fake::MemoryFilesystem;

fn contract(fs: &dyn HistoryExportFilesystem, root: &Path) {
    let dir = root.join("export");
    fs.create_dir_all(&dir.join("rooms/a")).unwrap();
    assert!(fs.is_dir(&dir.join("rooms")));

    let file = dir.join("rooms/a/messages.json");
    let mut staged = fs.create_file(&file).unwrap();
    staged.write_all(b"{}").unwrap();
    assert!(!fs.exists(&file), "staged bytes are invisible until commit");
    staged.commit().unwrap();
    assert_eq!(fs.read(&file).unwrap(), b"{}");

    let dropped = dir.join("rooms/a/dropped.json");
    let mut staged = fs.create_file(&dropped).unwrap();
    staged.write_all(b"x").unwrap();
    drop(staged);
    assert!(!fs.exists(&dropped));

    let manifest = dir.join("koushi-export.json");
    fs.write_atomic(&manifest, b"one").unwrap();
    fs.write_atomic(&manifest, b"two").unwrap();
    assert_eq!(fs.read(&manifest).unwrap(), b"two");

    fs.rename(&dir.join("rooms/a"), &dir.join("rooms/b")).unwrap();
    assert!(!fs.exists(&dir.join("rooms/a")));
    assert_eq!(fs.read(&dir.join("rooms/b/messages.json")).unwrap(), b"{}");

    let mut names = fs.list_dir(&dir).unwrap();
    names.sort();
    assert_eq!(names, vec!["koushi-export.json".to_owned(), "rooms".to_owned()]);

    fs.remove_dir_all(&dir.join("rooms")).unwrap();
    assert!(!fs.exists(&dir.join("rooms/b/messages.json")));
    assert_eq!(fs.read(&dir.join("missing")), Err(HistoryExportFsError::NotFound));
}

#[test]
fn native_filesystem_meets_the_port_contract() {
    let root = tempfile::tempdir().unwrap();
    contract(&NativeHistoryExportFilesystem, root.path());
}

#[test]
fn memory_filesystem_meets_the_port_contract() {
    contract(&MemoryFilesystem::default(), Path::new("/virtual"));
}

#[test]
fn memory_filesystem_can_simulate_a_full_disk() {
    let fs = MemoryFilesystem::default();
    fs.fail_writes_with(Some(HistoryExportFsError::NoSpace));
    assert_eq!(
        fs.write_atomic(Path::new("/x/y"), b"z"),
        Err(HistoryExportFsError::NoSpace)
    );
}
