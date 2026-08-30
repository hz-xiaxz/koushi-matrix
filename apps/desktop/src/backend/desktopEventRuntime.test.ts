/* @vitest-environment jsdom */

import { afterEach, expect, test, vi } from "vitest";

test("constructs one event adapter without subscribing eagerly", async () => {
  vi.resetModules();
  const port = {
    listenCoreEvents: vi.fn(),
    listenMenuActions: vi.fn(),
    listenStateUpdates: vi.fn()
  };
  const createTauriDesktopEventPort = vi.fn(() => port);
  vi.doMock("./tauri/desktopEventPort", () => ({ createTauriDesktopEventPort }));

  const runtime = await import("./desktopEventRuntime");

  expect(runtime.desktopEventPort).toBe(port);
  expect(createTauriDesktopEventPort).toHaveBeenCalledOnce();
  expect(port.listenCoreEvents).not.toHaveBeenCalled();
  expect(port.listenMenuActions).not.toHaveBeenCalled();
  expect(port.listenStateUpdates).not.toHaveBeenCalled();
});

afterEach(() => {
  vi.clearAllMocks();
});
