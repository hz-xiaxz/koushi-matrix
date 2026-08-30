use super::*;
use crate::commands::contracts::fake_request_id;

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
