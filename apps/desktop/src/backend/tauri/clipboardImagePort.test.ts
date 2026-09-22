import { afterEach, describe, expect, test, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { tauriClipboardImagePort } from "./clipboardImagePort";

afterEach(() => {
  vi.clearAllMocks();
});

describe("Tauri clipboard image port", () => {
  test("returns the raw PNG body of the native clipboard command", async () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47]).buffer;
    invoke.mockResolvedValueOnce(png);

    expect(await tauriClipboardImagePort.readImagePng()).toBe(png);
    expect(invoke).toHaveBeenCalledWith("read_clipboard_image_png");
  });

  test("maps the empty body of an image-less clipboard to null", async () => {
    invoke.mockResolvedValueOnce(new ArrayBuffer(0));

    expect(await tauriClipboardImagePort.readImagePng()).toBeNull();
  });
});
