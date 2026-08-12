import type { ContextMenuActionId, ContextMenuItem } from "../domain/contextMenus";
import { t } from "../i18n/messages";
import { useEffect, useRef } from "react";

const MENU_WIDTH = 184;
const MENU_ITEM_HEIGHT = 34;
const MENU_BORDER_HEIGHT = 2;
const VIEWPORT_PADDING = 8;

export function contextMenuPosition({
  itemCount,
  viewport,
  x,
  y
}: {
  itemCount: number;
  viewport: { height: number; width: number } | null;
  x: number;
  y: number;
}) {
  if (!viewport) {
    return { left: x, top: y };
  }

  const menuHeight = itemCount * MENU_ITEM_HEIGHT + MENU_BORDER_HEIGHT;
  const maxLeft = Math.max(VIEWPORT_PADDING, viewport.width - MENU_WIDTH - VIEWPORT_PADDING);
  const maxTop = Math.max(VIEWPORT_PADDING, viewport.height - menuHeight - VIEWPORT_PADDING);

  return {
    left: clamp(x, VIEWPORT_PADDING, maxLeft),
    top: clamp(y, VIEWPORT_PADDING, maxTop)
  };
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

/**
 * Roving focus over a menu's `[role="menuitem"]` children (#480 audit).
 * Arrow Up/Down move and wrap, Home/End jump to the ends. The browser keeps
 * the focused item visible, so no manual scroll management is needed.
 */
export function moveRovingMenuFocus(
  container: HTMLElement,
  target: "next" | "previous" | "first" | "last"
): void {
  const items = Array.from(
    container.querySelectorAll<HTMLElement>('[role="menuitem"]:not([hidden])')
  );
  if (items.length === 0) {
    return;
  }
  const currentIndex = items.indexOf(document.activeElement as HTMLElement);
  let nextIndex: number;
  switch (target) {
    case "first":
      nextIndex = 0;
      break;
    case "last":
      nextIndex = items.length - 1;
      break;
    case "next":
      nextIndex = currentIndex < 0 ? 0 : (currentIndex + 1) % items.length;
      break;
    case "previous":
      nextIndex =
        currentIndex < 0 ? items.length - 1 : (currentIndex + items.length - 1) % items.length;
      break;
  }
  items[nextIndex].focus();
}

/** Keydown handler for a menu container: arrows rove, Escape bubbles to the caller. */
export function onMenuKeyDown(
  event: React.KeyboardEvent<HTMLElement>,
  container: HTMLElement | null
): void {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    if (container) {
      moveRovingMenuFocus(container, "next");
    }
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    if (container) {
      moveRovingMenuFocus(container, "previous");
    }
  } else if (event.key === "Home") {
    event.preventDefault();
    if (container) {
      moveRovingMenuFocus(container, "first");
    }
  } else if (event.key === "End") {
    event.preventDefault();
    if (container) {
      moveRovingMenuFocus(container, "last");
    }
  }
}

export function ContextMenuSurface({
  items,
  x,
  y,
  onAction,
  onClose
}: {
  items: ContextMenuItem[];
  x: number;
  y: number;
  onAction: (actionId: ContextMenuActionId) => void;
  onClose: () => void;
}) {
  const firstItemRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    firstItemRef.current?.focus();
  }, []);

  if (!items.length) {
    return null;
  }

  const position = contextMenuPosition({
    itemCount: items.length,
    viewport:
      typeof window === "undefined" ? null : { height: window.innerHeight, width: window.innerWidth },
    x,
    y
  });

  return (
    <div className="context-menu-backdrop" onClick={onClose}>
      <div
        className="context-menu"
        role="menu"
        style={{ left: position.left, top: position.top }}
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
            return;
          }
          onMenuKeyDown(event, event.currentTarget);
        }}
      >
        {items.map((item, index) => (
          <button
            className={`context-menu-item ${item.destructive ? "destructive" : ""}`.trim()}
            key={item.id}
            ref={index === 0 ? firstItemRef : undefined}
            role="menuitem"
            type="button"
            onClick={() => onAction(item.id)}
          >
            {t(item.labelMessageId)}
          </button>
        ))}
      </div>
    </div>
  );
}
