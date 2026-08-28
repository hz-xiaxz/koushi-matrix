// @vitest-environment jsdom

import { readFileSync } from "node:fs";

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import { createBrowserFakeApi } from "./backend/browserFakeApi";
import type { DesktopApi } from "./backend/desktopApi";
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
  vi.doMock("./backend/appRuntime", () => ({
    api,
    startSessionVerificationWindowDrag: vi.fn()
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
  vi.doUnmock("./backend/appRuntime");
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

  test("automatically loads the active restored Space member projection", async () => {
    const api = createBrowserFakeApi();
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");

    await renderAppWithApi(api);

    await waitFor(() => {
      expect(loadSpaceMembers).toHaveBeenCalledWith(
        "!space-alpha:example.invalid",
        1
      );
    });
  });

  test("reloads the same Space generation for a replacement account and rejects the old result", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const first = deferred<DesktopSnapshot>();
    const second = deferred<DesktopSnapshot>();
    const loadSpaceMembers = vi
      .spyOn(api, "loadSpaceMembers")
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const replacement = structuredClone(initial);
    const resultGeneration = (initial.state_generation ?? 0) + 1;
    replacement.state_generation = resultGeneration;
    const initialSession = replacement.state.domain.session;
    if (initialSession.kind !== "ready") {
      throw new Error("expected a ready synthetic session");
    }
    replacement.state.domain.session = {
      ...initialSession,
      homeserver: "https://second.example.invalid",
      user_id: "@second:example.invalid",
      device_id: "SECONDDEVICE"
    };
    const replacementResult = structuredClone(replacement);
    const oldResult = structuredClone(initial);
    oldResult.state_generation = resultGeneration;
    const replacementMember = replacementResult.state.domain.space_members.space_joined[0];
    const oldMember = oldResult.state.domain.space_members.space_joined[0];
    if (!replacementMember || !oldMember) {
      throw new Error("expected synthetic Space members");
    }
    replacementMember.display_label = "Replacement account member";
    oldMember.display_label = "Old account member";

    await renderAppWithApi(api);
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(1));
    const { getAppStoreSnapshot, setAppStoreSnapshot } = await import("./domain/appStore");
    await act(async () => {
      setAppStoreSnapshot(replacement);
    });

    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(2));
    await act(async () => {
      second.resolve(replacementResult);
      await second.promise;
    });
    await waitFor(() => {
      expect(getAppStoreSnapshot()?.state.domain.session.user_id).toBe(
        "@second:example.invalid"
      );
      expect(
        getAppStoreSnapshot()?.state.domain.space_members.space_joined[0]?.display_label
      ).toBe("Replacement account member");
    });

    await act(async () => {
      first.resolve(oldResult);
      await first.promise;
    });
    expect(getAppStoreSnapshot()?.state.domain.session.user_id).toBe("@second:example.invalid");
    expect(
      getAppStoreSnapshot()?.state.domain.space_members.space_joined[0]?.display_label
    ).toBe("Replacement account member");
  });

  test("keeps Space-member load demand bounded and account-scoped", () => {
    const source = readFileSync("src/App.tsx", "utf8");

    expect(source).not.toContain("spaceMembersLoadInFlightRef");
    expect(source).not.toContain("spaceMembersLoadedRef");
    expect(source).toContain("spaceMembersLoadDemandRef");
    expect(source).toContain("spaceMembersPanelOpenIntentEpochRef");
    const loaderStart = source.indexOf("const ensureSpaceMembersLoaded");
    const loaderEnd = source.indexOf("const attentionSummary", loaderStart);
    const loaderSource = source.slice(loaderStart, loaderEnd);
    expect(loaderSource).not.toContain("new Map");
    expect(loaderSource).not.toContain("new Set");
    expect(loaderSource).toContain("spaceMembersLoadDemandRef.current === demand");

    const effectStart = source.indexOf("void ensureSpaceMembersLoaded(");
    const effectEnd = source.indexOf("async function refresh()", effectStart);
    const effectSource = source.slice(effectStart, effectEnd);
    expect(effectSource).toContain("snapshot?.state.domain.session.homeserver");
    expect(effectSource).toContain("snapshot?.state.domain.session.user_id");
    expect(effectSource).toContain("snapshot?.state.domain.session.device_id");
  });

  test("does not issue a second Space member request while selection loading is in flight", async () => {
    const api = createBrowserFakeApi();
    const first = deferred<DesktopSnapshot>();
    const second = deferred<DesktopSnapshot>();
    let betaCalls = 0;
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers").mockImplementation(
      (spaceId) => {
        if (spaceId !== "!space-beta:example.invalid") {
          return Promise.resolve(api.getSnapshot());
        }
        betaCalls += 1;
        return betaCalls === 1 ? first.promise : second.promise;
      }
    );

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledWith(
      "!space-beta:example.invalid",
      expect.any(Number)
    ));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Members,/ }));
    });
    await waitFor(() => expect(screen.getByRole("heading", {
      name: "Space members",
      level: 2
    })).toBeTruthy());

    expect(betaCalls).toBe(1);

    const current = await api.getSnapshot();
    await act(async () => {
      first.resolve(current);
      second.resolve(current);
      await first.promise;
      await second.promise;
    });
  });

  test("retries a failed automatic Space member load when opening the panel", async () => {
    const api = createBrowserFakeApi();
    const originalLoadSpaceMembers = api.loadSpaceMembers.bind(api);
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");
    loadSpaceMembers
      .mockRejectedValueOnce(new Error("synthetic load failure"))
      .mockImplementation((spaceId, generation) =>
        originalLoadSpaceMembers(spaceId, generation)
      );

    await renderAppWithApi(api);
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(1));

    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: /Members,/ }));
    });
    await screen.findByRole("heading", { name: "Space members", level: 2 });

    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(2));
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

  test("retries a rejected Room Info settings load after the panel reopens", async () => {
    const api = createBrowserFakeApi();
    const originalLoadRoomSettings = api.loadRoomSettings.bind(api);
    const loadRoomSettings = vi.spyOn(api, "loadRoomSettings");
    loadRoomSettings
      .mockRejectedValueOnce(new Error("synthetic room settings failure"))
      .mockImplementation((roomId) => originalLoadRoomSettings(roomId));

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledTimes(1));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });

    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledTimes(2));
    await screen.findByRole("textbox", { name: "Room name" });
  });

  test("does not duplicate a pending Room Info settings load when the panel reopens", async () => {
    const api = createBrowserFakeApi();
    const pending = deferred<DesktopSnapshot>();
    const settingsApi = createBrowserFakeApi();
    const loaded = await settingsApi.loadRoomSettings("!room-alpha:example.invalid");
    const loadRoomSettings = vi
      .spyOn(api, "loadRoomSettings")
      .mockReturnValue(pending.promise);

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledTimes(1));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Room info" }));
    });
    expect(loadRoomSettings).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve(loaded);
      await pending.promise;
    });
    await screen.findByRole("textbox", { name: "Room name" });
  });

  test("does not apply a late Space Info settings result after opening same-Space members", async () => {
    const api = createBrowserFakeApi();
    const pending = deferred<DesktopSnapshot>();
    const settingsApi = createBrowserFakeApi();
    const staleResult = await settingsApi.loadRoomSettings("!space-alpha:example.invalid");
    const staleMember = staleResult.state.domain.room_management.settings?.members[0];
    if (!staleMember) {
      throw new Error("expected a synthetic Space settings member");
    }
    staleMember.display_label = "Stale Space Info member";
    staleMember.original_display_label = "Stale Space Info member";
    const originalLoadRoomSettings = api.loadRoomSettings.bind(api);
    let spaceSettingsCalls = 0;
    vi.spyOn(api, "loadRoomSettings").mockImplementation((roomId) => {
      if (roomId === "!space-alpha:example.invalid" && ++spaceSettingsCalls === 1) {
        return pending.promise;
      }
      return originalLoadRoomSettings(roomId);
    });

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Space info and settings" }));
    });
    await waitFor(() => expect(spaceSettingsCalls).toBe(1));

    await openSpaceMembersFromSidebar();
    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.getByRole("heading", { name: "Space members", level: 2 })).toBeTruthy();
    expect(screen.queryByText("Stale Space Info member")).toBeNull();
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

  test("treats a pending Space invite cancellation as an operation-pending invite state", async () => {
    const api = createBrowserFakeApi({
      spaceMemberInviteCancellationOutcome: "pending"
    });
    await api.cancelSpaceInvite(
      "!space-alpha:example.invalid",
      "@invited:example.invalid",
      1
    );

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    expect(
      screen.getByRole("button", { name: "Invite to Space" }).hasAttribute("disabled")
    ).toBe(true);
    expect(
      screen.getByRole("button", { name: "Cancelling…" }).hasAttribute("disabled")
    ).toBe(true);
  });

  test("cancels an invited member through the exact Space and generation", async () => {
    const api = createBrowserFakeApi();
    const cancelSpaceInvite = vi.spyOn(api, "cancelSpaceInvite");

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });

    await waitFor(() => {
      expect(cancelSpaceInvite).toHaveBeenCalledWith(
        "!space-alpha:example.invalid",
        "@invited:example.invalid",
        1
      );
    });
    await waitFor(() => expect(screen.queryByText("Invited Member")).toBeNull());
  });

  test("disables cancellation when the exact Space settings deny kick", async () => {
    const api = createBrowserFakeApi({
      roomPermissions: {
        "!space-alpha:example.invalid": {
          can_edit_settings: true,
          can_edit_roles: true,
          can_invite: true,
          can_kick: false,
          can_ban: true,
          can_unban: true
        }
      }
    });
    const cancelSpaceInvite = vi.spyOn(api, "cancelSpaceInvite");

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Invite to Space" })).toHaveProperty(
        "disabled",
        false
      );
    });
    expect(screen.getByRole("button", { name: "Cancel invitation" })).toHaveProperty(
      "disabled",
      true
    );
    expect(cancelSpaceInvite).not.toHaveBeenCalled();
  });

  test("shows localized cancellation failure and records only fixed diagnostics", async () => {
    const api = createBrowserFakeApi();
    vi.spyOn(api, "cancelSpaceInvite").mockRejectedValueOnce(
      new Error("raw transport details")
    );

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toBe(
        "Could not cancel the invitation. Try again."
      );
    });
    expect(screen.getByText("Invited Member")).toBeTruthy();

    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Open diagnostics" }));
    });
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(dialog.textContent).toContain("cancel trigger=inline availability_reason=available");
    expect(dialog.textContent).toContain("cancel outcome=transport_rejected");
    for (const privateValue of [
      "raw transport details",
      "@invited:example.invalid",
      "Invited Member",
      "mxc://",
      "https://"
    ]) {
      expect(dialog.textContent).not.toContain(privateValue);
    }
  });

  test("does not apply a late cancellation completion after Space navigation", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const staleResult = structuredClone(initial);
    staleResult.state.domain.space_members.space_invited = [];
    const pending = deferred<DesktopSnapshot>();
    const cancelSpaceInvite = vi
      .spyOn(api, "cancelSpaceInvite")
      .mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });
    await waitFor(() => {
      expect(cancelSpaceInvite).toHaveBeenCalledWith(
        "!space-alpha:example.invalid",
        "@invited:example.invalid",
        1
      );
    });

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

    expect(screen.queryByText("Invited Member")).toBeNull();
  });

  test("does not apply or log a late cancellation completion after same-Space room navigation", async () => {
    const api = createBrowserFakeApi();
    const initial = await api.getSnapshot();
    const staleResult = structuredClone(initial);
    staleResult.state.domain.space_members.space_invited = [];
    const pending = deferred<DesktopSnapshot>();
    const cancelSpaceInvite = vi
      .spyOn(api, "cancelSpaceInvite")
      .mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });
    await waitFor(() => expect(cancelSpaceInvite).toHaveBeenCalledTimes(1));

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

    expect(screen.getByRole("button", { name: "planning-room" }).className).toContain(
      "is-active"
    );
    await openSpaceMembersFromSidebar();
    expect(screen.getByText("Invited Member")).toBeTruthy();

    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Open diagnostics" }));
    });
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(dialog.textContent).not.toContain("cancel outcome=");
  });

  test("does not apply or log a late cancellation rejection after same-Space room navigation", async () => {
    const api = createBrowserFakeApi();
    const pending = deferred<DesktopSnapshot>();
    const cancelSpaceInvite = vi
      .spyOn(api, "cancelSpaceInvite")
      .mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });
    await waitFor(() => expect(cancelSpaceInvite).toHaveBeenCalledTimes(1));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "planning-room" }));
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "planning-room" }).className).toContain(
        "is-active"
      );
    });

    await act(async () => {
      pending.reject(new Error("stale cancellation rejection"));
      await pending.promise.catch(() => undefined);
    });

    await openSpaceMembersFromSidebar();
    expect(screen.getByText("Invited Member")).toBeTruthy();
    expect(screen.queryByText("Could not cancel the invitation. Try again.")).toBeNull();

    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Open diagnostics" }));
    });
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(dialog.textContent).not.toContain("cancel outcome=");
  });

  test("retries an invitation cancellation after a failed attempt", async () => {
    const api = createBrowserFakeApi({
      spaceMemberInviteCancellationOutcomes: ["failure", "success"]
    });
    const cancelSpaceInvite = vi.spyOn(api, "cancelSpaceInvite");

    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });
    await waitFor(() => expect(cancelSpaceInvite).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("alert").textContent).toBe(
      "Could not cancel the invitation. Try again."
    );
    expect(screen.getByText("Invited Member")).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
    });
    await waitFor(() => expect(cancelSpaceInvite).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText("Invited Member")).toBeNull());
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

  test("does not apply a late room selection after a newer Home navigation", async () => {
    const api = createBrowserFakeApi();
    const pending = deferred<DesktopSnapshot>();
    const staleResult = structuredClone(await api.getSnapshot());
    const selectRoom = vi.spyOn(api, "selectRoom").mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "planning-room" }));
    });
    await waitFor(() => expect(selectRoom).toHaveBeenCalledWith(
      "!room-planning:example.invalid"
    ));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /^Home/ }));
    });
    await waitFor(() => expect(screen.getByRole("button", { name: /^Home/ }).className).toContain(
      "is-active"
    ));

    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.getByRole("button", { name: /^Home/ }).className).toContain("is-active");
  });

  test("does not apply a late room selection after a newer Space navigation", async () => {
    const api = createBrowserFakeApi();
    const pending = deferred<DesktopSnapshot>();
    const staleResult = structuredClone(await api.getSnapshot());
    const selectRoom = vi.spyOn(api, "selectRoom").mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "planning-room" }));
    });
    await waitFor(() => expect(selectRoom).toHaveBeenCalledWith(
      "!room-planning:example.invalid"
    ));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Synthetic Lab" }));
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Synthetic Lab" }).className).toContain(
      "is-active"
    ));

    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.getByRole("button", { name: "Synthetic Lab" }).className).toContain(
      "is-active"
    );
  });

  test("does not apply late DM settings or profile scope after a newer room navigation", async () => {
    const api = createBrowserFakeApi();
    await api.selectSpace(null);
    const settingsApi = createBrowserFakeApi();
    await settingsApi.selectSpace(null);
    await settingsApi.selectRoom("!dm-member-1:example.invalid");
    const staleResult = await settingsApi.loadRoomSettings("!dm-member-1:example.invalid");
    const staleMember = staleResult.state.domain.room_management.settings?.members[0];
    if (!staleMember) {
      throw new Error("expected a synthetic DM member");
    }
    staleMember.display_label = "Stale DM profile";
    staleMember.original_display_label = "Stale DM profile";
    const pending = deferred<DesktopSnapshot>();
    const loadRoomSettings = vi.spyOn(api, "loadRoomSettings").mockReturnValueOnce(pending.promise);

    await renderAppWithApi(api);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /DMs/ }));
    });
    fireEvent.contextMenu(screen.getByRole("button", { name: "Member 1" }));
    await act(async () => {
      fireEvent.click(await screen.findByRole("menuitem", { name: "User info" }));
    });
    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledWith(
      "!dm-member-1:example.invalid"
    ));

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Member 2" }));
    });

    await act(async () => {
      pending.resolve(staleResult);
      await pending.promise;
    });

    expect(screen.queryByText("Stale DM profile")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Profile", level: 2 })).toBeNull();
  });

  test("does not let a superseded Room Info People load replace the newer result", async () => {
    const api = createBrowserFakeApi();
    await api.selectSpace(null);
    await api.selectRoom("!room-alpha:example.invalid");
    const settingsApi = createBrowserFakeApi();
    await settingsApi.selectSpace(null);
    await settingsApi.selectRoom("!room-alpha:example.invalid");
    const settingsSnapshot = await settingsApi.loadRoomSettings("!room-alpha:example.invalid");
    const first = deferred<DesktopSnapshot>();
    const second = deferred<DesktopSnapshot>();
    const firstResult = structuredClone(settingsSnapshot);
    const secondResult = structuredClone(settingsSnapshot);
    const resultGeneration = (settingsSnapshot.state_generation ?? 0) + 1;
    firstResult.state_generation = resultGeneration;
    secondResult.state_generation = resultGeneration;
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
    await screen.findByRole("textbox", { name: "Room name" });
    loadRoomSettings.mockReset();
    let supersessionCallCount = 0;
    loadRoomSettings.mockImplementation(() => {
      supersessionCallCount += 1;
      return supersessionCallCount === 1 ? first.promise : second.promise;
    });

    const peopleButton = screen
      .getAllByRole("button", { name: "People" })
      .find((button) => button.classList.contains("icon-button"));
    expect(peopleButton).toBeTruthy();
    await act(async () => {
      fireEvent.click(peopleButton!);
    });
    await waitFor(() => expect(loadRoomSettings).toHaveBeenCalledTimes(1));
    await act(async () => {
      fireEvent.click(
        screen
          .getAllByRole("button", { name: "People" })
          .find((button) => button.classList.contains("icon-button"))!
      );
    });
    await waitFor(() => expect(loadRoomSettings.mock.calls.length).toBeGreaterThanOrEqual(2));

    await act(async () => {
      second.resolve(secondResult);
      await second.promise;
    });
    await screen.findByRole("heading", { name: "People", level: 2 });
    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Members" }).textContent).toContain(
        "Second result"
      );
    });

    await act(async () => {
      first.resolve(firstResult);
      await first.promise;
    });

    await waitFor(() => {
      expect(screen.getByText("Second result")).toBeTruthy();
      expect(screen.queryByText("First result")).toBeNull();
    });
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

  test("requires admin confirmation and keeps Cancel inert", async () => {
    const api = createBrowserFakeApi();
    const updateSpaceMemberRole = vi.spyOn(api, "updateSpaceMemberRole");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    const select = screen.getByRole("combobox", { name: "Role for Joined Member" });
    await act(async () => {
      fireEvent.change(select, { target: { value: "100" } });
    });
    expect(screen.getByRole("dialog", { name: "Confirm role change" })).toBeTruthy();
    expect(updateSpaceMemberRole).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    });
    expect(updateSpaceMemberRole).not.toHaveBeenCalled();
    expect((select as HTMLSelectElement).value).toBe("0");

    await act(async () => {
      fireEvent.change(select, { target: { value: "100" } });
      fireEvent.click(screen.getByRole("button", { name: "Confirm role change" }));
    });
    await waitFor(() => expect(updateSpaceMemberRole).toHaveBeenCalledTimes(1));
    expect(updateSpaceMemberRole).toHaveBeenCalledWith(
      "!space-alpha:example.invalid",
      "@joined:example.invalid",
      1,
      "revision-1",
      0,
      100,
      true
    );
    await waitFor(() =>
      expect(
        (screen.getByRole("combobox", { name: "Role for Joined Member" }) as HTMLSelectElement)
          .value
      ).toBe("100")
    );
  });

  test("does not optimistically change a pending role and applies the success projection", async () => {
    const api = createBrowserFakeApi({ spaceMemberRoleUpdateOutcome: "pending" });
    const updateSpaceMemberRole = vi.spyOn(api, "updateSpaceMemberRole");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    const select = screen.getByRole("combobox", { name: "Role for Joined Member" });
    await act(async () => {
      fireEvent.change(select, { target: { value: "50" } });
    });
    await waitFor(() => expect(updateSpaceMemberRole).toHaveBeenCalledTimes(1));
    expect((select as HTMLSelectElement).value).toBe("0");
    expect(screen.getByRole("combobox", { name: "Role for Joined Member" })).toHaveProperty(
      "disabled",
      true
    );
    expect((await api.getSnapshot()).state.domain.space_members.space_joined[0]?.power_level).toBe(0);
  });

  test("applies the authoritative success projection without a local role patch", async () => {
    const api = createBrowserFakeApi();
    const updateSpaceMemberRole = vi.spyOn(api, "updateSpaceMemberRole");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    const select = screen.getByRole("combobox", { name: "Role for Joined Member" });
    await act(async () => {
      fireEvent.change(select, { target: { value: "50" } });
    });
    await waitFor(() => expect(updateSpaceMemberRole).toHaveBeenCalledTimes(1));
    expect(updateSpaceMemberRole).toHaveBeenCalledWith(
      "!space-alpha:example.invalid",
      "@joined:example.invalid",
      1,
      "revision-1",
      0,
      50,
      false
    );
    await waitFor(() =>
      expect(
        (screen.getByRole("combobox", { name: "Role for Joined Member" }) as HTMLSelectElement)
          .value
      ).toBe("50")
    );
    expect(screen.getByText("Joined Member")).toBeTruthy();
  });

  test.each(["forbidden", "stale", "network"] as const)(
    "surfaces %s and retries the exact role operation",
    async (failureKind) => {
      const api = createBrowserFakeApi({
        spaceMemberRoleUpdateOutcomes: [failureKind, "success"]
      });
      const updateSpaceMemberRole = vi.spyOn(api, "updateSpaceMemberRole");
      await renderAppWithApi(api);
      await openSpaceMembersFromSidebar();

      const select = screen.getByRole("combobox", { name: "Role for Joined Member" });
      await act(async () => {
        fireEvent.change(select, { target: { value: "50" } });
      });
      await waitFor(() =>
        expect(screen.getByRole("alert").textContent).toBe(
          "Could not update this member's role. Try again."
        )
      );
      expect((await api.getSnapshot()).state.domain.space_members.space_joined[0]?.power_level).toBe(0);

      await act(async () => {
        fireEvent.change(select, { target: { value: "50" } });
      });
      await waitFor(() => expect(updateSpaceMemberRole).toHaveBeenCalledTimes(2));
      await waitFor(() =>
        expect(
          (screen.getByRole("combobox", { name: "Role for Joined Member" }) as HTMLSelectElement)
            .value
        ).toBe("50")
      );
      expect(updateSpaceMemberRole.mock.calls[1]).toEqual([
        "!space-alpha:example.invalid",
        "@joined:example.invalid",
        1,
        failureKind === "stale" ? "revision-1001" : "revision-1",
        0,
        50,
        false
      ]);
    }
  );

  test("reloads the same Space generation from a stale role failure", async () => {
    const api = createBrowserFakeApi({ spaceMemberRoleUpdateOutcome: "stale" });
    const loadSpaceMembers = vi.spyOn(api, "loadSpaceMembers");
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(1));

    await act(async () => {
      fireEvent.change(screen.getByRole("combobox", { name: "Role for Joined Member" }), {
        target: { value: "50" }
      });
    });
    await screen.findByRole("alert");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Reload roles" }));
    });
    await waitFor(() => expect(loadSpaceMembers).toHaveBeenCalledTimes(2));
    await waitFor(async () =>
      expect((await api.getSnapshot()).state.domain.space_members.operation.kind).toBe("idle")
    );
  });

  test("omits role controls when the exact Space permissions deny role edits", async () => {
    const api = createBrowserFakeApi({
      roomPermissions: {
        "!space-alpha:example.invalid": {
          can_edit_settings: true,
          can_edit_roles: false,
          can_invite: true,
          can_kick: true,
          can_ban: true,
          can_unban: true
        }
      }
    });
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    expect(
      screen.queryByRole("combobox", { name: "Role for Joined Member" })
    ).toBeNull();
  });

  test("keeps role controls enabled while child-room sync is incomplete", async () => {
    const api = createBrowserFakeApi();
    await renderAppWithApi(api);
    await openSpaceMembersFromSidebar();

    expect(screen.getByText("Some child rooms are still syncing")).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "Role for Joined Member" })).toHaveProperty(
      "disabled",
      false
    );
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
