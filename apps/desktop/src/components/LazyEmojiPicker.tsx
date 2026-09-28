import { type ComponentProps, lazy, Suspense } from "react";

import type { EmojiPicker as EmojiPickerComponent } from "./EmojiPicker";

// #1035: the picker and its emojibase dataset load on demand, the first time
// any caller opens it. This is the only importer of `./EmojiPicker`; a static
// import elsewhere would pull the dataset back into the startup chunk.
const loadEmojiPicker = () =>
  import("./EmojiPicker").then((module) => ({ default: module.EmojiPicker }));

const EmojiPickerChunk = lazy(loadEmojiPicker);

export type LazyEmojiPickerProps = ComponentProps<typeof EmojiPickerComponent>;

/**
 * The shared emoji picker behind an on-demand chunk. While the chunk loads,
 * nothing is rendered and focus stays on the trigger; once mounted, the picker
 * owns focus (search field), Escape, and outside-click dismissal exactly as
 * the eager component did.
 */
export function LazyEmojiPicker(props: LazyEmojiPickerProps) {
  return (
    <Suspense fallback={null}>
      <EmojiPickerChunk {...props} />
    </Suspense>
  );
}

/** Starts fetching the picker chunk ahead of the first open (idempotent). */
export function preloadEmojiPicker(): void {
  void loadEmojiPicker();
}
