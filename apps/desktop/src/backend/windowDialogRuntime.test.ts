/* @vitest-environment jsdom */

import { afterEach, expect, test, vi } from "vitest";

test("constructs one window/dialog adapter without performing platform operations", async () => {
  vi.resetModules();
  const port = {
    toggleFullscreen: vi.fn(),
    startDragging: vi.fn(),
    confirm: vi.fn(),
    saveFile: vi.fn(),
    openFile: vi.fn()
  };
  const createTauriWindowDialogPort = vi.fn(() => port);
  vi.doMock("./tauri/windowDialogPort", () => ({ createTauriWindowDialogPort }));

  const runtime = await import("./windowDialogRuntime");

  expect(runtime.windowDialogPort).toBe(port);
  expect(createTauriWindowDialogPort).toHaveBeenCalledOnce();
  expect(port.toggleFullscreen).not.toHaveBeenCalled();
  expect(port.startDragging).not.toHaveBeenCalled();
  expect(port.confirm).not.toHaveBeenCalled();
  expect(port.saveFile).not.toHaveBeenCalled();
  expect(port.openFile).not.toHaveBeenCalled();
});

afterEach(() => {
  vi.clearAllMocks();
});
