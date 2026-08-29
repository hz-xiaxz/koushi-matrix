use std::fmt;

use koushi_state::{AttachmentFilter, AttachmentScope, AttachmentSort, SearchRoomFilter};

use crate::ids::RequestId;

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

impl SearchScope {
    pub(crate) fn to_state(&self) -> koushi_state::SearchScope {
        match self {
            Self::AllRooms => koushi_state::SearchScope::AllRooms,
            Self::CurrentRoom { room_id } => koushi_state::SearchScope::CurrentRoom {
                room_id: room_id.clone(),
            },
            Self::CurrentSpace { space_id } => koushi_state::SearchScope::CurrentSpace {
                space_id: space_id.clone(),
            },
        }
    }
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
