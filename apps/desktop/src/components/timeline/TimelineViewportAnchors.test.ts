// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { captureAnchor, wholePixelScrollTop } from "./TimelineViewportAnchors";

function viewport(tops: number[]) {
  const container = document.createElement("div");
  container.getBoundingClientRect = () => ({ top: 10, bottom: 210, height: 200 } as DOMRect);
  Object.defineProperty(container, "clientHeight", { value: 200 });
  tops.forEach((top, index) => {
    const row = document.createElement("div");
    row.dataset.itemId = `row-${index}`;
    row.getBoundingClientRect = () => ({ top, bottom: top + 72, height: 72 } as DOMRect);
    container.appendChild(row);
  });
  return container;
}

describe("viewport anchor capture", () => {
  it("does not capture an old virtual window wholly below the viewport", () => {
    expect(captureAnchor(viewport([500, 572]))).toBeNull();
  });
  it("keeps a partially visible row and its negative offset", () => {
    expect(captureAnchor(viewport([-20, 52]))).toEqual({ itemId: "row-0", offsetTop: -30 });
  });
  it("excludes rows exactly beyond either edge", () => {
    expect(captureAnchor(viewport([-62, 210]))).toBeNull();
  });
});

describe("whole-pixel anchor writes (#1507)", () => {
  it("rounds to the nearest pixel instead of leaving truncation to the engine", () => {
    expect(wholePixelScrollTop(20553.71875)).toBe(20554);
    expect(wholePixelScrollTop(20724.40625)).toBe(20724);
  });
});
