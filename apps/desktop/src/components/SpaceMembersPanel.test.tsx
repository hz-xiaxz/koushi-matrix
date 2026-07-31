// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  SpaceMemberEntry,
  SpaceMembersState
} from "../domain/types";
import { SpaceMembersPanel } from "./SpaceMembersPanel";

afterEach(() => {
  cleanup();
});

function member(
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
    power_level: null,
    role: "user",
    membership,
    child_room_ids: [],
    invite_pending: false,
    ...overrides
  };
}

function state(overrides: Partial<SpaceMembersState> = {}): SpaceMembersState {
  return {
    selected_space_id: "!space:example.invalid",
    generation: 4,
    space_joined: [
      member("@alice:example.invalid", "Alice", "space_joined", {
        original_display_label: "Alicia"
      })
    ],
    space_invited: [member("@bob:example.invalid", "Bob", "space_invited")],
    child_room_only: [
      member("@carol:example.invalid", "Carol", "child_room_only", {
        child_room_ids: ["!room-alpha:example.invalid", "!room-beta:example.invalid"]
      })
    ],
    child_room_count: 2,
    complete_child_room_count: 2,
    incomplete_child_room_count: 0,
    operation: { kind: "idle" },
    ...overrides
  };
}

describe("SpaceMembersPanel", () => {
  it("renders the classified sections in Space, pending, then child-room order", () => {
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );

    expect(screen.getAllByRole("heading", { level: 3 }).map((heading) => heading.textContent)).toEqual([
      "Space members",
      "Invitation pending",
      "Not in Space"
    ]);
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.getByText("Bob")).toBeTruthy();
    expect(screen.getByText("Carol")).toBeTruthy();
  });

  it("searches all sections by label, original label, and user id", () => {
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );
    const search = screen.getByRole("searchbox", { name: "Search space members" });

    fireEvent.change(search, { target: { value: "Alicia" } });
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.queryByText("Bob")).toBeNull();
    expect(screen.queryByText("Carol")).toBeNull();

    fireEvent.change(search, { target: { value: "@carol:example.invalid" } });
    expect(screen.getByText("Carol")).toBeTruthy();
    expect(screen.queryByText("Alice")).toBeNull();
  });

  it("uses compact child-room labels without exposing raw room ids", () => {
    const onInviteUser = vi.fn();
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={onInviteUser}
        onOpenProfile={vi.fn()}
        childRoomLabels={new Map([
          ["!room-alpha:example.invalid", "Alpha"],
          ["!room-beta:example.invalid", "Beta"]
        ])}
      />
    );

    expect(screen.getByText("In child rooms: Alpha, Beta")).toBeTruthy();
    expect(screen.queryByText(/!room-alpha:example\.invalid/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));

    expect(onInviteUser).toHaveBeenCalledWith("@carol:example.invalid");
  });

  it("uses a localized count for more than two child rooms without exposing ids", () => {
    render(
      <SpaceMembersPanel
        state={state({
          child_room_only: [
            member("@carol:example.invalid", "Carol", "child_room_only", {
              child_room_ids: [
                "!room-alpha:example.invalid",
                "!room-beta:example.invalid",
                "!room-gamma:example.invalid"
              ]
            })
          ],
          child_room_count: 3
        })}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
        childRoomLabels={new Map([
          ["!room-alpha:example.invalid", "Alpha"],
          ["!room-beta:example.invalid", "Beta"],
          ["!room-gamma:example.invalid", "Gamma"]
        ])}
      />
    );

    expect(screen.getByText("In 3 child rooms")).toBeTruthy();
    expect(screen.queryByText(/!room-(alpha|beta|gamma):example\.invalid/)).toBeNull();
  });

  it("announces invite failure and keeps the child-only row retryable", () => {
    const onInviteUser = vi.fn();
    render(
      <SpaceMembersPanel
        state={state({
          operation: {
            kind: "failed",
            request_id: 12,
            space_id: "!space:example.invalid",
            user_id: "@carol:example.invalid",
            generation: 4,
            failureKind: "network"
          }
        })}
        canInvite={true}
        onInviteUser={onInviteUser}
        onOpenProfile={vi.fn()}
      />
    );

    expect(screen.getByRole("alert").textContent).toMatch(/invite failed/i);
    const inviteButton = screen.getByRole("button", { name: "Invite to Space" });
    expect((inviteButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(inviteButton);
    expect(onInviteUser).toHaveBeenCalledWith("@carol:example.invalid");
  });

  it("forwards child-only row context menus with the exact Space target fence", () => {
    const onOpenContextMenu = vi.fn();
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
        onOpenContextMenu={onOpenContextMenu}
      />
    );

    fireEvent.contextMenu(screen.getByText("Carol").closest("li")!);

    expect(onOpenContextMenu).toHaveBeenCalledTimes(1);
    expect(onOpenContextMenu.mock.calls[0]?.[1]).toEqual({
      kind: "spaceMember",
      spaceId: "!space:example.invalid",
      userId: "@carol:example.invalid",
      generation: 4
    });
    expect(
      (onOpenContextMenu.mock.calls[0]?.[2] as Array<{ id: string }>).map((item) => item.id)
    ).toEqual(["inviteUserToSpace"]);
  });

  it("disables invite actions while pending or when the caller lacks permission", () => {
    const pendingState = state({
      child_room_only: [
        member("@carol:example.invalid", "Carol", "child_room_only", {
          invite_pending: true
        })
      ]
    });
    const { rerender } = render(
      <SpaceMembersPanel
        state={pendingState}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")).toBe(
      true
    );

    rerender(
      <SpaceMembersPanel
        state={state()}
        canInvite={false}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")).toBe(
      true
    );
  });

  it("announces incomplete child-room synchronization from Rust state", () => {
    render(
      <SpaceMembersPanel
        state={state({ incomplete_child_room_count: 1 })}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );

    expect(screen.getByRole("status").textContent).toContain("Some child rooms are still syncing");
  });

  it("renders a useful empty state when search matches no section", () => {
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "nobody" } });

    expect(screen.getByRole("status").textContent).toContain("No space members found");
  });

  it("opens a member profile from an accessible row action", () => {
    const onOpenProfile = vi.fn();
    render(
      <SpaceMembersPanel
        state={state()}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={onOpenProfile}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open profile for Alice" }));

    expect(onOpenProfile).toHaveBeenCalledWith("@alice:example.invalid");
  });

  it("disables invite actions while any Rust-owned operation is pending", () => {
    const pendingOperation = state({
      operation: {
        kind: "loading",
        request_id: 8,
        space_id: "!space:example.invalid",
        generation: 4
      }
    });

    render(
      <SpaceMembersPanel
        state={pendingOperation}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );

    expect(
      screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")
    ).toBe(true);
  });

  it("disables the in-flight invite target from state even without an entry flag", () => {
    const invitingState = state({
      operation: {
        kind: "inviting",
        request_id: 9,
        space_id: "!space:example.invalid",
        user_id: "@carol:example.invalid",
        generation: 4
      },
      child_room_only: [
        member("@carol:example.invalid", "Carol", "child_room_only", {
          invite_pending: false
        })
      ]
    });

    render(
      <SpaceMembersPanel
        state={invitingState}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
      />
    );

    expect(
      screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")
    ).toBe(true);
  });

  it("records only private-data-free UI diagnostic facts", () => {
    const diagnostics: string[] = [];
    render(
      <SpaceMembersPanel
        state={state({
          space_joined: [
            member("@private-user:example.invalid", "Private Person", "space_joined", {
              avatar_url: "mxc://example.invalid/private-avatar"
            })
          ],
          child_room_only: [
            member("@private-child:example.invalid", "Private Child", "child_room_only", {
              child_room_ids: ["!private-room:example.invalid"]
            })
          ],
          incomplete_child_room_count: 1
        })}
        canInvite={true}
        onInviteUser={vi.fn()}
        onOpenProfile={vi.fn()}
        onDiagnostic={(message) => diagnostics.push(message)}
      />
    );

    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "Private" } });
    fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));

    const joined = diagnostics.join("\n");
    expect(joined).toContain("rendered");
    expect(joined).toContain("search");
    expect(joined).toContain("availability");
    expect(joined).toContain("incomplete_notice=true");
    for (const privateValue of [
      "@private-user:example.invalid",
      "@private-child:example.invalid",
      "Private Person",
      "Private Child",
      "!private-room:example.invalid",
      "mxc://example.invalid/private-avatar"
    ]) {
      expect(joined).not.toContain(privateValue);
    }
  });
});
