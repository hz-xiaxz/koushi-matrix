/**
 * Headless spec: a non-modal popup dismisses on an outside press (#1365/#1368).
 *
 * The reported case is the sidebar section's own `⋯` menu. It renders without a
 * backdrop, so before the fix nothing in the DOM owned a press outside it: the
 * menu stayed on screen while the user selected another conversation, changed
 * section, or clicked anywhere else. The press must dismiss the popup *and*
 * still reach what it was aimed at.
 *
 *  1. The press that selects another conversation also dismisses the menu.
 *  2. One section menu is open at a time.
 *  3. Escape closes the menu and returns focus to its trigger.
 *  4. The timeline pane and the shell outside the sidebar also dismiss it.
 *  5. A control surface that already owned its dismissal keeps working.
 *  6. Choosing a sort item leaves no explanation bubble over the sidebar (#1388).
 */

import { expect, test, type Page } from "@playwright/test";
import type { StateUpdateEnvelope } from "../src/domain/coreEvents";
import type { DesktopSnapshot, RoomListItem } from "../src/domain/types";
import { t } from "../src/i18n/messages";

interface Harness {
  currentSnapshot(): DesktopSnapshot;
  pushStateUpdate(envelope: StateUpdateEnvelope): void;
  invocationsOf(command: string): Array<{ args: Record<string, unknown> }>;
  clearInvocations(): void;
}

const labels = { rooms: t("roomList.categoryRooms"), dms: t("roomList.categoryDms") };

function row(room_id: string, display_name: string): RoomListItem {
  return {
    room_id, display_name, avatar: null,
    tags: { favourite: null, low_priority: null },
    unread_count: 0, notification_count: 0, display_count: 0, highlight_count: 0,
    has_unread_content: false, is_attention_highlighted: false,
    has_unread_mention: false, is_muted: false
  };
}

const dm = row("!dm-dismissal:example.invalid", "Synthetic DM");

function section(page: Page, label: string) {
  return page.getByRole("region", { name: label, exact: true });
}

function optionsTrigger(page: Page, label: string) {
  return section(page, label).getByRole("button", {
    name: t("roomList.sectionOptions", { section: label }), exact: true
  });
}

async function currentSection(page: Page, section: "rooms" | "people") {
  return page.evaluate((key) => {
    const rows = (window as unknown as { __harness: Harness }).__harness
      .currentSnapshot().sidebar.sections[key];
    return rows.map(({ room_id, display_name }) => ({ room_id, display_name }));
  }, section);
}

/** Add one synthetic DM, so the reported DM-section menu exists to open. */
async function seedDirectMessage(page: Page): Promise<void> {
  const base = await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.currentSnapshot());
  const generation = (base.state_generation ?? 0) + 1;
  await page.evaluate((envelope) => {
    (window as unknown as { __harness: Harness }).__harness.pushStateUpdate(envelope);
  }, {
    protocol_version: 1,
    kind: "snapshot",
    generation,
    reason: "settlement",
    snapshot: {
      ...base,
      state_generation: generation,
      sidebar: {
        ...base.sidebar,
        global_dms: [dm],
        sections: { ...base.sidebar.sections, people: [dm] }
      }
    }
  } satisfies StateUpdateEnvelope);
}

test("an outside press dismisses the section menu and still selects the conversation", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedDirectMessage(page);
  const [other] = await currentSection(page, "rooms");

  const dmsTrigger = optionsTrigger(page, labels.dms);
  await dmsTrigger.click();
  await expect(page.locator(".section-menu")).toHaveCount(1);
  await expect(dmsTrigger).toHaveAttribute("aria-expanded", "true");
  await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.clearInvocations());

  // The press lands on another conversation's row: it must dismiss the menu and
  // still select that conversation in the same press. (The reported case: the
  // menu stayed open over the room list until the user selected another room,
  // and selecting that room did not dismiss it either.)
  await section(page, labels.rooms)
    .getByRole("button", { name: other!.display_name })
    .click();

  await expect(page.locator(".section-menu")).toHaveCount(0);
  await expect(dmsTrigger).toHaveAttribute("aria-expanded", "false");
  const selections = await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.invocationsOf("select_room")
      .map((invocation) => invocation.args.roomId));
  expect(selections).toEqual([other!.room_id]);
});

// #1388: choosing an item returns focus to the trigger, and that visit must not
// leave the trigger's explanation bubble floating over the sidebar.
test("choosing a section sort option leaves no explanation bubble", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.dms)).toBeVisible();
  await seedDirectMessage(page);

  await optionsTrigger(page, labels.dms).click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  await page.getByRole("menuitemradio", { name: t("roomList.sortName") }).click();

  await expect(page.locator(".section-menu")).toHaveCount(0);
  await expect(page.locator(".tooltip-bubble.is-open")).toHaveCount(0);
});

test("only one section menu is open at a time", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedDirectMessage(page);

  await optionsTrigger(page, labels.rooms).click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  // The open menu overlays the next section's trigger (its placement inside the
  // scrolling sidebar is a separate defect), so the second menu is opened the
  // way it stays reachable: keyboard focus plus Enter.
  await optionsTrigger(page, labels.dms).focus();
  await page.keyboard.press("Enter");

  await expect(page.locator(".section-menu")).toHaveCount(1);
  await expect(optionsTrigger(page, labels.rooms)).toHaveAttribute("aria-expanded", "false");
  await expect(optionsTrigger(page, labels.dms)).toHaveAttribute("aria-expanded", "true");
});

test("Escape closes the section menu and returns focus to its trigger", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.dms)).toBeVisible();
  await seedDirectMessage(page);

  const dmsTrigger = optionsTrigger(page, labels.dms);
  await dmsTrigger.click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  await page.keyboard.press("Escape");

  await expect(page.locator(".section-menu")).toHaveCount(0);
  await expect(dmsTrigger).toBeFocused();
});

test("a press on the timeline pane or the shell outside the sidebar dismisses the menu", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.dms)).toBeVisible();
  await seedDirectMessage(page);
  const dmsTrigger = optionsTrigger(page, labels.dms);

  // The reported menu stayed open over the timeline and the rest of the shell,
  // not only over the room list.
  for (const target of [
    page.locator(".timeline-view"),
    page.getByRole("textbox", { name: t("workspace.search") })
  ]) {
    await dmsTrigger.click();
    await expect(page.locator(".section-menu")).toHaveCount(1);

    await target.click();

    await expect(page.locator(".section-menu")).toHaveCount(0);
  }
});

test("a surface that already owned its dismissal still closes on an outside press", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  const [other] = await currentSection(page, "rooms");

  // The emoji picker owns its own outside-press dismissal (EmojiPicker.tsx).
  // It is the green control: the shared contract must not take that over, and
  // the new document-level listener must not swallow the press before it.
  await page.getByRole("button", { name: t("composer.emoji"), exact: true }).click();
  const picker = page.getByRole("dialog", { name: t("composer.emoji"), exact: true });
  await expect(picker).toBeVisible();

  await section(page, labels.rooms)
    .getByRole("button", { name: other!.display_name })
    .click();

  await expect(picker).toHaveCount(0);
});
