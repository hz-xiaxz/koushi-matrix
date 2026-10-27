/**
 * Headless spec: an explanation bubble hugs the label it states.
 *
 * The shared tooltip used to force a 160px minimum width, so a short label
 * (the composer's Italic, the rail's Home) rendered inside a mostly empty
 * panel that looked wider than the control it explained. The bubble now sizes
 * to its label, and only a long label grows up to the tokenized maximum where
 * it wraps.
 *
 * The assertion compares the painted bubble against the label's own text range
 * plus its padding and border, so a change to a token or font does not make it
 * pass or fail for the wrong reason.
 */

import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

test("a tooltip bubble hugs its label", async ({ page }) => {
  await page.goto("/appHarness.html");
  const emojiButton = page.getByRole("button", { name: t("composer.emoji"), exact: true });
  await expect(emojiButton).toBeVisible();

  await emojiButton.hover();
  const bubble = page.locator(".tooltip-bubble.is-open");
  await expect(bubble).toHaveCount(1);
  await expect(bubble).toHaveText(t("composer.emoji"));

  const metrics = await bubble.evaluate((node) => {
    const element = node as HTMLElement;
    const style = getComputedStyle(element);
    const labelRange = document.createRange();
    labelRange.selectNodeContents(element);
    return {
      width: element.getBoundingClientRect().width,
      labelWidth: labelRange.getBoundingClientRect().width,
      horizontalSlack:
        parseFloat(style.paddingLeft) +
        parseFloat(style.paddingRight) +
        parseFloat(style.borderLeftWidth) +
        parseFloat(style.borderRightWidth)
    };
  });

  // 1px absorbs sub-pixel rounding between the text range and the border box.
  expect(metrics.width).toBeLessThanOrEqual(
    metrics.labelWidth + metrics.horizontalSlack + 1
  );
});
