// @vitest-environment jsdom
import { renderToStaticMarkup } from "react-dom/server";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import { SpaceInfoPanel } from "./SpaceInfoPanel";

afterEach(cleanup);

describe("SpaceInfoPanel", () => {
  test("renders space identity, child rooms, unread total, and Element-like entries", () => {
    const markup = renderToStaticMarkup(
      <SpaceInfoPanel
        fallbackName="Synthetic Workspace"
        rooms={[
          {
            room_id: "!room-alpha:example.invalid",
            display_name: "Alpha Upstream",
            display_label: "Alpha Local",
            original_display_label: "Alpha Upstream",
            avatar: null,
            is_dm: false,
            dm_user_ids: [],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: ["!space-work:example.invalid"],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 8
          },
          {
            room_id: "!room-beta:example.invalid",
            display_name: "Beta Room",
            display_label: "Beta Room",
            original_display_label: "Beta Room",
            avatar: null,
            is_dm: false,
            dm_user_ids: [],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: ["!space-work:example.invalid"],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 2
          },
          {
            room_id: "!dm-alice:example.invalid",
            display_name: "Alice",
            display_label: "Alice",
            original_display_label: "Alice",
            avatar: null,
            is_dm: true,
            dm_user_ids: ["@alice:example.invalid"],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: ["!space-work:example.invalid"],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 4
          }
        ]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: null,
          display_name: "Synthetic Workspace",
          avatar: null,
          child_room_ids: ["!room-alpha:example.invalid", "!room-beta:example.invalid"]
        }}
      />
    );

    expect(markup).toContain("Synthetic Workspace");
    expect(markup).toContain("!space-work:example.invalid");
    expect(markup).toContain("Rooms");
    expect(markup).toContain("2");
    expect(markup).toContain("Unread");
    expect(markup).toContain("10");
    expect(markup).toContain("Alpha Local");
    expect(markup).not.toContain("Alpha Upstream");
    expect(markup).toContain("Beta Room");
    expect(markup).not.toContain("Alice");
    expect(markup).toContain("Home");
    expect(markup).toContain("Preferences");
    expect(markup).toContain("Space settings");
    expect(markup).toContain("Invite");
    expect(markup).toContain("Space preferences");
    expect(markup).toContain("Room membership");
    expect(markup).toContain("Child rooms");
    expect(markup).toContain("Direct Messages");
    expect(markup).toContain("Global DM list");
  });

  test("renders account home summary when no Space is selected", () => {
    const markup = renderToStaticMarkup(
      <SpaceInfoPanel
        fallbackName="Home"
        rooms={[
          {
            room_id: "!room-alpha:example.invalid",
            display_name: "Alpha Room",
            display_label: "Alpha Room",
            original_display_label: "Alpha Room",
            avatar: null,
            is_dm: false,
            dm_user_ids: [],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: [],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 8
          },
          {
            room_id: "!dm-alice:example.invalid",
            display_name: "Alice",
            display_label: "Alice",
            original_display_label: "Alice",
            avatar: null,
            is_dm: true,
            dm_user_ids: ["@alice:example.invalid"],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: [],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 4
          }
        ]}
        space={null}
      />
    );

    expect(markup).toContain("Home");
    expect(markup).toContain("All rooms");
    expect(markup).toContain("Alpha Room");
    expect(markup).not.toContain("Alice");
  });

  test("does not render a dense member list in the space info panel", () => {
    render(
      <SpaceInfoPanel
        fallbackName="Synthetic Workspace"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: null,
          display_name: "Synthetic Workspace",
          avatar: null,
          child_room_ids: []
        }}
        roomManagement={{
          selected_room_id: "!space-work:example.invalid",
          settings: {
            room_id: "!space-work:example.invalid",
            name: "Synthetic Workspace",
            topic: null,
            avatar_url: null,
            join_rule: "invite",
            history_visibility: "shared",
            permissions: {
              can_edit_settings: false,
              can_edit_roles: false,
              can_invite: false,
              can_kick: false,
              can_ban: false,
              can_unban: false
            },
            members: [
              {
                user_id: "@ada:example.invalid",
                display_name: "Ada",
                display_label: "Ada",
                original_display_label: "Ada",
                avatar_url: null,
                power_level: 0,
                role: "user",
                membership: "joined" as const,
                role_options: []
              }
            ]
          },
          operation: { kind: "idle" }
        }}
      />
    );

    expect(screen.queryByRole("button", { name: "Message Ada" })).toBeNull();
    expect(screen.queryByText("@ada:example.invalid")).toBeNull();
  });

  test("opens the standalone people panel from the members entry", () => {
    const onOpenMembers = vi.fn();
    render(
      <SpaceInfoPanel
        fallbackName="Synthetic Workspace"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: null,
          display_name: "Synthetic Workspace",
          avatar: null,
          child_room_ids: []
        }}
        onOpenMembers={onOpenMembers}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    expect(onOpenMembers).toHaveBeenCalledTimes(1);
  });

  test("autosaves local presentation edits without a save button", () => {
    const onSetLocalPresentation = vi.fn();
    render(
      <SpaceInfoPanel
        fallbackName="Synthetic Workspace"
        localIcon="SW"
        localName="Synthetic Workspace"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: null,
          display_name: "Synthetic Workspace",
          avatar: null,
          child_room_ids: []
        }}
        onSetLocalPresentation={onSetLocalPresentation}
      />
    );

    expect(screen.queryByRole("button", { name: "Save local presentation" })).toBeNull();

    fireEvent.change(screen.getByLabelText("Local name"), {
      target: { value: "Research" }
    });
    expect(onSetLocalPresentation).toHaveBeenLastCalledWith({
      name: "Research",
      icon: "SW"
    });

    fireEvent.change(screen.getByLabelText("Local icon"), {
      target: { value: "R" }
    });
    expect(onSetLocalPresentation).toHaveBeenLastCalledWith({
      name: "Research",
      icon: "R"
    });
  });

  // Issue #960: a local presentation name must not hide what the Space is
  // called on Matrix, and a Space without an `m.room.name` must not have an
  // alias or a computed name presented as its canonical one.
  test("shows the canonical Matrix name and the local name as separate facts", () => {
    render(
      <SpaceInfoPanel
        fallbackName="Fallback"
        localName="My Shortcut"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: "Research Group",
          display_name: "Research Group",
          avatar: null,
          child_room_ids: []
        }}
      />
    );

    const canonical = screen.getByText("Matrix name").closest(".settings-detail-row");
    expect(canonical?.textContent).toContain("Research Group");
    const local = screen.getByText("Local name", { selector: "span" }).closest(".settings-detail-row");
    expect(local?.textContent).toContain("My Shortcut");
    // The local name still wins the panel title, as it did before.
    expect(screen.getByRole("heading", { name: "My Shortcut" })).toBeTruthy();
  });

  test("reports a Space with no m.room.name as unnamed rather than borrowing its computed name", () => {
    render(
      <SpaceInfoPanel
        fallbackName="Fallback"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: null,
          display_name: "Alice and Bob",
          avatar: null,
          child_room_ids: []
        }}
      />
    );

    const canonical = screen.getByText("Matrix name").closest(".settings-detail-row");
    expect(canonical?.textContent).toContain("Not set");
    expect(canonical?.textContent).not.toContain("Alice and Bob");
  });

  // Issue #961: the Space's own room list shows the whole Space, not only the
  // rooms this account happens to have joined.
  test("lists children the account is not in, with their membership and a join action", () => {
    const onJoinRoom = vi.fn();
    render(
      <SpaceInfoPanel
        fallbackName="Fallback"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: "Work",
          display_name: "Work",
          avatar: null,
          child_room_ids: []
        }}
        spaceChildren={[
          {
            room_id: "!open:example.invalid",
            display_name: "Open Room",
            avatar: null,
            membership: "not_joined",
            can_join: true,
            is_space: false,
            joined_members: 4
          },
          {
            room_id: "!invited:example.invalid",
            display_name: "Invited Room",
            avatar: null,
            membership: "invited",
            can_join: true,
            is_space: false,
            joined_members: 2
          },
          {
            room_id: "!private:example.invalid",
            display_name: "!private:example.invalid",
            avatar: null,
            membership: "unknown",
            can_join: false,
            is_space: false,
            joined_members: 0
          }
        ]}
        onJoinRoom={onJoinRoom}
      />
    );

    const open = screen.getByText("Open Room").closest(".settings-detail-row");
    expect(open?.textContent).toContain("Not joined");
    const invited = screen.getByText("Invited Room").closest(".settings-detail-row");
    expect(invited?.textContent).toContain("Invited");

    // A room whose details the server withheld offers no join: the server's
    // permission model decides, and the panel does not guess around it.
    const unavailable = screen
      .getByText("!private:example.invalid")
      .closest(".settings-detail-row");
    expect(unavailable?.textContent).toContain("Unavailable");
    expect(unavailable?.querySelector("button")).toBeNull();

    fireEvent.click(open?.querySelector("button") as HTMLButtonElement);
    expect(onJoinRoom).toHaveBeenCalledWith("!open:example.invalid");
  });

  // An invitation belongs to the invite workflow, which owns the account's
  // invite list; joining around it would leave that list stale.
  test("answers an invited child through the invite workflow, not a join", () => {
    const onAcceptInvite = vi.fn();
    const onJoinRoom = vi.fn();
    render(
      <SpaceInfoPanel
        fallbackName="Fallback"
        rooms={[]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: "Work",
          display_name: "Work",
          avatar: null,
          child_room_ids: []
        }}
        spaceChildren={[
          {
            room_id: "!invited:example.invalid",
            display_name: "Invited Room",
            avatar: null,
            membership: "invited",
            can_join: true,
            is_space: false,
            joined_members: 2
          }
        ]}
        onAcceptInvite={onAcceptInvite}
        onJoinRoom={onJoinRoom}
      />
    );

    const invited = screen.getByText("Invited Room").closest(".settings-detail-row");
    fireEvent.click(invited?.querySelector("button") as HTMLButtonElement);
    expect(onAcceptInvite).toHaveBeenCalledWith("!invited:example.invalid");
    expect(onJoinRoom).not.toHaveBeenCalled();
  });

  test("never repeats a joined room in the not-joined part of the list", () => {
    render(
      <SpaceInfoPanel
        fallbackName="Fallback"
        rooms={[
          {
            room_id: "!joined:example.invalid",
            display_name: "Joined Room",
            display_label: "Joined Room",
            original_display_label: "Joined Room",
            avatar: null,
            is_dm: false,
            dm_user_ids: [],
            tags: { favourite: null, low_priority: null },
            parent_space_ids: ["!space-work:example.invalid"],
            dm_space_ids: [],
            is_encrypted: false,
            unread_count: 0
          }
        ]}
        space={{
          space_id: "!space-work:example.invalid",
          raw_name: "Work",
          display_name: "Work",
          avatar: null,
          child_room_ids: ["!joined:example.invalid"]
        }}
        spaceChildren={[
          {
            room_id: "!joined:example.invalid",
            display_name: "Joined Room",
            avatar: null,
            membership: "joined",
            can_join: false,
            is_space: false,
            joined_members: 3
          }
        ]}
      />
    );

    expect(screen.getAllByText("Joined Room")).toHaveLength(1);
  });
});
