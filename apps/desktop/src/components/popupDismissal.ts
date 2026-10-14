//! Shared dismissal owner for non-modal floating popups (#1368).
//!
//! A non-modal popup (a menu, a details panel, a mention autocomplete) renders
//! without a backdrop and without focus containment, so nothing in the DOM
//! catches a press outside it. Each surface used to keep its own open flag and
//! handle only Escape, which left the popup on screen while the user kept
//! working elsewhere. This module owns that contract in one place:
//!
//! - a pointer press outside the popup and outside its trigger dismisses it,
//!   and the press is not swallowed, so the click still performs its own
//!   action (selecting the room, moving the caret);
//! - one popup per `kind` stays open: opening another closes the previous one;
//! - Escape and focus return stay with each surface, which owns its own key
//!   handling and opener. An outside press deliberately does not move focus,
//!   because the user is interacting with something else.
//!
//! This is deliberately not a click-guard backdrop. Swallowing the press is
//! right for a pointer-anchored context menu (`ContextMenuSurface`, whose
//! backdrop already supplies it) and wrong for a sidebar section menu or an
//! autocomplete, where the press must still reach what is underneath.

import { useEffect, useRef, type RefObject } from "react";

/** The open popup's closer per exclusivity group. */
const openPopups = new Map<string, { close: () => void }>();

export interface OutsidePressDismissalOptions {
  /** Whether the popup is currently open. */
  open: boolean;
  /**
   * Exclusivity group. Opening a popup closes any other open popup in the same
   * group, so a per-row or per-section flag can never stack instances.
   */
  kind: string;
  /** The popup element. Presses inside it never dismiss. */
  contentRef: RefObject<HTMLElement | null>;
  /**
   * The trigger element, or the editing surface the popup belongs to. Its own
   * press is left to that surface's own toggle or caret handling.
   */
  triggerRef?: RefObject<HTMLElement | null>;
  /** Called when a press lands outside the popup and its trigger. */
  onDismiss: () => void;
}

export function useOutsidePressDismissal({
  open,
  kind,
  contentRef,
  triggerRef,
  onDismiss
}: OutsidePressDismissalOptions): void {
  const dismissRef = useRef(onDismiss);
  // Latest-wins without re-subscribing the document listener on every render.
  useEffect(() => {
    dismissRef.current = onDismiss;
  });

  // A stable identity for this popup instance across effect re-runs, so
  // re-registering can never close the popup it belongs to.
  const instanceRef = useRef<{ close: () => void }>({ close: () => undefined });

  useEffect(() => {
    if (!open) {
      return;
    }
    const instance = instanceRef.current;
    instance.close = () => dismissRef.current();
    const previous = openPopups.get(kind);
    if (previous && previous !== instance) {
      previous.close();
    }
    openPopups.set(kind, instance);

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) {
        return;
      }
      if (contentRef.current?.contains(target)) {
        return;
      }
      if (triggerRef?.current?.contains(target)) {
        return;
      }
      instance.close();
    };
    document.addEventListener("pointerdown", handlePointerDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      if (openPopups.get(kind) === instance) {
        openPopups.delete(kind);
      }
    };
  }, [contentRef, kind, open, triggerRef]);
}
