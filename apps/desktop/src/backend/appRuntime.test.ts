/* @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from "vitest";

async function loadRuntime(tauriRuntime: boolean) {
  vi.resetModules();
  const tauriApi = { owner: "tauri" };
  const browserApi = { owner: "browser" };
  const TauriDesktopApi = vi.fn(function TauriDesktopApiMock() {
    return tauriApi;
  });
  const createBrowserFakeApi = vi.fn(() => browserApi);
  const windowDialogPort = {
    startDragging: vi.fn(async () => undefined)
  };

  vi.doMock("./client", () => ({ TauriDesktopApi }));
  vi.doMock("./browserFakeApi", () => ({ createBrowserFakeApi }));
  vi.doMock("./runtimeEnvironment", () => ({
    isTauriRuntime: () => tauriRuntime
  }));
  vi.doMock("./windowDialogRuntime", () => ({ windowDialogPort }));

  const runtime = await import("./appRuntime");
  return {
    runtime,
    tauriApi,
    browserApi,
    TauriDesktopApi,
    createBrowserFakeApi,
    windowDialogPort
  };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("desktop API composition", () => {
  test("constructs only the Tauri adapter in a Tauri runtime", async () => {
    const result = await loadRuntime(true);

    expect(result.runtime.api).toBe(result.tauriApi);
    expect(result.TauriDesktopApi).toHaveBeenCalledOnce();
    expect(result.createBrowserFakeApi).not.toHaveBeenCalled();
    result.runtime.startSessionVerificationWindowDrag();
    expect(result.windowDialogPort.startDragging).toHaveBeenCalledOnce();
  });

  test("constructs only the browser fake outside Tauri", async () => {
    const result = await loadRuntime(false);

    expect(result.runtime.api).toBe(result.browserApi);
    expect(result.createBrowserFakeApi).toHaveBeenCalledOnce();
    expect(result.TauriDesktopApi).not.toHaveBeenCalled();
    result.runtime.startSessionVerificationWindowDrag();
    expect(result.windowDialogPort.startDragging).not.toHaveBeenCalled();
  });

  test("swallows a Tauri title-bar drag rejection", async () => {
    const result = await loadRuntime(true);
    result.windowDialogPort.startDragging.mockRejectedValueOnce(new Error("drag failed"));

    result.runtime.startSessionVerificationWindowDrag();
    await Promise.resolve();

    expect(result.windowDialogPort.startDragging).toHaveBeenCalledOnce();
  });
});
