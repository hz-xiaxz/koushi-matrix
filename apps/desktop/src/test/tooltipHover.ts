import { fireEvent } from "@testing-library/react";

/**
 * The shared explanation bubble is hover-owned (#1388) and opens after a 250 ms
 * hover delay, so a component test that only needs an open bubble would have to
 * drive timers. jsdom implements no `window.matchMedia`, so this helper reports
 * reduced motion for the duration of the hover: the component then opens the
 * bubble synchronously. `Tooltip.test.tsx` covers the timed path itself.
 */
export function hoverTooltip(trigger: HTMLElement): void {
  const previousMatchMedia = window.matchMedia;
  window.matchMedia = ((query: string) => ({
    matches: query.includes("prefers-reduced-motion")
  })) as typeof window.matchMedia;
  try {
    fireEvent.mouseEnter(trigger);
  } finally {
    window.matchMedia = previousMatchMedia;
  }
}
