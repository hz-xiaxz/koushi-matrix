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
    render(<SpaceMembersPanel state={state()} canInvite={true} onInviteUser={vi.fn()} />);

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
    render(<SpaceMembersPanel state={state()} canInvite={true} onInviteUser={vi.fn()} />);
    const search = screen.getByRole("searchbox", { name: "Search space members" });

    fireEvent.change(search, { target: { value: "Alicia" } });
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.queryByText("Bob")).toBeNull();
    expect(screen.queryByText("Carol")).toBeNull();

    fireEvent.change(search, { target: { value: "@carol:example.invalid" } });
    expect(screen.getByText("Carol")).toBeTruthy();
    expect(screen.queryByText("Alice")).toBeNull();
  });

  it("shows the child-room context and invokes the invite callback", () => {
    const onInviteUser = vi.fn();
    render(<SpaceMembersPanel state={state()} canInvite={true} onInviteUser={onInviteUser} />);

    expect(
      screen.getByText(
        "In child rooms: !room-alpha:example.invalid, !room-beta:example.invalid"
      )
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));

    expect(onInviteUser).toHaveBeenCalledWith("@carol:example.invalid");
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
      <SpaceMembersPanel state={pendingState} canInvite={true} onInviteUser={vi.fn()} />
    );
    expect(screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")).toBe(
      true
    );

    rerender(<SpaceMembersPanel state={state()} canInvite={false} onInviteUser={vi.fn()} />);
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
      />
    );

    expect(screen.getByRole("status").textContent).toContain("Some child rooms are still syncing");
  });

  it("renders a useful empty state when search matches no section", () => {
    render(<SpaceMembersPanel state={state()} canInvite={true} onInviteUser={vi.fn()} />);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "nobody" } });

    expect(screen.getByText("No space members found")).toBeTruthy();
  });
});
