use super::*;

    async fn handle_pin_event(&self, request_id: RequestId, room_id: String, event_id: String) {
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

    async fn handle_unpin_event(&self, request_id: RequestId, room_id: String, event_id: String) {
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

    async fn handle_refresh_pinned_events(&self, request_id: RequestId, room_id: String) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        match load_pinned_events_for_room(session, &room_id).await {
            Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn handle_pinned_events_changed(&self, room_ids: BTreeSet<String>) {
        let Some(session) = &self.session else {
            return;
        };
        for room_id in room_ids {
            match load_pinned_events_for_room(session, &room_id).await {
                Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
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
            Ok(pinned) => self.project_pinned_events(room_id, pinned).await,
            Err(kind) => {
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    async fn project_pinned_events(&self, room_id: String, pinned: Vec<PinnedEvent>) {
        self.reduce_reliable(vec![AppAction::RoomPinnedEventsUpdated {
            room_id: room_id.clone(),
            pinned: pinned.clone(),
        }])
        .await;
        self.emit(CoreEvent::Room(RoomEvent::PinnedEventsUpdated {
            room_id,
            pinned,
        }));
    }

fn state_contains_pinned_events(state: &matrix_sdk_base::sync::State) -> bool {
    let events = match state {
        matrix_sdk_base::sync::State::Before(events)
        | matrix_sdk_base::sync::State::After(events) => events,
    };
    events.iter().any(|event| {
        serde_json::from_str::<serde_json::Value>(event.json().get())
            .ok()
            .and_then(|json| {
                json.get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("m.room.pinned_events")
    })
}

fn pinned_event_room_ids(updates: &matrix_sdk_base::sync::RoomUpdates) -> BTreeSet<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
        fn pin_success_settles_pending_before_pinned_projection_reload() {
            let source = include_str!("../room.rs");
            let pin_body = source
                .split("async fn handle_pin_event")
                .nth(1)
                .expect("pin handler")
                .split("async fn handle_unpin_event")
                .next()
                .expect("pin body");
            let unpin_body = source
                .split("async fn handle_unpin_event")
                .nth(1)
                .expect("unpin handler")
                .split("async fn project_pinned_events_after_success")
                .next()
                .expect("unpin body");
            let projection_body = source
                .split("async fn project_pinned_events_after_success")
                .nth(1)
                .expect("projection helper")
                .split("    /// Refresh the room list")
                .next()
                .expect("projection body");
    
            let pin_completion = pin_body
                .find("self.reduce_reliable(vec![AppAction::PinEventCompleted")
                .expect("pin completion action");
            let pin_reload = pin_body
                .find("project_pinned_events_after_success")
                .expect("pin projection reload");
            assert!(pin_completion < pin_reload);
    
            let unpin_completion = unpin_body
                .find("self.reduce_reliable(vec![AppAction::UnpinEventCompleted")
                .expect("unpin completion action");
            let unpin_reload = unpin_body
                .find("project_pinned_events_after_success")
                .expect("unpin projection reload");
            assert!(unpin_completion < unpin_reload);
    
            assert!(!projection_body.contains("AppAction::PinEventCompleted"));
            assert!(!projection_body.contains("AppAction::UnpinEventCompleted"));
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

    #[test]
        fn pin_and_unpin_commands_require_actor_known_room_guard_before_sdk_call() {
            let source = include_str!("../room.rs");
            let pin_body = source
                .split("async fn handle_pin_event")
                .nth(1)
                .expect("pin handler")
                .split("async fn handle_unpin_event")
                .next()
                .expect("pin body");
            let unpin_body = source
                .split("async fn handle_unpin_event")
                .nth(1)
                .expect("unpin handler")
                .split("async fn project_pinned_events_after_success")
                .next()
                .expect("unpin body");
    
            let pin_guard = pin_body
                .find("ensure_known_room_for_message_interaction")
                .expect("pin known-room guard");
            let pin_sdk = pin_body
                .find("koushi_sdk::pin_event")
                .expect("pin sdk call");
            assert!(pin_guard < pin_sdk);
    
            let unpin_guard = unpin_body
                .find("ensure_known_room_for_message_interaction")
                .expect("unpin known-room guard");
            let unpin_sdk = unpin_body
                .find("koushi_sdk::unpin_event")
                .expect("unpin sdk call");
            assert!(unpin_guard < unpin_sdk);
        }

}
