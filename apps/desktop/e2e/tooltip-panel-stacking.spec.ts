/**
 * Headless spec: an explanation bubble that belongs to a control inside a
 * floating panel must paint above that panel.
 *
 * The bubble renders in the shared body-level floating layer, so it competes
 * with the panel inside one stacking context. A panel whose z-index is higher
 * than the tooltip layer hides the explanation behind its own opaque surface,
 * which no class/role assertion can see — only the painted order shows it.
 *
 * `elementFromPoint` skips `pointer-events: none` elements, which the bubble
 * uses so it never steals the pointer, so the probe re-enables hit testing on
 * the bubble itself before asking which element is topmost at its centre.
 */

import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

async function topmostElementAtBubbleCentre(bubble: import("@playwright/test").Locator) {
  return bubble.evaluate((node) => {
    const element = node as HTMLElement;
    const previousPointerEvents = element.style.pointerEvents;
    element.style.pointerEvents = "auto";
    const rect = element.getBoundingClientRect();
    const hit = document.elementFromPoint(
      rect.left + rect.width / 2,
      rect.top + rect.height / 2
    );
    element.style.pointerEvents = previousPointerEvents;
    return hit === node
      ? "bubble"
      : `${hit?.tagName ?? "none"}.${(hit as HTMLElement | null)?.className ?? ""}`;
  });
}

test("an emoji-picker explanation bubble paints above the picker", async ({ page }) => {
  await page.goto("/appHarness.html");
  const emojiButton = page.getByRole("button", { name: t("composer.emoji"), exact: true });
  await expect(emojiButton).toBeVisible();
  await emojiButton.click();

  const picker = page.getByRole("dialog", { name: t("composer.emoji"), exact: true });
  await expect(picker).toBeVisible();

  const closeButton = picker.getByRole("button", { name: t("mediaGallery.close") });
  await closeButton.hover();

  const bubble = page.locator(".tooltip-bubble.is-open");
  await expect(bubble).toHaveCount(1);
  await expect(bubble).toBeVisible();

  expect(await topmostElementAtBubbleCentre(bubble)).toBe("bubble");
});
