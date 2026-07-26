use serde::{Deserialize, Serialize};

// SearchCrawlerState, SearchCrawlerRoomState, and SearchCrawlerFailureKind live
// in state/search_crawler.rs and are re-exported from mod.rs.

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchState {
    Closed,
    Editing {
        query: String,
        scope: SearchScope,
    },
    TooShort {
        request_id: u64,
        query: String,
        scope: SearchScope,
        min_chars: u8,
    },
    Searching {
        request_id: u64,
        query: String,
        scope: SearchScope,
    },
    Results {
        request_id: u64,
        query: String,
        scope: SearchScope,
        results: Vec<SearchResult>,
    },
    Failed {
        request_id: u64,
        query: String,
        scope: SearchScope,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchScope {
    CurrentRoom { room_id: String },
    CurrentSpace { space_id: String },
    Dms,
    AllRooms,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchRoomFilter {
    AllRooms,
    OnlyRooms(Vec<String>),
}

impl SearchRoomFilter {
    pub fn contains(&self, room_id: &str) -> bool {
        match self {
            Self::AllRooms => true,
            Self::OnlyRooms(room_ids) => room_ids.iter().any(|candidate| candidate == room_id),
        }
    }
}

pub fn search_min_chars(query: &str) -> u8 {
    if query.trim().chars().any(is_cjk_search_char) {
        2
    } else {
        3
    }
}

pub fn search_query_too_short(query: &str) -> Option<u8> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let min_chars = search_min_chars(query);
    if query.chars().count() < min_chars as usize {
        Some(min_chars)
    } else {
        None
    }
}

fn is_cjk_search_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{30ff}'
            | '\u{3400}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ac00}'..='\u{d7af}'
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub room_id: String,
    pub event_id: String,
    #[serde(default)]
    pub context_label: Option<String>,
    pub sender: String,
    pub timestamp_ms: u64,
    pub score_millis: u32,
    pub snippet: String,
    pub match_field: SearchMatchField,
    pub highlights: Vec<TextRange>,
    pub match_kind: SearchMatchKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextRange {
    /// Half-open range in UTF-16 code units relative to `SearchResult::snippet`.
    pub start_utf16: u32,
    pub end_utf16: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchMatchKind {
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchMatchField {
    MessageBody,
    AttachmentFileName,
}
