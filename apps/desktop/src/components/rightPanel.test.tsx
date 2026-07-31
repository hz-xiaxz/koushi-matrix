// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import type {
  DesktopSnapshot,
  RoomManagementState,
  RoomMemberSummary,
  RoomSummary,
  SpaceMemberEntry,
  SpaceMembersState,
  SpaceSummary
} from "../domain/types";
import { ContextualRightPanel } from "./rightPanel";

const room: RoomSummary = {
  room_id: "!room-alpha:example.invalid",
  display_name: "Alpha Room",
  display_label: "Alpha Room",
  original_display_label: "Alpha Room",
  avatar: null,
  is_dm: false,
  dm_user_ids: [],
  tags: { favourite: null, low_priority: null },
  unread_count: 0,
  parent_space_ids: [],
  dm_space_ids: [],
  is_encrypted: false
};

const space: SpaceSummary = {
  space_id: "!space-work:example.invalid",
  display_name: "Workspace",
  avatar: null,
  child_room_ids: [room.room_id]
};

const roomMember: RoomMemberSummary = {
  user_id: "@room-member:example.invalid",
  display_name: "Room member",
  display_label: "Room member",
  original_display_label: "Room member",
  avatar_url: null,
  power_level: 0,
  role: "user"
};

const roomManagement: RoomManagementState = {
  selected_room_id: room.room_id,
  settings: {
    room_id: room.room_id,
    name: room.display_name,
    topic: null,
    avatar_url: null,
    join_rule: "invite",
    history_visibility: "shared",
    permissions: {
      can_edit_settings: true,
      can_edit_roles: true,
      can_kick: true,
      can_ban: true,
      can_unban: true
    },
    members: [roomMember]
  },
  operation: { kind: "idle" }
};

function spaceMember(
  userId: string,
  displayLabel: string,
  membership: SpaceMemberEntry["membership"],
  overrides: Partial<SpaceMemberEntry> = {}
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
    child_room_ids: [],
    invite_pending: false,
    ...overrides
  };
}

const spaceMembers: SpaceMembersState = {
  selected_space_id: space.space_id,
  generation: 1,
  space_joined: [
    spaceMember("@space-member:example.invalid", "Space member", "space_joined")
  ],
  space_invited: [],
  child_room_only: [
    spaceMember("@child-member:example.invalid", "Child member", "child_room_only", {
      child_room_ids: [room.room_id]
    })
  ],
  child_room_count: 1,
  complete_child_room_count: 1,
  incomplete_child_room_count: 0,
  operation: { kind: "idle" }
};

const snapshot = {
  state: {
    domain: {
      session: { user_id: "@current:example.invalid" },
      rooms: [room],
      spaces: [space],
      profile: { ignored_user_ids: [], users: {} },
      room_management: roomManagement,
      space_members: spaceMembers
    },
    ui: { timeline: { media_downloads: {} } }
  }
} as unknown as DesktopSnapshot;

type RightPanelProps = Parameters<typeof ContextualRightPanel>[0];

const defaultProps = {
  activeRoom: room,
  activeSpace: space,
  activeSpaceName: space.display_name,
  isRecoveryBusy: false,
  mode: "people" as const,
  peoplePanelScope: { kind: "room" as const, roomId: room.room_id },
  recoverySecretFilled: false,
  snapshot,
  searchQuery: "",
  searchResults: [],
  savedSessions: [],
  onClosePanel: vi.fn(),
  onOpenThread: vi.fn(),
  onOpenFiles: vi.fn(),
  onRefreshFilesView: vi.fn(),
  onPaginateThreadsList: vi.fn(),
  onOpenKeyboardSettings: vi.fn(),
  onOpenRecovery: vi.fn(),
  onProbeLocalEncryption: vi.fn(),
  onResetLocalData: vi.fn(),
  onRecoverySecretPresenceChange: vi.fn(),
  onReply: vi.fn(),
  onResultSelect: vi.fn(),
  onSubmitRecovery: vi.fn(),
  onSwitchAccount: vi.fn(),
  onAcceptVerification: vi.fn(),
  onBootstrapCrossSigning: vi.fn(),
  onCancelVerification: vi.fn(),
  onConfirmSasVerification: vi.fn(),
  onExportRoomKeys: vi.fn(),
  onImportRoomKeys: vi.fn(),
  onBootstrapSecureBackup: vi.fn(),
  onChangeSecureBackupPassphrase: vi.fn(),
  onEnableKeyBackup: vi.fn(),
  onResetIdentity: vi.fn(),
  onCancelIdentityReset: vi.fn(),
  onSubmitIdentityResetOAuth: vi.fn(),
  onSubmitIdentityResetPassword: vi.fn(),
  onThreadComposerDraftChange: vi.fn(),
  onOpenProfile: vi.fn(),
  onInviteUserToSpace: vi.fn(),
  canInviteToSpace: true
} as unknown as RightPanelProps;

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function renderPanel(overrides: Partial<RightPanelProps> = {}) {
  return render(<ContextualRightPanel {...defaultProps} {...overrides} />);
}

describe("ContextualRightPanel people composition", () => {
  test("renders SpaceMembersPanel for a Space scope and forwards Space callbacks", () => {
    const onInviteUserToSpace = vi.fn();
    const onOpenProfile = vi.fn();

    renderPanel({
      peoplePanelScope: { kind: "space", spaceId: space.space_id },
      onInviteUserToSpace,
      onOpenProfile,
      canInviteToSpace: true
    });

    expect(screen.getByRole("heading", { name: "Space members", level: 2 })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "People", level: 2 })).toBeNull();
    expect(screen.getByText("Space member")).toBeTruthy();
    expect(screen.getByText("Child member")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open profile for Space member" }));
    fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));

    expect(onOpenProfile).toHaveBeenCalledWith("@space-member:example.invalid");
    expect(onInviteUserToSpace).toHaveBeenCalledWith("@child-member:example.invalid");
  });

  test("keeps a Room scope on PeoplePanel and does not classify from Space state", () => {
    renderPanel({
      peoplePanelScope: { kind: "room", roomId: room.room_id }
    });

    expect(screen.getByRole("heading", { name: "People", level: 2 })).toBeTruthy();
    expect(screen.getByText("Room member")).toBeTruthy();
    expect(screen.queryByText("Space member")).toBeNull();
    expect(screen.queryByText("Child member")).toBeNull();
    expect(screen.getByRole("searchbox", { name: "Search room members" })).toBeTruthy();
  });
});
