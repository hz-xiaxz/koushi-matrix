// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import { t } from "../i18n/messages";
import { Tooltip } from "./Tooltip";

// `import.meta.url` is not a file URL under the jsdom environment this suite
// needs, so read the source through the project root instead.
// This config runs without Vitest globals, so Testing Library cannot register
// its automatic cleanup and each render must be torn down explicitly.
afterEach(cleanup);

const tooltipSource = readFileSync(resolve(process.cwd(), "src/components/Tooltip.tsx"), "utf8");

function rect(top: number, left: number, width: number, height: number) {
  return {
    top,
    left,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({})
  };
}

/** `delayMs=0` makes the hover that opens the bubble synchronous. */
function renderTooltip(
  placement?: "right" | "above" | "below",
  align?: "start" | "end",
  delayMs = 0
) {
  const label = t("composer.bold");
  render(
    <Tooltip label={label} placement={placement} align={align} delayMs={delayMs}>
      {(triggerProps) => (
        <button type="button" aria-label={label} {...triggerProps}>
          <span aria-hidden="true" />
        </button>
      )}
    </Tooltip>
  );
  return screen.getByRole("button", { name: label });
}

/** The trigger sits at (300, 400); the bubble measures 120x30. */
function mockGeometry() {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement
  ) {
    if (this.getAttribute("role") === "tooltip") {
      return rect(0, 0, 120, 30);
    }
    return rect(400, 300, 24, 24);
  });
}

describe("Tooltip", () => {
  test("registers unmount cleanup for delayed hover timers", () => {
    expect(tooltipSource).toContain("return () => clearOpenTimer();");
    expect(tooltipSource).not.toContain("useEffect(() => clearOpenTimer, [])");
  });

  test("checks reduced-motion preference before scheduling hover delay", () => {
    expect(tooltipSource).toContain("(prefers-reduced-motion: reduce)");
    expect(tooltipSource).toContain("openNow();");
  });

  test("opens the bubble only after the hover delay and closes it on leave", () => {
    vi.useFakeTimers();
    try {
      const trigger = renderTooltip("below", "start", 250);

      fireEvent.mouseEnter(trigger);
      expect(screen.queryByRole("tooltip")).toBeNull();

      act(() => {
        vi.advanceTimersByTime(249);
      });
      expect(screen.queryByRole("tooltip")).toBeNull();

      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(screen.getByRole("tooltip")).toBeTruthy();

      fireEvent.mouseLeave(trigger);
      expect(screen.queryByRole("tooltip")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  // The reported case (#1388): choosing a section sort option returns focus to
  // the menu trigger, and that visit must not leave an explanation on screen.
  test("a keyboard visit does not open the bubble", () => {
    const trigger = renderTooltip("below");

    fireEvent.focus(trigger);
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(trigger.getAttribute("aria-describedby")).toBeNull();

    fireEvent.blur(trigger);
    fireEvent.focus(trigger);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  test("describes the trigger only while the bubble is open", () => {
    const trigger = renderTooltip("below");
    expect(trigger.getAttribute("aria-describedby")).toBeNull();

    fireEvent.mouseEnter(trigger);
    const tooltip = screen.getByRole("tooltip");
    expect(tooltip.classList.contains("is-open")).toBe(true);
    expect(trigger.getAttribute("aria-describedby")).toBe(tooltip.id);

    fireEvent.mouseLeave(trigger);
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(trigger.getAttribute("aria-describedby")).toBeNull();
  });

  test("an Escape press dismisses the bubble", () => {
    const trigger = renderTooltip("below");
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeTruthy();

    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(trigger.getAttribute("aria-describedby")).toBeNull();
  });

  test("a press dismisses the bubble and it stays dismissed while the pointer rests on the control", () => {
    const trigger = renderTooltip("below");
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeTruthy();

    fireEvent.pointerDown(trigger);
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(trigger.getAttribute("aria-describedby")).toBeNull();

    // The pointer is parked on the control, so the hover that follows the press
    // may not re-open the bubble over the label the action just revealed.
    fireEvent.mouseEnter(trigger);
    expect(screen.queryByRole("tooltip")).toBeNull();

    // Leaving the control re-arms the explanation.
    fireEvent.mouseLeave(trigger);
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeTruthy();
  });

  test("leaves the control as the flex/grid item for the measured placements", () => {
    const trigger = renderTooltip("above");
    expect(trigger.parentElement?.classList.contains("tooltip-host-anchored")).toBe(true);
    expect(trigger.parentElement?.classList.contains("tooltip-host")).toBe(true);
  });

  test("places a below bubble from the trigger rectangle", () => {
    mockGeometry();
    const trigger = renderTooltip("below", "start");
    fireEvent.mouseEnter(trigger);
    const style = screen.getByRole("tooltip").style;
    expect(style.position).toBe("fixed");
    expect(style.left).toBe("300px");
    expect(style.top).toBe(`${400 + 24 + 8}px`);
    vi.restoreAllMocks();
  });

  test("places an above bubble aligned to the trigger's end edge", () => {
    mockGeometry();
    const trigger = renderTooltip("above", "end");
    fireEvent.mouseEnter(trigger);
    const style = screen.getByRole("tooltip").style;
    expect(style.left).toBe(`${300 + 24 - 120}px`);
    expect(style.top).toBe(`${400 - 8 - 30}px`);
    vi.restoreAllMocks();
  });

  test("a secondary press dismisses without arming the suppression", () => {
    const trigger = renderTooltip("below");
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeTruthy();

    // The bubble renders above the context menu layer, so a secondary press
    // dismisses it instead of leaving it over the menu that just opened.
    fireEvent.pointerDown(trigger, { button: 2 });
    expect(screen.queryByRole("tooltip")).toBeNull();

    // A secondary press must not arm the suppression the next hover would hit.
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeTruthy();
  });

  test("keeps start/end logical for a right-to-left root", () => {
    mockGeometry();
    document.documentElement.dir = "rtl";
    try {
      const trigger = renderTooltip("below", "end");
      fireEvent.mouseEnter(trigger);
      // Inline-end in RTL is the physical left edge, so `end` aligns to it.
      expect(screen.getByRole("tooltip").style.left).toBe("300px");
    } finally {
      document.documentElement.dir = "";
    }
    vi.restoreAllMocks();
  });

  test("flips an above bubble below the trigger when it would leave the viewport", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement
    ) {
      if (this.getAttribute("role") === "tooltip") {
        return rect(0, 0, 120, 30);
      }
      return rect(4, 300, 24, 24);
    });
    const trigger = renderTooltip("above", "start");
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip").style.top).toBe(`${4 + 24 + 8}px`);
    vi.restoreAllMocks();
  });
});
