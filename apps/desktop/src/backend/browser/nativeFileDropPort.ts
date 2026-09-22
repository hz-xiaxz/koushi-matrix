import type { NativeFileDropPort } from "../nativeFileDropPort";

export const browserNativeFileDropPort: NativeFileDropPort = {
  // Browsers deliver dropped files through HTML5 DataTransfer.files.
  listen() {
    return () => undefined;
  },
  async claimDroppedFiles() {
    return [];
  }
};
