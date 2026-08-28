/* @vitest-environment jsdom */

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  cancelAll,
  isPermissionGranted,
  removeAllActive,
  requestPermission,
  sendNotification
} from "@tauri-apps/plugin-notification";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { createTauriDesktopAttentionPort } from "./desktopAttentionPort";

const currentWindow = vi.hoisted(() => ({
  setTitle: vi.fn(async () => undefined),
  setBadgeCount: vi.fn(async () => undefined),
  requestUserAttention: vi.fn(async () => undefined)
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn(() => currentWindow) }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  cancelAll: vi.fn(),
  isPermissionGranted: vi.fn(),
  removeAllActive: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn()
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(isPermissionGranted).mockResolvedValue(true);
  vi.mocked(sendNotification).mockResolvedValue(undefined);
  vi.mocked(cancelAll).mockResolvedValue(undefined);
  vi.mocked(removeAllActive).mockResolvedValue(undefined);
});

describe("Tauri desktop attention port", () => {
  test("acquires the current window and preserves native command contracts", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce("applied")
      .mockResolvedValueOnce("played");
    const port = createTauriDesktopAttentionPort();

    expect(port.currentWindow()).toBe(currentWindow);
    expect(getCurrentWindow).toHaveBeenCalledOnce();
    await expect(port.nativeBadge.setBadgeCount(3)).resolves.toBe("applied");
    expect(invoke).toHaveBeenNthCalledWith(1, "set_native_attention_badge", { count: 3 });
    await expect(port.sound.playAttentionSound?.()).resolves.toBe("played");
    expect(invoke).toHaveBeenNthCalledWith(2, "play_native_attention_sound");
  });

  test("caches an already-granted passive permission check across sends", async () => {
    const port = createTauriDesktopAttentionPort();
    const first = { title: "Mention in Announcements", body: "1 mention" };
    const second = { title: "Message in General", body: "2 unread" };

    await port.notifications.notify(first);
    await port.notifications.notify(second);

    expect(isPermissionGranted).toHaveBeenCalledOnce();
    expect(requestPermission).not.toHaveBeenCalled();
    expect(sendNotification).toHaveBeenNthCalledWith(1, first);
    expect(sendNotification).toHaveBeenNthCalledWith(2, second);
  });

  test("does not prompt or send when passive notification permission is denied", async () => {
    vi.mocked(isPermissionGranted).mockResolvedValue(false);
    const port = createTauriDesktopAttentionPort();

    await port.notifications.notify({ title: "Message in General", body: "1 unread" });

    expect(isPermissionGranted).toHaveBeenCalledOnce();
    expect(requestPermission).not.toHaveBeenCalled();
    expect(sendNotification).not.toHaveBeenCalled();
  });

  test.each([
    ["pending cancellation", () => vi.mocked(cancelAll).mockRejectedValue(new Error("failed"))],
    ["active removal", () => vi.mocked(removeAllActive).mockRejectedValue(new Error("failed"))]
  ])("settles both clear operations when %s fails", async (_operation, rejectOperation) => {
    rejectOperation();
    const port = createTauriDesktopAttentionPort();

    await expect(port.notifications.clear()).rejects.toThrow("native_notification_clear_failed");
    expect(cancelAll).toHaveBeenCalledOnce();
    expect(removeAllActive).toHaveBeenCalledOnce();
  });
});
