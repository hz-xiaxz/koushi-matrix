use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;

use image::{DynamicImage, ImageFormat, RgbImage};
use matrix_sdk::ruma::events::room::MediaSource;
use serde_json::json;

use super::attachments::*;
use super::fs::HistoryExportFilesystem;
use super::fs_fake::MemoryFilesystem;
use super::records::{AttachmentKind, AttachmentStatus};

fn png() -> Vec<u8> {
    let image = DynamicImage::ImageRgb8(RgbImage::new(8, 8));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn plain_url(source: &MediaSource) -> String {
    match source {
        MediaSource::Plain(uri) => uri.to_string(),
        MediaSource::Encrypted(file) => file.url.to_string(),
    }
}

#[derive(Default)]
struct FakeFetcher {
    answers: Mutex<HashMap<String, VecDeque<Result<Vec<u8>, FetchError>>>>,
    calls: Mutex<Vec<String>>,
}

impl FakeFetcher {
    fn answer(self, url: &str, answers: Vec<Result<Vec<u8>, FetchError>>) -> Self {
        self.answers
            .lock()
            .unwrap()
            .insert(url.to_owned(), answers.into());
        self
    }
}

impl AttachmentFetcher for FakeFetcher {
    async fn fetch(&self, source: &MediaSource) -> Result<Vec<u8>, FetchError> {
        let url = plain_url(source);
        self.calls.lock().unwrap().push(url.clone());
        self.answers
            .lock()
            .unwrap()
            .get_mut(&url)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Err(FetchError::Permanent))
    }
}

fn message(id: &str, content: serde_json::Value) -> serde_json::Value {
    json!({ "type": "m.room.message", "event_id": id, "sender": "@a:x", "origin_server_ts": 1, "content": content })
}

fn image_ref(id: &str, name: &str, url: &str) -> AttachmentRef {
    attachment_ref(&message(
        id,
        json!({ "msgtype": "m.image", "body": name, "url": url }),
    ))
    .unwrap()
}

fn room_dir(fs: &MemoryFilesystem) -> &'static Path {
    let dir = Path::new("/export/rooms/.room.partial");
    fs.create_dir_all(dir).unwrap();
    dir
}

#[test]
fn attachment_ref_reads_plain_and_encrypted_sources() {
    let plain = attachment_ref(&message(
        "$1",
        json!({
            "msgtype": "m.file", "body": "caption", "filename": "paper.pdf", "url": "mxc://h/plain",
            "info": { "size": 2048, "mimetype": "application/pdf" }
        }),
    ))
    .unwrap();
    assert_eq!(plain.kind, AttachmentKind::File);
    assert_eq!(plain.name, "paper.pdf");
    assert_eq!(plain.size, Some(2048));
    assert_eq!(plain.mimetype.as_deref(), Some("application/pdf"));
    assert_eq!(plain_url(&plain.source), "mxc://h/plain");

    let encrypted = attachment_ref(&message("$2", json!({
        "msgtype": "m.image", "body": "photo.jpg",
        "file": {
            "url": "mxc://h/enc", "v": "v2",
            "key": { "kty": "oct", "key_ops": ["encrypt", "decrypt"], "alg": "A256CTR", "k": "qcHVMSgYg-71CauWBezXI5qkaRb0LuIy-Wx5kIaHMIA", "ext": true },
            "iv": "X85+XgHN+HEAAAAAAAAAAA",
            "hashes": { "sha256": "5qG4fFnbbVdlAB1Q72JDKwCagV6Dbkx9uds4rSak37c" }
        }
    }))).unwrap();
    assert_eq!(encrypted.kind, AttachmentKind::Image);
    assert!(matches!(encrypted.source, MediaSource::Encrypted(_)));

    let sticker = attachment_ref(
        &json!({ "type": "m.sticker", "event_id": "$3", "sender": "@a:x",
        "content": { "body": "wave", "url": "mxc://h/s", "info": {} } }),
    )
    .unwrap();
    assert_eq!(sticker.kind, AttachmentKind::Sticker);
}

#[test]
fn attachment_ref_ignores_text_redacted_and_sourceless_events() {
    assert!(attachment_ref(&message("$1", json!({ "msgtype": "m.text", "body": "hi" }))).is_none());
    assert!(attachment_ref(&message("$2", json!({}))).is_none());
    assert!(
        attachment_ref(&message(
            "$3",
            json!({ "msgtype": "m.image", "body": "x.png" })
        ))
        .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn download_writes_sequenced_files_and_thumbs() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let pdf = attachment_ref(&message(
        "$3",
        json!({ "msgtype": "m.file", "body": "c.pdf", "url": "mxc://h/c" }),
    ))
    .unwrap();
    let fetcher = FakeFetcher::default()
        .answer("mxc://h/a", vec![Ok(png())])
        .answer("mxc://h/b", vec![Ok(png())])
        .answer("mxc://h/c", vec![Ok(b"%PDF".to_vec())]);
    let refs = vec![
        image_ref("$1", "a.png", "mxc://h/a"),
        image_ref("$2", "b.png", "mxc://h/b"),
        pdf,
    ];
    let mut progress = Vec::new();
    let index = download_attachments(
        &fetcher,
        &fs,
        dir,
        refs,
        &StopFlag::default(),
        |done, total, failed| progress.push((done, total, failed)),
    )
    .await
    .unwrap();
    assert_eq!(
        fs.files_below(dir),
        vec![
            "files/0001_a.png",
            "files/0002_b.png",
            "files/0003_c.pdf",
            "thumbs/0001.jpg",
            "thumbs/0002.jpg"
        ]
    );
    assert_eq!(
        index.attachments[0].file.as_deref(),
        Some("files/0001_a.png")
    );
    assert_eq!(
        index.attachments[0].thumb.as_deref(),
        Some("thumbs/0001.jpg")
    );
    assert_eq!(index.attachments[2].thumb, None);
    assert_eq!(progress.last(), Some(&(3, 3, 0)));
}

#[tokio::test(start_paused = true)]
async fn download_retries_transient_then_records_failure() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let fetcher = FakeFetcher::default().answer(
        "mxc://h/a",
        vec![
            Err(FetchError::Transient),
            Err(FetchError::Transient),
            Err(FetchError::Transient),
        ],
    );
    let index = download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &StopFlag::default(),
        |_, _, _| {},
    )
    .await
    .unwrap();
    assert_eq!(fetcher.calls.lock().unwrap().len(), FETCH_ATTEMPTS as usize);
    assert_eq!(index.attachments[0].status, AttachmentStatus::Failed);
    assert!(fs.files_below(dir).is_empty());
}

#[tokio::test(start_paused = true)]
async fn transient_failure_then_success_is_retrieved() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let fetcher =
        FakeFetcher::default().answer("mxc://h/a", vec![Err(FetchError::Transient), Ok(png())]);
    let index = download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &StopFlag::default(),
        |_, _, _| {},
    )
    .await
    .unwrap();
    assert_eq!(index.attachments[0].status, AttachmentStatus::Retrieved);
}

#[tokio::test(start_paused = true)]
async fn permanent_failure_is_not_retried() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let fetcher = FakeFetcher::default().answer("mxc://h/a", vec![Err(FetchError::Permanent)]);
    let mut last = None;
    download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &StopFlag::default(),
        |d, t, f| last = Some((d, t, f)),
    )
    .await
    .unwrap();
    assert_eq!(fetcher.calls.lock().unwrap().len(), 1);
    assert_eq!(last, Some((1, 1, 1)));
}

#[tokio::test(start_paused = true)]
async fn corrupt_image_keeps_file_without_thumb() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let fetcher = FakeFetcher::default().answer("mxc://h/a", vec![Ok(b"garbage".to_vec())]);
    let index = download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &StopFlag::default(),
        |_, _, _| {},
    )
    .await
    .unwrap();
    assert_eq!(fs.files_below(dir), vec!["files/0001_a.png"]);
    assert_eq!(index.attachments[0].thumb, None);
    assert_eq!(index.attachments[0].status, AttachmentStatus::Retrieved);
}

#[tokio::test(start_paused = true)]
async fn stop_flag_interrupts_between_files() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let fetcher = FakeFetcher::default()
        .answer("mxc://h/a", vec![Ok(png())])
        .answer("mxc://h/b", vec![Ok(png())]);
    let stop = StopFlag::default();
    let stopper = stop.clone();
    let result = download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![
            image_ref("$1", "a.png", "mxc://h/a"),
            image_ref("$2", "b.png", "mxc://h/b"),
        ],
        &stop,
        move |done, _, _| {
            if done == 1 {
                stopper.set()
            }
        },
    )
    .await;
    assert_eq!(result.unwrap_err(), ArchiveStepError::Stopped);
    assert_eq!(fetcher.calls.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_full_disk_fails_the_step() {
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    fs.fail_after_writes(0, super::fs::HistoryExportFsError::NoSpace);
    let fetcher = FakeFetcher::default().answer("mxc://h/a", vec![Ok(png())]);
    let result = download_attachments(
        &fetcher,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &StopFlag::default(),
        |_, _, _| {},
    )
    .await;
    assert_eq!(
        result.unwrap_err(),
        ArchiveStepError::Write(super::fs::HistoryExportFsError::NoSpace)
    );
}

#[tokio::test(start_paused = true)]
async fn stop_cancels_an_in_flight_fetch() {
    struct Hanging;
    impl AttachmentFetcher for Hanging {
        async fn fetch(&self, _source: &MediaSource) -> Result<Vec<u8>, FetchError> {
            std::future::pending().await
        }
    }
    let fs = MemoryFilesystem::default();
    let dir = room_dir(&fs);
    let stop = StopFlag::default();
    let stopper = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        stopper.set();
    });
    let result = download_attachments(
        &Hanging,
        &fs,
        dir,
        vec![image_ref("$1", "a.png", "mxc://h/a")],
        &stop,
        |_, _, _| {},
    )
    .await;
    assert_eq!(result.unwrap_err(), ArchiveStepError::Stopped);
}
