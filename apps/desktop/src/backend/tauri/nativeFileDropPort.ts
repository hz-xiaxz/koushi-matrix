import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import type { NativeFileDropEvent, NativeFileDropPort } from "../nativeFileDropPort";

type ClaimedDroppedFile = { token: number; filename: string; mimeType: string };

// Only Linux enables native drag/drop, and GTK reports widget coordinates in
// logical pixels, so the webview zoom is the only scale between them and CSS.
function cssPosition(position: { x: number; y: number }): { x: number; y: number } {
  const zoom =
    Number(getComputedStyle(document.documentElement).getPropertyValue("--webview-zoom")) || 1;
  return { x: position.x / zoom, y: position.y / zoom };
}

export const tauriNativeFileDropPort: NativeFileDropPort = {
  listen(handler) {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (disposed) return;
        let event: NativeFileDropEvent;
        if (payload.type === "leave") {
          event = { kind: "leave" };
        } else {
          event = { kind: payload.type === "drop" ? "drop" : "over", ...cssPosition(payload.position) };
        }
        handler(event);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
      unlisten = null;
    };
  },
  // The renderer never names a path: Rust hands out single-read tokens for the
  // files of the latest drop, and the bytes arrive as raw IPC bodies.
  async claimDroppedFiles() {
    const claimed = await invoke<ClaimedDroppedFile[]>("claim_dropped_files");
    const files: File[] = [];
    for (const { token, filename, mimeType } of claimed) {
      const bytes = await invoke<ArrayBuffer>("read_dropped_file", { token });
      if (bytes.byteLength > 0) {
        files.push(new File([bytes], filename, { type: mimeType }));
      }
    }
    return files;
  }
};
