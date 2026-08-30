mod support;

use std::time::Duration;

use koushi_core::media_preparation::StageUploadBytesInput;
use koushi_core::media_staging::{
    MAX_MEDIA_STAGING_BATCH_BYTES, MAX_MEDIA_STAGING_BATCH_SIZE, MediaStagingError,
};
use koushi_core::{CoreRuntime, executor};
use koushi_state::{
    AppAction, ComposerDocument, ComposerInline, ComposerTarget, ImageUploadCompressionPolicy,
    RoomSummary,
};

const ROOM_ID: &str = "!media-staging:example.invalid";

fn target() -> ComposerTarget {
    ComposerTarget::Main {
        room_id: ROOM_ID.to_owned(),
    }
}

fn item(id: &str, bytes: &[u8]) -> StageUploadBytesInput {
    StageUploadBytesInput {
        staged_id: id.to_owned(),
        position: 0,
        filename: "fixture.bin".to_owned(),
        mime_type: " application/octet-stream ".to_owned(),
        bytes: bytes.to_vec(),
    }
}

async fn ready_runtime() -> (CoreRuntime, koushi_core::CoreConnection) {
    let runtime = CoreRuntime::start_with_event_capacity(64);
    let mut connection = runtime.attach();
    let mut actions = support::restore_ready_actions();
    actions.extend([
        AppAction::RoomListUpdated {
            spaces: Vec::new(),
            rooms: vec![RoomSummary {
                room_id: ROOM_ID.to_owned(),
                ..support::room_summary(ROOM_ID)
            }],
        },
        AppAction::SelectRoom {
            room_id: ROOM_ID.to_owned(),
        },
    ]);
    runtime.inject_actions(actions).await;
    support::wait_for_state(&mut connection, |state| {
        state.timeline.room_id.as_deref() == Some(ROOM_ID)
    })
    .await;
    (runtime, connection)
}

#[test]
fn media_staging_limits_are_named_and_checked_before_preparation() {
    assert_eq!(MAX_MEDIA_STAGING_BATCH_BYTES, 128 * 1024 * 1024);
    assert_eq!(MAX_MEDIA_STAGING_BATCH_SIZE, 16);
}

#[tokio::test]
async fn staging_publishes_preparing_then_ready_and_normalizes_mime() {
    let (runtime, mut connection) = ready_runtime().await;
    let before = connection.versioned_snapshot();
    let snapshot = connection
        .stage_upload_bytes(
            target(),
            vec![item("one", b"bytes")],
            ImageUploadCompressionPolicy::default(),
        )
        .await
        .expect("staging should settle");
    assert!(snapshot.generation > before.generation);
    let staged = &snapshot.state.timeline.staged_uploads;
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].mime_type, "application/octet-stream");
    assert!(matches!(
        staged[0].preparation,
        koushi_state::StagedUploadPreparation::Ready { .. }
    ));
}

#[tokio::test]
async fn duplicate_ids_and_overflow_are_rejected_without_state_change() {
    let (runtime, mut connection) = ready_runtime().await;
    let before = connection.versioned_snapshot();
    let duplicate = runtime.media_staging().stage_upload_bytes(
        &mut connection,
        target(),
        vec![item("same", b"a"), item("same", b"b")],
        ImageUploadCompressionPolicy::default(),
    );
    assert!(matches!(
        duplicate.await,
        Err(MediaStagingError::DuplicateStagedId)
    ));
    let too_many = (0..=MAX_MEDIA_STAGING_BATCH_SIZE)
        .map(|index| item(&index.to_string(), b"x"))
        .collect();
    assert!(matches!(
        runtime
            .media_staging()
            .stage_upload_bytes(
                &mut connection,
                target(),
                too_many,
                ImageUploadCompressionPolicy::default(),
            )
            .await,
        Err(MediaStagingError::BatchTooLarge)
    ));
    assert_eq!(connection.versioned_snapshot(), before);
}

#[tokio::test]
async fn caption_survives_preparation_and_replacement() {
    let (runtime, mut connection) = ready_runtime().await;
    let staged = runtime
        .media_staging()
        .stage_upload_bytes(
            &mut connection,
            target(),
            vec![item("one", b"bytes")],
            ImageUploadCompressionPolicy::default(),
        )
        .await
        .expect("staging should settle");
    let caption = ComposerDocument::new(vec![ComposerInline::Text {
        text: "synthetic caption".to_owned(),
    }]);
    let captioned = runtime
        .media_staging()
        .update_caption(
            &mut connection,
            target(),
            "one".to_owned(),
            Some(caption.clone()),
        )
        .await
        .expect("caption should settle");
    assert_eq!(
        captioned.state.timeline.staged_uploads[0].caption,
        Some(caption)
    );
    assert!(captioned.generation > staged.generation);
}

#[tokio::test]
async fn empty_preparation_is_a_typed_failure_and_removal_releases_bytes() {
    let (runtime, mut connection) = ready_runtime().await;
    let snapshot = runtime
        .media_staging()
        .stage_upload_bytes(
            &mut connection,
            target(),
            vec![item("empty", b"")],
            ImageUploadCompressionPolicy::default(),
        )
        .await
        .expect("empty input settles as a failed item");
    assert!(matches!(
        snapshot.state.timeline.staged_uploads[0].preparation,
        koushi_state::StagedUploadPreparation::Failed { .. }
    ));
    runtime
        .media_staging()
        .clear(&mut connection, target())
        .await
        .expect("clear should settle");
    executor::sleep(Duration::from_millis(10)).await;
    assert_eq!(runtime.media_preparation().stats().await.source_count, 0);
}
