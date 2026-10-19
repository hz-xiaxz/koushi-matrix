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

test("account destinations display Rust credential state and enable their test action", async ({ page }) => {
  await gotoReadyShell(page);
  await page.evaluate(() => {
    const snapshot = window.__harness.currentSnapshot();
    snapshot.state.domain.settings.values.delivery = {
      sinks: [{
        id: "sink-account-fixture",
        config: { kind: "resend", endpoint: "https://api.example.invalid/emails" },
        destination: "member@example.invalid",
        from_address: "sender@example.invalid",
        sender_name: "Koushi",
        secret_binding: "resend:https://api.example.invalid/emails#1",
        targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
        content: { kind: "namesOnly" },
        content_consent: null,
        direct_partner_label: "localAlias",
        timing: { dwell_secs: 300, min_interval_secs: 900, only_when_unfocused: true }
      }]
    };
    snapshot.state.domain.notification_delivery = {
      ...snapshot.state.domain.notification_delivery,
      sinks: [{
        sink_id: "sink-account-fixture",
        secret_configured: true,
        secret_retired: false,
        last_outcome: null
      }]
    };
    window.__harness.setSnapshot(snapshot);
  });
  await page.getByRole("button", { name: "Account Settings", exact: true }).click();
  const account = page.getByRole("dialog", { name: "Account Settings", exact: true });
  await account.getByRole("tab", { name: "Notifications", exact: true }).click();
  await account.getByRole("tab", { name: "Sent from this device", exact: true }).click();
  const destination = account.getByTestId("notification-delivery-sink");
  await expect(destination).toHaveAttribute("data-setup", "configured");
  await expect(destination.getByTestId("notification-delivery-sink-status")).toHaveText("Configured");
  await destination.click();
  await expect(account.getByTestId("notification-delivery-sink-test")).toBeEnabled();
});
