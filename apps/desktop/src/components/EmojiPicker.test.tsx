// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  EMOJI_PICKER_BLOCK_SIZE_PX,
  EMOJI_PICKER_GRID_COLUMNS,
  EMOJI_PICKER_INLINE_SIZE_PX,
  EMOJI_PICKER_VIEWPORT_MARGIN_PX,
  EmojiPicker,
  type EmojiPickerPlacementInput,
  resolveEmojiPickerPlacement,
} from "./EmojiPicker";
import { EMOJI_BY_CATEGORY, EMOJI_CATEGORIES } from "./emojiData";

const MARGIN = EMOJI_PICKER_VIEWPORT_MARGIN_PX;

function placementInput(
  overrides: Partial<EmojiPickerPlacementInput> = {},
): EmojiPickerPlacementInput {
  return {
    align: "start",
    anchor: { top: 700, right: 340, bottom: 728, left: 312 },
    direction: "ltr",
    placement: "above",
    viewport: { height: 800, width: 1280 },
    ...overrides,
  };
}

describe("resolveEmojiPickerPlacement", () => {
  it("keeps the preferred above/start placement when the anchor has room", () => {
    const placement = resolveEmojiPickerPlacement(placementInput());

    expect(placement.placement).toBe("above");
    expect(placement.align).toBe("start");
    expect(placement.left).toBe(312);
    expect(placement.inlineSize).toBe(EMOJI_PICKER_INLINE_SIZE_PX);
    expect(placement.blockSize).toBe(EMOJI_PICKER_BLOCK_SIZE_PX);
    // Sits one gap above the anchor.
    expect(placement.top).toBe(700 - 6 - EMOJI_PICKER_BLOCK_SIZE_PX);
  });

  it("flips the inline alignment instead of overflowing the viewport", () => {
    // A thread-composer style anchor near the right edge: start alignment would
    // push the panel past the viewport, so it flips to end alignment.
    const placement = resolveEmojiPickerPlacement(
      placementInput({ anchor: { top: 700, right: 1240, bottom: 728, left: 1212 } }),
    );

    expect(placement.align).toBe("end");
    expect(placement.left).toBe(1240 - EMOJI_PICKER_INLINE_SIZE_PX);
    expect(placement.left + placement.inlineSize).toBeLessThanOrEqual(1280 - MARGIN);
  });

  it("clamps into the margin when neither alignment fits", () => {
    // The panel is as wide as the usable width, so neither edge alignment can
    // fit and the rectangle is clamped rather than clipped.
    const placement = resolveEmojiPickerPlacement(
      placementInput({
        anchor: { top: 400, right: 228, bottom: 428, left: 200 },
        viewport: { height: 800, width: 300 },
      }),
    );

    expect(placement.left).toBe(MARGIN);
    expect(placement.inlineSize).toBe(300 - 2 * MARGIN);
  });

  it("flips vertically and shrinks to the space that is actually available", () => {
    const placement = resolveEmojiPickerPlacement(
      placementInput({
        // Almost no room above, plenty below.
        anchor: { top: 40, right: 340, bottom: 68, left: 312 },
      }),
    );

    expect(placement.placement).toBe("below");
    expect(placement.top).toBe(68 + 6);
    expect(placement.blockSize).toBe(EMOJI_PICKER_BLOCK_SIZE_PX);
  });

  it("stays inside a boundary container and shrinks to its visible space", () => {
    const placement = resolveEmojiPickerPlacement(
      placementInput({
        anchor: { top: 320, right: 480, bottom: 344, left: 456 },
        boundary: { top: 120, right: 480, bottom: 380, left: 0 },
        placement: "below",
      }),
    );

    // Below has 30px, above has 194px: neither is comfortable, so the larger
    // side wins and the panel shrinks to it rather than being clipped.
    expect(placement.placement).toBe("above");
    expect(placement.blockSize).toBe(194);
    expect(placement.top).toBe(120);
  });

  it("ignores a collapsed boundary and falls back to the viewport", () => {
    const placement = resolveEmojiPickerPlacement(
      placementInput({ boundary: { top: 0, right: 0, bottom: 0, left: 0 } }),
    );

    expect(placement.inlineSize).toBe(EMOJI_PICKER_INLINE_SIZE_PX);
    expect(placement.blockSize).toBe(EMOJI_PICKER_BLOCK_SIZE_PX);
    expect(placement.left).toBe(312);
  });

  it("mirrors the inline alignment under right-to-left direction", () => {
    // Room on both sides, so neither direction has to flip.
    const anchor = { top: 700, right: 628, bottom: 728, left: 600 };
    const ltr = resolveEmojiPickerPlacement(placementInput({ anchor }));
    const rtl = resolveEmojiPickerPlacement(
      placementInput({ anchor, direction: "rtl" }),
    );

    // Both keep the preferred `start` alignment, mirrored around the anchor.
    expect(ltr.align).toBe("start");
    expect(ltr.left).toBe(600);
    expect(rtl.align).toBe("start");
    expect(rtl.left).toBe(628 - EMOJI_PICKER_INLINE_SIZE_PX);
  });

  it("never returns a rectangle outside the viewport margin", () => {
    for (const anchor of [
      { top: 0, right: 8, bottom: 4, left: 0 },
      { top: 796, right: 1280, bottom: 800, left: 1272 },
      { top: 400, right: 700, bottom: 428, left: 672 },
    ]) {
      for (const align of ["start", "end"] as const) {
        for (const preferred of ["above", "below"] as const) {
          const placement = resolveEmojiPickerPlacement(
            placementInput({ align, anchor, placement: preferred }),
          );
          expect(placement.left).toBeGreaterThanOrEqual(MARGIN);
          expect(placement.top).toBeGreaterThanOrEqual(MARGIN);
          expect(placement.left + placement.inlineSize).toBeLessThanOrEqual(1280 - MARGIN);
          expect(placement.top + placement.blockSize).toBeLessThanOrEqual(800 - MARGIN);
        }
      }
    }
  });
});

describe("EmojiPicker", () => {
  afterEach(() => {
    cleanup();
    // Recent emojis are persisted, so a selection in one test must not change
    // which grid another test renders first.
    localStorage.clear();
  });

  it("renders category tabs and an emoji grid", () => {
    render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);

    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByRole("searchbox")).toBeTruthy();
    expect(screen.getByRole("tab", { name: /smileys & people/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: "grinning face" })).toBeTruthy();
  });

  it("uses Element-compatible emoji categories and data coverage", () => {
    expect(EMOJI_CATEGORIES).toEqual([
      "people",
      "nature",
      "foods",
      "activity",
      "places",
      "objects",
      "symbols",
      "flags",
    ]);
    expect((EMOJI_BY_CATEGORY as Record<string, unknown[]>)["flags"]?.length).toBeGreaterThan(200);
    expect(
      EMOJI_CATEGORIES.reduce(
        (total, category) => total + EMOJI_BY_CATEGORY[category].length,
        0,
      ),
    ).toBeGreaterThan(1_500);
  });

  it("calls onSelect and onClose when an emoji is clicked", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(<EmojiPicker onSelect={onSelect} onClose={onClose} />);

    fireEvent.click(screen.getByRole("button", { name: /grinning face$/i }));

    expect(onSelect).toHaveBeenCalledWith("😀");
    expect(onClose).toHaveBeenCalled();
  });

  it("filters emojis by search query", async () => {
    render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);

    const searchbox = screen.getByRole("searchbox");
    fireEvent.change(searchbox, { target: { value: "pizza" } });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /pizza/i })).toBeTruthy();
    });
    expect(screen.queryByRole("button", { name: "grinning face" })).toBeNull();
  });

  it("searches Element emoji shortcodes and flags", async () => {
    render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);

    const searchbox = screen.getByRole("searchbox");
    fireEvent.change(searchbox, { target: { value: "checkered_flag" } });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /flag/i })).toBeTruthy();
    });
  });

  it("shows an empty state when search has no matches", async () => {
    render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);

    const searchbox = screen.getByRole("searchbox");
    fireEvent.change(searchbox, { target: { value: "xyzabc" } });

    await waitFor(() => {
      expect(screen.getByText(/no emojis match/i)).toBeTruthy();
    });
  });

  it("closes when Escape is pressed", () => {
    const onClose = vi.fn();
    render(<EmojiPicker onSelect={vi.fn()} onClose={onClose} />);

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });

  it("closes when clicking outside", () => {
    const onClose = vi.fn();
    render(
      <div>
        <EmojiPicker onSelect={vi.fn()} onClose={onClose} />
        <button type="button" data-testid="outside" />
      </div>,
    );

    fireEvent.mouseDown(screen.getByTestId("outside"));

    expect(onClose).toHaveBeenCalled();
  });

  it("renders in a viewport-fixed floating layer outside the anchor subtree", () => {
    function Host() {
      const anchorRef = useRef<HTMLButtonElement>(null);
      return (
        <div data-testid="clipped-pane" style={{ overflow: "hidden" }}>
          <button ref={anchorRef} type="button" data-testid="anchor" />
          <EmojiPicker anchorRef={anchorRef} onSelect={vi.fn()} onClose={vi.fn()} />
        </div>
      );
    }
    render(<Host />);

    const panel = screen.getByRole("dialog");
    expect(panel.parentElement).toBe(document.body);
    expect(screen.getByTestId("clipped-pane").contains(panel)).toBe(false);
    expect(panel.style.getPropertyValue("inline-size")).toBe(
      `${EMOJI_PICKER_INLINE_SIZE_PX}px`,
    );
    expect(panel.style.left).not.toBe("");
    expect(panel.style.top).not.toBe("");
  });

  it("keeps the rendered grid column count and the keyboard step in sync", () => {
    render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);

    const grid = screen.getAllByRole("grid")[0];
    expect(grid.style.getPropertyValue("--emoji-picker-columns")).toBe(
      String(EMOJI_PICKER_GRID_COLUMNS),
    );

    const cells = within(grid).getAllByRole("button");
    cells[0].focus();
    fireEvent.keyDown(cells[0], { key: "ArrowDown" });

    expect(document.activeElement).toBe(cells[EMOJI_PICKER_GRID_COLUMNS]);
  });
});
