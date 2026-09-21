export interface ClipboardImagePort {
  /** PNG bytes of the system clipboard image, or null when it holds none. */
  readImagePng(): Promise<ArrayBuffer | null>;
}
