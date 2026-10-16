use super::*;
use crate::MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS;
use crate::state::{RoomSummary, RoomTags};

fn unread_room() -> RoomSummary {
    RoomSummary {
        display_name_placeholder: None,
        display_label_placeholder: None,
        room_id: "!room:example.invalid".to_owned(),
        display_name: "Room".to_owned(),
        display_label: "Room".to_owned(),
        original_display_label: "Room".to_owned(),
        avatar: None,
        is_dm: false,
        dm_user_ids: Vec::new(),
        tags: RoomTags::default(),
        unread_count: 2,
        notification_count: 2,
        highlight_count: 0,
        thread_unread_count: 0,
        thread_highlight_count: 0,
        marked_unread: false,
        recency_stamp: Some(42),
        conversation_activity: None,
        latest_event: Some(RoomLatestEventSummary {
            event_id: "$observed:example.invalid".to_owned(),
            relation_type: None,
            relation_event_id: None,
            thread_root_event_id: None,
            sender_id: None,
            sender_label: None,
            sender_avatar: None,
            preview: None,
            message_text: None,
            timestamp_ms: 42,
            is_redacted: false,
        }),
        parent_space_ids: Vec::new(),
        dm_space_ids: Vec::new(),
        is_encrypted: false,
        joined_members: 2,
    }
}

/// The one candidate this room list produces, for the delivery-channel rules.
fn candidate_of(room: &RoomSummary) -> RoomAttentionCandidate {
    let rooms = [room.clone()];
    let mut candidates = attention_room_candidates(NativeAttentionProjectionInput {
        rooms: &rooms,
        active_room_id: None,
        muted_room_ids: &[],
        room_notification_modes: &HashMap::new(),
        ignored_user_ids: &BTreeSet::new(),
        window_focused: false,
        observation: NativeAttentionObservationKind::InitialSync,
        previous_candidate: None,
        message_previews: true,
        capabilities: NativeAttentionCapabilities::default(),
    });
    assert_eq!(candidates.len(), 1, "the fixture room is one candidate");
    candidates.remove(0)
}

fn room_with_latest_text(text: Option<&str>) -> RoomSummary {
    let mut room = unread_room();
    room.latest_event
        .as_mut()
        .expect("the fixture has a latest event")
        .message_text = text.map(str::to_owned);
    room
}

/// #1427: the candidate carries the room's newest readable message, bounded, and it
/// never prints it.
#[test]
fn a_candidate_carries_the_bounded_latest_message_text_and_never_prints_it() {
    let mut room = room_with_latest_text(Some("private delivery text"));
    room.display_label = "Private Room Label".to_owned();
    let candidate = candidate_of(&room);
    assert_eq!(
        candidate
            .newest_message_text
            .as_ref()
            .map(|text| text.text.as_str()),
        Some("private delivery text")
    );
    let debug = format!("{candidate:?}");
    assert!(debug.contains("MessageText(..)"));
    for private_value in [
        "private delivery text",
        "!room:example.invalid",
        "Private Room Label",
    ] {
        assert!(
            !debug.contains(private_value),
            "candidate Debug leaked {private_value}: {debug}"
        );
    }

    // A room whose newest event has nothing readable still qualifies; it just
    // carries no text.
    assert_eq!(
        candidate_of(&room_with_latest_text(None)).newest_message_text,
        None
    );

    // A redacted event is not text to carry, whatever a stale projection holds.
    let mut redacted = room_with_latest_text(Some("private delivery text"));
    redacted
        .latest_event
        .as_mut()
        .expect("the fixture has a latest event")
        .is_redacted = true;
    assert_eq!(candidate_of(&redacted).newest_message_text, None);

    // The candidate holds a bounded slice, and it cuts on a character boundary: a
    // body of two-byte characters must not panic or split one in half.
    let long = "日".repeat(MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS + 50);
    let bounded = candidate_of(&room_with_latest_text(Some(&long)))
        .newest_message_text
        .expect("a long body is still text");
    assert_eq!(
        bounded.text.chars().count(),
        MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS
    );
    assert!(bounded.truncated_at_source);
    assert!(bounded.text.chars().all(|character| character == '日'));
}

fn project(observation: NativeAttentionObservationKind) -> NativeAttentionProjection {
    let rooms = [unread_room()];
    native_attention_projection_from_rooms(NativeAttentionProjectionInput {
        rooms: &rooms,
        active_room_id: None,
        muted_room_ids: &[],
        room_notification_modes: &HashMap::new(),
        ignored_user_ids: &BTreeSet::new(),
        window_focused: false,
        observation,
        previous_candidate: None,
        message_previews: false,
        capabilities: NativeAttentionCapabilities::default(),
    })
}

#[test]
fn native_attention_observation_suppresses_non_live_candidates() {
    for (observation, reason) in [
        (
            NativeAttentionObservationKind::InitialSync,
            NativeAttentionSuppressionReason::InitialSync,
        ),
        (
            NativeAttentionObservationKind::Backfill,
            NativeAttentionSuppressionReason::Backfill,
        ),
        (
            NativeAttentionObservationKind::SelfEvent,
            NativeAttentionSuppressionReason::SelfMessage,
        ),
    ] {
        let projection = project(observation);
        assert_eq!(projection.state.summary.unread_count, 2);
        assert_eq!(projection.state.summary.badge_count, 2);
        assert_eq!(projection.state.summary.candidate, None);
        assert_eq!(
            projection.state.dispatch,
            NativeAttentionDispatchState::Suppressed { reason }
        );
    }

    let live = project(NativeAttentionObservationKind::Live);
    assert_eq!(live.state.summary.unread_count, 2);
    assert_eq!(live.state.summary.badge_count, 2);
    assert!(live.state.summary.candidate.is_some());
    assert_eq!(live.state.dispatch, NativeAttentionDispatchState::Idle);
    assert!(!live.active_room_match);
}

#[test]
fn native_badge_diagnostics_count_only_unique_non_muted_rooms() {
    let included = unread_room();
    let mut duplicate = included.clone();
    duplicate.unread_count = 9;
    let mut explicit_mute = unread_room();
    explicit_mute.room_id = "!explicit:example.invalid".to_owned();
    let mut mode_mute = unread_room();
    mode_mute.room_id = "!mode:example.invalid".to_owned();
    let rooms = [included, duplicate, explicit_mute, mode_mute];
    let modes = HashMap::from([(
        "!mode:example.invalid".to_owned(),
        RoomNotificationMode::Mute,
    )]);

    let projection = native_attention_projection_from_rooms(NativeAttentionProjectionInput {
        rooms: &rooms,
        active_room_id: None,
        muted_room_ids: &["!explicit:example.invalid".to_owned()],
        room_notification_modes: &modes,
        ignored_user_ids: &BTreeSet::new(),
        window_focused: false,
        observation: NativeAttentionObservationKind::Live,
        previous_candidate: None,
        message_previews: false,
        capabilities: NativeAttentionCapabilities::default(),
    });

    assert_eq!(projection.state.summary.badge_count, 2);
    assert_eq!(projection.badge_room_count, 1);
    assert_eq!(projection.badge_excluded_room_count, 3);
}

#[test]
fn a_one_to_one_direct_message_carries_its_partner_and_the_local_alias() {
    let mut dm = unread_room();
    dm.is_dm = true;
    dm.dm_user_ids = vec!["@partner:example.invalid".to_owned()];
    let partner = candidate_of(&dm).direct_partner.expect("a partner");
    assert_eq!(partner.user_id, "@partner:example.invalid");
    assert_eq!(partner.local_alias, None);

    // A group conversation, or a direct message with more than one other member, has
    // no single partner to name.
    let mut group_dm = dm.clone();
    group_dm
        .dm_user_ids
        .push("@other:example.invalid".to_owned());
    assert!(candidate_of(&group_dm).direct_partner.is_none());
    assert!(candidate_of(&unread_room()).direct_partner.is_none());

    let mut state = AppState {
        rooms: vec![dm],
        ..AppState::default()
    };
    state.profile.local_aliases.insert(
        "@partner:example.invalid".to_owned(),
        "  Lab partner ".to_owned(),
    );
    let candidates = attention_room_candidates_for_state(&state);
    let partner = candidates[0].direct_partner.as_ref().expect("a partner");
    assert_eq!(partner.local_alias.as_deref(), Some("Lab partner"));
    let debug = format!("{:?}", candidates[0]);
    assert!(!debug.contains("partner:example.invalid"));
    assert!(!debug.contains("Lab partner"));
}
