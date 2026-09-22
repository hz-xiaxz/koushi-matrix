use super::*;

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("fixture write");
    path
}

#[test]
fn a_drop_is_claimed_once_in_order_and_each_file_is_read_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = write(&dir, "first.png", b"first");
    let second = write(&dir, "second.pdf", b"second");
    let ledger = DroppedFileLedger::default();

    ledger.record_drop(&[first, second]);
    let claimed = ledger.claim();

    assert_eq!(
        claimed
            .iter()
            .map(|file| file.filename.as_str())
            .collect::<Vec<_>>(),
        ["first.png", "second.pdf"]
    );
    assert!(ledger.claim().is_empty());
    let path = ledger.take_claimed(claimed[1].token).expect("claimed path");
    assert_eq!(read_within_limit(&path).as_deref(), Some(&b"second"[..]));
    assert!(ledger.take_claimed(claimed[1].token).is_none());
}

#[test]
fn only_files_from_the_latest_drop_are_readable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stale = write(&dir, "stale.txt", b"stale");
    let fresh = write(&dir, "fresh.txt", b"fresh");
    let ledger = DroppedFileLedger::default();

    ledger.record_drop(&[stale]);
    let stale_token = ledger.claim()[0].token;
    ledger.record_drop(&[fresh]);

    assert!(ledger.take_claimed(stale_token).is_none());
    assert!(ledger.take_claimed(stale_token + 1).is_none());
    assert_eq!(ledger.claim().len(), 1);
}

#[test]
fn directories_missing_paths_and_overflow_are_not_claimed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("missing.png");
    let mut paths = vec![dir.path().to_path_buf(), missing];
    for index in 0..=MAX_MEDIA_STAGING_BATCH_SIZE {
        paths.push(write(&dir, &format!("file-{index}.txt"), b"x"));
    }
    let ledger = DroppedFileLedger::default();

    ledger.record_drop(&paths);
    let claimed = ledger.claim();

    assert_eq!(claimed.len(), MAX_MEDIA_STAGING_BATCH_SIZE);
    assert_eq!(claimed[0].filename, "file-0.txt");
}

#[cfg(target_os = "linux")]
#[test]
fn claimed_files_carry_the_desktop_mime_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let png = write(&dir, "shot.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    let pdf = write(&dir, "report.pdf", b"%PDF-1.7\n");
    let ledger = DroppedFileLedger::default();

    ledger.record_drop(&[png, pdf]);
    let claimed = ledger.claim();

    assert_eq!(claimed[0].mime_type, "image/png");
    assert_eq!(claimed[1].mime_type, "application/pdf");
}
