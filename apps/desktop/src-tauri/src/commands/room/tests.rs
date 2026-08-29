use super::*;
use crate::commands::contracts::fake_request_id;

#[test]
fn invite_workflow_snapshot_terminals_are_exact_and_short_bounded() {
    assert_eq!(
        INVITE_WORKFLOW_CONVERGENCE_TIMEOUT,
        std::time::Duration::from_secs(2)
    );
    let mut state = koushi_state::AppState::default();
    assert!(invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Closed,
    ));
    assert!(!invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Open {
            room_id: "!room:test"
        },
    ));

    state.invite_workflow.query.room_id = Some("!room:test".to_owned());
    state.invite_workflow.query.query = "alice".to_owned();
    assert!(invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Open {
            room_id: "!room:test"
        },
    ));
    assert!(invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Search {
            room_id: "!room:test",
            query: "alice",
        },
    ));
    assert!(!invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Search {
            room_id: "!room:test",
            query: "bob",
        },
    ));
    assert!(!invite_workflow_snapshot_matches(
        &state,
        &InviteWorkflowTerminal::Closed,
    ));
}

enum InviteWorkflowWaitStep {
    Snapshot(koushi_state::AppState, u64),
    Lag(u64),
}

struct ScriptedInviteWorkflowSource {
    current: InviteWorkflowVersionedSnapshot,
    steps: std::collections::VecDeque<InviteWorkflowWaitStep>,
}

impl InviteWorkflowSnapshotSource for ScriptedInviteWorkflowSource {
    fn versioned_snapshot(&self) -> InviteWorkflowVersionedSnapshot {
        InviteWorkflowVersionedSnapshot {
            state: self.current.state.clone(),
            generation: self.current.generation,
        }
    }

    fn recv_event(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventStreamLag>> + Send + '_>> {
        match self.steps.pop_front() {
            Some(InviteWorkflowWaitStep::Snapshot(state, generation)) => {
                self.current = InviteWorkflowVersionedSnapshot { state, generation };
                Box::pin(std::future::ready(Ok(())))
            }
            Some(InviteWorkflowWaitStep::Lag(skipped)) => {
                Box::pin(std::future::ready(Err(EventStreamLag { skipped })))
            }
            None => Box::pin(std::future::pending()),
        }
    }
}

#[tokio::test]
async fn invite_workflow_wait_rechecks_after_lag_and_times_out_with_fixed_error() {
    let initial = koushi_state::AppState::default();
    let mut matching = initial.clone();
    matching.invite_workflow.query.room_id = Some("!space:test".to_owned());
    matching.invite_workflow.query.query = "alice".to_owned();
    let mut lagged = ScriptedInviteWorkflowSource {
        current: InviteWorkflowVersionedSnapshot {
            state: initial.clone(),
            generation: 1,
        },
        steps: [
            InviteWorkflowWaitStep::Lag(3),
            InviteWorkflowWaitStep::Snapshot(matching, 2),
        ]
        .into(),
    };

    let settled = wait_for_invite_workflow_snapshot_from(
        &mut lagged,
        InviteWorkflowTerminal::Search {
            room_id: "!space:test",
            query: "alice",
        },
        std::time::Duration::from_millis(50),
    )
    .await
    .expect("matching snapshot after lag should settle");
    assert_eq!(settled.generation, 2);

    let mut stalled = ScriptedInviteWorkflowSource {
        current: InviteWorkflowVersionedSnapshot {
            state: initial,
            generation: 1,
        },
        steps: std::collections::VecDeque::new(),
    };
    let error = wait_for_invite_workflow_snapshot_from(
        &mut stalled,
        InviteWorkflowTerminal::Open {
            room_id: "!missing:test",
        },
        std::time::Duration::from_millis(1),
    )
    .await
    .expect_err("non-matching snapshot should hit the fixed deadline");
    assert_eq!(error, INVITE_WORKFLOW_CONVERGENCE_ERROR);
}

#[test]
fn load_space_members_and_invite_user_to_space_build_exact_commands_and_wait_for_events() {
    match super::build_load_space_members_command(
        fake_request_id(301),
        "!space:example.org".to_owned(),
        4,
    ) {
        CoreCommand::Room(RoomCommand::LoadSpaceMembers {
            request_id,
            space_id,
            generation,
        }) => {
            assert_eq!(request_id, fake_request_id(301));
            assert_eq!(space_id, "!space:example.org");
            assert_eq!(generation, 4);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    match super::build_update_space_member_role_command(
        fake_request_id(306),
        "!space:example.org".to_owned(),
        "@child:example.org".to_owned(),
        4,
        Some("$power:example.org".to_owned()),
        0,
        50,
        false,
    ) {
        CoreCommand::Room(RoomCommand::UpdateSpaceMemberRole {
            request_id,
            space_id,
            user_id,
            generation,
            expected_power_levels_revision,
            expected_power_level,
            power_level,
            confirmed,
        }) => {
            assert_eq!(request_id, fake_request_id(306));
            assert_eq!(space_id, "!space:example.org");
            assert_eq!(user_id, "@child:example.org");
            assert_eq!(generation, 4);
            assert_eq!(
                expected_power_levels_revision.as_deref(),
                Some("$power:example.org")
            );
            assert_eq!(expected_power_level, 0);
            assert_eq!(power_level, 50);
            assert!(!confirmed);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    match super::build_cancel_space_invite_command(
        fake_request_id(305),
        "!space:example.org".to_owned(),
        "@child:example.org".to_owned(),
        4,
    ) {
        CoreCommand::Room(RoomCommand::CancelSpaceInvite {
            request_id,
            space_id,
            user_id,
            generation,
        }) => {
            assert_eq!(request_id, fake_request_id(305));
            assert_eq!(space_id, "!space:example.org");
            assert_eq!(user_id, "@child:example.org");
            assert_eq!(generation, 4);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    match super::build_invite_user_to_space_command(
        fake_request_id(302),
        "!space:example.org".to_owned(),
        "@child:example.org".to_owned(),
        4,
    ) {
        CoreCommand::Room(RoomCommand::InviteUserToSpace {
            request_id,
            space_id,
            user_id,
            generation,
        }) => {
            assert_eq!(request_id, fake_request_id(302));
            assert_eq!(space_id, "!space:example.org");
            assert_eq!(user_id, "@child:example.org");
            assert_eq!(generation, 4);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn space_member_event_waits_reject_wrong_generation() {
    let wrong_load = koushi_core::RoomEvent::SpaceMembersLoaded {
        request_id: fake_request_id(303),
        generation: 3,
        joined_count: 0,
        invited_count: 0,
        child_room_only_count: 0,
        incomplete_child_room_count: 0,
    };
    let matching_load = koushi_core::RoomEvent::SpaceMembersLoaded {
        request_id: fake_request_id(303),
        generation: 4,
        joined_count: 0,
        invited_count: 0,
        child_room_only_count: 0,
        incomplete_child_room_count: 0,
    };
    assert!(!super::space_members_loaded_event_matches(
        &wrong_load,
        fake_request_id(303),
        4,
    ));
    assert!(super::space_members_loaded_event_matches(
        &matching_load,
        fake_request_id(303),
        4,
    ));

    let wrong_invite = koushi_core::RoomEvent::SpaceMemberInviteSettled {
        request_id: fake_request_id(304),
        generation: 3,
        outcome: koushi_state::SpaceMemberInviteOutcome::Invited,
    };
    let matching_invite = koushi_core::RoomEvent::SpaceMemberInviteSettled {
        request_id: fake_request_id(304),
        generation: 4,
        outcome: koushi_state::SpaceMemberInviteOutcome::Invited,
    };
    assert!(!super::space_member_invite_settled_event_matches(
        &wrong_invite,
        fake_request_id(304),
        4,
    ));
    assert!(super::space_member_invite_settled_event_matches(
        &matching_invite,
        fake_request_id(304),
        4,
    ));

    let wrong_cancel = koushi_core::RoomEvent::SpaceMemberInviteCancellationSettled {
        request_id: fake_request_id(305),
        generation: 3,
        outcome: koushi_state::SpaceMemberInviteOutcome::Cancelled,
    };
    let matching_cancel = koushi_core::RoomEvent::SpaceMemberInviteCancellationSettled {
        request_id: fake_request_id(305),
        generation: 4,
        outcome: koushi_state::SpaceMemberInviteOutcome::Cancelled,
    };
    assert!(
        !super::space_member_invite_cancellation_settled_event_matches(
            &wrong_cancel,
            fake_request_id(305),
            4,
        )
    );
    assert!(
        super::space_member_invite_cancellation_settled_event_matches(
            &matching_cancel,
            fake_request_id(305),
            4,
        )
    );
}
