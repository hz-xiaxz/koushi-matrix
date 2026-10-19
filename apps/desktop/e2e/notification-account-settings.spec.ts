import { expect, test } from "@playwright/test";
import { gotoReadyShell } from "./support/basicOperations";

test("all notification controls live under the named account and dispatch account patches", async ({ page }) => {
  await gotoReadyShell(page);
  await page.getByRole("button", { name: "Account Settings", exact: true }).click();
  const account = page.getByRole("dialog", { name: "Account Settings", exact: true });
  await expect(account.locator(".settings-account-owner")).toContainText(/@[^\s]+:/);
  await account.getByRole("tab", { name: "Notifications", exact: true }).click();
  await expect(account.getByRole("tab", { name: "OS notifications", exact: true })).toBeVisible();
  await expect(account.getByRole("tab", { name: "Sent from this device", exact: true })).toBeVisible();
  await expect(account.getByRole("tab", { name: "Homeserver notifications", exact: true })).toBeVisible();
  for (const label of ["Desktop notifications", "Show message content in notifications", "Sound", "Badges"]) {
    await expect(account.getByRole("switch", { name: label, exact: true })).toBeVisible();
  }
  await page.evaluate(() => window.__harness.clearInvocations());
  await account.getByRole("switch", { name: "Sound", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__harness.invocationsOf("update_settings").length)).toBe(1);
  const args = await page.evaluate(() => window.__harness.invocationsOf("update_settings")[0].args);
  expect(args.patch.scope).toBe("account");
  expect(args.patch.notifications.sound).toBe(false);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "App Settings", exact: true }).click();
  const app = page.getByRole("dialog", { name: "App Settings", exact: true });
  await expect(app.getByRole("tab", { name: "Notifications", exact: true })).toHaveCount(0);
});
