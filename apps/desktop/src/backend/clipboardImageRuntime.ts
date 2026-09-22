import { browserClipboardImagePort } from "./browser/clipboardImagePort";
import type { ClipboardImagePort } from "./clipboardImagePort";
import { isTauriRuntime } from "./runtimeEnvironment";
import { tauriClipboardImagePort } from "./tauri/clipboardImagePort";

function activePort(): ClipboardImagePort {
  return isTauriRuntime() ? tauriClipboardImagePort : browserClipboardImagePort;
}

/**
 * Native fallback for webviews (WebKitGTK) whose paste event hides clipboard
 * images. The name matches what Chromium and WebKit give a pasted image.
 */
export async function readClipboardImageFile(): Promise<File | null> {
  try {
    const bytes = await activePort().readImagePng();
    if (!bytes || bytes.byteLength === 0) {
      return null;
    }
    return new File([bytes], "image.png", { type: "image/png" });
  } catch {
    return null;
  }
}
