/* @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from "vitest";

async function loadRuntime(tauriRuntime: boolean, readImagePng: () => Promise<ArrayBuffer | null>) {
  vi.resetModules();
  const tauriClipboardImagePort = { readImagePng: vi.fn(readImagePng) };
  const browserClipboardImagePort = { readImagePng: vi.fn(async () => null) };
  vi.doMock("./runtimeEnvironment", () => ({ isTauriRuntime: () => tauriRuntime }));
  vi.doMock("./tauri/clipboardImagePort", () => ({ tauriClipboardImagePort }));
  vi.doMock("./browser/clipboardImagePort", () => ({ browserClipboardImagePort }));
  const runtime = await import("./clipboardImageRuntime");
  return { runtime, tauriClipboardImagePort, browserClipboardImagePort };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("clipboard image platform selection", () => {
  test("wraps native PNG bytes as an attachable image file", async () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
    const { runtime, browserClipboardImagePort } = await loadRuntime(
      true,
      async () => png.buffer
    );

    const file = await runtime.readClipboardImageFile();

    expect(file?.name).toBe("image.png");
    expect(file?.type).toBe("image/png");
    expect(new Uint8Array(await file!.arrayBuffer())).toEqual(png);
    expect(browserClipboardImagePort.readImagePng).not.toHaveBeenCalled();
  });

  test("reports no image for an empty clipboard, a native failure, or a browser runtime", async () => {
    const empty = await loadRuntime(true, async () => null);
    expect(await empty.runtime.readClipboardImageFile()).toBeNull();

    const zeroBytes = await loadRuntime(true, async () => new ArrayBuffer(0));
    expect(await zeroBytes.runtime.readClipboardImageFile()).toBeNull();

    const failing = await loadRuntime(true, async () => {
      throw new Error("clipboard unavailable");
    });
    expect(await failing.runtime.readClipboardImageFile()).toBeNull();

    const browser = await loadRuntime(false, async () => new Uint8Array([1]).buffer);
    expect(await browser.runtime.readClipboardImageFile()).toBeNull();
    expect(browser.tauriClipboardImagePort.readImagePng).not.toHaveBeenCalled();
  });
});
