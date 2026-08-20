use crate::{MatrixRoomListSnapshot, MatrixSearchCandidate, MatrixTimelineItem};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomListSmokeReport {
    pub rooms: usize,
    pub spaces: usize,
    pub dms: usize,
    pub unread_rooms: usize,
}

impl std::fmt::Display for RoomListSmokeReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rooms={} spaces={} dms={} unread_rooms={}",
            self.rooms, self.spaces, self.dms, self.unread_rooms
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TimelineSmokeReport {
    pub selected_room_present: bool,
    pub timeline_items: usize,
}

impl std::fmt::Display for TimelineSmokeReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "selected_room_present={} timeline_items={}",
            self.selected_room_present, self.timeline_items
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchSmokeReport {
    pub invoked: bool,
    pub candidates: usize,
}

impl std::fmt::Display for SearchSmokeReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "search_invoked={} search_candidates={}",
            self.invoked, self.candidates
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RealAccountQaReport {
    pub room_list: RoomListSmokeReport,
    pub timeline: TimelineSmokeReport,
    pub session_restored: bool,
    pub search: SearchSmokeReport,
}

impl std::fmt::Display for RealAccountQaReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} {} session_restored={} {}",
            self.room_list, self.timeline, self.session_restored, self.search
        )
    }
}

pub fn room_list_smoke_report(snapshot: &MatrixRoomListSnapshot) -> RoomListSmokeReport {
    RoomListSmokeReport {
        rooms: snapshot.rooms.len(),
        spaces: snapshot.spaces.len(),
        dms: snapshot.rooms.iter().filter(|room| room.is_dm).count(),
        unread_rooms: snapshot
            .rooms
            .iter()
            .filter(|room| room.unread_count > 0)
            .count(),
    }
}

pub fn timeline_smoke_report(
    selected_room_present: bool,
    initial_items: &[MatrixTimelineItem],
) -> TimelineSmokeReport {
    TimelineSmokeReport {
        selected_room_present,
        timeline_items: initial_items.len(),
    }
}

pub fn real_account_qa_report(
    snapshot: &MatrixRoomListSnapshot,
    selected_room_present: bool,
    initial_items: &[MatrixTimelineItem],
) -> RealAccountQaReport {
    real_account_qa_report_with_restore_state(snapshot, selected_room_present, initial_items, false)
}

pub fn restored_real_account_qa_report(
    snapshot: &MatrixRoomListSnapshot,
    selected_room_present: bool,
    initial_items: &[MatrixTimelineItem],
) -> RealAccountQaReport {
    real_account_qa_report_with_restore_state(snapshot, selected_room_present, initial_items, true)
}

pub fn real_account_qa_report_with_search(
    snapshot: &MatrixRoomListSnapshot,
    selected_room_present: bool,
    initial_items: &[MatrixTimelineItem],
    session_restored: bool,
    search_candidates: &[MatrixSearchCandidate],
) -> RealAccountQaReport {
    let mut report = real_account_qa_report_with_restore_state(
        snapshot,
        selected_room_present,
        initial_items,
        session_restored,
    );
    report.search = search_smoke_report(true, search_candidates);
    report
}

pub fn search_smoke_report(
    invoked: bool,
    candidates: &[MatrixSearchCandidate],
) -> SearchSmokeReport {
    SearchSmokeReport {
        invoked,
        candidates: candidates.len(),
    }
}

fn real_account_qa_report_with_restore_state(
    snapshot: &MatrixRoomListSnapshot,
    selected_room_present: bool,
    initial_items: &[MatrixTimelineItem],
    session_restored: bool,
) -> RealAccountQaReport {
    RealAccountQaReport {
        room_list: room_list_smoke_report(snapshot),
        timeline: timeline_smoke_report(selected_room_present, initial_items),
        session_restored,
        search: SearchSmokeReport::default(),
    }
}
