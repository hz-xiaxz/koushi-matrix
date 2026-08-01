// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type {
  DesktopSnapshot,
  RoomManagementState,
  RoomMemberSummary,
  RoomSummary,
  SpaceMemberEntry,
  SpaceMembersState,
  SpaceSummary,
  UserProfile
} from "../domain/types";
import { ContextualRightPanel } from "./rightPanel";

class MockIntersectionObserver {
  static callback: IntersectionObserverCallback | null = null;

  constructor(callback: IntersectionObserverCallback) {
    MockIntersectionObserver.callback = callback;
  }

  observe(_element: Element): void {}

  unobserve(_element: Element): void {}

  disconnect(): void {}

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  static trigger(element: Element): void {
    MockIntersectionObserver.callback?.(
      [
        {
          isIntersecting: true,
          intersectionRatio: 1,
          target: element
        } as IntersectionObserverEntry
      ],
      {} as IntersectionObserver
    );
  }
}

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
      can_invite: true,
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
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

beforeEach(() => {
  MockIntersectionObserver.callback = null;
  vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
});

function renderPanel(overrides: Partial<RightPanelProps> = {}) {
  return render(<ContextualRightPanel {...defaultProps} {...overrides} />);
}

describe("ContextualRightPanel people composition", () => {
  test("forwards Space presentation data and the close action", () => {
    const onClosePanel = vi.fn();
    const administratorId = "@space-administrator:example.invalid";
    const presentationSnapshot = {
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          profile: {
            ignored_user_ids: [],
            users: {
              [administratorId]: {
                user_id: administratorId,
                display_name: "Space member",
                display_label: "Space member",
                original_display_label: "Space member",
                mention_search_terms: ["space", "member"],
                avatar: {
                  mxc_uri: "mxc://example.invalid/space-member-avatar",
                  thumbnail: {
                    kind: "ready",
                    source_url: "asset://space-member-avatar",
                    width: null,
                    height: null,
                    mime_type: null
                  }
                }
              } satisfies UserProfile
            }
          },
          space_members: {
            ...spaceMembers,
            space_joined: [
              spaceMember(administratorId, "Space member", "space_joined", {
                role: "administrator"
              }),
              spaceMember("@space-creator:example.invalid", "Space creator", "space_joined", {
                role: "creator"
              })
            ],
            child_room_only: []
          }
        }
      }
    } as unknown as DesktopSnapshot;

    renderPanel({
      snapshot: presentationSnapshot,
      peoplePanelScope: { kind: "space", spaceId: space.space_id },
      onClosePanel
    });

    fireEvent.click(screen.getByRole("button", { name: "Close Space members" }));

    expect(onClosePanel).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Administrator")).toBeTruthy();
    expect(screen.getByText("Creator")).toBeTruthy();
    expect(screen.getByRole("img", { name: "" }).querySelector("img")?.getAttribute("src")).toBe(
      "asset://space-member-avatar"
    );
  });

  test("forwards visibility-triggered Space avatar thumbnail requests", () => {
    const onRequestMemberAvatarThumbnail = vi.fn();
    const administratorId = "@space-administrator:example.invalid";
    const requestSnapshot = {
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          profile: {
            ignored_user_ids: [],
            users: {
              [administratorId]: {
                user_id: administratorId,
                display_name: "Space member",
                display_label: "Space member",
                original_display_label: "Space member",
                mention_search_terms: ["space", "member"],
                avatar: {
                  mxc_uri: "mxc://example.invalid/space-member-avatar",
                  thumbnail: { kind: "notRequested" }
                }
              } satisfies UserProfile
            }
          },
          space_members: {
            ...spaceMembers,
            space_joined: [
              spaceMember(administratorId, "Space member", "space_joined", {
                role: "administrator"
              })
            ],
            child_room_only: []
          }
        }
      }
    } as unknown as DesktopSnapshot;

    renderPanel({
      snapshot: requestSnapshot,
      peoplePanelScope: { kind: "space", spaceId: space.space_id },
      onRequestMemberAvatarThumbnail
    });

    expect(onRequestMemberAvatarThumbnail).not.toHaveBeenCalled();
    const row = screen.getByText("Space member").closest("li");
    expect(row).not.toBeNull();
    MockIntersectionObserver.trigger(row!);

    expect(onRequestMemberAvatarThumbnail).toHaveBeenCalledTimes(1);
    expect(onRequestMemberAvatarThumbnail).toHaveBeenCalledWith(
      "mxc://example.invalid/space-member-avatar"
    );
  });

  test("renders SpaceMembersPanel for a Space scope and forwards Space callbacks", () => {
    const onInviteUserToSpace = vi.fn();
    const onOpenProfile = vi.fn();
    const onOpenContextMenu = vi.fn();

    renderPanel({
      peoplePanelScope: { kind: "space", spaceId: space.space_id },
      onInviteUserToSpace,
      onOpenProfile,
      onOpenContextMenu,
      canInviteToSpace: true
    });

    expect(screen.getByRole("heading", { name: "Space members", level: 2 })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "People", level: 2 })).toBeNull();
    expect(screen.getByText("Space member")).toBeTruthy();
    expect(screen.getByText("Child member")).toBeTruthy();
    expect(screen.getByText("In child rooms: Alpha Room")).toBeTruthy();
    expect(screen.queryByText(room.room_id)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open profile for Space member" }));
    fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));
    fireEvent.contextMenu(screen.getByText("Child member").closest("li")!);

    expect(onOpenProfile).toHaveBeenCalledWith("@space-member:example.invalid");
    expect(onInviteUserToSpace).toHaveBeenCalledWith("@child-member:example.invalid");
    expect(onOpenContextMenu).toHaveBeenCalledWith(
      expect.anything(),
      {
        kind: "spaceMember",
        spaceId: space.space_id,
        userId: "@child-member:example.invalid",
        generation: 1
      },
      expect.arrayContaining([expect.objectContaining({ id: "inviteUserToSpace" })])
    );
  });

  test("forwards the inline Space invite cancellation callback and gate", () => {
    const invitedUserId = "@invited-member:example.invalid";
    const onCancelInvite = vi.fn();
    const cancellationSnapshot = structuredClone(snapshot);
    cancellationSnapshot.state.domain.space_members = {
      ...spaceMembers,
      space_invited: [spaceMember(invitedUserId, "Invited member", "space_invited")]
    };

    renderPanel({
      snapshot: cancellationSnapshot,
      peoplePanelScope: { kind: "space", spaceId: space.space_id },
      onCancelInvite,
      canCancelInvite: true,
      cancelAvailabilityReason: "available"
    });

    fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));

    expect(onCancelInvite).toHaveBeenCalledWith(invitedUserId);
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

  test("uses a child-room count when the only room label is its identifier fallback", () => {
    const identifierRoomId = "!identifier-only:example.invalid";
    const identifierRoom: RoomSummary = {
      ...room,
      room_id: identifierRoomId,
      display_name: identifierRoomId,
      display_label: identifierRoomId,
      original_display_label: identifierRoomId
    };
    const identifierSpace: SpaceSummary = {
      ...space,
      child_room_ids: [identifierRoomId]
    };
    const identifierSnapshot = {
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          rooms: [identifierRoom],
          spaces: [identifierSpace],
          space_members: {
            ...spaceMembers,
            child_room_only: [
              spaceMember("@identifier-child:example.invalid", "Child member", "child_room_only", {
                child_room_ids: [identifierRoomId]
              })
            ],
            child_room_count: 1
          }
        }
      }
    } as unknown as DesktopSnapshot;

    renderPanel({
      snapshot: identifierSnapshot,
      activeRoom: identifierRoom,
      activeSpace: identifierSpace,
      peoplePanelScope: { kind: "space", spaceId: identifierSpace.space_id }
    });

    expect(screen.getByText("In 1 child rooms")).toBeTruthy();
    expect(screen.queryByText(identifierRoomId)).toBeNull();
  });
});
