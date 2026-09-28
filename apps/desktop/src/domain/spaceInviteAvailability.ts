import type {
  DesktopSnapshot,
  SpaceInviteAvailabilityReason,
  SpaceInviteCancellationAvailabilityReason
} from "./types";

// #1033: pure derivations over Rust-owned snapshot state (room-management
// settings slot and Space Members operation). React never infers permission
// beyond these projected facts; Rust keeps the authoritative checks.
export function exactRoomSettingsForRoom(
  snapshot: Pick<DesktopSnapshot, "state"> | null,
  roomId: string
) {
  if (!snapshot) {
    return null;
  }
  const roomManagement = snapshot.state.domain.room_management;
  return roomManagement.selected_room_id === roomId &&
    roomManagement.settings?.room_id === roomId
    ? roomManagement.settings
    : null;
}

export function spaceInviteAvailabilityReasonForSnapshot(
  snapshot: Pick<DesktopSnapshot, "state"> | null,
  spaceId: string
): SpaceInviteAvailabilityReason {
  if (
    snapshot?.state.ui.navigation.active_space_id !== spaceId ||
    snapshot.state.domain.space_members.selected_space_id !== spaceId
  ) {
    return "settings_unavailable";
  }
  const settings = exactRoomSettingsForRoom(snapshot, spaceId);
  if (!settings) {
    return "settings_unavailable";
  }
  if (!settings.permissions.can_invite) {
    return "permission_denied";
  }
  const operation = snapshot.state.domain.space_members.operation.kind;
  return operation === "loading" || operation === "inviting" || operation === "cancellingInvite"
    ? "operation_pending"
    : "available";
}

export function spaceInviteCancellationAvailabilityReasonForSnapshot(
  snapshot: Pick<DesktopSnapshot, "state"> | null,
  spaceId: string
): SpaceInviteCancellationAvailabilityReason {
  if (
    snapshot?.state.ui.navigation.active_space_id !== spaceId ||
    snapshot.state.domain.space_members.selected_space_id !== spaceId
  ) {
    return "settings_unavailable";
  }
  const settings = exactRoomSettingsForRoom(snapshot, spaceId);
  if (!settings) {
    return "settings_unavailable";
  }
  if (!settings.permissions.can_kick) {
    return "permission_denied";
  }
  const operation = snapshot.state.domain.space_members.operation.kind;
  return operation === "loading" || operation === "inviting" || operation === "cancellingInvite"
    ? "operation_pending"
    : "available";
}
