import { browserNativeFileDropPort } from "./browser/nativeFileDropPort";
import type { NativeFileDropEvent, NativeFileDropPort } from "./nativeFileDropPort";
import { isTauriRuntime } from "./runtimeEnvironment";
import { tauriNativeFileDropPort } from "./tauri/nativeFileDropPort";

export type { NativeFileDropEvent } from "./nativeFileDropPort";

function activePort(): NativeFileDropPort {
  return isTauriRuntime() ? tauriNativeFileDropPort : browserNativeFileDropPort;
}

/**
 * File-manager drops on webviews (WebKitGTK) that hide dragged files from
 * HTML5 DataTransfer. Inert wherever native drag/drop is disabled.
 */
export function subscribeNativeFileDrops(
  handler: (event: NativeFileDropEvent) => void
): () => void {
  return activePort().listen(handler);
}

export async function claimNativeDroppedFiles(): Promise<File[]> {
  try {
    return await activePort().claimDroppedFiles();
  } catch {
    return [];
  }
}
