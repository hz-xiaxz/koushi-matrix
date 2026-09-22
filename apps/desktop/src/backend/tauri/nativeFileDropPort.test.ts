/* @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from "vitest";

type DragDropHandler = (event: { payload: unknown }) => void;

const invoke = vi.hoisted(() => vi.fn());
const webview = vi.hoisted(() => ({
  handler: null as DragDropHandler | null,
  unlisten: vi.fn()
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: DragDropHandler) => {
      webview.handler = handler;
      return webview.unlisten;
    }
  })
}));

import { tauriNativeFileDropPort } from "./nativeFileDropPort";

afterEach(() => {
  vi.clearAllMocks();
  webview.handler = null;
  document.documentElement.style.removeProperty("--webview-zoom");
});

describe("Tauri native file drop port", () => {
  test("reports drag positions in CSS pixels of the zoomed webview", async () => {
    document.documentElement.style.setProperty("--webview-zoom", "1.2");
    const handler = vi.fn();
    tauriNativeFileDropPort.listen(handler);
    await vi.waitFor(() => expect(webview.handler).not.toBeNull());

    webview.handler!({ payload: { type: "enter", paths: ["/p"], position: { x: 120, y: 60 } } });
    webview.handler!({ payload: { type: "over", position: { x: 240, y: 120 } } });
    webview.handler!({ payload: { type: "drop", paths: ["/p"], position: { x: 240, y: 120 } } });
    webview.handler!({ payload: { type: "leave" } });

    expect(handler.mock.calls.map(([event]) => event)).toEqual([
      { kind: "over", x: 100, y: 50 },
      { kind: "over", x: 200, y: 100 },
      { kind: "drop", x: 200, y: 100 },
      { kind: "leave" }
    ]);
  });

  test("stops delivering and unlistens after unsubscribe", async () => {
    const handler = vi.fn();
    const unsubscribe = tauriNativeFileDropPort.listen(handler);
    await vi.waitFor(() => expect(webview.handler).not.toBeNull());

    unsubscribe();
    webview.handler!({ payload: { type: "leave" } });

    expect(handler).not.toHaveBeenCalled();
    expect(webview.unlisten).toHaveBeenCalledTimes(1);
  });

  test("claims dropped files by token and never sends a path", async () => {
    invoke.mockImplementation(async (command: string, args?: { token: number }) => {
      if (command === "claim_dropped_files") {
        return [
          { token: 7, filename: "shot.png", mimeType: "image/png" },
          { token: 8, filename: "gone.pdf", mimeType: "application/pdf" }
        ];
      }
      return args?.token === 7 ? new Uint8Array([1, 2, 3]).buffer : new ArrayBuffer(0);
    });

    const files = await tauriNativeFileDropPort.claimDroppedFiles();

    expect(files.map((file) => [file.name, file.type, file.size])).toEqual([
      ["shot.png", "image/png", 3]
    ]);
    expect(invoke.mock.calls).toEqual([
      ["claim_dropped_files"],
      ["read_dropped_file", { token: 7 }],
      ["read_dropped_file", { token: 8 }]
    ]);
  });
});
