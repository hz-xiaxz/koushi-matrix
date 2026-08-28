/* @vitest-environment jsdom */

import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  confirm as confirmDialog,
  open as openDialog,
  save as saveDialog
} from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { createTauriWindowDialogPort } from "./windowDialogPort";

const currentWindow = vi.hoisted(() => ({
  isFullscreen: vi.fn(),
  setFullscreen: vi.fn(),
  startDragging: vi.fn()
}));

vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn(() => currentWindow) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn()
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(currentWindow.isFullscreen).mockResolvedValue(false);
  vi.mocked(currentWindow.setFullscreen).mockResolvedValue(undefined);
  vi.mocked(currentWindow.startDragging).mockResolvedValue(undefined);
});

describe("Tauri window/dialog port", () => {
  test("constructs without performing a platform operation", () => {
    createTauriWindowDialogPort();

    expect(getCurrentWindow).not.toHaveBeenCalled();
    expect(confirmDialog).not.toHaveBeenCalled();
    expect(saveDialog).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  test("reads and inverts fullscreen before setting it", async () => {
    const calls: string[] = [];
    vi.mocked(currentWindow.isFullscreen).mockImplementation(async () => {
      calls.push("read");
      return true;
    });
    vi.mocked(currentWindow.setFullscreen).mockImplementation(async (fullscreen) => {
      calls.push(`set:${fullscreen}`);
    });

    await createTauriWindowDialogPort().toggleFullscreen();

    expect(getCurrentWindow).toHaveBeenCalledOnce();
    expect(calls).toEqual(["read", "set:false"]);
  });

  test("acquires the current window and propagates drag rejection", async () => {
    vi.mocked(currentWindow.startDragging).mockRejectedValue(new Error("drag failed"));

    await expect(createTauriWindowDialogPort().startDragging()).rejects.toThrow("drag failed");
    expect(getCurrentWindow).toHaveBeenCalledOnce();
  });

  test("forwards confirm, save and open arguments and return values", async () => {
    vi.mocked(confirmDialog).mockResolvedValue(true);
    vi.mocked(saveDialog).mockResolvedValue("/tmp/export.txt");
    vi.mocked(openDialog).mockResolvedValue("/tmp/import.txt");
    const port = createTauriWindowDialogPort();
    const confirmOptions = { title: "Sign out", kind: "warning" } as const;
    const saveOptions = {
      title: "Export room keys",
      defaultPath: "koushi-room-keys.txt",
      filters: [{ name: "Export room keys", extensions: ["txt", "json"] }]
    };
    const openOptions = {
      title: "Import room keys",
      multiple: false,
      filters: [{ name: "Import room keys", extensions: ["txt", "json"] }],
      fileAccessMode: "scoped" as const
    };

    await expect(port.confirm("Are you sure?", confirmOptions)).resolves.toBe(true);
    await expect(port.saveFile(saveOptions)).resolves.toBe("/tmp/export.txt");
    await expect(port.openFile(openOptions)).resolves.toBe("/tmp/import.txt");
    expect(confirmDialog).toHaveBeenCalledWith("Are you sure?", confirmOptions);
    expect(saveDialog).toHaveBeenCalledWith(saveOptions);
    expect(openDialog).toHaveBeenCalledWith(openOptions);
  });
});
