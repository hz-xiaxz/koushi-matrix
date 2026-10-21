import { expect, test, type Page } from "@playwright/test";
import { gotoReadyShell } from "./support/basicOperations";

async function openDescription(page: Page, canChange = true) {
  await gotoReadyShell(page);
  await page.getByRole("navigation", { name: "Workspaces" }).getByRole("button", { name: "Harness Space", exact: true }).click();
  const id = await page.evaluate(canChange => {
    const next = structuredClone(window.__harness.currentSnapshot());
    const space = next.state.domain.spaces.find(space => space.display_name === "Harness Space")!;
    next.state_generation = (next.state_generation ?? 0) + 1;
    next.state.domain.room_management = {
      selected_room_id: space.space_id, operation: { kind: "idle" },
      settings: { room_id: space.space_id, name: space.display_name, topic: "Original shared description", avatar_url: null, join_rule: "invite", history_visibility: "shared", members: [],
        permissions: { can_edit_settings: !canChange, can_change_topic: canChange, can_change_join_rule: false, can_edit_roles: false, can_invite: false, can_kick: false, can_ban: false, can_unban: false } }
    };
    window.__harness.setSnapshot(next);
    window.__harness.setCommandResponse("load_room_settings", () => window.__harness.currentSnapshot());
    window.__harness.setCommandResponse("update_room_setting", () => window.__harness.currentSnapshot());
    window.__harness.pushStateUpdate(); window.__harness.clearInvocations();
    return space.space_id;
  }, canChange);
  await page.getByRole("button", { name: "Space info and settings" }).click();
  const card = page.getByRole("group", { name: "Description" });
  await expect(card.getByText("Original shared description", { exact: true })).toBeVisible();
  return { card, id };
}
async function observe(page: Page, topic: string | null, pending = false) {
  await page.evaluate(({ topic, pending }) => {
    const next = structuredClone(window.__harness.currentSnapshot());
    next.state_generation = (next.state_generation ?? 0) + 1;
    const management = next.state.domain.room_management;
    management.settings!.topic = topic;
    management.operation = pending ? { kind: "pending", request_id: 99, room_id: management.settings!.room_id, operation: "settings" } : { kind: "idle" };
    window.__harness.setSnapshot(next); window.__harness.pushStateUpdate();
  }, { topic, pending });
}
test("Space description uses shared commands, preserves IME drafts, and follows Rust updates", async ({ page }) => {
  const { card, id } = await openDescription(page);
  await card.getByRole("button", { name: "Edit description" }).click();
  const control = card.getByRole("textbox", { name: "Description" });
  await control.fill("共有の説明\nSecond line");
  await control.evaluate(input => {
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", keyCode: 229, isComposing: true, bubbles: true }));
    input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
  expect(await page.evaluate(() => window.__harness.invocationsOf("update_room_setting").length)).toBe(0);
  await observe(page, "Remote shared update");
  await expect(control).toHaveValue("共有の説明\nSecond line");
  await control.dispatchEvent("compositionend");
  await card.getByRole("button", { name: "Save description" }).click();
  await expect.poll(() => page.evaluate(() => window.__harness.invocationsOf("update_room_setting").at(-1)?.args)).toEqual({ roomId: id, change: { topic: "共有の説明\nSecond line" } });
  await expect(card.getByText("Remote shared update", { exact: true })).toBeVisible();
  await expect(card.getByText("Saved", { exact: true })).toHaveCount(0);
  await observe(page, "Remote shared update", true);
  await expect(card.getByRole("button", { name: "Edit description" })).toBeDisabled();
  await observe(page, "共有の説明\nSecond line");
  await expect(card.getByRole("status")).toHaveText("Saved");
  await card.getByRole("button", { name: "Clear description" }).click();
  await expect.poll(() => page.evaluate(() => window.__harness.invocationsOf("update_room_setting").at(-1)?.args)).toEqual({ roomId: id, change: { topic: null } });
  await observe(page, null);
  await expect(card.getByText("No description", { exact: true })).toBeVisible();
});
test("read-only description stays in its card without overflow in a narrow panel", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 900, height: 700 });
  const { card } = await openDescription(page, false);
  await expect(card.getByRole("button")).toHaveCount(0);
  await expect(card).toContainText("does not let you change the description");
  await expect(card).toContainText("Shared with everyone in this Space");
  await observe(page, "共有の説明\n" + "long-description-".repeat(50));
  await expect(card).toContainText("共有の説明");
  expect(await card.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await card.scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("space-description.png"), fullPage: true });
});
