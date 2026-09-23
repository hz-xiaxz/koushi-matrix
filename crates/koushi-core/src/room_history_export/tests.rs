use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use koushi_state::{
    RoomHistoryExportFailureKind, RoomHistoryExportProgress, RoomHistoryExportRange,
};
use serde_json::{Value, json};

use super::driver::{
    AsyncProgress, ExportCounters, HistoryPage, HistoryPageError, HistoryPageSource,
    MAX_CONSECUTIVE_EMPTY_PAGES, run_export,
};
use super::element::{
    ExportDateLocale, ExportHeader, ExportSourceEvent, UndecryptableReason, format_export_date,
};
use super::sink::{
    NativeRoomHistoryExportSink, RoomHistoryExportFile, RoomHistoryExportSink,
    RoomHistoryExportSinkError,
};

const SOURCE: &str = include_str!("../../tests/fixtures/room_history_export/source_events.json");
const EXPECTED: &str =
    include_str!("../../tests/fixtures/room_history_export/element_expected.json");
const OWN_USER: &str = "@member-2:example.invalid";

fn fixture_events() -> Vec<ExportSourceEvent> {
    let source: Value = serde_json::from_str(SOURCE).unwrap();
    source["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let event = entry["event"].clone();
            match entry["kind"].as_str().unwrap() {
                "plain" => ExportSourceEvent::Plain(event),
                "decrypted" => ExportSourceEvent::Decrypted(event),
                "undecryptable" => ExportSourceEvent::Undecryptable {
                    wire: event,
                    reason: match entry["reason"].as_str().unwrap() {
                        "missingRoomKey" => UndecryptableReason::MissingRoomKey,
                        "unknownMessageIndex" => UndecryptableReason::UnknownMessageIndex,
                        _ => UndecryptableReason::Other,
                    },
                },
                other => panic!("unknown fixture kind {other}"),
            }
        })
        .collect()
}

fn header() -> ExportHeader {
    ExportHeader {
        room_name: "Synthetic Room".to_owned(),
        room_creator: Some("Member 1".to_owned()),
        topic: "Synthetic topic".to_owned(),
        export_date: "9/23/2026".to_owned(),
        exported_by: "Member 2".to_owned(),
    }
}

#[derive(Clone, Default)]
struct MemoryOutput {
    bytes: Arc<Mutex<Vec<u8>>>,
    committed: Arc<Mutex<bool>>,
    fail_after_writes: Option<usize>,
}

impl MemoryOutput {
    fn file(&self) -> Box<dyn RoomHistoryExportFile> {
        Box::new(MemoryFile {
            output: self.clone(),
            writes: 0,
        })
    }

    fn text(&self) -> String {
        String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
    }

    fn committed(&self) -> bool {
        *self.committed.lock().unwrap()
    }
}

struct MemoryFile {
    output: MemoryOutput,
    writes: usize,
}

impl RoomHistoryExportFile for MemoryFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), RoomHistoryExportSinkError> {
        if self
            .output
            .fail_after_writes
            .is_some_and(|limit| self.writes >= limit)
        {
            return Err(RoomHistoryExportSinkError::Write);
        }
        self.writes += 1;
        self.output.bytes.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn commit(self: Box<Self>) -> Result<(), RoomHistoryExportSinkError> {
        *self.output.committed.lock().unwrap() = true;
        Ok(())
    }
}

/// Pages served in order; records the `from` token of every request.
struct FakeSource {
    pages: VecDeque<Result<HistoryPage, HistoryPageError>>,
    requested_from: Vec<Option<String>>,
}

impl FakeSource {
    fn new(pages: Vec<Result<HistoryPage, HistoryPageError>>) -> Self {
        Self {
            pages: pages.into(),
            requested_from: Vec::new(),
        }
    }
}

impl HistoryPageSource for FakeSource {
    async fn next_page(&mut self, from: Option<String>) -> Result<HistoryPage, HistoryPageError> {
        self.requested_from.push(from);
        self.pages.pop_front().unwrap_or(Ok(HistoryPage {
            events: Vec::new(),
            end: None,
        }))
    }
}

#[derive(Default)]
struct RecordedProgress(Vec<RoomHistoryExportProgress>);

impl AsyncProgress for &mut RecordedProgress {
    async fn page_completed(&mut self, progress: RoomHistoryExportProgress) {
        self.0.push(progress);
    }
}

fn page(
    events: Vec<ExportSourceEvent>,
    end: Option<&str>,
) -> Result<HistoryPage, HistoryPageError> {
    Ok(HistoryPage {
        events,
        end: end.map(str::to_owned),
    })
}

fn message(index: u64, timestamp: u64) -> ExportSourceEvent {
    ExportSourceEvent::Plain(json!({
        "type": "m.room.message",
        "sender": "@member-1:example.invalid",
        "room_id": "!history:example.invalid",
        "event_id": format!("$m{index}:example.invalid"),
        "origin_server_ts": timestamp,
        "content": { "msgtype": "m.text", "body": format!("Synthetic message {index}") },
        "unsigned": {}
    }))
}

struct Run {
    result: Result<(), RoomHistoryExportFailureKind>,
    output: MemoryOutput,
    progress: Vec<RoomHistoryExportProgress>,
    counters: RoomHistoryExportProgress,
    requested_from: Vec<Option<String>>,
}

fn run(
    pages: Vec<Result<HistoryPage, HistoryPageError>>,
    range: RoomHistoryExportRange,
    output: MemoryOutput,
) -> Run {
    let mut source = FakeSource::new(pages);
    let counters = Arc::new(ExportCounters::default());
    let mut progress = RecordedProgress::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let result = runtime.block_on(run_export(
        &mut source,
        output.file(),
        &header(),
        &range,
        OWN_USER,
        &counters,
        &mut progress,
    ));
    Run {
        result,
        output,
        progress: progress.0,
        counters: counters.snapshot(),
        requested_from: source.requested_from,
    }
}

fn progress(fetched: u64, exported: u64, undecryptable: u64) -> RoomHistoryExportProgress {
    RoomHistoryExportProgress {
        fetched_events: fetched,
        exported_events: exported,
        undecryptable_events: undecryptable,
    }
}

fn exported_ids(run: &Run) -> Vec<String> {
    let value: Value = serde_json::from_str(&run.output.text()).unwrap();
    value["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn fixture_matches_element_export_across_pages_with_duplicates_and_empty_pages() {
    let mut events = fixture_events();
    let tail = events.split_off(15);
    let middle = events.split_off(8);
    let duplicate_of_last_middle = middle.last().cloned().unwrap();
    let mut tail_with_duplicate = vec![duplicate_of_last_middle];
    tail_with_duplicate.extend(tail);

    let run = run(
        vec![
            page(events, Some("t1")),
            page(Vec::new(), Some("t2")),
            page(middle, Some("t3")),
            page(tail_with_duplicate, None),
        ],
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );

    assert_eq!(run.result, Ok(()));
    assert!(run.output.committed());
    assert_eq!(
        run.requested_from,
        vec![
            None,
            Some("t1".into()),
            Some("t2".into()),
            Some("t3".into())
        ]
    );

    let mut actual: Value = serde_json::from_str(&run.output.text()).unwrap();
    let mut expected: Value = serde_json::from_str(EXPECTED).unwrap();
    assert_eq!(actual["export_date"], "9/23/2026");
    actual["export_date"] = json!("<normalized>");
    expected["export_date"] = json!("<normalized>");
    assert_eq!(actual, expected);

    // 24 distinct events fetched, 15 rendered, one of them undecryptable.
    assert_eq!(run.counters, progress(24, 15, 1));
    assert_eq!(run.progress.len(), 4);
    assert_eq!(run.progress.last().copied(), Some(progress(24, 15, 1)));
}

#[test]
fn output_uses_element_json_stringify_layout() {
    let run = run(
        vec![page(vec![message(1, 5)], None)],
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    let expected = "{\n  \"room_name\": \"Synthetic Room\",\n  \"room_creator\": \"Member 1\",\n  \"topic\": \"Synthetic topic\",\n  \"export_date\": \"9/23/2026\",\n  \"exported_by\": \"Member 2\",\n  \"messages\": [\n    {\n      \"content\": {\n        \"body\": \"Synthetic message 1\",\n        \"msgtype\": \"m.text\"\n      },\n      \"event_id\": \"$m1:example.invalid\",\n      \"origin_server_ts\": 5,\n      \"room_id\": \"!history:example.invalid\",\n      \"sender\": \"@member-1:example.invalid\",\n      \"type\": \"m.room.message\",\n      \"unsigned\": {}\n    }\n  ]\n}";
    assert_eq!(run.output.text(), expected);
}

#[test]
fn empty_export_writes_an_empty_messages_array_and_omits_an_unknown_creator() {
    let mut source = FakeSource::new(vec![page(Vec::new(), None)]);
    let output = MemoryOutput::default();
    let counters = Arc::new(ExportCounters::default());
    let mut progress = RecordedProgress::default();
    let header = ExportHeader {
        room_creator: None,
        topic: String::new(),
        ..header()
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(run_export(
            &mut source,
            output.file(),
            &header,
            &RoomHistoryExportRange::AllAvailable,
            OWN_USER,
            &counters,
            &mut progress,
        ));
    assert_eq!(result, Ok(()));
    assert_eq!(
        output.text(),
        "{\n  \"room_name\": \"Synthetic Room\",\n  \"topic\": \"\",\n  \"export_date\": \"9/23/2026\",\n  \"exported_by\": \"Member 2\",\n  \"messages\": []\n}"
    );
}

#[test]
fn period_includes_the_start_instant_and_excludes_the_end_instant() {
    let start = 1_700_000_000_000;
    let end = 1_700_086_400_000;
    let range = RoomHistoryExportRange::Period {
        start_ms: start,
        end_exclusive_ms: end,
        time_zone: "Asia/Tokyo".to_owned(),
    };
    let run = run(
        vec![page(
            vec![
                message(1, start - 1),
                message(2, start),
                message(3, end - 1),
                message(4, end),
            ],
            None,
        )],
        range,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(
        exported_ids(&run),
        vec!["$m2:example.invalid", "$m3:example.invalid"]
    );
    assert_eq!(run.counters, progress(4, 2, 0));
}

#[test]
fn period_walks_past_out_of_order_timestamps_instead_of_stopping_early() {
    // Topological order does not follow origin_server_ts: an in-range event
    // can follow events that are already past the end of the period.
    let range = RoomHistoryExportRange::Period {
        start_ms: 100,
        end_exclusive_ms: 200,
        time_zone: "UTC".to_owned(),
    };
    let run = run(
        vec![
            page(vec![message(1, 150), message(2, 900)], Some("t1")),
            page(vec![message(3, 950)], Some("t2")),
            page(vec![message(4, 120)], None),
        ],
        range,
        MemoryOutput::default(),
    );
    assert_eq!(
        exported_ids(&run),
        vec!["$m1:example.invalid", "$m4:example.invalid"]
    );
}

#[test]
fn an_empty_period_still_writes_a_complete_file() {
    let range = RoomHistoryExportRange::Period {
        start_ms: 10_000,
        end_exclusive_ms: 20_000,
        time_zone: "UTC".to_owned(),
    };
    let run = run(
        vec![page(vec![message(1, 5), message(2, 30_000)], None)],
        range,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.output.committed());
    assert!(exported_ids(&run).is_empty());
    assert_eq!(run.counters, progress(2, 0, 0));
}

#[test]
fn a_repeated_token_ends_the_walk() {
    let run = run(
        vec![
            page(vec![message(1, 1)], Some("t1")),
            page(vec![message(2, 2)], Some("t1")),
        ],
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(run.requested_from.len(), 2);
    assert_eq!(exported_ids(&run).len(), 2);
}

#[test]
fn a_bounded_run_of_empty_pages_ends_the_walk() {
    let pages = (0..MAX_CONSECUTIVE_EMPTY_PAGES + 10)
        .map(|index| page(Vec::new(), Some(&format!("t{index}"))))
        .collect();
    let run = run(
        pages,
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(
        run.requested_from.len(),
        MAX_CONSECUTIVE_EMPTY_PAGES as usize
    );
}

#[test]
fn a_page_failure_fails_the_export_without_committing() {
    let run = run(
        vec![
            page(vec![message(1, 1), message(2, 2)], Some("t1")),
            Err(HistoryPageError::Network),
        ],
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Err(RoomHistoryExportFailureKind::Network));
    assert!(!run.output.committed());
    assert_eq!(run.counters, progress(2, 2, 0));

    let sdk = self::run(
        vec![Err(HistoryPageError::Sdk)],
        RoomHistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(sdk.result, Err(RoomHistoryExportFailureKind::Sdk));
}

#[test]
fn a_write_failure_fails_the_export_without_committing() {
    let output = MemoryOutput {
        fail_after_writes: Some(2),
        ..MemoryOutput::default()
    };
    let run = run(
        vec![page(
            vec![message(1, 1), message(2, 2), message(3, 3)],
            None,
        )],
        RoomHistoryExportRange::AllAvailable,
        output,
    );
    assert_eq!(run.result, Err(RoomHistoryExportFailureKind::Write));
    assert!(!run.output.committed());
}

#[test]
fn export_date_uses_the_local_calendar_day_and_locale_order() {
    // 2026-09-23T15:30:00Z.
    let now = 1_790_177_400_000;
    assert_eq!(
        format_export_date(now, 0, ExportDateLocale::En),
        "9/23/2026"
    );
    assert_eq!(
        format_export_date(now, 0, ExportDateLocale::Ja),
        "2026/9/23"
    );
    // UTC+09:00 has already reached the next day.
    assert_eq!(
        format_export_date(now, 540, ExportDateLocale::Ja),
        "2026/9/24"
    );
    // UTC-10:00 is still on 2026-09-23.
    assert_eq!(
        format_export_date(now, -600, ExportDateLocale::En),
        "9/23/2026"
    );
    assert_eq!(
        format_export_date(0, -60, ExportDateLocale::En),
        "12/31/1969"
    );
    assert_eq!(
        format_export_date(951_782_400_000, 0, ExportDateLocale::En),
        "2/29/2000"
    );
}

#[test]
fn native_sink_replaces_the_destination_only_on_commit() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("export.json");
    let sink = NativeRoomHistoryExportSink;

    let mut file = sink.create(&destination).unwrap();
    file.write_all(b"{\"partial\":").unwrap();
    drop(file);
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

    std::fs::write(&destination, b"previous").unwrap();
    let mut file = sink.create(&destination).unwrap();
    file.write_all(b"{}").unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"previous");
    file.commit().unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"{}");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);

    assert_eq!(
        sink.create(std::path::Path::new("relative.json")).err(),
        Some(RoomHistoryExportSinkError::InvalidDestination)
    );
}
