// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import { createBrowserFakeApi, type DesktopApi } from "./backend/browserFakeApi";
import type { DesktopSnapshot } from "./domain/types";

const tauriEventListeners = vi.hoisted(
  () => new Map<string, (event: { payload: unknown }) => void>()
);

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (eventName: string, listener: (event: { payload: unknown }) => void) => {
    tauriEventListeners.set(eventName, listener);
    return () => tauriEventListeners.delete(eventName);
  }
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFullscreen: async () => false,
    setFullscreen: async () => undefined,
    setTitle: async () => undefined,
    setBadgeCount: async () => undefined,
    startDragging: async () => undefined
  })
}));

async function renderAppWithApi(api: DesktopApi) {
  vi.resetModules();
  vi.doMock("./backend/client", () => ({
    createDesktopApi: () => api
  }));
  const { App } = await import("./App");
  return render(<App />);
}

async function clearProjectedSnapshot() {
  const { clearAppStoreSnapshot } = await import("./domain/appStore");
  clearAppStoreSnapshot();
}

async function openSpaceMembersFromSidebar() {
  const button = await screen.findByRole("button", {
    name: /Members, 1 joined, 1 only in child rooms/
  });
  await act(async () => {
    fireEvent.click(button);
  });
  await screen.findByRole("heading", { name: "Space members", level: 2 });
}

afterEach(async () => {
  cleanup();
  await clearProjectedSnapshot();
  vi.doUnmock("./backend/client");
  tauriEventListeners.clear();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.restoreAllMocks();
  vi.resetModules();
});

describe("App Space Members integration", () => {
  test("loads the selected Space projection before the panel is opened", async () => {
    const api = createBrowserFakeApi();
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });

    await waitFor(() => {
      expect(loadSpaceMembers).toHaveBeenCalledWith(
        "!space-beta:example.invalid",
        expect.any(Number)
      );
    });
    expect(screen.queryByRole("heading", { name: "Space members", level: 2 })).toBeNull();
  });

  test("clears the Space People scope on Space and Home navigation", async () => {
    const api = createBrowserFakeApi();
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Synthetic Lab" }).className).toContain(
        "is-active"
      );
    });
    expect(screen.queryByRole("heading", { name: "Space members", level: 2 })).toBeNull();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /^Home/ }));
    });
    expect(screen.queryByRole("heading", { name: "Space members", level: 2 })).toBeNull();
  });

  test("opens from the sidebar with Rust-owned counts and the exact Space generation", async () => {
    const api = createBrowserFakeApi();
    const loadRoomSettings = vi.spyOn(api, "loadRoomSettings");
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");
    const initial = await api.getSnapshot();
    const spaceId = initial.state.ui.navigation.active_space_id!;
    const generation = initial.state.domain.space_members.generation;

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    expect(loadRoomSettings).toHaveBeenCalledWith(spaceId);
    expect(loadSpaceMembers).toHaveBeenCalledWith(spaceId, generation);
    expect(screen.getByText("Joined Member")).toBeTruthy();
    expect(screen.getByText("Child-only Member")).toBeTruthy();
    expect(screen.getByText("Some child rooms are still syncing")).toBeTruthy();
  });

  test("uses the same open path from Space Info Members", async () => {
    const api = createBrowserFakeApi();
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");
    const initial = await api.getSnapshot();
    const spaceId = initial.state.ui.navigation.active_space_id!;
    const generation = initial.state.domain.space_members.generation;

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(
        await screen.findByRole("button", { name: "Space info and settings" })
      );
    });
    await screen.findByRole("button", { name: /^Members$/ });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /^Members$/ }));
    });
    await screen.findByRole("heading", { name: "Space members", level: 2 });

    expect(loadSpaceMembers).toHaveBeenCalledWith(spaceId, generation);
  });

  test("fences a late prior-Space member result after navigation", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const staleResult = structuredClone(initial);
    staleResult.state.domain.space_members.space_joined = [
      {
        ...staleResult.state.domain.space_members.space_joined[0]!,
        user_id: "@stale:example.invalid",
        display_label: "Stale prior Space member",
        original_display_label: "Stale prior Space member"
      }
    ];
    const pending = deferred<DesktopSnapshot>();
    const loadSpaceMembers = vi
      .spyOn(api, "loadSpaceMembers")
      .mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    const membersButton = await screen.findByRole("button", {
      name: /Members, 1 joined, 1 only in child rooms/
    });
    await act(async () => {
      fireEvent.click(membersButton);
    });
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledWith(
      "!space-alpha:example.invalid",
      1
    ));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Synthetic Lab" }).className).toContain(
        "is-active"
      );
    });

    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.queryByText("Stale prior Space member")).toBeNull();
  });

  test("uses the shared inline invite command and moves a child-only user to pending", async () => {
    const api = createBrowserFakeApi({ spaceMemberInviteOutcome: "pending" });
    const inviteUserToSpace = vi.spyOn(api, "inviteUserToSpace");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));
    });
    await waitFor(() => {
      expect(inviteUserToSpace).toHaveBeenCalledWith(
        "!space-alpha:example.invalid",
        "@child-only:example.invalid",
        1
      );
    });

    expect(screen.getByRole("list", { name: "Invitation pending" }).textContent).toContain(
      "Child-only Member"
    );
    expect(screen.queryByRole("list", { name: "Not in Space" })).toBeNull();
  });

  test("uses the same shared invite command from the child-only context menu", async () => {
    const api = createBrowserFakeApi({ spaceMemberInviteOutcome: "pending" });
    const inviteUserToSpace = vi.spyOn(api, "inviteUserToSpace");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    fireEvent.contextMenu(screen.getByText("Child-only Member").closest("li")!);
    await act(async () => {
      fireEvent.click(await screen.findByRole("menuitem", { name: "Invite to Space" }));
    });

    expect(inviteUserToSpace).toHaveBeenCalledTimes(1);
    expect(inviteUserToSpace).toHaveBeenCalledWith(
      "!space-alpha:example.invalid",
      "@child-only:example.invalid",
      1
    );
  });

  test("does not apply a late Timeline People load after the active room changes", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const pending = deferred<DesktopSnapshot>();
    const staleResult = structuredClone(initial);
    const loadRoomSettings = vi
      .spyOn(api, "loadRoomSettings")
      .mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "People" }));
    });
    await waitFor(() => {
      expect(loadRoomSettings).toHaveBeenCalledWith("!room-alpha:example.invalid");
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "planning-room" }));
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "planning-room" }).className).toContain(
        "is-active"
      );
    });

    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.queryByRole("heading", { name: "People", level: 2 })).toBeNull();
  });

  test("does not let a superseded Room Info People load replace the newer result", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const settingsApi = createBrowserFakeApi();
    const settingsSnapshot = await settingsApi.loadRoomSettings("!room-alpha:example.invalid");
    const first = deferred<DesktopSnapshot>();
    const firstResult = structuredClone(initial);
    const secondResult = structuredClone(initial);
    firstResult.state.domain.room_management = structuredClone(
      settingsSnapshot.state.domain.room_management
    );
    secondResult.state.domain.room_management = structuredClone(
      settingsSnapshot.state.domain.room_management
    );
    const firstMember = firstResult.state.domain.room_management.settings?.members[0];
    const secondMember = secondResult.state.domain.room_management.settings?.members[0];
    if (firstMember && secondMember) {
      firstResult.state.domain.room_management = {
        ...firstResult.state.domain.room_management,
        selected_room_id: "!room-alpha:example.invalid",
        settings: {
          ...firstResult.state.domain.room_management.settings!,
          members: [{ ...firstMember, display_label: "First result" }]
        }
      };
      secondResult.state.domain.room_management = {
        ...secondResult.state.domain.room_management,
        selected_room_id: "!room-alpha:example.invalid",
        settings: {
          ...secondResult.state.domain.room_management.settings!,
          members: [{ ...secondMember, display_label: "Second result" }]
        }
      };
    }
    const loadRoomSettings = vi.spyOn(api, "loadRoomSettings");

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    await waitFor(() => {
      expect(loadRoomSettings).toHaveBeenCalledWith("!room-alpha:example.invalid");
    });
    loadRoomSettings.mockReset();
    loadRoomSettings
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(secondResult);

    const peopleButton = screen
      .getAllByRole("button", { name: "People" })
      .find((button) => button.classList.contains("icon-button"));
    expect(peopleButton).toBeTruthy();
    await act(async () => {
      fireEvent.click(peopleButton!);
      fireEvent.click(peopleButton!);
    });
    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledTimes(2));
    await screen.findByRole("heading", { name: "People", level: 2 });

    await act(async () => {
      first.resolve(firstResult);
      await first.promise;
    });

    expect(screen.getByText("Second result")).toBeTruthy();
    expect(screen.queryByText("First result")).toBeNull();
  });

  test("dismisses a stale Space-member context target during navigation", async () => {
    const api = createBrowserFakeApi({ spaceMemberInviteOutcome: "pending" });
    const inviteUserToSpace = vi.spyOn(api, "inviteUserToSpace");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    fireEvent.contextMenu(screen.getByText("Child-only Member").closest("li")!);
    await screen.findByRole("menuitem", { name: "Invite to Space" });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });

    expect(screen.queryByRole("menuitem", { name: "Invite to Space" })).toBeNull();
    expect(inviteUserToSpace).not.toHaveBeenCalled();
  });

  test("disables and omits Space invites when exact Space settings deny permission", async () => {
    const api = createBrowserFakeApi();
    const readonlySpaceId = "!space-readonly:example.invalid";
    const readonlySnapshot = await api.getSnapshot();
    readonlySnapshot.state.ui.navigation.active_space_id = readonlySpaceId;
    readonlySnapshot.state.domain.spaces = readonlySnapshot.state.domain.spaces.map((space) =>
      space.space_id === "!space-alpha:example.invalid"
        ? { ...space, space_id: readonlySpaceId }
        : space
    );
    readonlySnapshot.state.domain.space_members.selected_space_id = readonlySpaceId;
    readonlySnapshot.sidebar.space_rail = readonlySnapshot.sidebar.space_rail.map((space) =>
      space.space_id === "!space-alpha:example.invalid"
        ? { ...space, space_id: readonlySpaceId }
        : space
    );
    vi.spyOn(api, "getSnapshot").mockResolvedValue(readonlySnapshot);

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    expect(screen.getByRole("button", { name: "Invite to Space" })).toHaveProperty(
      "disabled",
      true
    );
    fireEvent.contextMenu(screen.getByText("Child-only Member").closest("li")!);
    expect(screen.queryByRole("menuitem", { name: "Invite to Space" })).toBeNull();
  });

  test("writes Space panel diagnostics through the exact private-data-free source", async () => {
    const api = createBrowserFakeApi();
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    const diagnosticsButton = await screen.findByRole("button", { name: "Open diagnostics" });
    await act(async () => {
      fireEvent.click(diagnosticsButton);
    });
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });

    expect(dialog.textContent).toContain("ui.space_members_panel");
    expect(dialog.textContent).toContain("open trigger=sidebar");
    for (const privateValue of [
      "@joined:example.invalid",
      "Joined Member",
      "!room-alpha:example.invalid",
      "@child-only:example.invalid",
      "Child-only Member"
    ]) {
      expect(dialog.textContent).not.toContain(privateValue);
    }
  });

  test("records only a fixed diagnostic when the invite transport rejects", async () => {
    const api = createBrowserFakeApi();
    const inviteUserToSpace = vi
      .spyOn(api, "inviteUserToSpace")
      .mockRejectedValueOnce(new Error("raw transport details"));
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Invite to Space" }));
    });
    await waitFor(() => expect(inviteUserToSpace).toHaveBeenCalledTimes(1));
    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Open diagnostics" }));
    });
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(dialog.textContent).toContain("invite outcome=transport_rejected");
    for (const privateValue of [
      "raw transport details",
      "@child-only:example.invalid",
      "Child-only Member"
    ]) {
      expect(dialog.textContent).not.toContain(privateValue);
    }
  });
});

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}
