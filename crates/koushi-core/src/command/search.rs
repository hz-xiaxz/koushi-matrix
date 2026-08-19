use super::*;

pub enum SearchCommand {
    Query {
        request_id: RequestId,
        query: String,
        scope: SearchScope,
        room_filter: SearchRoomFilter,
    },
    Attachments {
        request_id: RequestId,
        scope: AttachmentScope,
        filter: AttachmentFilter,
        sort: AttachmentSort,
    },
    StartHistoryCrawl {
        request_id: RequestId,
        room_id: String,
        settings: koushi_state::SearchCrawlerSettings,
    },
    StopHistoryCrawl {
        request_id: RequestId,
        room_id: String,
    },
}

#[derive(Clone, Debug)]
pub enum ThreadsListCommand {
    Open {
        request_id: RequestId,
        scope: koushi_state::ThreadsListScope,
        room_ids: Vec<String>,
    },
    Close {
        request_id: RequestId,
    },
    Paginate {
        request_id: RequestId,
        scope: koushi_state::ThreadsListScope,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchScope {
    AllRooms,
    CurrentRoom { room_id: String },
    CurrentSpace { space_id: String },
}

fn search_room_filter_debug(filter: &SearchRoomFilter) -> (&'static str, usize) {
    match filter {
        SearchRoomFilter::AllRooms => ("all_rooms", 0),
        SearchRoomFilter::OnlyRooms(room_ids) => ("only_rooms", room_ids.len()),
    }
}

// Search queries can quote message content; redact like bodies.
impl fmt::Debug for SearchCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query {
                request_id,
                scope,
                room_filter,
                ..
            } => formatter
                .debug_struct("Query")
                .field("request_id", request_id)
                .field("query", &"SearchQuery(..)")
                .field("scope", scope)
                .field("room_filter", &search_room_filter_debug(room_filter))
                .finish(),
            Self::Attachments {
                request_id,
                scope,
                filter,
                sort,
            } => formatter
                .debug_struct("Attachments")
                .field("request_id", request_id)
                .field("scope", scope)
                .field("filter", filter)
                .field("sort", sort)
                .finish(),
            Self::StartHistoryCrawl {
                request_id,
                room_id: _,
                settings,
            } => formatter
                .debug_struct("StartHistoryCrawl")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("settings", settings)
                .finish(),
            Self::StopHistoryCrawl {
                request_id,
                room_id: _,
            } => formatter
                .debug_struct("StopHistoryCrawl")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fake_rid;
    use super::*;
    use koushi_state::{
        ImageUploadCompressionMode, MentionIntent, MentionTarget, NativeAttentionCandidate,
        NativeAttentionCapabilities, NativeAttentionCapability, NativeAttentionDispatchState,
        NativeAttentionState, NativeAttentionSummary, NativeAttentionSuppressionReason,
        RoomAttentionKind, ThreadOpenIntent,
    };

    #[test]
    fn open_thread_command_retains_typed_intent_and_redacts_identifiers() {
        let request_id = fake_rid(75);
        let command = AppCommand::OpenThread {
            request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            root_event_id: "$private-root:example.invalid".to_owned(),
            intent: ThreadOpenIntent::NewThreadDraft,
        };

        assert_eq!(CoreCommand::App(command).request_id(), request_id);
        let debug = format!(
            "{:?}",
            AppCommand::OpenThread {
                request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                root_event_id: "$private-root:example.invalid".to_owned(),
                intent: ThreadOpenIntent::NewThreadDraft,
            }
        );
        assert!(debug.contains("NewThreadDraft"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
        assert!(!debug.contains("$private-root:example.invalid"), "{debug}");
    }
}
