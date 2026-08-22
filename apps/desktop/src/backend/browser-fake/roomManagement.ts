import type {
  DesktopSnapshot,
  RoomModerationAction,
  RoomPermissionFacts,
  RoomSettingChange,
  RoomSettingsSnapshot
} from "../../domain/types";

export function defaultRoomManagementState(): DesktopSnapshot["state"]["domain"]["room_management"] {
  return {
    selected_room_id: null,
    settings: null,
    operation: { kind: "idle" }
  };
}

export function editableRoomPermissionFacts(): RoomPermissionFacts {
  return {
    can_edit_settings: true,
    can_edit_roles: true,
    can_invite: true,
    can_kick: true,
    can_ban: true,
    can_unban: true
  };
}

export function readonlyRoomPermissionFacts(): RoomPermissionFacts {
  return {
    can_edit_settings: false,
    can_edit_roles: false,
    can_invite: false,
    can_kick: false,
    can_ban: false,
    can_unban: false
  };
}

export function roomMemberRoleFromPowerLevel(powerLevel: number): RoomSettingsSnapshot["members"][number]["role"] {
  if (powerLevel >= 100) {
    return "administrator";
  }
  if (powerLevel >= 50) {
    return "moderator";
  }
  return "user";
}

export function applyRoomSettingChange(
  settings: RoomSettingsSnapshot,
  change: RoomSettingChange
): RoomSettingsSnapshot {
  if ("name" in change) {
    return { ...settings, name: change.name };
  }
  if ("topic" in change) {
    return { ...settings, topic: change.topic };
  }
  if ("avatarUrl" in change) {
    return { ...settings, avatar_url: change.avatarUrl };
  }
  if ("joinRule" in change) {
    return { ...settings, join_rule: change.joinRule };
  }
  return { ...settings, history_visibility: change.historyVisibility };
}

export function roomModerationAllowed(
  permissions: RoomPermissionFacts,
  action: RoomModerationAction
): boolean {
  switch (action) {
    case "kick":
      return permissions.can_kick;
    case "ban":
      return permissions.can_ban;
    case "unban":
      return permissions.can_unban;
  }
}
