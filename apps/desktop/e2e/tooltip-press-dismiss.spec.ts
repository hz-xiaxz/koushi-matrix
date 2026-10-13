/**
 * Headless spec: a press dismisses an explanation without disabling it for the
 * pointer (#1345, #1388).
 *
 * Pressing a control answers the question its explanation was asking, and the
 * pointer usually stays parked on the control afterwards, so the bubble must not
 * re-open for that parked pointer. Leaving and re-entering the control is a new
 * question. The shared bubble is hover-owned (#1388), so a keyboard visit never
 * opens it.
 *
 *  1. Hovering the control opens the bubble.
 *  2. The press dismisses it and it stays dismissed while the pointer rests.
 *  3. Keyboard focus does not open it.
 *  4. A fresh hover explains the control again.
 */

import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

test("a press keeps a parked hover dismissed and a fresh hover re-arms it", async ({ page }) => {
  await page.goto("/appHarness.html");
  const emojiButton = page.getByRole("button", { name: t("composer.emoji") });
  await expect(emojiButton).toBeVisible();

  const bubble = page.locator(".tooltip-bubble.is-open");
  await emojiButton.hover();
  await expect(bubble).toHaveCount(1);
  await expect(bubble).toHaveText(t("composer.emoji"));

  // The click parks the pointer on the trigger and must dismiss the bubble.
  await emojiButton.click();
  const picker = page.getByRole("dialog", { name: t("composer.emoji") });
  await expect(picker).toBeVisible();
  await expect(bubble).toHaveCount(0);

  // Escape closes the picker and returns focus to the trigger; focus is not a
  // reason to explain an icon-only control.
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(emojiButton).toBeFocused();
  await expect(bubble).toHaveCount(0);

  // The pointer leaves and returns: the bubble explains the control again.
  await page.mouse.move(0, 0);
  await emojiButton.hover();
  await expect(bubble).toHaveCount(1);
  await expect(bubble).toHaveText(t("composer.emoji"));
});
