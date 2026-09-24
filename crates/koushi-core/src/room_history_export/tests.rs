use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use koushi_state::{HistoryExportRange, HistoryExportRoomCounts};
use serde_json::{Value, json};

use super::attachments::StopFlag;
use super::driver::{
    AsyncProgress, ExportCounters, FetchFailure, FetchOutputs, FetchResult, HistoryPage,
    HistoryPageError, HistoryPageSource, MAX_CONSECUTIVE_EMPTY_PAGES, PERIOD_MARGIN_MS, run_fetch,
};
use super::element::{
    ExportDateLocale, ExportHeader, ExportSourceEvent, UndecryptableReason, effective_event,
    element_renders, format_export_date,
};
use super::fs::{HistoryExportFsError, StagedFile};

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
    fn file(&self) -> Box<dyn StagedFile> {
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

impl StagedFile for MemoryFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        if self
            .output
            .fail_after_writes
            .is_some_and(|limit| self.writes >= limit)
        {
            return Err(HistoryExportFsError::Io);
        }
        self.writes += 1;
        self.output.bytes.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError> {
        *self.output.committed.lock().unwrap() = true;
        Ok(())
    }
}

/// Pages served in order; records the `from` token of every request and the
/// timestamp of every seek.
struct FakeSource {
    seek: Option<Result<Option<HistoryPage>, HistoryPageError>>,
    pages: VecDeque<Result<HistoryPage, HistoryPageError>>,
    requested_from: Vec<Option<String>>,
    seeks: Vec<u64>,
}

impl FakeSource {
    fn new(pages: Vec<Result<HistoryPage, HistoryPageError>>) -> Self {
        Self {
            seek: None,
            pages: pages.into(),
            requested_from: Vec::new(),
            seeks: Vec::new(),
        }
    }
}

impl HistoryPageSource for FakeSource {
    async fn seek(&mut self, at_ms: u64) -> Result<Option<HistoryPage>, HistoryPageError> {
        self.seeks.push(at_ms);
        self.seek.take().unwrap_or(Ok(None))
    }

    async fn next_page(&mut self, from: Option<String>) -> Result<HistoryPage, HistoryPageError> {
        self.requested_from.push(from);
        self.pages.pop_front().unwrap_or(Ok(HistoryPage {
            events: Vec::new(),
            end: None,
        }))
    }
}

#[derive(Default)]
struct RecordedProgress(Vec<HistoryExportRoomCounts>);

impl AsyncProgress for &mut RecordedProgress {
    async fn page_completed(&mut self, progress: HistoryExportRoomCounts) {
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

fn seek_page(
    events: Vec<ExportSourceEvent>,
    end: Option<&str>,
) -> Option<Result<Option<HistoryPage>, HistoryPageError>> {
    Some(page(events, end).map(Some))
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
    result: Result<(), FetchFailure>,
    fetched: Option<FetchResult>,
    output: MemoryOutput,
    events: MemoryOutput,
    progress: Vec<HistoryExportRoomCounts>,
    counters: HistoryExportRoomCounts,
    requested_from: Vec<Option<String>>,
    seeks: Vec<u64>,
}

fn run(
    pages: Vec<Result<HistoryPage, HistoryPageError>>,
    range: HistoryExportRange,
    output: MemoryOutput,
) -> Run {
    run_with_seek(None, pages, range, output)
}

fn run_with_seek(
    seek: Option<Result<Option<HistoryPage>, HistoryPageError>>,
    pages: Vec<Result<HistoryPage, HistoryPageError>>,
    range: HistoryExportRange,
    output: MemoryOutput,
) -> Run {
    let mut source = FakeSource::new(pages);
    source.seek = seek;
    let counters = Arc::new(ExportCounters::default());
    let mut progress = RecordedProgress::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let events = MemoryOutput::default();
    let result = runtime.block_on(run_fetch(
        &mut source,
        FetchOutputs {
            messages: output.file(),
            events: events.file(),
        },
        &header(),
        &range,
        OWN_USER,
        &counters,
        &StopFlag::default(),
        &mut progress,
    ));
    let (result, fetched) = match result {
        Ok(fetched) => (Ok(()), Some(fetched)),
        Err(failure) => (Err(failure), None),
    };
    Run {
        result,
        fetched,
        output,
        events,
        progress: progress.0,
        counters: counters.snapshot(),
        requested_from: source.requested_from,
        seeks: source.seeks,
    }
}

fn period(start_ms: u64, end_exclusive_ms: u64) -> HistoryExportRange {
    HistoryExportRange::Period {
        start_ms,
        end_exclusive_ms,
        time_zone: "UTC".to_owned(),
    }
}

fn progress(fetched: u64, exported: u64, undecryptable: u64) -> HistoryExportRoomCounts {
    HistoryExportRoomCounts {
        fetched_events: fetched,
        exported_events: exported,
        undecryptable_events: undecryptable,
        ..HistoryExportRoomCounts::default()
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
        HistoryExportRange::AllAvailable,
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

    // 26 distinct events fetched, 17 rendered, two of them undecryptable.
    assert_eq!(run.counters, progress(26, 17, 2));
    assert_eq!(run.progress.len(), 4);
    assert_eq!(run.progress.last().copied(), Some(progress(26, 17, 2)));
}

#[test]
fn output_uses_element_json_stringify_layout() {
    let run = run(
        vec![page(vec![message(1, 5)], None)],
        HistoryExportRange::AllAvailable,
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
        .block_on(run_fetch(
            &mut source,
            FetchOutputs {
                messages: output.file(),
                events: MemoryOutput::default().file(),
            },
            &header,
            &HistoryExportRange::AllAvailable,
            OWN_USER,
            &counters,
            &StopFlag::default(),
            &mut progress,
        ));
    assert!(result.is_ok());
    assert_eq!(
        output.text(),
        "{\n  \"room_name\": \"Synthetic Room\",\n  \"topic\": \"\",\n  \"export_date\": \"9/23/2026\",\n  \"exported_by\": \"Member 2\",\n  \"messages\": []\n}"
    );
}

#[test]
fn period_includes_the_start_instant_and_excludes_the_end_instant() {
    let start = 1_700_000_000_000;
    let end = 1_700_086_400_000;
    let range = HistoryExportRange::Period {
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
fn period_walks_past_out_of_order_timestamps_within_the_margin() {
    // Topological order does not follow origin_server_ts: an in-range event
    // can follow events that are already past the end of the period but
    // still within the margin.
    let end = 200;
    let run = run(
        vec![
            page(vec![message(1, 150), message(2, 900)], Some("t1")),
            page(vec![message(3, end + PERIOD_MARGIN_MS - 1)], Some("t2")),
            page(vec![message(4, 120)], None),
        ],
        period(100, end),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(
        exported_ids(&run),
        vec!["$m1:example.invalid", "$m4:example.invalid"]
    );
    assert_eq!(run.requested_from.len(), 3);
}

#[test]
fn period_stops_after_the_page_with_an_event_past_the_margin() {
    let start = 10 * PERIOD_MARGIN_MS;
    let end = start + 1_000;
    let run = run(
        vec![
            page(vec![message(1, start)], Some("t1")),
            // Events after the cutoff event on the same page are still judged.
            page(
                vec![message(2, end + PERIOD_MARGIN_MS), message(3, end - 1)],
                Some("t2"),
            ),
            page(vec![message(4, start + 1)], None),
        ],
        period(start, end),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.output.committed());
    assert_eq!(
        exported_ids(&run),
        vec!["$m1:example.invalid", "$m3:example.invalid"]
    );
    assert_eq!(run.requested_from, vec![None, Some("t1".into())]);
    assert_eq!(run.counters, progress(3, 2, 0));
    assert_eq!(run.progress.len(), 2);
}

#[test]
fn period_starts_from_the_seek_page_and_its_token() {
    let start = 10 * PERIOD_MARGIN_MS;
    let end = start + 1_000;
    let run = run_with_seek(
        seek_page(
            vec![message(1, start - PERIOD_MARGIN_MS), message(2, start)],
            Some("seek"),
        ),
        vec![page(vec![message(3, end - 1)], None)],
        period(start, end),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(run.seeks, vec![start - PERIOD_MARGIN_MS]);
    assert_eq!(run.requested_from, vec![Some("seek".into())]);
    assert_eq!(
        exported_ids(&run),
        vec!["$m2:example.invalid", "$m3:example.invalid"]
    );
    assert_eq!(run.counters, progress(3, 2, 0));
    assert_eq!(run.progress.len(), 2);
}

#[test]
fn a_seek_page_without_a_token_ends_the_walk() {
    let run = run_with_seek(
        seek_page(vec![message(1, 150)], None),
        Vec::new(),
        period(100, 200),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.requested_from.is_empty());
    assert_eq!(exported_ids(&run), vec!["$m1:example.invalid"]);
}

#[test]
fn an_unsupported_seek_falls_back_to_the_first_visible_event() {
    let run = run_with_seek(
        Some(Ok(None)),
        vec![page(vec![message(1, 5), message(2, 150)], None)],
        period(100, 200),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    // A start earlier than the margin saturates at the epoch.
    assert_eq!(run.seeks, vec![0]);
    assert_eq!(run.requested_from, vec![None]);
    assert_eq!(exported_ids(&run), vec!["$m2:example.invalid"]);
}

#[test]
fn a_failed_seek_fails_the_export_without_committing() {
    let run = run_with_seek(
        Some(Err(HistoryPageError::Network)),
        vec![page(vec![message(1, 150)], None)],
        period(100, 200),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Err(FetchFailure::Network));
    assert!(!run.output.committed());
    assert!(run.requested_from.is_empty());
}

#[test]
fn a_full_export_neither_seeks_nor_stops_at_late_timestamps() {
    let run = run(
        vec![
            page(vec![message(1, u64::MAX)], Some("t1")),
            page(vec![message(2, 1)], None),
        ],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.seeks.is_empty());
    assert_eq!(exported_ids(&run).len(), 2);
}

#[test]
fn a_period_ending_near_the_maximum_timestamp_does_not_overflow_the_cutoff() {
    let run = run(
        vec![
            page(vec![message(1, u64::MAX - 1)], Some("t1")),
            page(vec![message(2, u64::MAX - 2)], None),
        ],
        period(u64::MAX - 10, u64::MAX),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(exported_ids(&run).len(), 2);
}

#[test]
fn an_empty_period_still_writes_a_complete_file() {
    let start = 10 * PERIOD_MARGIN_MS;
    let end = start + 1_000;
    let run = run_with_seek(
        seek_page(vec![message(1, start - 1)], Some("seek")),
        vec![
            page(vec![message(2, end + PERIOD_MARGIN_MS)], Some("t1")),
            page(vec![message(3, start)], None),
        ],
        period(start, end),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.output.committed());
    assert!(exported_ids(&run).is_empty());
    assert_eq!(run.requested_from, vec![Some("seek".into())]);
    assert_eq!(run.counters, progress(2, 0, 0));
}

#[test]
fn a_repeated_token_ends_the_walk() {
    let run = run(
        vec![
            page(vec![message(1, 1)], Some("t1")),
            page(vec![message(2, 2)], Some("t1")),
        ],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(run.requested_from.len(), 2);
    assert_eq!(exported_ids(&run).len(), 2);
}

#[test]
fn an_unbounded_run_of_empty_pages_fails_instead_of_committing() {
    let pages = (0..MAX_CONSECUTIVE_EMPTY_PAGES + 10)
        .map(|index| page(Vec::new(), Some(&format!("t{index}"))))
        .collect();
    let run = run(
        pages,
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Err(FetchFailure::Sdk));
    assert!(!run.output.committed());
    assert_eq!(
        run.requested_from.len(),
        MAX_CONSECUTIVE_EMPTY_PAGES as usize
    );
}

#[test]
fn empty_pages_within_the_bound_do_not_end_the_walk() {
    let mut pages: Vec<_> = (0..MAX_CONSECUTIVE_EMPTY_PAGES - 1)
        .map(|index| page(Vec::new(), Some(&format!("t{index}"))))
        .collect();
    pages.push(page(vec![message(1, 1)], None));
    let run = run(
        pages,
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert_eq!(exported_ids(&run), vec!["$m1:example.invalid"]);
}

#[test]
fn a_page_failure_fails_the_export_without_committing() {
    let run = run(
        vec![
            page(vec![message(1, 1), message(2, 2)], Some("t1")),
            Err(HistoryPageError::Network),
        ],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Err(FetchFailure::Network));
    assert!(!run.output.committed());
    assert_eq!(run.counters, progress(2, 2, 0));

    let sdk = self::run(
        vec![Err(HistoryPageError::Sdk)],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    assert_eq!(sdk.result, Err(FetchFailure::Sdk));
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
        HistoryExportRange::AllAvailable,
        output,
    );
    assert_eq!(
        run.result,
        Err(FetchFailure::Write(HistoryExportFsError::Io))
    );
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

fn with_bundled_edit(mut event: Value, edit: Value) -> Value {
    event["unsigned"]["m.relations"] = json!({ "m.replace": edit });
    event
}

fn bundled_edit(edit_type: &str, content: Value) -> Value {
    json!({
        "type": edit_type,
        "sender": "@member-1:example.invalid",
        "event_id": "$edit:example.invalid",
        "origin_server_ts": 2,
        "content": content
    })
}

#[test]
fn a_bundled_edit_of_an_encrypted_original_keeps_its_wire_relation() {
    let original = json!({
        "type": "m.room.message",
        "sender": "@member-1:example.invalid",
        "event_id": "$thread-reply:example.invalid",
        "origin_server_ts": 1,
        "content": {
            "msgtype": "m.text",
            "body": "Synthetic original",
            "m.relates_to": { "rel_type": "m.thread", "event_id": "$root:example.invalid" }
        },
        "unsigned": {}
    });
    let edit = bundled_edit(
        "m.room.message",
        json!({ "m.new_content": { "msgtype": "m.text", "body": "Synthetic edited" } }),
    );
    let event = effective_event(ExportSourceEvent::Decrypted(with_bundled_edit(
        original, edit,
    )));
    assert_eq!(
        event.json["content"],
        json!({
            "msgtype": "m.text",
            "body": "Synthetic edited",
            "m.relates_to": { "rel_type": "m.thread", "event_id": "$root:example.invalid" }
        })
    );
}

#[test]
fn bundled_edits_without_new_content_empty_the_content_and_encrypted_ones_are_kept() {
    let ExportSourceEvent::Plain(original) = message(1, 1) else {
        unreachable!()
    };
    let without_new_content = effective_event(ExportSourceEvent::Plain(with_bundled_edit(
        original.clone(),
        bundled_edit("m.room.message", json!({ "body": "* no new content" })),
    )));
    assert_eq!(without_new_content.json["content"], json!({}));

    let encrypted_edit = effective_event(ExportSourceEvent::Plain(with_bundled_edit(
        original.clone(),
        bundled_edit("m.room.encrypted", json!({ "ciphertext": "c3ludGhldGlj" })),
    )));
    assert_eq!(encrypted_edit.json["content"], original["content"]);

    let mut redacted = original.clone();
    redacted["content"] = json!({});
    redacted["unsigned"]["redacted_because"] = json!({ "type": "m.room.redaction" });
    let redacted = effective_event(ExportSourceEvent::Plain(with_bundled_edit(
        redacted,
        bundled_edit(
            "m.room.message",
            json!({ "m.new_content": { "body": "x" } }),
        ),
    )));
    assert_eq!(redacted.json["content"], json!({}));
}

#[test]
fn a_non_state_jitsi_widget_event_renders_like_element() {
    let event = effective_event(ExportSourceEvent::Plain(json!({
        "type": "im.vector.modular.widgets",
        "sender": "@member-1:example.invalid",
        "event_id": "$widget:example.invalid",
        "origin_server_ts": 1,
        "content": { "type": "jitsi", "url": "https://example.invalid/widget" },
        "unsigned": {}
    })));
    assert!(element_renders(&event, OWN_USER));
}

fn reaction(index: u64, target: u64, timestamp: u64) -> ExportSourceEvent {
    ExportSourceEvent::Plain(json!({
        "type": "m.reaction",
        "sender": "@member-1:example.invalid",
        "room_id": "!history:example.invalid",
        "event_id": format!("$r{index}:example.invalid"),
        "origin_server_ts": timestamp,
        "content": { "m.relates_to": {
            "rel_type": "m.annotation",
            "event_id": format!("$m{target}:example.invalid"),
            "key": "👍"
        } }
    }))
}

fn jsonl_ids(output: &MemoryOutput) -> Vec<String> {
    output
        .text()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["event_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn events_jsonl_keeps_every_in_range_event_while_messages_json_is_unchanged() {
    let run = run(
        vec![page(
            vec![
                message(1, 10),
                reaction(1, 1, 11),
                message(1, 10),
                message(2, 99),
            ],
            None,
        )],
        period(0, 50),
        MemoryOutput::default(),
    );
    assert_eq!(run.result, Ok(()));
    assert!(run.events.committed());
    assert_eq!(exported_ids(&run), vec!["$m1:example.invalid"]);
    assert_eq!(
        jsonl_ids(&run.events),
        vec!["$m1:example.invalid", "$r1:example.invalid"],
        "reactions are kept, duplicates and out-of-range events are not"
    );
}

#[test]
fn the_fixture_writes_one_jsonl_line_per_distinct_fetched_event() {
    let run = run(
        vec![page(fixture_events(), None)],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    let mut actual: Value = serde_json::from_str(&run.output.text()).unwrap();
    let mut expected: Value = serde_json::from_str(EXPECTED).unwrap();
    actual["export_date"] = json!("<normalized>");
    expected["export_date"] = json!("<normalized>");
    assert_eq!(actual, expected, "messages.json stays Element-compatible");
    assert_eq!(
        run.events.text().lines().count() as u64,
        run.counters.fetched_events
    );
}

#[test]
fn fetch_collects_rendered_attachments_and_senders_in_order() {
    let file = |index: u64, msgtype: &str| {
        ExportSourceEvent::Plain(json!({
            "type": "m.room.message",
            "sender": format!("@member-{index}:example.invalid"),
            "room_id": "!history:example.invalid",
            "event_id": format!("$f{index}:example.invalid"),
            "origin_server_ts": index,
            "content": { "msgtype": msgtype, "body": format!("f{index}.bin"), "url": "mxc://h/x" }
        }))
    };
    let run = run(
        vec![page(
            vec![file(1, "m.file"), message(9, 2), file(3, "m.image")],
            None,
        )],
        HistoryExportRange::AllAvailable,
        MemoryOutput::default(),
    );
    let fetched = run.fetched.unwrap();
    let ids: Vec<_> = fetched
        .attachments
        .iter()
        .map(|a| a.event_id.as_str())
        .collect();
    assert_eq!(ids, vec!["$f1:example.invalid", "$f3:example.invalid"]);
    assert!(fetched.senders.contains("@member-1:example.invalid"));
    assert!(fetched.senders.contains("@member-3:example.invalid"));
}

#[test]
fn fetch_stops_between_pages_without_committing() {
    let mut source = FakeSource::new(vec![
        page(vec![message(1, 1)], Some("t1")),
        page(vec![message(2, 2)], None),
    ]);
    let stop = StopFlag::default();
    struct StopAfterFirstPage(StopFlag);
    impl AsyncProgress for StopAfterFirstPage {
        async fn page_completed(&mut self, _progress: HistoryExportRoomCounts) {
            self.0.set();
        }
    }
    let messages = MemoryOutput::default();
    let events = MemoryOutput::default();
    let result = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(run_fetch(
            &mut source,
            FetchOutputs {
                messages: messages.file(),
                events: events.file(),
            },
            &header(),
            &HistoryExportRange::AllAvailable,
            OWN_USER,
            &Arc::new(ExportCounters::default()),
            &stop,
            StopAfterFirstPage(stop.clone()),
        ));
    assert_eq!(result.err(), Some(FetchFailure::Stopped));
    assert_eq!(source.requested_from, vec![None]);
    assert!(!messages.committed() && !events.committed());
}
