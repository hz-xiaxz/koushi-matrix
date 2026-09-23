//! Paging and streaming loop for one room-history export.
//!
//! History is read oldest-first with `/messages?dir=f` starting at the first
//! event visible to this account, so events are written in Element's order
//! without buffering the room. Period exports still walk the whole visible
//! history: event order is topological and `origin_server_ts` is
//! sender-controlled, so stopping at the first event past the end of the
//! period could drop later in-range events.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use koushi_state::{
    RoomHistoryExportFailureKind, RoomHistoryExportProgress, RoomHistoryExportRange,
};

use super::element::{
    ElementJsonWriter, ExportHeader, ExportSourceEvent, effective_event, element_renders,
};
use super::sink::{RoomHistoryExportFile, RoomHistoryExportSinkError};

/// Events requested per `/messages` page.
pub(crate) const PAGE_LIMIT: u32 = 250;

/// Consecutive empty pages that may still carry a new token. Servers return
/// empty filtered pages across events hidden by history visibility, so a run
/// is tolerated, but an export that exceeds this bound fails rather than
/// committing a file that might silently omit later history.
pub(crate) const MAX_CONSECUTIVE_EMPTY_PAGES: u32 = 200;

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

fn sink_failure(_: RoomHistoryExportSinkError) -> RoomHistoryExportFailureKind {
    RoomHistoryExportFailureKind::Write
}

fn page_failure(error: HistoryPageError) -> RoomHistoryExportFailureKind {
    match error {
        HistoryPageError::Network => RoomHistoryExportFailureKind::Network,
        HistoryPageError::Sdk => RoomHistoryExportFailureKind::Sdk,
    }
}

/// Export the room into `file`, committing it only when the history is
/// exhausted. `on_page` receives the counters after every page.
pub(crate) async fn run_export<S, P>(
    source: &mut S,
    file: Box<dyn RoomHistoryExportFile>,
    header: &ExportHeader,
    range: &RoomHistoryExportRange,
    own_user_id: &str,
    counters: &Arc<ExportCounters>,
    mut on_page: P,
) -> Result<(), RoomHistoryExportFailureKind>
where
    S: HistoryPageSource,
    P: AsyncProgress,
{
    let mut file = file;
    let (mut writer, head) = ElementJsonWriter::begin(header);
    file.write_all(&head).map_err(sink_failure)?;

    let mut seen_event_ids = HashSet::<String>::new();
    let mut from: Option<String> = None;
    let mut empty_pages = 0_u32;
    loop {
        let page = source.next_page(from.clone()).await.map_err(page_failure)?;
        let page_was_empty = page.events.is_empty();
        for source_event in page.events {
            let event = effective_event(source_event);
            let Some(event_id) = event.event_id() else {
                continue;
            };
            if !seen_event_ids.insert(event_id.to_owned()) {
                continue;
            }
            counters.fetched.fetch_add(1, Ordering::AcqRel);
            if !event
                .origin_server_ts()
                .is_some_and(|timestamp| range.contains(timestamp))
            {
                continue;
            }
            if !element_renders(&event, own_user_id) {
                continue;
            }
            file.write_all(&writer.event(&event.json))
                .map_err(sink_failure)?;
            counters.exported.fetch_add(1, Ordering::AcqRel);
            if event.undecryptable {
                counters.undecryptable.fetch_add(1, Ordering::AcqRel);
            }
        }
        on_page.page_completed(counters.snapshot()).await;

        empty_pages = if page_was_empty { empty_pages + 1 } else { 0 };
        match page.end {
            Some(end) if from.as_deref() != Some(end.as_str()) => {
                if empty_pages >= MAX_CONSECUTIVE_EMPTY_PAGES {
                    return Err(RoomHistoryExportFailureKind::Sdk);
                }
                from = Some(end);
            }
            _ => break,
        }
    }

    file.write_all(&writer.finish()).map_err(sink_failure)?;
    file.commit().map_err(sink_failure)
}

/// Progress callback for [`run_export`].
pub(crate) trait AsyncProgress: Send {
    fn page_completed(
        &mut self,
        progress: RoomHistoryExportProgress,
    ) -> impl Future<Output = ()> + Send;
}
