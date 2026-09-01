use std::fmt;

use koushi_state::{
    ActivityStream, ActivityTab, NativeAttentionDispatchId, NativeAttentionSummary,
};

use serde::{Deserialize, Serialize};

use crate::ids::RequestId;

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ActivityEvent {
    Opened {
        request_id: RequestId,
    },
    Closed {
        request_id: RequestId,
    },
    SnapshotLoaded {
        request_id: RequestId,
        active_tab: ActivityTab,
        recent: ActivityStream,
        unread: ActivityStream,
    },
    TabSelected {
        request_id: RequestId,
        tab: ActivityTab,
    },
    ResolutionRetried {
        request_id: RequestId,
        generation: u64,
    },
    MarkedRead {
        request_id: RequestId,
        cleared_event_ids: Vec<String>,
    },
}
impl fmt::Debug for ActivityEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened { request_id } => formatter
                .debug_struct("ActivityOpened")
                .field("request_id", request_id)
                .finish(),
            Self::Closed { request_id } => formatter
                .debug_struct("ActivityClosed")
                .field("request_id", request_id)
                .finish(),
            Self::SnapshotLoaded {
                request_id,
                active_tab,
                recent,
                unread,
            } => formatter
                .debug_struct("ActivitySnapshotLoaded")
                .field("request_id", request_id)
                .field("active_tab", active_tab)
                .field("recent", recent)
                .field("unread", unread)
                .finish(),
            Self::TabSelected { request_id, tab } => formatter
                .debug_struct("ActivityTabSelected")
                .field("request_id", request_id)
                .field("tab", tab)
                .finish(),
            Self::ResolutionRetried {
                request_id,
                generation,
            } => formatter
                .debug_struct("ActivityResolutionRetried")
                .field("request_id", request_id)
                .field("generation", generation)
                .finish(),
            Self::MarkedRead {
                request_id,
                cleared_event_ids,
            } => formatter
                .debug_struct("ActivityMarkedRead")
                .field("request_id", request_id)
                .field(
                    "cleared_event_ids",
                    &format_args!("{} event id(s)", cleared_event_ids.len()),
                )
                .finish(),
        }
    }
}
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeAttentionEvent {
    SummaryUpdated {
        summary: NativeAttentionSummary,
    },
    DispatchAdmission {
        dispatch_id: NativeAttentionDispatchId,
        accepted: bool,
    },
}
impl fmt::Debug for NativeAttentionEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SummaryUpdated { summary } => formatter
                .debug_struct("SummaryUpdated")
                .field("unread_count", &summary.unread_count)
                .field("highlight_count", &summary.highlight_count)
                .field("badge_count", &summary.badge_count)
                .field(
                    "candidate",
                    &summary.candidate.as_ref().map(|_| "AttentionCandidate(..)"),
                )
                .finish(),
            Self::DispatchAdmission { accepted, .. } => formatter
                .debug_struct("DispatchAdmission")
                .field("dispatch_id", &"NativeAttentionDispatchId(..)")
                .field("accepted", accepted)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fake_rid;
    use super::*;
    fn activity_row(room_id: &str, event_id: &str, timestamp_ms: u64) -> koushi_state::ActivityRow {
        koushi_state::ActivityRow::event(
            room_id.to_owned(),
            event_id.to_owned(),
            Some("@private:sender".to_owned()),
            "Private Room".to_owned(),
            Some("Private Sender".to_owned()),
            Some("private message body".to_owned()),
            timestamp_ms,
            true,
            false,
        )
    }

    fn activity_stream(rows: Vec<koushi_state::ActivityRow>) -> koushi_state::ActivityStream {
        koushi_state::ActivityStream {
            rows,
            next_batch: Some("private-page-token".to_owned()),
            resolution: Default::default(),
        }
    }

    #[test]
    fn activity_events_debug_redacts_rows_targets_and_page_tokens() {
        let snapshot = ActivityEvent::SnapshotLoaded {
            request_id: fake_rid(1),
            active_tab: koushi_state::ActivityTab::Recent,
            recent: activity_stream(vec![activity_row(
                "!private-room:example.invalid",
                "$private-event:example.invalid",
                20,
            )]),
            unread: activity_stream(vec![activity_row(
                "!private-room:example.invalid",
                "$private-unread:example.invalid",
                10,
            )]),
        };
        let marked = ActivityEvent::MarkedRead {
            request_id: fake_rid(2),
            cleared_event_ids: vec!["$private-event:example.invalid".to_owned()],
        };

        for debug in [format!("{snapshot:?}"), format!("{marked:?}")] {
            assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
            assert!(!debug.contains("$private-event:example.invalid"), "{debug}");
            assert!(
                !debug.contains("$private-unread:example.invalid"),
                "{debug}"
            );
            assert!(!debug.contains("Private Room"), "{debug}");
            assert!(!debug.contains("Private Sender"), "{debug}");
            assert!(!debug.contains("private message body"), "{debug}");
            assert!(!debug.contains("private-page-token"), "{debug}");
        }
    }
}
