import { Search, X } from "lucide-react";
import {
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

import { t } from "../i18n/messages";
import { ImeTextField } from "./ImeTextControl";
import {
  EMOJI_BY_CATEGORY,
  EMOJI_CATEGORIES,
  type EmojiCategory,
  type EmojiEntry,
} from "./emojiData";

const RECENT_EMOJIS_KEY = "koushi-recent-emojis";
const MAX_RECENT = 24;
const EMOJI_CATEGORY_ICONS: Record<EmojiCategory, string> = {
  people: "😀",
  nature: "🐕",
  foods: "🍎",
  activity: "⚽️",
  places: "🚗",
  objects: "💡",
  symbols: "⁉️",
  flags: "🏁",
};

function readRecentEmojis(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_EMOJIS_KEY);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw);
    if (
      Array.isArray(parsed) &&
      parsed.every((item) => typeof item === "string")
    ) {
      return parsed.slice(0, MAX_RECENT);
    }
  } catch {
    // ignore corrupt storage
  }
  return [];
}

function writeRecentEmojis(emojis: string[]) {
  try {
    localStorage.setItem(
      RECENT_EMOJIS_KEY,
      JSON.stringify(emojis.slice(0, MAX_RECENT)),
    );
  } catch {
    // ignore storage errors
  }
}

function pushRecentEmoji(emoji: string) {
  const current = readRecentEmojis();
  const next = [emoji, ...current.filter((item) => item !== emoji)];
  writeRecentEmojis(next);
}

/**
 * Number of emoji columns in the grid. Single source of truth: it drives both
 * the rendered CSS grid (through `--emoji-picker-columns`) and the ArrowUp /
 * ArrowDown keyboard step, so the two can never disagree.
 */
export const EMOJI_PICKER_GRID_COLUMNS = 10;
/** Preferred panel inline size; narrowed to the available space when smaller. */
export const EMOJI_PICKER_INLINE_SIZE_PX = 380;
/** Preferred panel block size; narrowed to the available space when smaller. */
export const EMOJI_PICKER_BLOCK_SIZE_PX = 520;
/** Minimum distance kept between the panel and the boundary edges. */
export const EMOJI_PICKER_VIEWPORT_MARGIN_PX = 16;
/** Distance between the anchor button and the panel. */
const EMOJI_PICKER_ANCHOR_GAP_PX = 6;
/** Space that makes a side comfortable enough to keep the preferred placement. */
const EMOJI_PICKER_COMFORTABLE_BLOCK_SIZE_PX = 360;

export interface EmojiPickerRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface EmojiPickerPlacementInput {
  /** Trigger rectangle in viewport coordinates, or null when unanchored. */
  anchor: EmojiPickerRect | null;
  /** Container the panel must stay inside, in addition to the viewport. */
  boundary?: EmojiPickerRect | null;
  viewport: { width: number; height: number };
  /** Preferred block-axis side; flipped when that side cannot fit the panel. */
  placement: "above" | "below";
  /** Preferred inline-axis alignment; flipped when it would overflow. */
  align: "start" | "end";
  direction: "ltr" | "rtl";
}

export interface EmojiPickerPlacementResult {
  placement: "above" | "below";
  align: "start" | "end";
  /** Physical viewport coordinates: the panel is positioned `fixed`. */
  left: number;
  top: number;
  inlineSize: number;
  blockSize: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function isUsableRect(rect: EmojiPickerRect): boolean {
  return rect.right > rect.left && rect.bottom > rect.top;
}

/**
 * Resolve the panel rectangle for a viewport-fixed floating layer.
 *
 * The panel prefers the caller's placement/alignment, flips to the opposite
 * side when the preferred one cannot fit, and is finally clamped inside the
 * boundary so an overflow-clipped pane (the thread pane) or a small window can
 * never cut it off.
 */
export function resolveEmojiPickerPlacement(
  input: EmojiPickerPlacementInput,
): EmojiPickerPlacementResult {
  const margin = EMOJI_PICKER_VIEWPORT_MARGIN_PX;
  const viewportBounds: EmojiPickerRect = {
    left: margin,
    top: margin,
    right: Math.max(margin, input.viewport.width - margin),
    bottom: Math.max(margin, input.viewport.height - margin),
  };
  const requested = input.boundary;
  const intersected: EmojiPickerRect | null =
    requested && isUsableRect(requested)
      ? {
          left: Math.max(viewportBounds.left, requested.left),
          top: Math.max(viewportBounds.top, requested.top),
          right: Math.min(viewportBounds.right, requested.right),
          bottom: Math.min(viewportBounds.bottom, requested.bottom),
        }
      : null;
  // A missing, collapsed, or fully off-screen boundary cannot host the panel;
  // the viewport is then the only meaningful constraint.
  const bounds =
    intersected && isUsableRect(intersected) ? intersected : viewportBounds;

  const inlineSize = Math.min(
    EMOJI_PICKER_INLINE_SIZE_PX,
    bounds.right - bounds.left,
  );
  const anchor: EmojiPickerRect = input.anchor ?? {
    left: bounds.left,
    right: bounds.left,
    top: bounds.top,
    bottom: bounds.top,
  };

  const gap = EMOJI_PICKER_ANCHOR_GAP_PX;
  const availableAbove = Math.max(0, anchor.top - bounds.top - gap);
  const availableBelow = Math.max(0, bounds.bottom - anchor.bottom - gap);
  const preferredAvailable =
    input.placement === "above" ? availableAbove : availableBelow;
  const oppositeAvailable =
    input.placement === "above" ? availableBelow : availableAbove;
  const opposite = input.placement === "above" ? "below" : "above";
  let placement = input.placement;
  if (preferredAvailable < EMOJI_PICKER_COMFORTABLE_BLOCK_SIZE_PX) {
    if (oppositeAvailable >= EMOJI_PICKER_COMFORTABLE_BLOCK_SIZE_PX) {
      placement = opposite;
    } else {
      placement = availableAbove >= availableBelow ? "above" : "below";
    }
  }
  const availableBlock = placement === "above" ? availableAbove : availableBelow;
  const blockSize = Math.min(EMOJI_PICKER_BLOCK_SIZE_PX, availableBlock);
  const top =
    placement === "above" ? anchor.top - gap - blockSize : anchor.bottom + gap;

  // `start`/`end` are logical: in RTL the inline-start edge is the right one.
  const startLeft =
    input.direction === "rtl" ? anchor.right - inlineSize : anchor.left;
  const endLeft =
    input.direction === "rtl" ? anchor.left : anchor.right - inlineSize;
  let align = input.align;
  let left = align === "start" ? startLeft : endLeft;
  if (left < bounds.left || left + inlineSize > bounds.right) {
    const flipped = align === "start" ? endLeft : startLeft;
    if (flipped >= bounds.left && flipped + inlineSize <= bounds.right) {
      align = align === "start" ? "end" : "start";
      left = flipped;
    }
  }

  return {
    align,
    blockSize,
    inlineSize,
    left: clamp(left, bounds.left, Math.max(bounds.left, bounds.right - inlineSize)),
    placement,
    top: clamp(top, bounds.top, Math.max(bounds.top, bounds.bottom - blockSize)),
  };
}

function placementsEqual(
  current: EmojiPickerPlacementResult | null,
  next: EmojiPickerPlacementResult,
): boolean {
  return (
    current != null &&
    current.align === next.align &&
    current.blockSize === next.blockSize &&
    current.inlineSize === next.inlineSize &&
    current.left === next.left &&
    current.placement === next.placement &&
    current.top === next.top
  );
}

function viewportRectOf(element: Element | null): EmojiPickerRect | null {
  if (!element) {
    return null;
  }
  const { top, right, bottom, left } = element.getBoundingClientRect();
  return { bottom, left, right, top };
}

interface EmojiPickerProps {
  onSelect: (emoji: string) => void;
  onClose: () => void;
  /** Element that triggered the picker; excluded from outside-click detection
   * so the trigger button can handle its own toggle without the picker
   * re-opening after the outside-click handler fires. It is also the anchor
   * rectangle the floating layer is positioned against. */
  anchorRef?: RefObject<Element | null>;
  /**
   * Resolves an extra container the panel must stay inside. Layout knowledge
   * stays with the caller; the panel re-resolves it on every measurement so a
   * resize or scroll cannot leave a stale boundary behind.
   */
  resolveBoundaryElement?: (anchor: Element) => Element | null;
  placement?: "above" | "below";
  align?: "start" | "end";
  className?: string;
}

export function EmojiPicker({
  onSelect,
  onClose,
  anchorRef,
  resolveBoundaryElement,
  placement = "above",
  align = "start",
  className,
}: EmojiPickerProps) {
  const [query, setQuery] = useState("");
  const [activeCategory, setActiveCategory] = useState<
    EmojiCategory | "recent"
  >("people");
  const [recentEmojis, setRecentEmojis] = useState<string[]>(() =>
    readRecentEmojis(),
  );
  const [resolvedPlacement, setPlacement] =
    useState<EmojiPickerPlacementResult | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const categoryRefs = useRef<Record<EmojiCategory, HTMLDivElement | null>>({
    people: null,
    nature: null,
    foods: null,
    activity: null,
    places: null,
    objects: null,
    symbols: null,
    flags: null,
  });
  const recentRef = useRef<HTMLDivElement | null>(null);

  const trimmedQuery = query.trim().toLowerCase();
  const searching = trimmedQuery.length > 0;

  const filtered = useMemo(() => {
    if (!searching) {
      return null;
    }
    const results: EmojiEntry[] = [];
    for (const category of EMOJI_CATEGORIES) {
      for (const entry of EMOJI_BY_CATEGORY[category]) {
        if (entry.search.includes(trimmedQuery)) {
          results.push(entry);
        }
      }
    }
    return results;
  }, [searching, trimmedQuery]);

  const recentEntries = useMemo(() => {
    const all = new Map<string, EmojiEntry>();
    for (const category of EMOJI_CATEGORIES) {
      for (const entry of EMOJI_BY_CATEGORY[category]) {
        all.set(entry.emoji, entry);
      }
    }
    return recentEmojis
      .map((emoji) => all.get(emoji))
      .filter((entry): entry is EmojiEntry => entry != null);
  }, [recentEmojis]);

  useLayoutEffect(() => {
    searchRef.current?.focus();
  }, []);

  const measurePlacement = useCallback(() => {
    const anchorElement = anchorRef?.current ?? null;
    const boundaryElement =
      anchorElement && resolveBoundaryElement
        ? resolveBoundaryElement(anchorElement)
        : null;
    const next = resolveEmojiPickerPlacement({
      align,
      anchor: viewportRectOf(anchorElement),
      boundary: viewportRectOf(boundaryElement),
      // Root `dir` is Rust-owned locale profile output; the panel only reads it.
      direction: document.documentElement.dir === "rtl" ? "rtl" : "ltr",
      placement,
      viewport: { height: window.innerHeight, width: window.innerWidth },
    });
    setPlacement((current) => (placementsEqual(current, next) ? current : next));
  }, [align, anchorRef, placement, resolveBoundaryElement]);

  // Measured after every commit, not only on mount: an ancestor re-render can
  // move the anchor without firing resize or scroll (right-panel resize drag,
  // pane open/close). Identical geometry keeps the previous state object, so
  // this cannot loop.
  useLayoutEffect(() => {
    measurePlacement();
  });

  useEffect(() => {
    window.addEventListener("resize", measurePlacement);
    document.addEventListener("scroll", measurePlacement, true);
    return () => {
      window.removeEventListener("resize", measurePlacement);
      document.removeEventListener("scroll", measurePlacement, true);
    };
  }, [measurePlacement]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    }
    function handleClickOutside(event: MouseEvent) {
      if (
        panelRef.current &&
        !panelRef.current.contains(event.target as Node) &&
        !(anchorRef?.current?.contains(event.target as Node) ?? false)
      ) {
        onClose();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [onClose, anchorRef]);

  const handleSelect = useCallback(
    (emoji: string) => {
      pushRecentEmoji(emoji);
      setRecentEmojis(readRecentEmojis());
      onSelect(emoji);
      onClose();
    },
    [onSelect, onClose],
  );

  const scrollToCategory = useCallback((category: EmojiCategory | "recent") => {
    const node =
      category === "recent"
        ? recentRef.current
        : categoryRefs.current[category];
    if (node) {
      node.scrollIntoView({ block: "start" });
    }
  }, []);

  // Rendered in a body-level floating layer: the composer lives inside
  // overflow-clipped panes (the thread pane), so an anchored popup would be cut
  // off at the pane boundary. Coordinates are physical because they come from
  // viewport rect measurements; the logical side is carried by `align-*`.
  const panel = (
    <div
      ref={panelRef}
      className={[
        "emoji-picker",
        `is-${resolvedPlacement?.placement ?? placement}`,
        `align-${resolvedPlacement?.align ?? align}`,
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      role="dialog"
      aria-label={t("composer.emoji")}
      style={
        resolvedPlacement
          ? {
              blockSize: `${resolvedPlacement.blockSize}px`,
              inlineSize: `${resolvedPlacement.inlineSize}px`,
              left: `${resolvedPlacement.left}px`,
              top: `${resolvedPlacement.top}px`,
            }
          : { visibility: "hidden" }
      }
    >
      <div className="emoji-picker-header">
        <div className="emoji-picker-search">
          <Search size={14} aria-hidden="true" />
          <ImeTextField
            ref={searchRef}
            type="search"
            value={query}
            syncKey="emoji-search"
            placeholder={t("composer.emojiSearch")}
            aria-label={t("composer.emojiSearch")}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
        </div>
        <button
          className="icon-button"
          type="button"
          aria-label={t("mediaGallery.close")}
          onClick={onClose}
        >
          <X size={14} />
        </button>
      </div>

      {!searching && (
        <div className="emoji-picker-tabs" role="tablist">
          {recentEntries.length > 0 && (
            <button
              className={`emoji-picker-tab ${activeCategory === "recent" ? "active" : ""}`}
              type="button"
              role="tab"
              aria-selected={activeCategory === "recent"}
              aria-label={t("composer.emojiRecent")}
              title={t("composer.emojiRecent")}
              onClick={() => {
                setActiveCategory("recent");
                scrollToCategory("recent");
              }}
            >
              <span aria-hidden="true">🕒</span>
            </button>
          )}
          {EMOJI_CATEGORIES.map((category) => (
            <button
              key={category}
              className={`emoji-picker-tab ${activeCategory === category ? "active" : ""}`}
              type="button"
              role="tab"
              aria-selected={activeCategory === category}
              aria-label={t(`emoji.category.${category}` as const)}
              title={t(`emoji.category.${category}` as const)}
              onClick={() => {
                setActiveCategory(category);
                scrollToCategory(category);
              }}
            >
              <span aria-hidden="true">{EMOJI_CATEGORY_ICONS[category]}</span>
            </button>
          ))}
        </div>
      )}

      <div className="emoji-picker-body">
        {searching ? (
          filtered && filtered.length > 0 ? (
            <EmojiGrid entries={filtered} onSelect={handleSelect} />
          ) : (
            <div className="emoji-picker-empty">{t("emoji.noResults")}</div>
          )
        ) : (
          <>
            {recentEntries.length > 0 && (
              <div
                ref={(node) => {
                  recentRef.current = node;
                }}
                className="emoji-picker-section"
              >
                <h3>{t("composer.emojiRecent")}</h3>
                <EmojiGrid entries={recentEntries} onSelect={handleSelect} />
              </div>
            )}
            {EMOJI_CATEGORIES.map((category) => (
              <div
                key={category}
                ref={(node) => {
                  categoryRefs.current[category] = node;
                }}
                className="emoji-picker-section"
              >
                <h3>{t(`emoji.category.${category}` as const)}</h3>
                <EmojiGrid
                  entries={EMOJI_BY_CATEGORY[category]}
                  onSelect={handleSelect}
                />
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  );

  return createPortal(panel, document.body);
}

function EmojiGrid({
  entries,
  onSelect,
}: {
  entries: EmojiEntry[];
  onSelect: (emoji: string) => void;
}) {
  // focusedIndex drives tabIndex so the roving-tabindex pattern is consistent;
  // actual DOM focus is moved synchronously in handleKeyDown so no async
  // React-cycle races arise during Playwright keyboard events.
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Sync itemRefs array length with entries
  itemRefs.current = itemRefs.current.slice(0, entries.length);

  function handleKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>, index: number) {
    let next: number | null = null;
    if (event.key === "ArrowRight") {
      next = index + 1 < entries.length ? index + 1 : index;
    } else if (event.key === "ArrowLeft") {
      next = index - 1 >= 0 ? index - 1 : index;
    } else if (event.key === "ArrowDown") {
      const candidate = index + EMOJI_PICKER_GRID_COLUMNS;
      next = candidate < entries.length ? candidate : index;
    } else if (event.key === "ArrowUp") {
      const candidate = index - EMOJI_PICKER_GRID_COLUMNS;
      next = candidate >= 0 ? candidate : index;
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(entries[index].emoji);
      return;
    }
    if (next !== null && next !== index) {
      event.preventDefault();
      // Update tabIndex state and focus synchronously so the DOM reflects the
      // change before any Playwright assertion runs.
      setFocusedIndex(next);
      itemRefs.current[next]?.focus();
    }
  }

  return (
    <div
      className="emoji-picker-grid"
      role="grid"
      // The rendered column count and the keyboard step share this constant.
      style={
        { "--emoji-picker-columns": EMOJI_PICKER_GRID_COLUMNS } as CSSProperties
      }
    >
      {entries.map((entry, index) => (
        <button
          key={entry.emoji}
          ref={(node) => {
            itemRefs.current[index] = node;
          }}
          className="emoji-picker-item"
          type="button"
          title={entry.label}
          aria-label={entry.label}
          // Roving tabindex: only the active cell participates in the tab
          // sequence; all others are skipped by Tab.
          tabIndex={index === (focusedIndex ?? 0) ? 0 : -1}
          onFocus={() => setFocusedIndex(index)}
          onKeyDown={(e) => handleKeyDown(e, index)}
          onClick={() => onSelect(entry.emoji)}
        >
          {entry.emoji}
        </button>
      ))}
    </div>
  );
}
