import { Component, type ComponentProps, type ReactNode, lazy, Suspense } from "react";

import type { EmojiPicker as EmojiPickerComponent } from "./EmojiPicker";

// #1035: the picker and its emojibase dataset load on demand, the first time
// any caller opens it. This is the only importer of `./EmojiPicker`; a static
// import elsewhere would pull the dataset back into the startup chunk.
const EmojiPickerChunk = lazy(() =>
  import("./EmojiPicker").then((module) => ({ default: module.EmojiPicker }))
);

export type LazyEmojiPickerProps = ComponentProps<typeof EmojiPickerComponent>;

interface EmojiPickerLoadBoundaryProps {
  onLoadFailed: () => void;
  children: ReactNode;
}

/**
 * Contains a failed picker chunk load: renders nothing and asks the caller to
 * close the picker, instead of unmounting the surrounding shell.
 */
class EmojiPickerLoadBoundary extends Component<
  EmojiPickerLoadBoundaryProps,
  { failed: boolean }
> {
  override state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  override componentDidCatch(): void {
    // React.lazy and the browser module map both keep the rejection, so a
    // later open fails the same way and closes again; the shell survives.
    this.props.onLoadFailed();
  }

  override render(): ReactNode {
    return this.state.failed ? null : this.props.children;
  }
}

/**
 * The shared emoji picker behind an on-demand chunk. While the chunk loads,
 * nothing is rendered and focus stays on the trigger; once mounted, the picker
 * owns focus (search field), Escape, and outside-click dismissal exactly as
 * the eager component did. A failed load closes the picker through `onClose`.
 */
export function LazyEmojiPicker(props: LazyEmojiPickerProps) {
  return (
    <EmojiPickerLoadBoundary onLoadFailed={props.onClose}>
      <Suspense fallback={null}>
        <EmojiPickerChunk {...props} />
      </Suspense>
    </EmojiPickerLoadBoundary>
  );
}
