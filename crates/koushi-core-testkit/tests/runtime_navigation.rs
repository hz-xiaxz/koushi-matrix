//! Runtime navigation persistence integration tests.

use std::time::Duration;

use koushi_core::{CoreCommand, CoreRuntime, executor};
use koushi_protocol::command::AppCommand;
use koushi_state::{
    AppAction, HomeSelection, NavigationPreferenceUpdate, RoomSummary, SessionState,
    SpaceLocalPresentation, SpaceLocalPresentations, SpaceSummary,
};

mod support;
use support::*;

#[tokio::test]
async fn navigation_selection_persists_when_runtime_restarts() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let credential_dir = tempfile::tempdir().expect("credential dir");
    {
        let runtime = CoreRuntime::start_with_data_dir_and_file_credentials(
            data_dir.path().to_path_buf(),
            credential_dir.path().to_path_buf(),
        );
        let mut connection = runtime.attach();
        runtime
            .inject_actions(restore_ready_actions![
                AppAction::RoomListUpdated {
                    spaces: vec![space_summary(
                        "!space-a:example.test",
                        &["!room-a:example.test"],
                    )],
                    rooms: vec![
                        room_in_space("!room-a:example.test", "!space-a:example.test"),
                        room_summary("!room-home:example.test"),
                    ],
                },
                AppAction::SelectSpace {
                    space_id: Some("!space-a:example.test".to_owned()),
                },
                AppAction::SelectRoom {
                    room_id: "!room-a:example.test".to_owned(),
                },
            ])
            .await;

        wait_for_state(&mut connection, |state| {
            state.navigation.active_space_id.as_deref() == Some("!space-a:example.test")
                && state.navigation.active_room_id.as_deref() == Some("!room-a:example.test")
        })
        .await;
        // Causally wait for the post-commit navigation persist before exercising
        // a memory-clearing verification-gate transition.
        runtime
            .inject_composer_drafts_and_wait_for_testing(
                connection.snapshot().composer_drafts.clone(),
            )
            .await;
        runtime
            .inject_actions(vec![
                AppAction::SessionLocked,
                AppAction::AuthoritativeDeviceTrustChanged {
                    generation: 1,
                    transition_id: 1,
                    trust: koushi_state::CurrentDeviceTrustState::Verified,
                },
            ])
            .await;
        wait_for_state(&mut connection, |state| {
            matches!(state.session, SessionState::Ready(_))
                && state.navigation.active_space_id.as_deref() == Some("!space-a:example.test")
                && state.navigation.active_room_id.as_deref() == Some("!room-a:example.test")
        })
        .await;
        // Selection state is published before post-commit persistence. Ordered
        // shutdown is the causal barrier that proves persistence completed.
        drop(connection);
        runtime.shutdown().await;
    }

    let restarted = CoreRuntime::start_with_data_dir_and_file_credentials(
        data_dir.path().to_path_buf(),
        credential_dir.path().to_path_buf(),
    );
    let mut connection = restarted.attach();
    restarted
        .inject_actions(restore_ready_actions![AppAction::RoomListUpdated {
            spaces: vec![space_summary(
                "!space-a:example.test",
                &["!room-a:example.test"],
            )],
            rooms: vec![
                room_in_space("!room-a:example.test", "!space-a:example.test"),
                room_summary("!room-home:example.test"),
            ],
        },])
        .await;

    let snapshot = executor::timeout(Duration::from_secs(1), async {
        wait_for_state(&mut connection, |state| {
            matches!(state.session, SessionState::Ready(_))
                && state.navigation.active_space_id.as_deref() == Some("!space-a:example.test")
                && state.navigation.active_room_id.as_deref() == Some("!room-a:example.test")
        })
        .await
    })
    .await
    .expect("persisted navigation should be restored after room list reload");

    assert_eq!(
        snapshot
            .navigation
            .last_room_by_space_id
            .get("!space-a:example.test"),
        Some(&"!room-a:example.test".to_owned())
    );
}

#[tokio::test]
async fn legacy_navigation_import_persists_once_in_the_encrypted_store() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let credential_dir = tempfile::tempdir().expect("credential dir");
    let runtime = CoreRuntime::start_with_data_dir_and_file_credentials(
        data_dir.path().to_path_buf(),
        credential_dir.path().to_path_buf(),
    );
    let mut connection = runtime.attach();
    runtime.inject_actions(restore_ready_actions()).await;
    wait_for_state(&mut connection, |state| {
        matches!(state.session, SessionState::Ready(_))
    })
    .await;

    let imported = NavigationPreferenceUpdate::ImportLegacy {
        home_selection: Some(HomeSelection::DirectMessage {
            room_id: "!dm:example.test".to_owned(),
        }),
        space_local_presentations: SpaceLocalPresentations(std::collections::BTreeMap::from([(
            "!space:example.test".to_owned(),
            SpaceLocalPresentation {
                name: Some("Private local label".to_owned()),
                icon: Some("🧪".to_owned()),
            },
        )])),
    };
    connection
        .command_with_admission(CoreCommand::App(AppCommand::UpdateNavigationPreference {
            request_id: connection.next_request_id(),
            update: imported,
        }))
        .await
        .expect("import navigation preferences");

    let snapshot = connection.snapshot();
    assert!(snapshot.navigation.legacy_frontend_preferences_imported);
    assert!(matches!(
        snapshot.navigation.home_selection,
        HomeSelection::DirectMessage { ref room_id } if room_id == "!dm:example.test"
    ));

    connection
        .command_with_admission(CoreCommand::App(AppCommand::UpdateNavigationPreference {
            request_id: connection.next_request_id(),
            update: NavigationPreferenceUpdate::ImportLegacy {
                home_selection: Some(HomeSelection::Activity),
                space_local_presentations: SpaceLocalPresentations::default(),
            },
        }))
        .await
        .expect("admit ignored replay");
    assert!(matches!(
        connection.snapshot().navigation.home_selection,
        HomeSelection::DirectMessage { ref room_id } if room_id == "!dm:example.test"
    ));

    drop(connection);
    runtime.shutdown().await;

    let restarted = CoreRuntime::start_with_data_dir_and_file_credentials(
        data_dir.path().to_path_buf(),
        credential_dir.path().to_path_buf(),
    );
    let mut connection = restarted.attach();
    restarted.inject_actions(restore_ready_actions()).await;
    let restored = wait_for_state(&mut connection, |state| {
        state.navigation.legacy_frontend_preferences_imported
    })
    .await;
    assert_eq!(
        restored
            .navigation
            .space_local_presentations
            .0
            .get("!space:example.test")
            .and_then(|presentation| presentation.name.as_deref()),
        Some("Private local label")
    );
}

fn space_summary(space_id: &str, child_room_ids: &[&str]) -> SpaceSummary {
    SpaceSummary {
        space_id: space_id.to_owned(),
        display_name: "QA Space".to_owned(),
        avatar: None,
        child_room_ids: child_room_ids
            .iter()
            .map(|room_id| (*room_id).to_owned())
            .collect(),
    }
}

fn room_in_space(room_id: &str, space_id: &str) -> RoomSummary {
    RoomSummary {
        parent_space_ids: vec![space_id.to_owned()],
        ..room_summary(room_id)
    }
}
