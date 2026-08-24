import type {
  SpaceMemberEntry,
  SpaceMemberRoleOption,
  SpaceMembersState
} from "../../domain/types";

export function compareSpaceMemberEntries(left: SpaceMemberEntry, right: SpaceMemberEntry): number {
  return (
    left.display_label.localeCompare(right.display_label) ||
    left.user_id.localeCompare(right.user_id)
  );
}

export function spaceMemberRoleOptionsForPowerLevel(
  powerLevel: number | null,
  callerPowerLevel = 101
): SpaceMemberRoleOption[] {
  if (powerLevel === null || callerPowerLevel <= powerLevel) {
    return [];
  }
  return [0, 50, 100]
    .filter((candidate) => candidate !== powerLevel && candidate < callerPowerLevel)
    .map((candidate) => ({
      power_level: candidate,
      role: candidate === 100 ? "administrator" : candidate === 50 ? "moderator" : "user",
      requires_confirmation: powerLevel >= 100 || candidate >= 100
    }));
}

function browserFakeSpaceMemberEntry(
  userId: string,
  displayLabel: string,
  membership: SpaceMemberEntry["membership"],
  childRoomIds: string[] = []
): SpaceMemberEntry {
  return {
    user_id: userId,
    display_name: displayLabel,
    display_label: displayLabel,
    original_display_label: displayLabel,
    avatar_url: null,
    power_level: 0,
    role: "user",
    membership,
    child_room_ids: childRoomIds,
    invite_pending: false,
    role_options: []
  };
}

export function emptyBrowserFakeSpaceMembersState(): SpaceMembersState {
  return {
    selected_space_id: null,
    generation: 0,
    space_joined: [],
    space_invited: [],
    child_room_only: [],
    child_room_count: 0,
    complete_child_room_count: 0,
    incomplete_child_room_count: 0,
    power_levels_revision: null,
    can_edit_roles: false,
    operation: { kind: "idle" }
  };
}

export function createBrowserFakeSpaceMembersState(spaceId: string): SpaceMembersState {
  const spaceJoined = [
    {
      ...browserFakeSpaceMemberEntry(
        "@joined:example.invalid",
        "Joined Member",
        "space_joined"
      ),
      role_options: spaceMemberRoleOptionsForPowerLevel(0)
    }
  ];
  const spaceInvited = [
    browserFakeSpaceMemberEntry(
      "@invited:example.invalid",
      "Invited Member",
      "space_invited"
    )
  ];
  const childRoomOnly = [
    browserFakeSpaceMemberEntry(
      "@child-only:example.invalid",
      "Child-only Member",
      "child_room_only",
      ["!room-alpha:example.invalid", "!room-planning:example.invalid"]
    )
  ];

  return {
    selected_space_id: spaceId,
    generation: 1,
    space_joined: spaceJoined,
    space_invited: spaceInvited,
    child_room_only: childRoomOnly,
    child_room_count: 2,
    complete_child_room_count: 1,
    incomplete_child_room_count: 1,
    power_levels_revision: "revision-1",
    can_edit_roles: true,
    operation: { kind: "idle" }
  };
}
