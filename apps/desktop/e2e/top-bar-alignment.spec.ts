/**
 * Headless spec: top-bar control alignment (#320).
 *
 * #320 reports that the macOS traffic lights and top-bar controls sit higher
 * than the search field. The native traffic lights are placed by
 * `tauri.conf.json` and cannot be measured here, but whether the DOM controls
 * agree with each other is a plain layout fact — so measure it rather than
 * reasoning about the CSS.
 *
 * This pins the answer: every top-bar control shares one center line. If a
 * future change to `--icon-button-size`, a control height, or the titlebar row
 * breaks that, this fails instead of becoming a pixel-hunting bug report.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

/** Every interactive control in the top bar, in visual order. */
const TOP_BAR_CONTROLS = [
  ".top-search input",
  ".scope-select",
  ".top-actions .sync-status",
  ".top-actions .icon-button"
] as const;

async function gotoReadyShell(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
}

test("every top-bar control shares one vertical center line", async ({ page }) => {
  await gotoReadyShell(page);
  await expect(page.locator(".titlebar .history")).toHaveCount(0);

  const measured = await page.evaluate((selectors) => {
    const bar = document.querySelector<HTMLElement>(".titlebar");
    if (!bar) {
      return null;
    }
    const barRect = bar.getBoundingClientRect();
    const centers = selectors.map((selector) => {
      const element = document.querySelector<HTMLElement>(selector);
      if (!element) {
        return { selector, center: null as number | null };
      }
      const rect = element.getBoundingClientRect();
      return { selector, center: rect.top + rect.height / 2 };
    });
    return {
      barCenter: barRect.top + barRect.height / 2,
      centers
    };
  }, TOP_BAR_CONTROLS);

  expect(measured).not.toBeNull();
  const { barCenter, centers } = measured!;

  for (const { selector, center } of centers) {
    expect(center, `${selector} should be present in the top bar`).not.toBeNull();
  }

  const values = centers.map((entry) => entry.center as number);
  const spread = Math.max(...values) - Math.min(...values);
  expect(
    spread,
    `top-bar controls disagree on their center line: ${JSON.stringify(centers)}`
  ).toBeLessThanOrEqual(0.5);

  // The controls are centered inside the titlebar's content box, which the 1px
  // bottom border makes 1px shorter than the window row the native macOS
  // traffic lights are positioned against. Half a pixel is not perceptible,
  // but a larger drift would mean the two coordinate systems have diverged.
  expect(Math.abs(values[0]! - barCenter)).toBeLessThanOrEqual(0.5);
});
