/** Viewport positions are CSS pixels, comparable with getBoundingClientRect. */
export type NativeFileDropEvent =
  | { kind: "over"; x: number; y: number }
  | { kind: "drop"; x: number; y: number }
  | { kind: "leave" };

export interface NativeFileDropPort {
  listen(handler: (event: NativeFileDropEvent) => void): () => void;
  /** Takes the files of the latest native drop; each drop can be claimed once. */
  claimDroppedFiles(): Promise<File[]>;
}
