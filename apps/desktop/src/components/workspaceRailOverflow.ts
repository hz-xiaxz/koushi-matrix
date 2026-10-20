/**
 * Viewport arithmetic for the scrolling Space rail (#1509).
 *
 * The rail only measures where its items sit in the scrollport; every count it
 * aggregates comes from the Rust-projected `SpaceRailItem` attention fields.
 * Nothing here decides Matrix semantics, so it stays presentation-owned.
 */

/** One rail item's box in scroll-content coordinates and its projected counts. */
export interface RailItemMeasure {
  spaceId: string;
  top: number;
  bottom: number;
  unreadCount: number;
  highlightCount: number;
}

/** The visible slice of the scroll content, in the same coordinates. */
export interface RailViewport {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

/** Aggregated attention on one side of the visible range. */
export interface RailEdgeAttention {
  /** Unread messages summed over the off-screen Spaces on this side. */
  count: number;
  hasHighlight: boolean;
  /** The off-screen Space with attention closest to the visible range. */
  nearestSpaceId: string;
}

export interface RailOverflow {
  overflowAbove: boolean;
  overflowBelow: boolean;
  above: RailEdgeAttention | null;
  below: RailEdgeAttention | null;
}

/** Sub-pixel slack: scroll offsets and boxes may be fractional. */
const EDGE_TOLERANCE_PX = 1;

export const EMPTY_RAIL_OVERFLOW: RailOverflow = {
  overflowAbove: false,
  overflowBelow: false,
  above: null,
  below: null
};

function attentionCount(item: RailItemMeasure): number {
  // A highlight is always also unread, but keep a highlight-only projection
  // visible rather than summing it as zero.
  return Math.max(item.unreadCount, item.highlightCount);
}

/**
 * Where an item sits relative to the visible range. An item that is only
 * partly visible counts as off-screen on the side it is cut, because its unread
 * badge sits on that edge and may be clipped. `insetAbove`/`insetBelow` shrink
 * the visible range by the edge fade on a side that overflows, since a badge
 * under the fade is washed out.
 */
export function railItemPlacement(
  item: Pick<RailItemMeasure, "top" | "bottom">,
  viewport: RailViewport,
  insetAbove = 0,
  insetBelow = 0
): "above" | "visible" | "below" {
  const visibleTop = viewport.scrollTop + insetAbove;
  const visibleBottom = viewport.scrollTop + viewport.clientHeight - insetBelow;
  if (item.top < visibleTop - EDGE_TOLERANCE_PX) return "above";
  if (item.bottom > visibleBottom + EDGE_TOLERANCE_PX) return "below";
  return "visible";
}

/** `inset` is the edge fade height, applied only on a side that overflows. */
export function summarizeRailOverflow(
  items: readonly RailItemMeasure[],
  viewport: RailViewport,
  inset = 0
): RailOverflow {
  const overflowAbove = viewport.scrollTop > EDGE_TOLERANCE_PX;
  const overflowBelow =
    viewport.scrollTop + viewport.clientHeight < viewport.scrollHeight - EDGE_TOLERANCE_PX;
  const insetAbove = overflowAbove ? inset : 0;
  const insetBelow = overflowBelow ? inset : 0;
  const above: RailItemMeasure[] = [];
  const below: RailItemMeasure[] = [];
  for (const item of items) {
    if (attentionCount(item) === 0) continue;
    const placement = railItemPlacement(item, viewport, insetAbove, insetBelow);
    if (placement === "above") above.push(item);
    else if (placement === "below") below.push(item);
  }
  // Items arrive in rail order: the nearest Space above is the last one above,
  // the nearest below is the first one below.
  return {
    overflowAbove,
    overflowBelow,
    above: edgeAttention(above, above[above.length - 1]),
    below: edgeAttention(below, below[0])
  };
}

function edgeAttention(
  items: readonly RailItemMeasure[],
  nearest: RailItemMeasure | undefined
): RailEdgeAttention | null {
  if (!nearest) return null;
  return {
    count: items.reduce((sum, item) => sum + attentionCount(item), 0),
    hasHighlight: items.some((item) => item.highlightCount > 0),
    nearestSpaceId: nearest.spaceId
  };
}

/**
 * The `scrollTop` that brings an item fully into view with the least movement
 * (`block: "nearest"`), keeping `inset` clear of the edge so the item is not
 * left under the edge fade or indicator. Returns null when no scroll is needed.
 *
 * Computed instead of `Element.scrollIntoView`, which would also scroll
 * `overflow: hidden` ancestors of the rail.
 */
export function nearestRailScrollTop(
  item: Pick<RailItemMeasure, "top" | "bottom">,
  viewport: RailViewport,
  inset = 0
): number | null {
  const maxScrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
  const visibleTop = viewport.scrollTop;
  const visibleBottom = viewport.scrollTop + viewport.clientHeight;
  let target: number | null = null;
  if (item.top < visibleTop + inset - EDGE_TOLERANCE_PX) {
    target = item.top - inset;
  } else if (item.bottom > visibleBottom - inset + EDGE_TOLERANCE_PX) {
    target = item.bottom + inset - viewport.clientHeight;
  }
  if (target === null) return null;
  const clamped = Math.min(maxScrollTop, Math.max(0, target));
  return Math.abs(clamped - viewport.scrollTop) <= EDGE_TOLERANCE_PX ? null : clamped;
}

export function sameRailOverflow(left: RailOverflow, right: RailOverflow): boolean {
  const sameEdge = (a: RailEdgeAttention | null, b: RailEdgeAttention | null) =>
    a === b ||
    (a !== null &&
      b !== null &&
      a.count === b.count &&
      a.hasHighlight === b.hasHighlight &&
      a.nearestSpaceId === b.nearestSpaceId);
  return (
    left.overflowAbove === right.overflowAbove &&
    left.overflowBelow === right.overflowBelow &&
    sameEdge(left.above, right.above) &&
    sameEdge(left.below, right.below)
  );
}
