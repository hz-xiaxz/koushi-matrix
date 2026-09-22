import type { ClipboardImagePort } from "../clipboardImagePort";

export const browserClipboardImagePort: ClipboardImagePort = {
  // Browsers expose pasted images on the paste event itself.
  async readImagePng() {
    return null;
  }
};
