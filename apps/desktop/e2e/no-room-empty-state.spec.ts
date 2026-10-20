import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

/**
 * #1545: with no room selected (a Space with no rooms, or Home after closing
 * Activity), the main pane is an empty state. It must not render a fake room
 * named "No room selected" with room header actions and a composer whose Send
 * is silently dropped.
 */
test("no selected room renders an empty main pane without a composer or room actions", async ({
  page
}) => {
  await page.goto("/appHarness.html");
  const conversation = page.getByRole("main", { name: t("timeline.conversation") });
  await expect(conversation).toBeVisible();
  await expect(page.getByRole("textbox", { name: t("composer.messageComposer") })).toBeVisible();

  await page.evaluate(() => {
    const next = structuredClone(window.__harness.currentSnapshot());
    next.state.ui.navigation.active_room_id = null;
    next.state.ui.timeline.room_id = null;
    window.__harness.setSnapshot(next);
    window.__harness.pushStateUpdate();
  });

  // Path B of #1545: Home opens on Activity, and its X leaves no room selected.
  await page
    .getByRole("navigation", { name: t("workspace.workspaces") })
    .getByRole("button", { name: /^Home/ })
    .click();
  const activity = page.getByRole("main", { name: t("workspace.activity") });
  await expect(activity).toBeVisible();
  await activity
    .getByRole("button", { name: t("action.close", { title: t("workspace.activity") }) })
    .click();
  await expect(activity).toHaveCount(0);

  const emptyPane = page.getByRole("main", { name: t("room.noRoomSelected") });
  await expect(emptyPane).toBeVisible();
  await expect(emptyPane).toContainText(t("room.noRoomSelected"));
  await expect(conversation).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: t("composer.messageComposer") })).toHaveCount(0);
  await expect(page.locator(".channel-name")).toHaveCount(0);
  await expect(page.getByRole("button", { name: t("action.send") })).toHaveCount(0);
});
