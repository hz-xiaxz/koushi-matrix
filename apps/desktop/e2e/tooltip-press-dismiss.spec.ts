/**
 * Headless spec: a press dismisses an explanation without disabling it for
 * keyboard users (#1345).
 *
 * Pressing a control answers the question its explanation was asking, and the
 * pointer usually stays parked on the control afterwards, so the bubble must
 * not re-open for that pointer. A keyboard visit is a different question: it
 * must still explain the control even while the pointer has not moved.
 *
 *  1. A press dismisses the bubble that hover had opened.
 *  2. The bubble stays dismissed while the pointer rests on the control.
 *  3. Keyboard focus on that same control still shows the explanation.
 */

import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

test("keyboard focus explains a control whose explanation a press dismissed", async ({ page }) => {
  await page.goto("/appHarness.html");
  const emojiButton = page.getByRole("button", { name: t("composer.emoji") });
  await expect(emojiButton).toBeVisible();

  // The click parks the pointer on the trigger: hover may open the bubble
  // first, and the press must dismiss it and keep it dismissed.
  await emojiButton.click();
  const picker = page.getByRole("dialog", { name: t("composer.emoji") });
  await expect(picker).toBeVisible();
  await expect(page.locator(".tooltip-bubble.is-open")).toHaveCount(0);

  // Escape closes the picker and returns focus to the trigger.
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);

  // Keyboard focus, with the pointer still resting on the control.
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(emojiButton).toBeFocused();

  const bubble = page.locator(".tooltip-bubble.is-open");
  await expect(bubble).toHaveCount(1);
  await expect(bubble).toHaveText(t("composer.emoji"));
});
