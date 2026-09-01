use super::actor::RoomActor;
use super::list_observer::state_contains_pinned_events;
use super::operations::{classify_room_error, operation_failure_kind};
use koushi_protocol::event::{CoreEvent, RoomEvent};
use koushi_protocol::failure::{CoreFailure, RoomFailureKind};
use koushi_protocol::ids::RequestId;
use koushi_sdk::MatrixClientSession;
use koushi_state::{AppAction, PinnedEvent, PinnedEventState};
use std::collections::{BTreeSet, HashSet};

pub(super) fn pinned_event_room_ids(
    updates: &matrix_sdk_base::sync::RoomUpdates,
) -> BTreeSet<String> {
    updates
        .joined
        .iter()
        .filter(|(_, update)| state_contains_pinned_events(&update.state))
        .map(|(room_id, _)| room_id.to_string())
        .collect()
}

async fn load_pinned_events_for_room(
    session: &MatrixClientSession,
    room_id: &str,
) -> Result<Vec<PinnedEvent>, RoomFailureKind> {
    let event_ids = koushi_sdk::load_pinned_event_ids(session, room_id)
        .await
        .map_err(|error| classify_room_error(&error))?;
    Ok(load_pinned_events(session, room_id, event_ids).await)
}

async fn load_pinned_events(
    session: &MatrixClientSession,
    room_id: &str,
    event_ids: Vec<String>,
) -> Vec<PinnedEvent> {
    let Ok(parsed_room_id) = matrix_sdk::ruma::RoomId::parse(room_id) else {
        return event_ids
            .into_iter()
            .map(pinned_event_unavailable)
            .collect();
    };
    let Some(room) = session.client().get_room(&parsed_room_id) else {
        return event_ids
            .into_iter()
            .map(pinned_event_unavailable)
            .collect();
    };

    let mut seen = HashSet::new();
    let mut projected = Vec::new();
    for event_id in event_ids {
        if !seen.insert(event_id.clone()) {
            continue;
        }
        let Ok(parsed_event_id) = matrix_sdk::ruma::EventId::parse(&event_id) else {
            projected.push(pinned_event_unavailable(event_id));
            continue;
        };
        let pinned = match room.load_or_fetch_event(&parsed_event_id, None).await {
            Ok(event) => pinned_event_from_raw(event_id.clone(), event.raw().json().get()),
            Err(_) => pinned_event_unavailable(event_id),
        };
        projected.push(pinned);
    }
    projected
}

fn pinned_event_unavailable(event_id: String) -> PinnedEvent {
    PinnedEvent {
        event_id,
        sender: None,
        sender_label: None,
        body_preview: None,
        redacted: false,
        timestamp_ms: None,
        state: PinnedEventState::Unavailable,
        thread_root_event_id: None,
    }
}

fn pinned_event_from_raw(event_id: String, raw_json: &str) -> PinnedEvent {
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return pinned_event_unavailable(event_id);
    };
    let event_id = raw
        .get("event_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or(event_id);
    let sender = raw
        .get("sender")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let timestamp_ms = raw
        .get("origin_server_ts")
        .and_then(serde_json::Value::as_u64);
    let redacted = raw
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("redacted_because"))
        .is_some();
    let content = raw.get("content").unwrap_or(&serde_json::Value::Null);
    let thread_root_event_id = content
        .get("m.relates_to")
        .and_then(|relation| relation.get("rel_type"))
        .and_then(serde_json::Value::as_str)
        .filter(|rel_type| *rel_type == "m.thread")
        .and_then(|_| content.get("m.relates_to"))
        .and_then(|relation| relation.get("event_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let is_encrypted =
        raw.get("type").and_then(serde_json::Value::as_str) == Some("m.room.encrypted");
    let body_preview = content
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    PinnedEvent {
        event_id,
        sender,
        sender_label: None,
        body_preview,
        redacted,
        timestamp_ms,
        state: if is_encrypted {
            PinnedEventState::UnableToDecrypt
        } else {
            PinnedEventState::Ready
        },
        thread_root_event_id,
    }
}

impl RoomActor {
    pub(super) async fn handle_pin_event(
        &self,
        request_id: RequestId,
        room_id: String,
        event_id: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        self.reduce_reliable(vec![AppAction::PinEventRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::pin_event(session, &room_id, &event_id).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::PinEventCompleted {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::PinEventCompleted {
                    request_id,
                    room_id: room_id.clone(),
                    event_id,
                }));
                self.project_pinned_events_after_success(request_id, room_id)
                    .await;
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::PinEventFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_unpin_event(
        &self,
        request_id: RequestId,
        room_id: String,
        event_id: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        self.reduce_reliable(vec![AppAction::UnpinEventRequested {
            request_id: request_id.sequence,
            room_id: room_id.clone(),
            event_id: event_id.clone(),
        }])
        .await;
        if !self.ensure_known_room_for_message_interaction(request_id, &room_id) {
            return;
        }
        match koushi_sdk::unpin_event(session, &room_id, &event_id).await {
            Ok(()) => {
                self.reduce_reliable(vec![AppAction::UnpinEventCompleted {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::UnpinEventCompleted {
                    request_id,
                    room_id: room_id.clone(),
                    event_id,
                }));
                self.project_pinned_events_after_success(request_id, room_id)
                    .await;
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::UnpinEventFailed {
                    request_id: request_id.sequence,
                    room_id,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_refresh_pinned_events(
        &self,
        request_id: RequestId,
        room_id: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match load_pinned_events_for_room(session, &room_id).await {
            Ok(pinned) => {
                self.project_pinned_events(room_id, pinned, Some(request_id))
                    .await
            }
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_pinned_events_changed(&self, room_ids: BTreeSet<String>) {
        let Some(session) = &self.session else {
            return;
        };
        for room_id in room_ids {
            match load_pinned_events_for_room(session, &room_id).await {
                Ok(pinned) => self.project_pinned_events(room_id, pinned, None).await,
                Err(_kind) => {
                    // A background sync refresh has no request to fail. Keep
                    // the previous projection and wait for the next state
                    // update; only classified failure state may cross the
                    // Core boundary.
                }
            }
        }
    }

    async fn project_pinned_events_after_success(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            return;
        };
        match load_pinned_events_for_room(session, &room_id).await {
            Ok(pinned) => {
                self.project_pinned_events(room_id, pinned, Some(request_id))
                    .await
            }
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn project_pinned_events(
        &self,
        room_id: String,
        pinned: Vec<PinnedEvent>,
        request_id: Option<RequestId>,
    ) {
        self.reduce_reliable(vec![AppAction::RoomPinnedEventsUpdated {
            room_id: room_id.clone(),
            pinned: pinned.clone(),
        }])
        .await;
        self.emit(CoreEvent::Room(RoomEvent::PinnedEventsUpdated {
            request_id,
            room_id,
            pinned,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::pinned_event_from_raw;

    use crate::command::RoomCommand;

    use koushi_protocol::event::CoreEvent;

    use koushi_protocol::failure::CoreFailure;

    use crate::room::actor::make_request_id;
    use crate::room::actor::{RoomActor, RoomMessage};

    use koushi_state::PinnedEventState;

    use tokio::sync::{broadcast, mpsc};

    #[tokio::test]
    async fn pin_event_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(
            action_tx,
            event_tx,
            crate::SlidingSyncDiagnostics::default(),
        );

        let request_id = make_request_id(8);
        handle
            .send(RoomMessage::Command(RoomCommand::PinEvent {
                request_id,
                room_id: "!room:example.test".to_owned(),
                event_id: "$event:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unpin_event_without_session_emits_session_required() {
        let (action_tx, _action_rx) = mpsc::channel(16);
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let handle = RoomActor::spawn(
            action_tx,
            event_tx,
            crate::SlidingSyncDiagnostics::default(),
        );

        let request_id = make_request_id(9);
        handle
            .send(RoomMessage::Command(RoomCommand::UnpinEvent {
                request_id,
                room_id: "!room:example.test".to_owned(),
                event_id: "$event:example.test".to_owned(),
            }))
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } => {
                assert_eq!(ev_id, request_id);
                assert_eq!(failure, CoreFailure::SessionRequired);
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    #[test]
    fn pinned_raw_projection_preserves_event_order_metadata_and_thread_relation() {
        let event = pinned_event_from_raw(
            "$fallback:example.invalid".to_owned(),
            r#"{
                "event_id":"$reply:example.invalid",
                "sender":"@bob:example.invalid",
                "origin_server_ts":1800000000000,
                "type":"m.room.message",
                "content":{
                    "msgtype":"m.text",
                    "body":"Pinned reply",
                    "m.relates_to":{"rel_type":"m.thread","event_id":"$root:example.invalid"}
                }
            }"#,
        );

        assert_eq!(event.event_id, "$reply:example.invalid");
        assert_eq!(event.sender.as_deref(), Some("@bob:example.invalid"));
        assert_eq!(event.timestamp_ms, Some(1_800_000_000_000));
        assert_eq!(event.body_preview.as_deref(), Some("Pinned reply"));
        assert_eq!(
            event.thread_root_event_id.as_deref(),
            Some("$root:example.invalid")
        );
        assert_eq!(event.state, PinnedEventState::Ready);
    }
}
