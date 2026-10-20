//! #1554: Core must keep large state off the stack.
//!
//! Tokio worker threads get 2 MiB stacks on every platform, and Windows MSVC
//! code generation makes frames larger than Linux's, so a frame that fits here
//! can still overflow on a user's machine. The attachment send runs through
//! `AppCommand::AcceptComposerDraft` in the App command dispatcher, which used
//! to materialize a 1.2 MiB poll frame.
//!
//! This test runs the same attachment path on a deliberately small worker
//! stack in optimized builds (512 KiB; unoptimized builds use the normal
//! 2 MiB worker stack because their frames are larger than the shipped
//! binary's). It is the behavioral counterpart to the CI
//! `clippy::large_stack_frames` / `clippy::large_futures` gate: before #1554
//! was fixed the first command overflowed the stack and aborted the test
//! process; after the fix the whole stage-and-send path settles normally.

mod support;

use std::time::Duration;

use koushi_core::composer_draft_lifecycle::{ComposerDraftScope, ComposerRendererGeneration};
use koushi_core::media_preparation::StageUploadBytesInput;
use koushi_core::runtime::CoreRuntime;
use koushi_protocol::{AppCommand, CoreCommand};
use koushi_state::{AppAction, ComposerDraftRevision, ComposerTarget, RoomSummary};

const ROOM_ID: &str = "!stack-budget:example.invalid";

/// Optimized builds assert the production budget (512 KiB, a quarter of the
/// 2 MiB Tokio worker stack). Unoptimized builds have larger frames than the
/// shipped binary, so they use the real worker-stack size instead; the
/// release run is the one that proves the small budget, and the CI clippy
/// "Rust stack budget" gate covers both profiles structurally.
const WORKER_STACK_BYTES: usize = if cfg!(debug_assertions) {
    2 * 1024 * 1024
} else {
    512 * 1024
};

fn worker_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_stack_size(WORKER_STACK_BYTES)
        .enable_all()
        .build()
        .expect("small-stack test runtime")
}

fn target() -> ComposerTarget {
    ComposerTarget::Main {
        room_id: ROOM_ID.to_owned(),
    }
}

fn png_item() -> StageUploadBytesInput {
    // Synthetic 1x1 PNG; test-only bytes with no user data.
    StageUploadBytesInput {
        staged_id: "stack-budget-png".to_owned(),
        position: 1,
        filename: "fixture.png".to_owned(),
        mime_type: "image/png".to_owned(),
        bytes: vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ],
    }
}

#[test]
fn attachment_send_fits_a_small_worker_stack() {
    let runtime = worker_runtime();
    runtime.block_on(async {
        let data_dir = tempfile::tempdir().expect("runtime data directory");
        let credential_dir = tempfile::tempdir().expect("runtime credential directory");
        let core = CoreRuntime::start_with_data_dir_and_file_credentials(
            data_dir.path().to_owned(),
            credential_dir.path().to_owned(),
        );
        let mut connection = core.attach();

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
        core.inject_actions(actions).await;
        support::wait_for_state_event(&mut connection, |state| {
            matches!(state.session, koushi_state::SessionState::Ready(_))
                && state.timeline.room_id.as_deref() == Some(ROOM_ID)
        })
        .await;

        // Stage a real prepared attachment through the media-preparation
        // worker, then accept the composer draft that owns it. The upload
        // itself cannot reach a homeserver in this harness; the point is that
        // the dispatcher and its effects stay within a small worker stack.
        connection
            .stage_upload_bytes(target(), vec![png_item()], None)
            .await
            .expect("attachment staging settles on a small stack");
        support::wait_for_state_event(&mut connection, |state| {
            !state.timeline.staged_uploads.is_empty()
        })
        .await;

        let request_id = connection.next_request_id();
        support::submit_composer_command(
            &connection,
            CoreCommand::App(AppCommand::AcceptComposerDraft {
                request_id,
                expected_account: support::session_key(),
                target: target(),
                submitted_revision: ComposerDraftRevision::default(),
                consumes_draft: true,
            }),
        )
        .await
        .expect("accepting the composer draft submits");

        let generation: ComposerRendererGeneration = connection
            .begin_composer_draft_renderer_generation()
            .expect("renderer generation");
        let lease = connection
            .acquire_composer_draft_lease(
                generation,
                ComposerDraftScope {
                    account: support::session_key(),
                    target: target(),
                },
            )
            .expect("composer lease");
        // The fake account cannot resolve a room, so the send settles as a
        // typed error; what matters is that getting there does not overflow.
        let send = connection
            .send_prepared_uploads(
                support::session_key(),
                generation,
                lease,
                target(),
                ComposerDraftRevision::default(),
                None,
            )
            .await;
        assert!(
            send.is_err(),
            "an unresolvable room must fail the send, not succeed"
        );
        connection
            .release_composer_draft_lease(generation, lease)
            .expect("release composer lease");

        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            drop(connection);
            core.shutdown().await;
        })
        .await;
    });
}
