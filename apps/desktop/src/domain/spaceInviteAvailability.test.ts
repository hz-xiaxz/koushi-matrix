import { describe, expect, it } from "vitest";

import type { DesktopSnapshot, RoomManagementState, SpaceMembersState } from "./types";
import {
  exactRoomSettingsForRoom,
  spaceInviteAvailabilityReasonForSnapshot,
  spaceInviteCancellationAvailabilityReasonForSnapshot
} from "./spaceInviteAvailability";

const spaceId = "!space:example.test";
const childRoomId = "!child:example.test";

function settingsFor(roomId: string, canInvite: boolean): NonNullable<RoomManagementState["settings"]> {
  return {
    room_id: roomId,
    name: "Synthetic",
    topic: null,
    avatar_url: null,
    join_rule: "invite",
    history_visibility: "shared",
    permissions: {
      can_edit_settings: false,
      can_change_join_rule: false,
      can_edit_roles: false,
      can_invite: canInvite,
      can_kick: canInvite,
      can_ban: false,
      can_unban: false
    },
    members: []
  } as NonNullable<RoomManagementState["settings"]>;
}

function snapshotWith(
  roomManagement: RoomManagementState,
  operation: SpaceMembersState["operation"] = { kind: "idle" }
): Pick<DesktopSnapshot, "state"> {
  return {
    state: {
      domain: {
        room_management: roomManagement,
        space_members: { selected_space_id: spaceId, generation: 1, operation }
      },
      ui: { navigation: { active_space_id: spaceId } }
    }
  } as unknown as Pick<DesktopSnapshot, "state">;
}

describe("space invite availability from Rust snapshots (#1033)", () => {
  it("reports temporary settings unavailability when the settings slot moves to another room and recovers", () => {
    const authorized = snapshotWith({
      selected_room_id: spaceId,
      settings: settingsFor(spaceId, true),
      operation: { kind: "idle" }
    });
    expect(spaceInviteAvailabilityReasonForSnapshot(authorized, spaceId)).toBe("available");

    // The single Rust room-management slot now holds a child room's settings
    // (for example after a room-scoped settings load); the Space's own
    // settings are no longer exactly available.
    const replaced = snapshotWith({
      selected_room_id: childRoomId,
      settings: settingsFor(childRoomId, true),
      operation: { kind: "idle" }
    });
    expect(exactRoomSettingsForRoom(replaced, spaceId)).toBeNull();
    expect(spaceInviteAvailabilityReasonForSnapshot(replaced, spaceId)).toBe("settings_unavailable");
    expect(spaceInviteCancellationAvailabilityReasonForSnapshot(replaced, spaceId)).toBe(
      "settings_unavailable"
    );

    const reloading = snapshotWith({
      selected_room_id: spaceId,
      settings: null,
      operation: { kind: "idle" }
    } as unknown as RoomManagementState);
    expect(spaceInviteAvailabilityReasonForSnapshot(reloading, spaceId)).toBe("settings_unavailable");

    expect(spaceInviteAvailabilityReasonForSnapshot(authorized, spaceId)).toBe("available");
  });

  it("distinguishes real permission denial and pending operations from unavailability", () => {
    const denied = snapshotWith({
      selected_room_id: spaceId,
      settings: settingsFor(spaceId, false),
      operation: { kind: "idle" }
    });
    expect(spaceInviteAvailabilityReasonForSnapshot(denied, spaceId)).toBe("permission_denied");
    expect(spaceInviteCancellationAvailabilityReasonForSnapshot(denied, spaceId)).toBe(
      "permission_denied"
    );

    const pending = snapshotWith(
      { selected_room_id: spaceId, settings: settingsFor(spaceId, true), operation: { kind: "idle" } },
      { kind: "loading", request_id: null, space_id: spaceId, generation: 1 }
    );
    expect(spaceInviteAvailabilityReasonForSnapshot(pending, spaceId)).toBe("operation_pending");
  });
});
