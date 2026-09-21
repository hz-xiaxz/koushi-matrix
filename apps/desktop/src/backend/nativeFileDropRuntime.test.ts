/* @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from "vitest";

function port() {
  return {
    listen: vi.fn(() => () => undefined),
    claimDroppedFiles: vi.fn(async () => [new File(["x"], "dropped.png", { type: "image/png" })])
  };
}

async function loadRuntime(tauriRuntime: boolean) {
  vi.resetModules();
  const tauriNativeFileDropPort = port();
  const browserNativeFileDropPort = port();
  vi.doMock("./runtimeEnvironment", () => ({ isTauriRuntime: () => tauriRuntime }));
  vi.doMock("./tauri/nativeFileDropPort", () => ({ tauriNativeFileDropPort }));
  vi.doMock("./browser/nativeFileDropPort", () => ({ browserNativeFileDropPort }));
  const runtime = await import("./nativeFileDropRuntime");
  return { runtime, tauriNativeFileDropPort, browserNativeFileDropPort };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("native file drop platform selection", () => {
  test("routes subscriptions and claims only to the Tauri port", async () => {
    const { runtime, tauriNativeFileDropPort, browserNativeFileDropPort } =
      await loadRuntime(true);
    const handler = vi.fn();

    runtime.subscribeNativeFileDrops(handler);
    const files = await runtime.claimNativeDroppedFiles();

    expect(tauriNativeFileDropPort.listen).toHaveBeenCalledWith(handler);
    expect(files.map((file) => file.name)).toEqual(["dropped.png"]);
    expect(browserNativeFileDropPort.listen).not.toHaveBeenCalled();
  });

  test("reports no files when the native claim fails", async () => {
    const { runtime, tauriNativeFileDropPort } = await loadRuntime(true);
    tauriNativeFileDropPort.claimDroppedFiles.mockRejectedValueOnce(new Error("ipc"));

    expect(await runtime.claimNativeDroppedFiles()).toEqual([]);
  });

  test("uses the inert browser port outside Tauri", async () => {
    const { runtime, tauriNativeFileDropPort, browserNativeFileDropPort } =
      await loadRuntime(false);

    runtime.subscribeNativeFileDrops(vi.fn());

    expect(browserNativeFileDropPort.listen).toHaveBeenCalledTimes(1);
    expect(tauriNativeFileDropPort.listen).not.toHaveBeenCalled();
  });
});
