use std::collections::{BTreeMap, BTreeSet, HashMap};

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_state::{
    ActivityMarkReadTarget, ActivityResolutionState, ActivityRow, ActivityRowKind, ActivityState,
    ActivityStream, ActivityTab, AppAction, AppState, OperationFailureKind, RoomLatestEventSummary,
    RoomNotificationMode, RoomSummary, SpaceSummary,
};

use super::{ActivityResolutionRequest, RequestId, unread_trace};

const MAX_ACTIVITY_RESOLUTION_ROOMS: usize = 16;

pub const ACTIVITY_RECENT_MAX_ROWS: usize = 200;

pub(super) fn activity_tab_token(tab: ActivityTab) -> &'static str {
    match tab {
        ActivityTab::Recent => "recent",
        ActivityTab::Unread => "unread",
    }
}

pub(super) fn record_activity_transition(
    stage: &'static str,
    request_id: RequestId,
    outcome: &'static str,
    previous_tab: ActivityTab,
    selected_tab: ActivityTab,
) {
    record(
        DiagnosticEvent::new(DiagnosticLevel::Info, "core.activity", stage)
            .field(DiagnosticField::request_id(
                "request_id",
                request_id.connection_id.0,
                request_id.sequence,
            ))
            .field(DiagnosticField::token("outcome", outcome))
            .field(DiagnosticField::token(
                "previous_tab",
                activity_tab_token(previous_tab),
            ))
            .field(DiagnosticField::token(
                "selected_tab",
                activity_tab_token(selected_tab),
            )),
    );
}

#[derive(Default)]
pub(super) struct ActivityProjection {
    rows_by_event_id: BTreeMap<String, ActivityRow>,
    cleared_event_ids: BTreeSet<String>,
    /// Rooms whose placeholder unread row has just been cleared by a local
    /// mark-read. Suppresses re-synthesizing the placeholder until the reducer
    /// has had a chance to zero out the room's unread counts.
    cleared_placeholder_room_ids: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct ActivityMarkReadResult {
    pub(super) cleared_event_ids: Vec<String>,
    pub(super) cleared_placeholder_room_ids: Vec<String>,
}

fn activity_latest_display_event_id(latest: &RoomLatestEventSummary) -> Option<&str> {
    match latest.relation_type.as_deref() {
        Some("m.replace") => latest
            .relation_event_id
            .as_deref()
            .filter(|event_id| !event_id.trim().is_empty()),
        Some("m.annotation") => None,
        _ => Some(latest.event_id.as_str()),
    }
}

impl ActivityProjection {
    pub(super) fn ingest(&mut self, rows: Vec<ActivityRow>) {
        for mut row in rows {
            if row.kind != ActivityRowKind::Event
                || row
                    .event_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                || row.room_id.trim().is_empty()
            {
                continue;
            }
            row.room_label.clear();
            row.unread = false;
            if let Some(event_id) = row.event_id.clone() {
                self.rows_by_event_id.insert(event_id, row);
            }
        }
    }

    pub(super) fn mark_read(
        &mut self,
        state: &AppState,
        target: &ActivityMarkReadTarget,
    ) -> ActivityMarkReadResult {
        let (_recent, unread, _excluded) = self.snapshot(state);
        let mut cleared_event_ids = Vec::new();
        let mut cleared_placeholder_room_ids = Vec::new();
        let mut cleared_event_row_room_ids = BTreeSet::new();
        match target {
            ActivityMarkReadTarget::All => {
                for row in unread.rows {
                    match row.kind {
                        ActivityRowKind::Event => {
                            if let Some(event_id) = row.event_id {
                                cleared_event_ids.push(event_id);
                                cleared_event_row_room_ids.insert(row.room_id);
                            }
                        }
                        ActivityRowKind::RoomUnread => {
                            cleared_placeholder_room_ids.push(row.room_id);
                        }
                    }
                }
            }
            ActivityMarkReadTarget::Room {
                room_id,
                up_to_event_id,
            } => {
                let target_timestamp = unread
                    .rows
                    .iter()
                    .find(|row| {
                        row.room_id == *room_id
                            && row.event_id.as_deref() == Some(up_to_event_id.as_str())
                    })
                    .map(|row| row.timestamp_ms);
                for row in unread.rows {
                    if row.room_id != *room_id {
                        continue;
                    }
                    let matches_timestamp = target_timestamp
                        .map(|timestamp| row.timestamp_ms <= timestamp)
                        .unwrap_or(true);
                    if !matches_timestamp {
                        continue;
                    }
                    match row.kind {
                        ActivityRowKind::Event => {
                            if let Some(event_id) = row.event_id {
                                cleared_event_ids.push(event_id);
                                cleared_event_row_room_ids.insert(row.room_id);
                            }
                        }
                        ActivityRowKind::RoomUnread => {
                            cleared_placeholder_room_ids.push(row.room_id);
                        }
                    }
                }
            }
        }
        for event_id in &cleared_event_ids {
            self.cleared_event_ids.insert(event_id.clone());
        }
        for room_id in &cleared_placeholder_room_ids {
            self.cleared_placeholder_room_ids.insert(room_id.clone());
        }
        // Suppress placeholder synthesis for rooms whose event rows are being
        // cleared, until the reducer has zeroed out the room's unread counts.
        for room_id in cleared_event_row_room_ids {
            self.cleared_placeholder_room_ids.insert(room_id);
        }
        ActivityMarkReadResult {
            cleared_event_ids,
            cleared_placeholder_room_ids,
        }
    }

    pub(super) fn fully_read_marker_updates(
        &mut self,
        state: &AppState,
        target: &ActivityMarkReadTarget,
    ) -> Vec<(String, String)> {
        match target {
            ActivityMarkReadTarget::Room {
                room_id,
                up_to_event_id,
            } => vec![(room_id.clone(), up_to_event_id.clone())],
            ActivityMarkReadTarget::All => {
                let (_recent, unread, _excluded) = self.snapshot(state);
                let rooms_by_id = state
                    .rooms
                    .iter()
                    .map(|room| (room.room_id.as_str(), room))
                    .collect::<HashMap<_, _>>();
                let mut latest_by_room: BTreeMap<String, (u64, String)> = BTreeMap::new();
                for row in unread.rows {
                    let event_id = match row.kind {
                        ActivityRowKind::Event => row.event_id,
                        ActivityRowKind::RoomUnread => rooms_by_id
                            .get(row.room_id.as_str())
                            .and_then(|room| room.latest_event.as_ref())
                            .map(|event| event.event_id.clone()),
                    };
                    if let Some(event_id) = event_id {
                        latest_by_room
                            .entry(row.room_id)
                            .and_modify(|(timestamp_ms, existing_event_id)| {
                                if row.timestamp_ms > *timestamp_ms {
                                    *timestamp_ms = row.timestamp_ms;
                                    *existing_event_id = event_id.clone();
                                }
                            })
                            .or_insert((row.timestamp_ms, event_id));
                    }
                }
                latest_by_room
                    .into_iter()
                    .map(|(room_id, (_timestamp_ms, event_id))| (room_id, event_id))
                    .collect()
            }
        }
    }

    pub(super) fn event_at_or_after(&self, room_id: &str, timestamp_ms: u64) -> Option<String> {
        let mut rows = self
            .rows_by_event_id
            .values()
            .filter(|row| row.room_id == room_id)
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.timestamp_ms
                .cmp(&right.timestamp_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });

        rows.iter()
            .find(|row| row.timestamp_ms >= timestamp_ms)
            .or_else(|| rows.last())
            .filter(|row| row.kind == ActivityRowKind::Event)
            .and_then(|row| row.event_id.clone())
    }

    pub(super) fn update_action_for_open_state(&mut self, state: &AppState) -> Option<AppAction> {
        if !matches!(state.activity, ActivityState::Open { .. }) {
            return None;
        }
        let (mut recent, mut unread, excluded_room_ids) = self.snapshot(state);
        if let ActivityState::Open {
            recent: current_recent,
            unread: current_unread,
            ..
        } = &state.activity
        {
            recent.resolution = current_recent.resolution;
            unread.resolution = current_unread.resolution;
        }
        Some(AppAction::ActivityRowsUpdated {
            recent,
            unread,
            excluded_room_ids,
        })
    }

    pub(super) fn room_ids_without_remaining_unread(
        &mut self,
        state: &AppState,
        cleared_event_ids: &[String],
    ) -> Vec<String> {
        let affected_room_ids = cleared_event_ids
            .iter()
            .filter_map(|event_id| self.rows_by_event_id.get(event_id))
            .map(|row| row.room_id.clone())
            .collect::<BTreeSet<_>>();
        if affected_room_ids.is_empty() {
            return Vec::new();
        }

        let (_recent, unread, _excluded_room_ids) = self.snapshot(state);
        let remaining_unread_room_ids = unread
            .rows
            .into_iter()
            .map(|row| row.room_id)
            .collect::<BTreeSet<_>>();
        affected_room_ids
            .into_iter()
            .filter(|room_id| !remaining_unread_room_ids.contains(room_id))
            .collect()
    }

    pub(super) fn snapshot(
        &mut self,
        state: &AppState,
    ) -> (ActivityStream, ActivityStream, Vec<String>) {
        let rooms_by_id: HashMap<&str, &RoomSummary> = state
            .rooms
            .iter()
            .map(|room| (room.room_id.as_str(), room))
            .collect();
        let excluded_room_ids = state
            .rooms
            .iter()
            .filter(|room| {
                room.tags.low_priority.is_some()
                    || state
                        .room_notification_settings
                        .get(&room.room_id)
                        .is_some_and(|settings| settings.mode == RoomNotificationMode::Mute)
            })
            .map(|room| room.room_id.clone())
            .collect::<Vec<_>>();
        let excluded: BTreeSet<&str> = excluded_room_ids.iter().map(String::as_str).collect();

        let mut recent = Vec::new();
        let mut unread = Vec::new();
        let mut recent_event_ids = BTreeSet::new();
        let mut unread_event_ids = BTreeSet::new();
        let mut unread_event_room_ids = BTreeSet::new();
        for row in self.rows_by_event_id.values() {
            if excluded.contains(row.room_id.as_str()) {
                continue;
            }
            let Some(room) = rooms_by_id.get(row.room_id.as_str()) else {
                continue;
            };
            let fully_read_event_id = state
                .live_signals
                .rooms
                .get(row.room_id.as_str())
                .and_then(|signals| signals.fully_read_event_id.as_deref());
            let mode = state
                .room_notification_settings
                .get(&room.room_id)
                .map(|settings| settings.mode);
            let room_activity_unread = room_has_activity_unread(room, mode);
            let unread_by_marker = room_activity_unread
                && match fully_read_event_id {
                    Some(event_id) => match row.event_id.as_deref() {
                        Some(row_event_id) if row_event_id == event_id => false,
                        Some(_) => self
                            .rows_by_event_id
                            .get(event_id)
                            .map(|fully_read_row| row.timestamp_ms > fully_read_row.timestamp_ms)
                            .unwrap_or(room_activity_unread),
                        None => false,
                    },
                    None => true,
                };
            let unread_row = unread_by_marker
                && !self
                    .cleared_event_ids
                    .contains(row.event_id.as_deref().unwrap_or(""));
            if unread_by_marker {
                if let Some(event_id) = row.event_id.clone() {
                    unread_event_ids.insert(event_id);
                }
            }
            if !activity_recent_row_visible(mode, row.highlight, room_activity_unread) {
                continue;
            }
            let sender_avatar = row
                .sender_id
                .as_ref()
                .and_then(|user_id| state.profile.users.get(user_id))
                .and_then(|profile| profile.avatar.clone())
                .or_else(|| row.sender_avatar.clone());
            let context_label = activity_row_context_label(room, &state.spaces);
            let row = ActivityRow {
                room_label: room.display_label.clone(),
                sender_avatar,
                context_label,
                unread: unread_row,
                highlight: row.highlight || (unread_row && room.highlight_count > 0),
                ..row.clone()
            };
            if let Some(event_id) = row.event_id.clone() {
                recent_event_ids.insert(event_id);
            }
            if row.unread {
                unread_event_room_ids.insert(row.room_id.clone());
                unread.push(row.clone());
            }
            recent.push(row);
        }

        for room in state.rooms.iter() {
            if excluded.contains(room.room_id.as_str()) {
                continue;
            }
            let Some(latest_event) = &room.latest_event else {
                continue;
            };
            let Some(display_event_id) = activity_latest_display_event_id(latest_event) else {
                continue;
            };
            if recent_event_ids.contains(display_event_id) {
                continue;
            }
            let fully_read_event_id = state
                .live_signals
                .rooms
                .get(room.room_id.as_str())
                .and_then(|signals| signals.fully_read_event_id.as_deref());
            let mode = state
                .room_notification_settings
                .get(&room.room_id)
                .map(|settings| settings.mode);
            let room_activity_unread = room_has_activity_unread(room, mode);
            let has_room_metrics = room.unread_count > 0 || room_activity_unread;
            let unread_row = room_activity_unread
                && fully_read_event_id != Some(display_event_id)
                && !self.cleared_event_ids.contains(display_event_id);
            if has_room_metrics {
                let reason = if !room_activity_unread {
                    "plain_unread_only"
                } else if unread_row {
                    "unread"
                } else if fully_read_event_id == Some(display_event_id) {
                    "fully_read_latest"
                } else {
                    "cleared_latest"
                };
                unread_trace::trace_activity_room(
                    "activity_recent_event",
                    room,
                    mode,
                    unread_row,
                    reason,
                );
            }
            let latest_event_highlight = unread_row && room.highlight_count > 0;
            if !activity_recent_row_visible(mode, latest_event_highlight, room_activity_unread) {
                continue;
            }
            let context_label = activity_row_context_label(room, &state.spaces);
            let mut row = ActivityRow::event(
                room.room_id.clone(),
                display_event_id.to_owned(),
                latest_event.sender_id.clone(),
                room.display_label.clone(),
                latest_event.sender_label.clone(),
                latest_event.preview.clone(),
                latest_event.timestamp_ms,
                unread_row,
                latest_event_highlight,
            );
            row.sender_avatar = latest_event.sender_avatar.clone();
            row.context_label = context_label;
            if row.unread {
                unread_event_room_ids.insert(row.room_id.clone());
                unread.push(row.clone());
            }
            recent.push(row);
        }

        for room in state.rooms.iter() {
            if excluded.contains(room.room_id.as_str()) {
                continue;
            }
            let mode = state
                .room_notification_settings
                .get(&room.room_id)
                .map(|settings| settings.mode);
            let has_room_metrics = room.unread_count > 0 || room_has_activity_unread(room, mode);
            if !has_room_metrics {
                continue;
            }
            if !room_has_activity_unread(room, mode) {
                unread_trace::trace_activity_room(
                    "activity_placeholder",
                    room,
                    mode,
                    false,
                    "plain_unread_only",
                );
                continue;
            }
            let latest_display_event_id = room
                .latest_event
                .as_ref()
                .and_then(activity_latest_display_event_id);
            let fully_read_event_id = state
                .live_signals
                .rooms
                .get(room.room_id.as_str())
                .and_then(|signals| signals.fully_read_event_id.as_deref());
            if latest_display_event_id.is_some_and(|event_id| {
                self.cleared_event_ids.contains(event_id) || fully_read_event_id == Some(event_id)
            }) {
                unread_trace::trace_activity_room(
                    "activity_placeholder",
                    room,
                    mode,
                    false,
                    "latest_event_read",
                );
                continue;
            }
            if self.cleared_placeholder_room_ids.contains(&room.room_id) {
                unread_trace::trace_activity_room(
                    "activity_placeholder",
                    room,
                    mode,
                    false,
                    "cleared_local",
                );
                continue;
            }
            if unread_event_room_ids.contains(&room.room_id) {
                continue;
            }
            let highlight = room.highlight_count > 0;
            let timestamp_ms = room
                .latest_event
                .as_ref()
                .map(|event| event.timestamp_ms)
                .or_else(|| {
                    room.conversation_activity
                        .map(|activity| activity.timestamp_ms)
                })
                .unwrap_or_default();
            let context_label = activity_row_context_label(room, &state.spaces);
            let placeholder = ActivityRow::room_unread_placeholder(
                room.room_id.clone(),
                room.display_label.clone(),
                timestamp_ms,
                highlight,
            );
            let placeholder = ActivityRow {
                context_label,
                ..placeholder
            };
            unread_trace::trace_activity_room(
                "activity_placeholder",
                room,
                mode,
                true,
                "room_metrics",
            );
            unread.push(placeholder);
        }

        self.cleared_placeholder_room_ids.retain(|room_id| {
            rooms_by_id
                .get(room_id.as_str())
                .map(|room| {
                    let mode = state
                        .room_notification_settings
                        .get(&room.room_id)
                        .map(|settings| settings.mode);
                    room_has_activity_unread(room, mode)
                })
                .unwrap_or(false)
        });

        sort_activity_rows(&mut recent);
        sort_activity_rows(&mut unread);

        let recent_retained_event_ids = recent
            .iter()
            .take(ACTIVITY_RECENT_MAX_ROWS)
            .filter_map(|row| row.event_id.clone())
            .collect::<BTreeSet<_>>();
        let marker_event_ids = state
            .live_signals
            .rooms
            .values()
            .filter_map(|signals| signals.fully_read_event_id.as_deref())
            .filter(|event_id| self.rows_by_event_id.contains_key(*event_id))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let reconciliation_event_ids = self
            .cleared_event_ids
            .intersection(&unread_event_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let stored_before = self.rows_by_event_id.len();
        let mut retained_event_ids = recent_retained_event_ids.clone();
        retained_event_ids.extend(unread_event_ids.iter().cloned());
        retained_event_ids.extend(marker_event_ids.iter().cloned());
        retained_event_ids.extend(reconciliation_event_ids.iter().cloned());
        self.rows_by_event_id
            .retain(|event_id, _| retained_event_ids.contains(event_id));
        self.cleared_event_ids
            .retain(|event_id| reconciliation_event_ids.contains(event_id));
        let evicted = stored_before.saturating_sub(self.rows_by_event_id.len());
        if evicted > 0 || stored_before > ACTIVITY_RECENT_MAX_ROWS {
            record(
                DiagnosticEvent::new(DiagnosticLevel::Debug, "core.activity", "projection_pruned")
                    .field(DiagnosticField::count("observed", stored_before as u64))
                    .field(DiagnosticField::count(
                        "stored_before",
                        stored_before as u64,
                    ))
                    .field(DiagnosticField::count(
                        "stored_after",
                        self.rows_by_event_id.len() as u64,
                    ))
                    .field(DiagnosticField::count(
                        "recent_returned",
                        recent.len().min(ACTIVITY_RECENT_MAX_ROWS) as u64,
                    ))
                    .field(DiagnosticField::count(
                        "unread_returned",
                        unread.len() as u64,
                    ))
                    .field(DiagnosticField::count(
                        "marker_retained",
                        marker_event_ids.intersection(&retained_event_ids).count() as u64,
                    ))
                    .field(DiagnosticField::count(
                        "reconciliation_retained",
                        reconciliation_event_ids.len() as u64,
                    ))
                    .field(DiagnosticField::count("evicted", evicted as u64)),
            );
        }
        recent.truncate(ACTIVITY_RECENT_MAX_ROWS);

        (
            ActivityStream {
                rows: recent,
                next_batch: None,
                resolution: Default::default(),
            },
            ActivityStream {
                rows: unread,
                next_batch: None,
                resolution: Default::default(),
            },
            excluded_room_ids,
        )
    }
}

fn room_has_activity_unread(room: &RoomSummary, mode: Option<RoomNotificationMode>) -> bool {
    room_activity_unread_count_for_mode(room, mode) > 0
}

fn room_activity_unread_count_for_mode(
    room: &RoomSummary,
    mode: Option<RoomNotificationMode>,
) -> u64 {
    if matches!(mode, Some(RoomNotificationMode::Mentions)) && room.highlight_count == 0 {
        0
    } else {
        let count = room.notification_count.max(room.highlight_count);
        if count > 0 {
            count
        } else if room.marked_unread {
            1
        } else {
            0
        }
    }
}

fn activity_recent_row_visible(
    mode: Option<RoomNotificationMode>,
    row_highlight: bool,
    room_activity_unread: bool,
) -> bool {
    !matches!(mode, Some(RoomNotificationMode::Mentions)) || row_highlight || room_activity_unread
}

fn activity_row_context_label(room: &RoomSummary, spaces: &[SpaceSummary]) -> String {
    if room.is_dm {
        return "DM".to_owned();
    }
    let parent_space = room
        .parent_space_ids
        .iter()
        .filter_map(|space_id| spaces.iter().find(|space| space.space_id == *space_id))
        .next()
        .or_else(|| {
            spaces.iter().find(|space| {
                space
                    .child_room_ids
                    .iter()
                    .any(|room_id| room_id == &room.room_id)
            })
        });
    if let Some(space) = parent_space {
        return format!("{} / {}", space.display_name, room.display_label);
    }
    room.display_label.clone()
}

fn sort_activity_rows(rows: &mut [ActivityRow]) {
    rows.sort_by(|left, right| {
        right
            .timestamp_ms
            .cmp(&left.timestamp_ms)
            .then_with(|| left.room_id.cmp(&right.room_id))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
}

pub(super) fn guard_activity_resolution_completion(
    state: &AppState,
    action: AppAction,
) -> AppAction {
    let (generation, failure_kind) = match &action {
        AppAction::ActivityResolutionSucceeded { generation } => (*generation, None),
        AppAction::ActivityResolutionFailed {
            generation, kind, ..
        } => (*generation, Some(*kind)),
        _ => return action,
    };
    let ActivityState::Open { unread, .. } = &state.activity else {
        return action;
    };
    let ActivityResolutionState::Resolving {
        generation: active_generation,
        ..
    } = unread.resolution
    else {
        return action;
    };
    if active_generation != generation {
        return action;
    }

    let unresolved_room_count = unread
        .rows
        .iter()
        .filter(|row| row.kind == ActivityRowKind::RoomUnread)
        .count() as u32;
    if unresolved_room_count == 0 {
        AppAction::ActivityResolutionSucceeded { generation }
    } else {
        AppAction::ActivityResolutionFailed {
            generation,
            unresolved_room_count,
            kind: failure_kind.unwrap_or(OperationFailureKind::Timeout),
        }
    }
}

pub(super) fn normalize_activity_resolution_action(
    state: &AppState,
    action: AppAction,
) -> Option<AppAction> {
    let AppAction::ActivityResolutionRowsObserved { generation, rows } = action else {
        return Some(action);
    };
    let ActivityState::Open { unread, .. } = &state.activity else {
        return None;
    };
    if !matches!(
        unread.resolution,
        ActivityResolutionState::Resolving { generation: current, .. } if current == generation
    ) {
        return None;
    }
    Some(AppAction::ActivityRowsObserved { rows })
}

pub(super) fn cap_activity_resolution_requests(
    mut requests: Vec<ActivityResolutionRequest>,
    generation: u64,
) -> Vec<ActivityResolutionRequest> {
    if requests.len() > MAX_ACTIVITY_RESOLUTION_ROOMS {
        let room_count = requests.len() as u64;
        let generation_offset = generation.saturating_sub(1) % room_count;
        let batch_width = MAX_ACTIVITY_RESOLUTION_ROOMS as u64 % room_count;
        let start = (generation_offset * batch_width % room_count) as usize;
        requests.rotate_left(start);
    }
    requests.truncate(MAX_ACTIVITY_RESOLUTION_ROOMS);
    requests
}

#[cfg(test)]
mod tests {
    use super::super::tests::unread_diagnostic_room;
    use super::*;
    use koushi_state::{
        RoomNotificationModeOperation, RoomNotificationSettings, RoomTags, UserProfile,
    };
    use std::collections::BTreeSet;

    #[test]
    fn activity_resolution_cannot_succeed_while_room_placeholders_remain() {
        let generation = 7;
        let mut state = AppState::default();
        state.activity = ActivityState::Open {
            active_tab: ActivityTab::Unread,
            recent: ActivityStream::default(),
            unread: ActivityStream {
                rows: vec![ActivityRow {
                    kind: ActivityRowKind::RoomUnread,
                    room_id: "!room:example.invalid".to_owned(),
                    ..ActivityRow::default()
                }],
                next_batch: None,
                resolution: ActivityResolutionState::Resolving {
                    generation,
                    unresolved_room_count: 1,
                },
            },
            mark_read: Default::default(),
        };

        assert_eq!(
            guard_activity_resolution_completion(
                &state,
                AppAction::ActivityResolutionSucceeded { generation },
            ),
            AppAction::ActivityResolutionFailed {
                generation,
                unresolved_room_count: 1,
                kind: OperationFailureKind::Timeout,
            }
        );
    }

    #[test]
    fn activity_resolution_rows_are_generation_guarded() {
        let generation = 7;
        let mut state = AppState::default();
        state.activity = ActivityState::Open {
            active_tab: ActivityTab::Unread,
            recent: ActivityStream::default(),
            unread: ActivityStream {
                rows: Vec::new(),
                next_batch: None,
                resolution: ActivityResolutionState::Resolving {
                    generation,
                    unresolved_room_count: 1,
                },
            },
            mark_read: Default::default(),
        };
        let row = ActivityRow::event(
            "!room:example.invalid".to_owned(),
            "$event:example.invalid".to_owned(),
            None,
            String::new(),
            None,
            None,
            1,
            false,
            false,
        );

        assert!(
            normalize_activity_resolution_action(
                &state,
                AppAction::ActivityResolutionRowsObserved {
                    generation: generation - 1,
                    rows: vec![row.clone()],
                },
            )
            .is_none()
        );
        assert_eq!(
            normalize_activity_resolution_action(
                &state,
                AppAction::ActivityResolutionRowsObserved {
                    generation,
                    rows: vec![row.clone()],
                },
            ),
            Some(AppAction::ActivityRowsObserved { rows: vec![row] })
        );
    }

    #[test]
    fn activity_resolution_request_batch_has_an_account_wide_cap() {
        let requests = (0..(MAX_ACTIVITY_RESOLUTION_ROOMS + 3))
            .map(|index| ActivityResolutionRequest {
                room_id: format!("!room-{index}:example.invalid"),
                fully_read_event_id: None,
                minimum_unread_count: 1,
            })
            .collect::<Vec<_>>();
        let first = cap_activity_resolution_requests(requests.clone(), 1);
        let second = cap_activity_resolution_requests(requests, 2);
        assert_eq!(first.len(), MAX_ACTIVITY_RESOLUTION_ROOMS);
        assert_eq!(second.len(), MAX_ACTIVITY_RESOLUTION_ROOMS);
        let attempted = first
            .into_iter()
            .chain(second)
            .map(|request| request.room_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(attempted.len(), MAX_ACTIVITY_RESOLUTION_ROOMS + 3);
    }

    #[test]
    fn activity_projection_ignores_plain_unread_count_for_activity_unread() {
        let mut state = AppState::default();
        state.rooms = vec![RoomSummary {
            room_id: "!room:example.invalid".to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Room".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 3,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: Some(RoomLatestEventSummary {
                event_id: "$latest:example.invalid".to_owned(),
                relation_type: None,
                relation_event_id: None,
                sender_id: Some("@sender:example.invalid".to_owned()),
                sender_label: Some("Sender".to_owned()),
                sender_avatar: None,
                preview: Some("body".to_owned()),
                timestamp_ms: 42,
            }),
            parent_space_ids: Vec::new(),
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];

        let mut projection = ActivityProjection::default();
        let (recent, unread, _excluded_room_ids) = projection.snapshot(&state);

        assert!(
            unread.rows.is_empty(),
            "Activity Unread should not invent un-navigable rows from plain unread message counts"
        );
        assert_eq!(recent.rows.len(), 1);
        assert!(
            !recent.rows[0].unread,
            "plain unread message counts should not mark Activity recent rows unread"
        );
    }

    #[test]
    fn activity_projection_bounds_recent_history_to_newest_observed_rows() {
        let mut state = AppState::default();
        let mut room = unread_diagnostic_room("!room:example.invalid");
        room.unread_count = 0;
        room.notification_count = 0;
        room.highlight_count = 0;
        room.marked_unread = false;
        state.rooms = vec![room];

        let mut projection = ActivityProjection::default();
        projection.ingest(
            (0..=ACTIVITY_RECENT_MAX_ROWS)
                .map(|index| {
                    ActivityRow::event(
                        "!room:example.invalid".to_owned(),
                        format!("$event-{index}:example.invalid"),
                        Some("@sender:example.invalid".to_owned()),
                        "Room".to_owned(),
                        Some("Sender".to_owned()),
                        Some(format!("body {index}")),
                        index as u64,
                        false,
                        false,
                    )
                })
                .collect(),
        );

        let (recent, _unread, _excluded_room_ids) = projection.snapshot(&state);

        assert_eq!(recent.rows.len(), ACTIVITY_RECENT_MAX_ROWS);
        assert_eq!(
            recent.rows.first().and_then(|row| row.event_id.as_deref()),
            Some("$event-200:example.invalid")
        );
        assert_eq!(
            recent.rows.last().and_then(|row| row.event_id.as_deref()),
            Some("$event-1:example.invalid")
        );
        assert_eq!(projection.rows_by_event_id.len(), ACTIVITY_RECENT_MAX_ROWS);
    }

    #[test]
    fn activity_projection_keeps_old_unread_rows_outside_recent_window() {
        let mut state = AppState::default();
        state.rooms = vec![unread_diagnostic_room("!room:example.invalid")];

        let rows = (0..=ACTIVITY_RECENT_MAX_ROWS)
            .map(|index| {
                ActivityRow::event(
                    "!room:example.invalid".to_owned(),
                    format!("$event-{index}:example.invalid"),
                    Some("@sender:example.invalid".to_owned()),
                    "Room".to_owned(),
                    Some("Sender".to_owned()),
                    Some(format!("body {index}")),
                    index as u64,
                    false,
                    false,
                )
            })
            .collect::<Vec<_>>();
        let mut projection = ActivityProjection::default();
        projection.ingest(rows);

        let (recent, unread, _excluded_room_ids) = projection.snapshot(&state);

        assert_eq!(recent.rows.len(), ACTIVITY_RECENT_MAX_ROWS);
        assert_eq!(unread.rows.len(), ACTIVITY_RECENT_MAX_ROWS + 1);
        assert!(
            unread
                .rows
                .iter()
                .any(|row| { row.event_id.as_deref() == Some("$event-0:example.invalid") })
        );
        assert_eq!(
            projection.rows_by_event_id.len(),
            ACTIVITY_RECENT_MAX_ROWS + 1
        );
    }

    #[test]
    fn activity_projection_ignores_plain_unread_count_for_ingested_event_rows() {
        let mut state = AppState::default();
        state.rooms = vec![RoomSummary {
            room_id: "!room:example.invalid".to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Room".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 3,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: None,
            parent_space_ids: Vec::new(),
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];

        let mut projection = ActivityProjection::default();
        projection.ingest(vec![ActivityRow::event(
            "!room:example.invalid".to_owned(),
            "$event:example.invalid".to_owned(),
            Some("@sender:example.invalid".to_owned()),
            "Room".to_owned(),
            Some("Sender".to_owned()),
            Some("body".to_owned()),
            42,
            true,
            false,
        )]);
        let (recent, unread, _excluded_room_ids) = projection.snapshot(&state);

        assert!(unread.rows.is_empty());
        assert_eq!(recent.rows.len(), 1);
        assert!(
            !recent.rows[0].unread,
            "ingested event rows must not inherit plain unread-only state"
        );
    }

    #[test]
    fn activity_projection_skips_recent_rows_for_mentions_mode_without_highlight() {
        let mut state = AppState::default();
        state.rooms = vec![RoomSummary {
            room_id: "!room:example.invalid".to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Room".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 1,
            notification_count: 1,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: Some(RoomLatestEventSummary {
                event_id: "$latest:example.invalid".to_owned(),
                relation_type: None,
                relation_event_id: None,
                sender_id: Some("@sender:example.invalid".to_owned()),
                sender_label: Some("Sender".to_owned()),
                sender_avatar: None,
                preview: Some("body".to_owned()),
                timestamp_ms: 42,
            }),
            parent_space_ids: Vec::new(),
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];
        state.room_notification_settings.insert(
            "!room:example.invalid".to_owned(),
            RoomNotificationSettings {
                mode: RoomNotificationMode::Mentions,
                operation: RoomNotificationModeOperation::Idle,
            },
        );

        let mut projection = ActivityProjection::default();
        projection.ingest(vec![ActivityRow::event(
            "!room:example.invalid".to_owned(),
            "$event:example.invalid".to_owned(),
            Some("@sender:example.invalid".to_owned()),
            "Room".to_owned(),
            Some("Sender".to_owned()),
            Some("body".to_owned()),
            41,
            true,
            false,
        )]);
        let (recent, unread, _excluded_room_ids) = projection.snapshot(&state);

        assert!(recent.rows.is_empty());
        assert!(unread.rows.is_empty());
    }

    #[test]
    fn activity_projection_context_label_uses_space_and_room_names() {
        let mut state = AppState::default();
        state.spaces = vec![SpaceSummary {
            space_id: "!space:example.invalid".to_owned(),
            display_name: "Science".to_owned(),
            avatar: None,
            child_room_ids: vec!["!room:example.invalid".to_owned()],
        }];
        state.rooms = vec![RoomSummary {
            room_id: "!room:example.invalid".to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Papers".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 0,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: Some(RoomLatestEventSummary {
                event_id: "$latest:example.invalid".to_owned(),
                relation_type: None,
                relation_event_id: None,
                sender_id: Some("@sender:example.invalid".to_owned()),
                sender_label: Some("Sender".to_owned()),
                sender_avatar: None,
                preview: Some("body".to_owned()),
                timestamp_ms: 42,
            }),
            parent_space_ids: vec!["!space:example.invalid".to_owned()],
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];

        let mut projection = ActivityProjection::default();
        let (recent, _unread, _excluded_room_ids) = projection.snapshot(&state);

        assert_eq!(recent.rows[0].context_label, "Science / Papers");
    }

    #[test]
    fn activity_projection_reconciles_replacement_latest_with_original_timeline_row() {
        let room_id = "!room:example.invalid";
        let original_event_id = "$original:example.invalid";
        let sender_id = "@sender:example.invalid";
        let mut state = AppState::default();
        state.profile.users.insert(
            sender_id.to_owned(),
            UserProfile {
                user_id: sender_id.to_owned(),
                display_name: Some("Sender".to_owned()),
                display_label: "Sender".to_owned(),
                original_display_label: "Sender".to_owned(),
                mention_search_terms: vec!["Sender".to_owned()],
                avatar: Some(koushi_state::AvatarImage {
                    mxc_uri: "mxc://example.invalid/enriched".to_owned(),
                    thumbnail: Default::default(),
                }),
            },
        );
        state.rooms = vec![RoomSummary {
            room_id: room_id.to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Room".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 0,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: Some(RoomLatestEventSummary {
                event_id: "$edit:example.invalid".to_owned(),
                relation_type: Some("m.replace".to_owned()),
                relation_event_id: Some(original_event_id.to_owned()),
                sender_id: Some(sender_id.to_owned()),
                sender_label: Some("Sender".to_owned()),
                sender_avatar: None,
                preview: Some("edited body".to_owned()),
                timestamp_ms: 42,
            }),
            parent_space_ids: Vec::new(),
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];
        state.rooms[0].unread_count = 1;
        state.rooms[0].notification_count = 1;

        let mut fallback_projection = ActivityProjection::default();
        let (_recent, unread_before_clear, _excluded) = fallback_projection.snapshot(&state);
        assert_eq!(
            unread_before_clear.rows[0].event_id.as_deref(),
            Some(original_event_id)
        );
        fallback_projection
            .cleared_event_ids
            .insert(original_event_id.to_owned());
        let (_recent, unread_after_clear, _excluded) = fallback_projection.snapshot(&state);
        assert!(
            unread_after_clear.rows.is_empty(),
            "clearing the canonical original event must keep an edited fallback row read"
        );

        let mut projection = ActivityProjection::default();
        projection.ingest(vec![ActivityRow::event(
            room_id.to_owned(),
            original_event_id.to_owned(),
            Some(sender_id.to_owned()),
            "Room".to_owned(),
            Some("Sender".to_owned()),
            Some("edited body".to_owned()),
            41,
            false,
            false,
        )]);

        let (recent, _unread, _excluded) = projection.snapshot(&state);

        assert_eq!(recent.rows.len(), 1);
        assert_eq!(recent.rows[0].event_id.as_deref(), Some(original_event_id));
        assert_eq!(recent.rows[0].timestamp_ms, 41);
        assert_eq!(
            recent.rows[0]
                .sender_avatar
                .as_ref()
                .map(|avatar| avatar.mxc_uri.as_str()),
            Some("mxc://example.invalid/enriched")
        );
    }

    #[test]
    fn activity_projection_does_not_append_annotation_latest_event() {
        let mut state = AppState::default();
        state.rooms = vec![RoomSummary {
            room_id: "!room:example.invalid".to_owned(),
            display_name: "Room".to_owned(),
            display_label: "Room".to_owned(),
            original_display_label: "Room".to_owned(),
            avatar: None,
            is_dm: false,
            dm_user_ids: Vec::new(),
            tags: RoomTags::default(),
            unread_count: 0,
            notification_count: 0,
            highlight_count: 0,
            marked_unread: false,
            recency_stamp: Some(42),
            conversation_activity: None,
            latest_event: Some(RoomLatestEventSummary {
                event_id: "$reaction:example.invalid".to_owned(),
                relation_type: Some("m.annotation".to_owned()),
                relation_event_id: Some("$target:example.invalid".to_owned()),
                sender_id: Some("@sender:example.invalid".to_owned()),
                sender_label: Some("Sender".to_owned()),
                sender_avatar: None,
                preview: None,
                timestamp_ms: 42,
            }),
            parent_space_ids: Vec::new(),
            dm_space_ids: Vec::new(),
            is_encrypted: false,
            joined_members: 2,
        }];

        let (recent, unread, _excluded) = ActivityProjection::default().snapshot(&state);

        assert!(recent.rows.is_empty());
        assert!(unread.rows.is_empty());
    }
}
