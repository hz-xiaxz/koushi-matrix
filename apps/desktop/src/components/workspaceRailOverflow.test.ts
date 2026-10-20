import { describe, expect, it } from "vitest";
import {
  EMPTY_RAIL_OVERFLOW,
  nearestRailScrollTop,
  railItemPlacement,
  sameRailOverflow,
  summarizeRailOverflow,
  type RailItemMeasure
} from "./workspaceRailOverflow";

const ITEM = 40;
const GAP = 8;

function items(count: number, attention: Record<number, [number, number]> = {}): RailItemMeasure[] {
  return Array.from({ length: count }, (_, index) => ({
    spaceId: `!s${index}`,
    top: index * (ITEM + GAP),
    bottom: index * (ITEM + GAP) + ITEM,
    unreadCount: attention[index]?.[0] ?? 0,
    highlightCount: attention[index]?.[1] ?? 0
  }));
}

const scrollHeight = 20 * (ITEM + GAP) - GAP;

describe("railItemPlacement", () => {
  const viewport = { scrollTop: 100, clientHeight: 200, scrollHeight };
  it("treats a partly clipped item as off-screen on the clipped side", () => {
    expect(railItemPlacement({ top: 90, bottom: 130 }, viewport)).toBe("above");
    expect(railItemPlacement({ top: 280, bottom: 320 }, viewport)).toBe("below");
    expect(railItemPlacement({ top: 100, bottom: 140 }, viewport)).toBe("visible");
    expect(railItemPlacement({ top: 260, bottom: 300.5 }, viewport)).toBe("visible");
  });
});

describe("summarizeRailOverflow", () => {
  it("reports no overflow when everything fits", () => {
    const summary = summarizeRailOverflow(items(3, { 1: [2, 0] }), {
      scrollTop: 0,
      clientHeight: 400,
      scrollHeight: 136
    });
    expect(summary).toEqual(EMPTY_RAIL_OVERFLOW);
  });

  it("aggregates unread counts per side and picks the nearest Space", () => {
    const summary = summarizeRailOverflow(
      items(20, { 0: [1, 0], 2: [3, 1], 5: [7, 0], 15: [9, 0], 18: [2, 0] }),
      { scrollTop: 4 * (ITEM + GAP), clientHeight: 5 * (ITEM + GAP), scrollHeight }
    );
    expect(summary.overflowAbove).toBe(true);
    expect(summary.overflowBelow).toBe(true);
    expect(summary.above).toEqual({ count: 4, hasHighlight: true, nearestSpaceId: "!s2" });
    expect(summary.below).toEqual({ count: 11, hasHighlight: false, nearestSpaceId: "!s15" });
  });

  it("counts a Space under an overflowing edge's fade as off-screen", () => {
    // Item 3 spans 144..184; the viewport starts at 136, so it is inside the
    // scrollport but its badge sits under an 18px fade.
    const viewport = { scrollTop: 136, clientHeight: 200, scrollHeight };
    const rail = items(20, { 3: [5, 0] });
    expect(summarizeRailOverflow(rail, viewport).above).toBeNull();
    expect(summarizeRailOverflow(rail, viewport, 18).above).toEqual({
      count: 5,
      hasHighlight: false,
      nearestSpaceId: "!s3"
    });
    // No fade on an edge that does not overflow.
    const top = { scrollTop: 0, clientHeight: 200, scrollHeight };
    expect(summarizeRailOverflow(items(20, { 0: [5, 0] }), top, 18).above).toBeNull();
  });

  it("keeps a highlight-only projection visible", () => {
    const summary = summarizeRailOverflow(items(20, { 19: [0, 1] }), {
      scrollTop: 0,
      clientHeight: 200,
      scrollHeight
    });
    expect(summary.below).toEqual({ count: 1, hasHighlight: true, nearestSpaceId: "!s19" });
    expect(summary.above).toBeNull();
  });
});

describe("nearestRailScrollTop", () => {
  const viewport = { scrollTop: 200, clientHeight: 200, scrollHeight };
  it("does not scroll an item already in view", () => {
    expect(nearestRailScrollTop({ top: 240, bottom: 280 }, viewport, 12)).toBeNull();
  });
  it("aligns an item below to the bottom edge, clear of the inset", () => {
    expect(nearestRailScrollTop({ top: 480, bottom: 520 }, viewport, 12)).toBe(332);
  });
  it("aligns an item above to the top edge, clear of the inset", () => {
    expect(nearestRailScrollTop({ top: 48, bottom: 88 }, viewport, 12)).toBe(36);
  });
  it("clamps to the scroll range", () => {
    expect(nearestRailScrollTop({ top: 0, bottom: 40 }, viewport, 12)).toBe(0);
    expect(
      nearestRailScrollTop({ top: scrollHeight - ITEM, bottom: scrollHeight }, viewport, 12)
    ).toBe(scrollHeight - 200);
  });
});

describe("sameRailOverflow", () => {
  it("compares edge attention by value", () => {
    const a = summarizeRailOverflow(items(20, { 19: [2, 0] }), {
      scrollTop: 0,
      clientHeight: 200,
      scrollHeight
    });
    const b = summarizeRailOverflow(items(20, { 19: [2, 0] }), {
      scrollTop: 0,
      clientHeight: 200,
      scrollHeight
    });
    expect(sameRailOverflow(a, b)).toBe(true);
    expect(sameRailOverflow(a, EMPTY_RAIL_OVERFLOW)).toBe(false);
  });
});
