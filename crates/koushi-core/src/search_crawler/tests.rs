use super::*;

#[test]
fn crawler_page_producer_records_typed_progress_without_environment_switch() {
    let _diagnostic_lock = koushi_diagnostics::test_support::lock();
    trace_crawler_page(DiagnosticLevel::Debug, "test_progress", 12, 9, 4);
    let record = koushi_diagnostics::snapshot()
        .records
        .into_iter()
        .rev()
        .find(|record| {
            record.event.source == "core.startup" && record.event.stage == "crawler_page"
        })
        .expect("crawler producer should record");
    assert!(
        record
            .event
            .fields
            .iter()
            .any(|field| field.key == "processed")
    );
    assert!(
        record
            .event
            .fields
            .iter()
            .any(|field| field.key == "indexed")
    );
}

#[test]
fn crawler_indexes_text_message_without_attachment_bytes() {
    let json = r#"{
            "event_id": "$e1:test",
            "sender": "@alice:test",
            "origin_server_ts": 1000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "hello historical world"
            }
        }"#;
    let settings = SearchCrawlerSettings {
        speed: SearchCrawlerSpeed::Standard,
        include_media_captions: true,
        include_filenames: true,
    };
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();
    match message {
        SearchIndexMessage::Upsert {
            room_id,
            event_id,
            sender,
            body,
            attachment,
            ..
        } => {
            assert_eq!(room_id, "!r:test");
            assert_eq!(event_id, "$e1:test");
            assert_eq!(sender, "@alice:test");
            assert_eq!(body.as_deref(), Some("hello historical world"));
            assert!(attachment.is_none());
        }
        other => panic!("expected Upsert, got {other:?}"),
    }
}

#[test]
fn crawler_indexes_image_metadata_and_filename_not_bytes() {
    let json = r#"{
            "event_id": "$e2:test",
            "sender": "@bob:test",
            "origin_server_ts": 2000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "sunset.png",
                "url": "mxc://example/a",
                "info": {
                    "mimetype": "image/png",
                    "size": 12345,
                    "w": 800,
                    "h": 600,
                    "thumbnail_url": "mxc://example/t"
                }
            }
        }"#;
    let settings = SearchCrawlerSettings::default();
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();
    match message {
        SearchIndexMessage::Upsert {
            body,
            attachment_filename,
            attachment,
            ..
        } => {
            assert_eq!(body.as_deref(), Some("sunset.png"));
            assert_eq!(attachment_filename.as_deref(), Some("sunset.png"));
            let attachment = attachment.expect("attachment metadata should be indexed");
            assert_eq!(attachment.source_mxc, "mxc://example/a");
            assert_eq!(attachment.thumbnail_mxc.as_deref(), Some("mxc://example/t"));
            assert_eq!(attachment.mimetype.as_deref(), Some("image/png"));
            assert_eq!(attachment.size, Some(12345));
            assert_eq!(attachment.width, Some(800));
            assert_eq!(attachment.height, Some(600));
        }
        other => panic!("expected Upsert, got {other:?}"),
    }
}

#[test]
fn crawler_does_not_index_edit_wrapper_as_standalone_message() {
    let json = r#"{
            "event_id": "$edit:test",
            "sender": "@alice:test",
            "origin_server_ts": 3000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "* wrapper body",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited body"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$e1:test"
                }
            }
        }"#;
    let settings = SearchCrawlerSettings::default();
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();
    assert!(
        matches!(message, SearchIndexMessage::Edit { .. }),
        "edit wrapper must become a target mutation, not an Upsert"
    );
}

#[test]
fn crawler_indexes_edit_events_as_replacement_mutations() {
    let json = r#"{
            "event_id": "$edit:test",
            "sender": "@alice:test",
            "origin_server_ts": 3000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "* wrapper body must not be indexed",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited historical body"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$e1:test"
                }
            }
        }"#;
    let settings = SearchCrawlerSettings::default();
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();

    match message {
        SearchIndexMessage::Edit {
            edit_event_id,
            target_event_id,
            sender,
            body,
            ..
        } => {
            assert_eq!(edit_event_id, "$edit:test");
            assert_eq!(target_event_id, "$e1:test");
            assert_eq!(sender, "@alice:test");
            assert_eq!(body.as_deref(), Some("edited historical body"));
        }
        other => panic!("expected Edit, got {other:?}"),
    }
}

#[test]
fn crawler_respects_include_media_captions_setting() {
    let json = r#"{
            "event_id": "$e3:test",
            "sender": "@bob:test",
            "origin_server_ts": 4000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "image.png",
                "url": "mxc://example/b",
                "info": { "mimetype": "image/png" }
            }
        }"#;
    let mut settings = SearchCrawlerSettings::default();
    settings.include_media_captions = false;
    settings.include_filenames = true;
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();
    match message {
        SearchIndexMessage::Upsert {
            body,
            attachment_filename,
            ..
        } => {
            assert!(body.is_none());
            assert_eq!(attachment_filename.as_deref(), Some("image.png"));
        }
        other => panic!("expected Upsert, got {other:?}"),
    }
}

#[test]
fn crawler_respects_include_filenames_setting() {
    let json = r#"{
            "event_id": "$e4:test",
            "sender": "@bob:test",
            "origin_server_ts": 5000,
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "image.png",
                "url": "mxc://example/c",
                "info": { "mimetype": "image/png" }
            }
        }"#;
    let mut settings = SearchCrawlerSettings::default();
    settings.include_media_captions = true;
    settings.include_filenames = false;
    let mut pending = HashSet::new();
    let message = event_json_to_index_message("!r:test", json, &settings, &mut pending).unwrap();
    match message {
        SearchIndexMessage::Upsert {
            body,
            attachment_filename,
            attachment,
            ..
        } => {
            assert_eq!(body.as_deref(), Some("image.png"));
            assert!(attachment_filename.is_none());
            assert!(attachment.is_none());
        }
        other => panic!("expected Upsert, got {other:?}"),
    }
}

#[test]
fn crawler_redaction_targets_redacts_field_not_event_id() {
    // A backward crawl sees the redaction first (newer), then the original
    // (older).  The redaction must remove the TARGET event id, not itself,
    // and must record the target in `pending_redactions` so a subsequent
    // Upsert for the original is suppressed.
    let redaction_json = r#"{
            "event_id": "$redact:test",
            "sender": "@alice:test",
            "origin_server_ts": 9000,
            "type": "m.room.redaction",
            "redacts": "$original:test"
        }"#;
    let settings = SearchCrawlerSettings::default();
    let mut pending = HashSet::new();
    let msg =
        event_json_to_index_message("!r:test", redaction_json, &settings, &mut pending).unwrap();
    // Must Redact the TARGET, not the redaction event itself.
    match msg {
        SearchIndexMessage::Redact { event_id } => {
            assert_eq!(
                event_id, "$original:test",
                "Redact must target the original event, not the redaction event"
            );
        }
        other => panic!("expected Redact, got {other:?}"),
    }
    // The target must be in pending_redactions so a later Upsert is skipped.
    assert!(
        pending.contains("$original:test"),
        "target should be in pending_redactions set"
    );
    assert!(
        !pending.contains("$redact:test"),
        "redaction event id itself must not be in pending_redactions"
    );
}

#[test]
fn crawler_redaction_via_content_field() {
    // MSC2174: some servers nest `redacts` inside `content`.
    let redaction_json = r#"{
            "event_id": "$redact2:test",
            "sender": "@alice:test",
            "origin_server_ts": 9001,
            "type": "m.room.redaction",
            "content": {
                "redacts": "$original2:test",
                "reason": "spam"
            }
        }"#;
    let settings = SearchCrawlerSettings::default();
    let mut pending = HashSet::new();
    let msg =
        event_json_to_index_message("!r:test", redaction_json, &settings, &mut pending).unwrap();
    match msg {
        SearchIndexMessage::Redact { event_id } => {
            assert_eq!(event_id, "$original2:test");
        }
        other => panic!("expected Redact, got {other:?}"),
    }
    assert!(pending.contains("$original2:test"));
}

// -----------------------------------------------------------------------
// P1-B: Shutdown drain — channel backpressure must not cause deadlock
// -----------------------------------------------------------------------

/// Verifies the drain-while-await pattern used in `SearchActor::run`'s
/// Shutdown arm.  A task that is blocked on `channel.send().await` must
/// be able to complete after the receiver resumes draining, and the whole
/// sequence must finish within a bounded time (no deadlock).
///
/// This is a pure channel-level test; it does not require the full
/// SDK/actor infrastructure and runs without a network connection.
#[tokio::test]
async fn shutdown_drain_completes_within_bounded_time_when_channel_was_full() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::mpsc;

    // Capacity 2: the task can queue 2 messages without blocking, but
    // the 3rd send will block until the receiver drains one slot.
    let (tx, mut rx) = mpsc::channel::<u32>(2);
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();

    let tx_clone = tx.clone();
    let task = tokio::spawn(async move {
        // Fill to capacity without blocking.
        tx_clone.send(1).await.ok();
        tx_clone.send(2).await.ok();
        // This send blocks until the receiver drains at least one slot.
        let _ = tx_clone.send(3).await;
        done_clone.store(true, Ordering::Relaxed);
    });
    tokio::pin!(task);

    // Simulate the actor's Shutdown drain loop: drain the receiver while
    // awaiting the task handle.  Without draining, the task would be
    // stuck on the blocked send forever.
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            tokio::select! {
                biased;
                _ = &mut task => break,
                _ = rx.recv() => {}
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "shutdown drain must complete within 5 s — timed out (deadlock regression)"
    );
    assert!(
        done.load(Ordering::Relaxed),
        "task must have signalled completion after drain unblocked it"
    );
}
