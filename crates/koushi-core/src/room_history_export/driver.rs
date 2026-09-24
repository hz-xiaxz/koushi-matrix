//! Paging and streaming loop for one room-history export.
//!
//! History is read oldest-first with `/messages?dir=f`, so events are written
//! in Element's order without buffering the room. A full export starts at the
//! first event visible to this account and reads to the end.
//!
//! A period export bounds the walk with [`PERIOD_MARGIN_MS`]. It starts at
//! the event `timestamp_to_event` finds at `start - margin` (or at the first
//! visible event when the server cannot seek), and stops after the page that
//! holds an event at or after `end + margin`. Event order is topological and
//! `origin_server_ts` is sender-controlled, so the margin absorbs clock skew
//! and reordering; an in-range event displaced by more than the margin is
//! omitted. That tradeoff keeps short periods of long rooms fast.

use std::collections::{BTreeSet, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use koushi_state::{
    RoomHistoryExportFailureKind, RoomHistoryExportProgress, RoomHistoryExportRange,
};

use super::attachments::{AttachmentRef, StopFlag, attachment_ref};
use super::element::{
    ElementJsonWriter, ExportHeader, ExportSourceEvent, effective_event, element_renders,
};
use super::fs::{HistoryExportFsError, StagedFile};
use super::sink::RoomHistoryExportFile;

/// Events requested per `/messages` page.
pub(crate) const PAGE_LIMIT: u32 = 250;

/// Consecutive empty pages that may still carry a new token. Servers return
/// empty filtered pages across events hidden by history visibility, so a run
/// is tolerated, but an export that exceeds this bound fails rather than
/// committing a file that might silently omit later history.
pub(crate) const MAX_CONSECUTIVE_EMPTY_PAGES: u32 = 200;

/// How far outside a period the walk still reads: 24 hours.
pub(crate) const PERIOD_MARGIN_MS: u64 = 24 * 60 * 60 * 1000;

/// One `/messages` page in forward order.
pub(crate) struct HistoryPage {
    pub(crate) events: Vec<ExportSourceEvent>,
    /// Token for the next page; `None` when the visible history is exhausted.
    pub(crate) end: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryPageError {
    Network,
    Sdk,
}

/// A forward `/messages` reader. `from = None` starts at the first visible
/// event.
pub(crate) trait HistoryPageSource: Send {
    /// The page at the first event at or after `at_ms`, with the token that
    /// continues forward from it. `None` when the server cannot seek or finds
    /// no such event; the walk then starts at the first visible event.
    fn seek(
        &mut self,
        at_ms: u64,
    ) -> impl Future<Output = Result<Option<HistoryPage>, HistoryPageError>> + Send;

    fn next_page(
        &mut self,
        from: Option<String>,
    ) -> impl Future<Output = Result<HistoryPage, HistoryPageError>> + Send;
}

/// Counters shared with the owning actor so a cancelled or aborted export can
/// still report how far it got.
#[derive(Debug, Default)]
pub(crate) struct ExportCounters {
    fetched: AtomicU64,
    exported: AtomicU64,
    undecryptable: AtomicU64,
}

impl ExportCounters {
    pub(crate) fn snapshot(&self) -> RoomHistoryExportProgress {
        RoomHistoryExportProgress {
            fetched_events: self.fetched.load(Ordering::Acquire),
            exported_events: self.exported.load(Ordering::Acquire),
            undecryptable_events: self.undecryptable.load(Ordering::Acquire),
        }
    }
}

/// Why the fetch stage of one room did not finish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FetchFailure {
    Network,
    Sdk,
    Write(HistoryExportFsError),
    Stopped,
}

fn page_failure(error: HistoryPageError) -> FetchFailure {
    match error {
        HistoryPageError::Network => FetchFailure::Network,
        HistoryPageError::Sdk => FetchFailure::Sdk,
    }
}

/// The two originals the fetch stage writes.
pub(crate) struct FetchOutputs {
    /// `messages.json`, Element's chat-export JSON.
    pub(crate) messages: Box<dyn StagedFile>,
    /// `events.jsonl`, every distinct in-range event, one per line.
    pub(crate) events: Box<dyn StagedFile>,
}

/// What the later stages need from the fetched history.
#[derive(Debug, Default)]
pub(crate) struct FetchResult {
    /// Senders of in-range events, for `room.json` display names.
    pub(crate) senders: BTreeSet<String>,
    /// Attachments of rendered in-range events, in history order.
    pub(crate) attachments: Vec<AttachmentRef>,
}

/// Read the room's history into `outputs`, committing both only when the
/// history is exhausted or a period export has read past its margin.
/// `on_page` receives the counters after every page; `stop` is checked
/// between pages.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_fetch<S, P>(
    source: &mut S,
    outputs: FetchOutputs,
    header: &ExportHeader,
    range: &RoomHistoryExportRange,
    own_user_id: &str,
    counters: &Arc<ExportCounters>,
    stop: &StopFlag,
    mut on_page: P,
) -> Result<FetchResult, FetchFailure>
where
    S: HistoryPageSource,
    P: AsyncProgress,
{
    let FetchOutputs {
        mut messages,
        mut events,
    } = outputs;
    let (mut writer, head) = ElementJsonWriter::begin(header);
    messages.write_all(&head).map_err(FetchFailure::Write)?;
    let mut fetched = FetchResult::default();

    let (mut seeked, cutoff_ms) = match range {
        RoomHistoryExportRange::AllAvailable => (None, None),
        RoomHistoryExportRange::Period {
            start_ms,
            end_exclusive_ms,
            ..
        } => (
            source
                .seek(start_ms.saturating_sub(PERIOD_MARGIN_MS))
                .await
                .map_err(page_failure)?,
            Some(end_exclusive_ms.saturating_add(PERIOD_MARGIN_MS)),
        ),
    };

    let mut seen_event_ids = HashSet::<String>::new();
    let mut from: Option<String> = None;
    let mut empty_pages = 0_u32;
    loop {
        if stop.is_set() {
            return Err(FetchFailure::Stopped);
        }
        let page = match seeked.take() {
            Some(page) => page,
            None => source.next_page(from.clone()).await.map_err(page_failure)?,
        };
        let page_was_empty = page.events.is_empty();
        let mut past_cutoff = false;
        for source_event in page.events {
            let event = effective_event(source_event);
            let Some(event_id) = event.event_id() else {
                continue;
            };
            if !seen_event_ids.insert(event_id.to_owned()) {
                continue;
            }
            counters.fetched.fetch_add(1, Ordering::AcqRel);
            let timestamp = event.origin_server_ts();
            if let (Some(timestamp), Some(cutoff_ms)) = (timestamp, cutoff_ms) {
                past_cutoff |= timestamp >= cutoff_ms;
            }
            if !timestamp.is_some_and(|timestamp| range.contains(timestamp)) {
                continue;
            }
            let mut line = serde_json::to_vec(&event.json).unwrap_or_default();
            line.push(b'\n');
            events.write_all(&line).map_err(FetchFailure::Write)?;
            if let Some(sender) = event.json.get("sender").and_then(serde_json::Value::as_str) {
                if !fetched.senders.contains(sender) {
                    fetched.senders.insert(sender.to_owned());
                }
            }
            if !element_renders(&event, own_user_id) {
                continue;
            }
            if let Some(attachment) = attachment_ref(&event.json) {
                fetched.attachments.push(attachment);
            }
            messages
                .write_all(&writer.event(&event.json))
                .map_err(FetchFailure::Write)?;
            counters.exported.fetch_add(1, Ordering::AcqRel);
            if event.undecryptable {
                counters.undecryptable.fetch_add(1, Ordering::AcqRel);
            }
        }
        on_page.page_completed(counters.snapshot()).await;
        if past_cutoff {
            break;
        }

        empty_pages = if page_was_empty { empty_pages + 1 } else { 0 };
        match page.end {
            Some(end) if from.as_deref() != Some(end.as_str()) => {
                if empty_pages >= MAX_CONSECUTIVE_EMPTY_PAGES {
                    return Err(FetchFailure::Sdk);
                }
                from = Some(end);
            }
            _ => break,
        }
    }

    messages
        .write_all(&writer.finish())
        .map_err(FetchFailure::Write)?;
    messages.commit().map_err(FetchFailure::Write)?;
    events.commit().map_err(FetchFailure::Write)?;
    Ok(fetched)
}

/// Adapter kept until the actor moves to the archive pipeline.
struct SinkFile(Box<dyn RoomHistoryExportFile>);

impl StagedFile for SinkFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        self.0.write_all(bytes).map_err(|_| HistoryExportFsError::Io)
    }

    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError> {
        self.0.commit().map_err(|_| HistoryExportFsError::Io)
    }
}

struct DiscardFile;

impl StagedFile for DiscardFile {
    fn write_all(&mut self, _bytes: &[u8]) -> Result<(), HistoryExportFsError> {
        Ok(())
    }

    fn commit(self: Box<Self>) -> Result<(), HistoryExportFsError> {
        Ok(())
    }
}

/// Single-file JSON export used by the current actor.
pub(crate) async fn run_export<S, P>(
    source: &mut S,
    file: Box<dyn RoomHistoryExportFile>,
    header: &ExportHeader,
    range: &RoomHistoryExportRange,
    own_user_id: &str,
    counters: &Arc<ExportCounters>,
    on_page: P,
) -> Result<(), RoomHistoryExportFailureKind>
where
    S: HistoryPageSource,
    P: AsyncProgress,
{
    run_fetch(
        source,
        FetchOutputs {
            messages: Box::new(SinkFile(file)),
            events: Box::new(DiscardFile),
        },
        header,
        range,
        own_user_id,
        counters,
        &StopFlag::default(),
        on_page,
    )
    .await
    .map(|_| ())
    .map_err(|failure| match failure {
        FetchFailure::Network => RoomHistoryExportFailureKind::Network,
        FetchFailure::Sdk | FetchFailure::Stopped => RoomHistoryExportFailureKind::Sdk,
        FetchFailure::Write(_) => RoomHistoryExportFailureKind::Write,
    })
}

/// Progress callback for [`run_export`].
pub(crate) trait AsyncProgress: Send {
    fn page_completed(
        &mut self,
        progress: RoomHistoryExportProgress,
    ) -> impl Future<Output = ()> + Send;
}
