import { expect, test, type Locator } from "@playwright/test";
import { t } from "../src/i18n/messages";
import { gotoReadyShell, seedTimelineItems } from "./support/basicOperations";

async function checkHistory(editor: Locator) {
  await editor.click();
  await editor.press("End");
  await editor.press("x");
  await editor.press("y");
  const before = await editor.textContent();
  for (const [undo, redo] of [["Control+z", "Control+Shift+z"], ["Meta+z", "Meta+Shift+z"], ["Control+z", "Control+y"]]) {
    await editor.press(undo);
    await expect(editor).toHaveText(before!.slice(0, -1));
    await editor.press(redo);
    await expect(editor).toHaveText(before!);
  }
}

test("main composer keyboard history", async ({ page }) => {
  await gotoReadyShell(page);
  await checkHistory(page.locator(".composer-inline-editor").first());
});

test("thread composer keyboard history", async ({ page }) => {
  await gotoReadyShell(page);
  const main = page.locator(".composer-inline-editor").first();
  await main.click();
  await main.press("m");
  await page.getByRole("button", { name: /2 replies/ }).click();
  await checkHistory(page.getByRole("complementary", { name: t("panel.context") }).locator(".composer-inline-editor"));
  await expect(main).toHaveText("m");
});

test("inline message edit keyboard history", async ({ page }) => {
  await gotoReadyShell(page);
  await seedTimelineItems(page, [{
    id: { Event: { event_id: "$edit-history:example.invalid" } },
    sender: "@harness-user:example.invalid", sender_label: "Harness User", body: "old body",
    timestamp_ms: 1_800_000_100_000, in_reply_to_event_id: null, thread_root: null,
    thread_summary: null, reactions: [], can_react: true, is_redacted: false,
    is_hidden: false, can_redact: true, is_edited: false, can_edit: true,
    actions: { can_copy: true, can_forward: true, can_reply: true, can_permalink: true,
      can_view_source: true, editable_document: { version: 2, inlines: [{ kind: "text", text: "old body" }] } }
  }]);
  await page.getByRole("button", { name: /edit message/i }).click();
  await checkHistory(page.getByRole("textbox", { name: t("timeline.editBody") }));
});
