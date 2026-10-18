import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

test("dragging the free corner saves the size and reopening restores it", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
  const trigger = page.getByRole("button", { name: t("composer.emoji"), exact: true });
  await trigger.click();
  const picker = page.getByRole("dialog", { name: t("composer.emoji") });
  await expect(picker).toBeVisible();
  const before = (await picker.boundingBox())!;
  const handle = picker.getByRole("button", { name: t("composer.emojiResize") });
  const corner = (await handle.boundingBox())!;
  const above = await handle.evaluate(element => element.classList.contains("at-top"));
  const left = await handle.evaluate(element => element.classList.contains("at-left"));
  await page.mouse.move(corner.x + corner.width / 2, corner.y + corner.height / 2);
  await page.mouse.down();
  await page.mouse.move(corner.x + corner.width / 2 + (left ? -100 : 100), corner.y + corner.height / 2 + (above ? -60 : 60), { steps: 8 });
  await page.mouse.up();
  await expect.poll(async () => (await picker.boundingBox())!.width).toBeCloseTo(before.width + 100, 0);
  await expect.poll(async () => (await picker.boundingBox())!.height).toBeCloseTo(before.height + 60, 0);
  await expect.poll(() => page.evaluate(() => window.__harness.currentSnapshot().state.domain.settings.values.window.emoji_picker_size))
    .toEqual([before.width + 100, before.height + 60]);
  await page.keyboard.press("Escape");
  await trigger.click();
  await expect.poll(async () => (await picker.boundingBox())!.width).toBeCloseTo(before.width + 100, 0);
  await expect.poll(async () => (await picker.boundingBox())!.height).toBeCloseTo(before.height + 60, 0);
  await page.setViewportSize({ width: 700, height: 650 });
  await expect.poll(async () => {
    const rect = (await picker.boundingBox())!;
    return rect.x >= 16 && rect.y >= 16 && rect.x + rect.width <= 684 && rect.y + rect.height <= 634;
  }).toBe(true);
  await page.setViewportSize({ width: 1280, height: 1000 });
  await expect.poll(async () => (await picker.boundingBox())!.width).toBeCloseTo(before.width + 100, 0);
  await expect.poll(async () => (await picker.boundingBox())!.height).toBeCloseTo(before.height + 60, 0);
});
