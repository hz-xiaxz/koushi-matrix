use koushi_state::{
    AppAction, AppEffect, AppState, RoomHistoryExportFailureKind, RoomHistoryExportProgress,
    RoomHistoryExportRange, RoomHistoryExportState, RoomSummary, RoomTags, SessionInfo,
    SessionState, UiEvent, reduce,
};

const ROOM: &str = "!history:example.invalid";

fn changed() -> Vec<AppEffect> {
    vec![AppEffect::EmitUiEvent(UiEvent::RoomHistoryExportChanged)]
}

fn session_info() -> SessionInfo {
    SessionInfo {
        homeserver: "https://matrix.example.invalid".to_owned(),
        user_id: "@user-a:example.invalid".to_owned(),
        device_id: "DEVICE".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    }
}

fn room_summary(room_id: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: "Synthetic Room".to_owned(),
        display_label: "Synthetic Room".to_owned(),
        original_display_label: "Synthetic Room".to_owned(),
        avatar: None,
        is_dm: false,
        dm_user_ids: Vec::new(),
        tags: RoomTags::default(),
        unread_count: 0,
        notification_count: 0,
        highlight_count: 0,
        marked_unread: false,
        recency_stamp: None,
        conversation_activity: None,
        latest_event: None,
        parent_space_ids: Vec::new(),
        dm_space_ids: Vec::new(),
        is_encrypted: true,
        joined_members: 2,
    }
}

fn ready_state() -> AppState {
    AppState {
        session: SessionState::Ready(session_info()),
        rooms: vec![room_summary(ROOM)],
        ..AppState::default()
    }
}

fn period() -> RoomHistoryExportRange {
    RoomHistoryExportRange::Period {
        start_ms: 1_700_000_000_000,
        end_exclusive_ms: 1_700_086_400_000,
        time_zone: "Asia/Tokyo".to_owned(),
    }
}

fn progress(fetched: u64, exported: u64, undecryptable: u64) -> RoomHistoryExportProgress {
    RoomHistoryExportProgress {
        fetched_events: fetched,
        exported_events: exported,
        undecryptable_events: undecryptable,
    }
}

fn request(state: &mut AppState, request_id: u64, range: RoomHistoryExportRange) -> Vec<AppEffect> {
    reduce(
        state,
        AppAction::RoomHistoryExportRequested {
            request_id,
            room_id: ROOM.to_owned(),
            range,
        },
    )
}

fn exporting(state: &AppState) -> (u64, RoomHistoryExportProgress, bool) {
    match &state.room_history_export {
        RoomHistoryExportState::Exporting {
            request_id,
            progress,
            cancel_requested,
            ..
        } => (*request_id, *progress, *cancel_requested),
        other => panic!("expected Exporting, got {other:?}"),
    }
}

#[test]
fn request_requires_ready_session_known_room_and_valid_range() {
    let mut signed_out = AppState {
        rooms: vec![room_summary(ROOM)],
        ..AppState::default()
    };
    assert!(request(&mut signed_out, 1, RoomHistoryExportRange::AllAvailable).is_empty());
    assert_eq!(signed_out.room_history_export, RoomHistoryExportState::Idle);

    let mut state = ready_state();
    let unknown_room = reduce(
        &mut state,
        AppAction::RoomHistoryExportRequested {
            request_id: 1,
            room_id: "!unknown:example.invalid".to_owned(),
            range: RoomHistoryExportRange::AllAvailable,
        },
    );
    assert!(unknown_room.is_empty());

    let empty_period = RoomHistoryExportRange::Period {
        start_ms: 10,
        end_exclusive_ms: 10,
        time_zone: "UTC".to_owned(),
    };
    assert!(request(&mut state, 2, empty_period).is_empty());
    let missing_zone = RoomHistoryExportRange::Period {
        start_ms: 10,
        end_exclusive_ms: 20,
        time_zone: " ".to_owned(),
    };
    assert!(request(&mut state, 3, missing_zone).is_empty());
    assert_eq!(state.room_history_export, RoomHistoryExportState::Idle);

    assert_eq!(request(&mut state, 4, period()), changed());
    assert_eq!(exporting(&state), (4, progress(0, 0, 0), false));
}

#[test]
fn second_request_is_rejected_while_one_is_in_flight() {
    let mut state = ready_state();
    request(&mut state, 1, RoomHistoryExportRange::AllAvailable);
    assert!(request(&mut state, 2, period()).is_empty());
    assert_eq!(exporting(&state).0, 1);
}

#[test]
fn progress_applies_only_to_the_active_request_and_skips_duplicates() {
    let mut state = ready_state();
    request(&mut state, 1, RoomHistoryExportRange::AllAvailable);

    let stale = reduce(
        &mut state,
        AppAction::RoomHistoryExportProgressed {
            request_id: 9,
            progress: progress(5, 5, 0),
        },
    );
    assert!(stale.is_empty());

    let applied = reduce(
        &mut state,
        AppAction::RoomHistoryExportProgressed {
            request_id: 1,
            progress: progress(200, 150, 3),
        },
    );
    assert_eq!(applied, changed());
    assert_eq!(exporting(&state).1, progress(200, 150, 3));

    let duplicate = reduce(
        &mut state,
        AppAction::RoomHistoryExportProgressed {
            request_id: 1,
            progress: progress(200, 150, 3),
        },
    );
    assert!(duplicate.is_empty());
}

#[test]
fn completion_is_terminal_and_duplicate_or_stale_completions_are_ignored() {
    let mut state = ready_state();
    request(&mut state, 1, period());

    let stale = reduce(
        &mut state,
        AppAction::RoomHistoryExportCompleted {
            request_id: 2,
            progress: progress(1, 1, 0),
        },
    );
    assert!(stale.is_empty());

    let completed = reduce(
        &mut state,
        AppAction::RoomHistoryExportCompleted {
            request_id: 1,
            progress: progress(400, 120, 2),
        },
    );
    assert_eq!(completed, changed());
    assert_eq!(
        state.room_history_export,
        RoomHistoryExportState::Completed {
            request_id: 1,
            room_id: ROOM.to_owned(),
            range: period(),
            progress: progress(400, 120, 2),
        }
    );

    for late in [
        AppAction::RoomHistoryExportCompleted {
            request_id: 1,
            progress: progress(1, 1, 0),
        },
        AppAction::RoomHistoryExportFailed {
            request_id: 1,
            kind: RoomHistoryExportFailureKind::Network,
            progress: progress(1, 1, 0),
        },
        AppAction::RoomHistoryExportCancelled {
            request_id: 1,
            progress: progress(1, 1, 0),
        },
        AppAction::RoomHistoryExportProgressed {
            request_id: 1,
            progress: progress(9, 9, 9),
        },
        AppAction::RoomHistoryExportCancelRequested { request_id: 1 },
    ] {
        assert!(reduce(&mut state, late).is_empty());
    }
    assert!(matches!(
        state.room_history_export,
        RoomHistoryExportState::Completed { .. }
    ));

    // A terminal export does not block the next one.
    assert_eq!(
        request(&mut state, 3, RoomHistoryExportRange::AllAvailable),
        changed()
    );
    assert_eq!(exporting(&state).0, 3);
}

#[test]
fn cancellation_is_requested_then_settled_by_the_core() {
    let mut state = ready_state();
    request(&mut state, 1, RoomHistoryExportRange::AllAvailable);

    assert!(
        reduce(
            &mut state,
            AppAction::RoomHistoryExportCancelRequested { request_id: 7 }
        )
        .is_empty()
    );
    assert_eq!(
        reduce(
            &mut state,
            AppAction::RoomHistoryExportCancelRequested { request_id: 1 }
        ),
        changed()
    );
    assert!(exporting(&state).2);
    assert!(
        reduce(
            &mut state,
            AppAction::RoomHistoryExportCancelRequested { request_id: 1 }
        )
        .is_empty()
    );

    assert_eq!(
        reduce(
            &mut state,
            AppAction::RoomHistoryExportCancelled {
                request_id: 1,
                progress: progress(50, 20, 0),
            }
        ),
        changed()
    );
    assert_eq!(
        state.room_history_export,
        RoomHistoryExportState::Cancelled {
            request_id: 1,
            room_id: ROOM.to_owned(),
            progress: progress(50, 20, 0),
        }
    );
}

#[test]
fn a_completion_racing_a_cancel_request_still_settles_as_completed() {
    let mut state = ready_state();
    request(&mut state, 1, RoomHistoryExportRange::AllAvailable);
    reduce(
        &mut state,
        AppAction::RoomHistoryExportCancelRequested { request_id: 1 },
    );
    reduce(
        &mut state,
        AppAction::RoomHistoryExportCompleted {
            request_id: 1,
            progress: progress(3, 3, 0),
        },
    );
    assert!(matches!(
        state.room_history_export,
        RoomHistoryExportState::Completed { .. }
    ));
}

#[test]
fn failure_keeps_partial_counts_for_the_user_to_judge() {
    let mut state = ready_state();
    request(&mut state, 1, RoomHistoryExportRange::AllAvailable);
    assert_eq!(
        reduce(
            &mut state,
            AppAction::RoomHistoryExportFailed {
                request_id: 1,
                kind: RoomHistoryExportFailureKind::Write,
                progress: progress(10, 8, 1),
            }
        ),
        changed()
    );
    assert_eq!(
        state.room_history_export,
        RoomHistoryExportState::Failed {
            request_id: 1,
            room_id: ROOM.to_owned(),
            progress: progress(10, 8, 1),
            failure_kind: RoomHistoryExportFailureKind::Write,
        }
    );
}

#[test]
fn logout_and_lock_reset_the_export() {
    for action in [AppAction::LogoutRequested, AppAction::SessionLocked] {
        let mut state = ready_state();
        request(&mut state, 1, RoomHistoryExportRange::AllAvailable);
        let effects = reduce(&mut state, action);
        assert_eq!(state.room_history_export, RoomHistoryExportState::Idle);
        assert!(effects.contains(&AppEffect::EmitUiEvent(UiEvent::RoomHistoryExportChanged)));
    }
}

#[test]
fn range_inclusion_is_start_inclusive_end_exclusive() {
    let range = period();
    assert!(!range.contains(1_699_999_999_999));
    assert!(range.contains(1_700_000_000_000));
    assert!(range.contains(1_700_086_399_999));
    assert!(!range.contains(1_700_086_400_000));
    assert!(RoomHistoryExportRange::AllAvailable.contains(0));
}

#[test]
fn state_serializes_with_tagged_camel_case_and_debug_redacts_identifiers() {
    let mut state = ready_state();
    request(&mut state, 1, period());
    let value = serde_json::to_value(&state.room_history_export).unwrap();
    assert_eq!(value["kind"], "exporting");
    assert_eq!(value["request_id"], 1);
    assert_eq!(value["range"]["kind"], "period");
    assert_eq!(value["range"]["end_exclusive_ms"], 1_700_086_400_000_u64);
    assert_eq!(value["progress"]["undecryptable_events"], 0);

    let debug = format!("{:?}", state.room_history_export);
    assert!(!debug.contains(ROOM), "{debug}");
    assert!(!debug.contains("Asia/Tokyo"), "{debug}");
    let action_debug = format!(
        "{:?}",
        AppAction::RoomHistoryExportRequested {
            request_id: 1,
            room_id: ROOM.to_owned(),
            range: period(),
        }
    );
    assert!(!action_debug.contains(ROOM), "{action_debug}");
}
