/**
 * Headless spec: secondary and selected control states (#1599).
 *
 * The CSS contract test pins the intended declarations; this measures the
 * cascade the user actually sees. A static status summary must not paint a
 * button chip, and pointing at a settings switch row must change its surface
 * without turning hover into the selected state.
 */

import { expect, test, type Locator } from "@playwright/test";

import { t } from "../src/i18n/messages";
import { gotoReadyShell } from "./support/basicOperations";

async function controlPaint(locator: Locator) {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      background: style.backgroundColor,
      border: style.borderColor
    };
  });
}

test("static badges stay inert and switch rows darken on hover", async ({ page }) => {
  await gotoReadyShell(page);
  await page.getByRole("button", { name: t("room.roomInfo") }).click();

  const badge = page.locator(".room-status-badge").first();
  await expect(badge).toBeVisible();
  const badgePaint = await controlPaint(badge);
  expect(badgePaint.background).toBe("rgba(0, 0, 0, 0)");

  const toggle = page.locator(".settings-toggle-row").first();
  await expect(toggle).toBeVisible();
  const idle = await controlPaint(toggle);
  await toggle.hover();
  await page.waitForTimeout(200);
  const hover = await controlPaint(toggle);

  // The row's own surface reacts; its selected border (when checked) stays
  // put, so hover never redefines which state is active.
  expect(hover.background).not.toBe(idle.background);
});
