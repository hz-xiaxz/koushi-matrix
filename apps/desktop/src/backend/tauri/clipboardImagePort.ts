import { invoke } from "@tauri-apps/api/core";

import type { ClipboardImagePort } from "../clipboardImagePort";

export const tauriClipboardImagePort: ClipboardImagePort = {
  // The command answers with a raw IPC body, so the PNG never becomes JSON.
  async readImagePng() {
    const bytes = await invoke<ArrayBuffer>("read_clipboard_image_png");
    return bytes.byteLength > 0 ? bytes : null;
  }
};
