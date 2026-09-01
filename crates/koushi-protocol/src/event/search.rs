use std::fmt;

use koushi_state::AttachmentResult;
use serde::{Deserialize, Serialize};

use crate::ids::RequestId;

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchEvent {
    Results {
        request_id: RequestId,
        results: Vec<SearchResultItem>,
    },
    AttachmentsResults {
        request_id: RequestId,
        results: Vec<AttachmentResult>,
    },
    AttachmentsFailed {
        request_id: RequestId,
        message: String,
    },
    /// The encrypted search index applied a document mutation for this event.
    /// Carries only app-owned visible-state identifiers (room/event ids) so
    /// pollers can wake on indexing progress instead of sleeping; the message
    /// body is never included (Security Model — Search).
    IndexUpdated {
        room_id: String,
        event_id: String,
    },
    HistoryCrawlProgress {
        room_id: String,
        processed: u64,
        indexed: u64,
    },
    HistoryCrawlCompleted {
        room_id: String,
        indexed: u64,
    },
    HistoryCrawlFailed {
        room_id: String,
        #[serde(rename = "failureKind")]
        kind: koushi_state::SearchCrawlerFailureKind,
    },
}
impl fmt::Debug for SearchEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchEvent::Results {
                request_id,
                results,
            } => formatter
                .debug_struct("Results")
                .field("request_id", request_id)
                .field("result_count", &results.len())
                .finish(),
            SearchEvent::AttachmentsResults {
                request_id,
                results,
            } => formatter
                .debug_struct("AttachmentsResults")
                .field("request_id", request_id)
                .field("result_count", &results.len())
                .finish(),
            SearchEvent::AttachmentsFailed { request_id, .. } => formatter
                .debug_struct("AttachmentsFailed")
                .field("request_id", request_id)
                .field("message", &"SearchFailure(..)")
                .finish(),
            SearchEvent::IndexUpdated { .. } => formatter
                .debug_struct("IndexUpdated")
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            SearchEvent::HistoryCrawlProgress {
                room_id: _,
                processed,
                indexed,
            } => formatter
                .debug_struct("HistoryCrawlProgress")
                .field("room_id", &"RoomId(..)")
                .field("processed", processed)
                .field("indexed", indexed)
                .finish(),
            SearchEvent::HistoryCrawlCompleted {
                room_id: _,
                indexed,
            } => formatter
                .debug_struct("HistoryCrawlCompleted")
                .field("room_id", &"RoomId(..)")
                .field("indexed", indexed)
                .finish(),
            SearchEvent::HistoryCrawlFailed { kind, .. } => formatter
                .debug_struct("HistoryCrawlFailed")
                .field("room_id", &"RoomId(..)")
                .field("kind", kind)
                .finish(),
        }
    }
}
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub room_id: String,
    pub event_id: String,
    pub snippet: String,
}
impl fmt::Debug for SearchResultItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchResultItem")
            .field("room_id", &"RoomId(..)")
            .field("event_id", &"EventId(..)")
            .field("snippet", &"Snippet(..)")
            .finish()
    }
}
