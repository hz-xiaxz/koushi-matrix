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

import {
  FloatingLayer,
  floatingPlacementStyle,
  useFloatingPlacement,
} from "./floatingLayer";
import { t } from "../i18n/messages";
import { ImeTextField } from "./ImeTextControl";
import { Tooltip } from "./Tooltip";
import { useEmojiPickerSize } from "./useEmojiPickerSize";
import {
  EMOJI_BY_CATEGORY,
  EMOJI_CATEGORIES,
  type EmojiCategory,
  type EmojiEntry,
} from "./emojiData";

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

/**
 * Placement inputs kept as named constants: the picker prefers opening above
 * its trigger, and only flips when a comfortable panel would not fit.
 */
const EMOJI_PICKER_COMFORTABLE_BLOCK_SIZE_PX = 360;

const MIN_PICKER_WIDTH = 300;
const MIN_PICKER_HEIGHT = 200;
const MAX_PICKER_SIZE = 1200;
const RESIZE_KEY_STEP = 20;

function constrainPickerSize(width: number, height: number): [number, number] {
  return [
    Math.round(Math.max(MIN_PICKER_WIDTH, Math.min(MAX_PICKER_SIZE, width))),
    Math.round(Math.max(MIN_PICKER_HEIGHT, Math.min(MAX_PICKER_SIZE, height))),
  ];
}

interface PickerResizeDrag {
  x: number;
  y: number;
  width: number;
  height: number;
  horizontal: number;
  vertical: number;
  size: [number, number];
}

interface EmojiPickerProps {
  onSelect: (emoji: string) => void;
  onClose: () => void;
  size?: [number, number] | null;
  onSizeChange?: (size: [number, number]) => void;
  recentEmojis?: string[];
  onRecentEmojisChange?: (emojis: string[]) => void;
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
  size,
  onSizeChange,
  recentEmojis = [],
  onRecentEmojisChange,
  anchorRef,
  resolveBoundaryElement,
  placement = "above",
  align = "start",
  className,
}: EmojiPickerProps) {
  const savedSize = useEmojiPickerSize();
  size ??= savedSize.size;
  onSizeChange ??= savedSize.save;
  const [draftSize, setDraftSize] = useState<[number, number]>(() =>
    size ? constrainPickerSize(...size) : [EMOJI_PICKER_INLINE_SIZE_PX, EMOJI_PICKER_BLOCK_SIZE_PX]
  );
  const drag = useRef<PickerResizeDrag | null>(null);
  const [query, setQuery] = useState("");
  const [activeCategory, setActiveCategory] = useState<
    EmojiCategory | "recent"
  >("people");
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

  const resolvedPlacement = useFloatingPlacement({
    align,
    anchorRef,
    blockSize: draftSize[1],
    comfortableBlockSize: EMOJI_PICKER_COMFORTABLE_BLOCK_SIZE_PX,
    inlineSize: draftSize[0],
    placement,
    resolveBoundaryElement
  });

  const resizeAbove = resolvedPlacement?.placement === "above";
  const resizeLeft = (resolvedPlacement?.align === "end") !== (document.documentElement.dir === "rtl");

  const initiallyFocused = useRef(false);
  useLayoutEffect(() => {
    if (resolvedPlacement && searchRef.current && !initiallyFocused.current) {
      searchRef.current.focus();
      initiallyFocused.current = true;
    }
  }, [resolvedPlacement]);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        panelRef.current &&
        !panelRef.current.contains(event.target as Node) &&
        !(anchorRef?.current?.contains(event.target as Node) ?? false)
      ) {
        onClose();
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [onClose, anchorRef]);

  const handleSelect = useCallback(
    (emoji: string) => {
      onRecentEmojisChange?.(
        [emoji, ...recentEmojis.filter((item) => item !== emoji)].slice(0, MAX_RECENT)
      );
      onSelect(emoji);
      onClose();
    },
    [onSelect, onClose, onRecentEmojisChange, recentEmojis],
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
      style={floatingPlacementStyle(resolvedPlacement)}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.defaultPrevented && !event.nativeEvent.isComposing && event.keyCode !== 229) {
          event.preventDefault();
          event.stopPropagation();
          onClose();
          if (anchorRef?.current instanceof HTMLElement) anchorRef.current.focus();
        }
      }}
    >
      <button
        type="button"
        className={`emoji-picker-resize ${resizeAbove ? "at-top" : "at-bottom"} ${resizeLeft ? "at-left" : "at-right"} ${resolvedPlacement?.align === "end" ? "at-start" : "at-end"}`}
        aria-label={t("composer.emojiResize")}
        onPointerDown={(event) => {
          if (event.button !== 0 || !panelRef.current) return;
          event.preventDefault();
          const rect = panelRef.current.getBoundingClientRect();
          drag.current = { x: event.clientX, y: event.clientY, width: rect.width, height: rect.height, horizontal: resizeLeft ? -1 : 1, vertical: resizeAbove ? -1 : 1, size: draftSize };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const start = drag.current;
          if (!start) return;
          start.size = constrainPickerSize(start.width + start.horizontal * (event.clientX - start.x), start.height + start.vertical * (event.clientY - start.y));
          setDraftSize(start.size);
        }}
        onPointerUp={(event) => {
          const start = drag.current;
          if (!start) return;
          drag.current = null;
          event.currentTarget.releasePointerCapture(event.pointerId);
          onSizeChange?.(start.size);
        }}
        onLostPointerCapture={() => { drag.current = null; }}
        onPointerCancel={() => {
          drag.current = null;
          setDraftSize(size ?? [EMOJI_PICKER_INLINE_SIZE_PX, EMOJI_PICKER_BLOCK_SIZE_PX]);
        }}
        onKeyDown={(event) => {
          const horizontal = event.key === "ArrowRight" ? RESIZE_KEY_STEP : event.key === "ArrowLeft" ? -RESIZE_KEY_STEP : 0;
          const vertical = event.key === "ArrowDown" ? RESIZE_KEY_STEP : event.key === "ArrowUp" ? -RESIZE_KEY_STEP : 0;
          if (!horizontal && !vertical) return;
          event.preventDefault();
          const next = constrainPickerSize(draftSize[0] + horizontal * (resizeLeft ? -1 : 1), draftSize[1] + vertical * (resizeAbove ? -1 : 1));
          setDraftSize(next);
          onSizeChange?.(next);
        }}
      >
        <span aria-hidden="true">⋰</span>
      </button>
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
        <Tooltip label={t("mediaGallery.close")} placement="below" align="end">
          {(tooltipProps) => (
            <button
              className="icon-button"
              type="button"
              aria-label={t("mediaGallery.close")}
              onClick={onClose}
              {...tooltipProps}
            >
              <X size={14} />
            </button>
          )}
        </Tooltip>
      </div>

      {!searching && (
        <div className="emoji-picker-tabs" role="tablist">
          {recentEntries.length > 0 && (
            <Tooltip label={t("composer.emojiRecent")} placement="below">
              {(tooltipProps) => (
                <button
                  className={`emoji-picker-tab ${activeCategory === "recent" ? "active" : ""}`}
                  type="button"
                  role="tab"
                  aria-selected={activeCategory === "recent"}
                  aria-label={t("composer.emojiRecent")}
                  onClick={() => {
                    setActiveCategory("recent");
                    scrollToCategory("recent");
                  }}
                  {...tooltipProps}
                >
                  <span aria-hidden="true">🕒</span>
                </button>
              )}
            </Tooltip>
          )}
          {EMOJI_CATEGORIES.map((category) => (
            <Tooltip
              key={category}
              label={t(`emoji.category.${category}` as const)}
              placement="below"
            >
              {(tooltipProps) => (
                <button
                  className={`emoji-picker-tab ${activeCategory === category ? "active" : ""}`}
                  type="button"
                  role="tab"
                  aria-selected={activeCategory === category}
                  aria-label={t(`emoji.category.${category}` as const)}
                  onClick={() => {
                    setActiveCategory(category);
                    scrollToCategory(category);
                  }}
                  {...tooltipProps}
                >
                  <span aria-hidden="true">{EMOJI_CATEGORY_ICONS[category]}</span>
                </button>
              )}
            </Tooltip>
          ))}
        </div>
      )}

      <div className="emoji-picker-body">
        {savedSize.failed && <p role="alert">{t("composer.emojiSizeSaveFailed")}</p>}
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

  return <FloatingLayer>{panel}</FloatingLayer>;
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
