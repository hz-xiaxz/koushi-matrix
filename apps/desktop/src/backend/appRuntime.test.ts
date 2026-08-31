/* @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from "vitest";

async function loadRuntime(tauriRuntime: boolean) {
  vi.resetModules();
  const tauriApi = { owner: "tauri" };
  const TauriDesktopApi = vi.fn(function TauriDesktopApiMock() {
    return tauriApi;
  });
  const windowDialogPort = { startDragging: vi.fn(async () => undefined) };

  vi.doMock("./client", () => ({ TauriDesktopApi }));
  vi.doMock("./runtimeEnvironment", () => ({ isTauriRuntime: () => tauriRuntime }));
  vi.doMock("./windowDialogRuntime", () => ({ windowDialogPort }));

  const runtime = await import("./appRuntime");
  return { runtime, tauriApi, TauriDesktopApi, windowDialogPort };
}

afterEach(() => vi.clearAllMocks());

describe("desktop API composition", () => {
  test.each([true, false])("always constructs the Tauri transport adapter (%s)", async (isTauri) => {
    const result = await loadRuntime(isTauri);
    expect(result.runtime.api).toBe(result.tauriApi);
    expect(result.TauriDesktopApi).toHaveBeenCalledOnce();
  });

  test("starts window drag only inside Tauri", async () => {
    const tauri = await loadRuntime(true);
    tauri.runtime.startSessionVerificationWindowDrag();
    expect(tauri.windowDialogPort.startDragging).toHaveBeenCalledOnce();

    const browser = await loadRuntime(false);
    browser.runtime.startSessionVerificationWindowDrag();
    expect(browser.windowDialogPort.startDragging).not.toHaveBeenCalled();
  });

  test("swallows a title-bar drag rejection", async () => {
    const result = await loadRuntime(true);
    result.windowDialogPort.startDragging.mockRejectedValueOnce(new Error("drag failed"));
    result.runtime.startSessionVerificationWindowDrag();
    await Promise.resolve();
    expect(result.windowDialogPort.startDragging).toHaveBeenCalledOnce();
  });
});
