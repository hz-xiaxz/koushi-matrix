import { expect, test } from "@playwright/test";

import { t } from "../src/i18n/messages";
import { gotoReadyShell, invocationCount } from "./support/basicOperations";

/**
 * #1363/#1366: the sidebar row bound end to end in the browser tier.
 *
 * The harness fake never computes a bound: it attaches the Rust-authored cases
 * (`sidebarSectionBounds.generated.json`) to the snapshot it installs. This spec
 * proves the whole path a user takes — Rust's numbers decide the rows, the
 * control names the hidden count, pressing it dispatches the typed `expanded`
 * preference, and the filter exception shows every match.
 */
test("a long section shows five rows and the control reveals and bounds it again", async ({
  page
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReadyShell(page);
  await page.evaluate(() => {
    const initial = window.__harness.currentSnapshot();
    const template = initial.sidebar.sections.rooms[0];
    const rooms = Array.from({ length: 7 }, (_, index) => ({
      ...template,
      room_id: `!bound-room-${index}:example.invalid`,
      display_name: `Bound room ${index}`
    }));
    window.__harness.setSnapshot({
      ...initial,
      sidebar: {
        ...initial.sidebar,
        sections: { ...initial.sidebar.sections, rooms },
        space_rooms: rooms
      }
    });
    window.__harness.pushStateUpdate();
  });

  const section = page.locator('[data-room-section="rooms"]');
  const rows = section.locator(".room-item");
  const control = section.locator(".room-section-bound");

  // The bound came from the fake's Rust-authored lookup: five of seven rows,
  // with a full-width control naming the two the bound hides.
  await expect(rows).toHaveCount(5);
  await expect(control).toHaveAttribute("aria-expanded", "false");
  await expect(control).toHaveText(t("roomList.showMore", { count: 2 }));
  await expect(control).toHaveAttribute(
    "aria-label",
    t("roomList.showMoreInSection", { count: 2, section: t("roomList.categoryRooms") })
  );

  await control.click();
  await expect(rows).toHaveCount(7);
  await expect(control).toHaveAttribute("aria-expanded", "true");
  await expect(control).toHaveText(t("roomList.showFewer"));
  // The control changes only the bound, and it reaches Rust as one typed patch.
  expect(await invocationCount(page, "update_settings")).toBe(1);
  const patches = await page.evaluate(() => window.__harness.invocationsOf("update_settings"));
  expect(patches[0]?.args.patch.sidebar_section).toEqual({
    scope: "__home__",
    section: "rooms",
    expanded: true
  });

  await control.click();
  await expect(rows).toHaveCount(5);
  await expect(control).toHaveAttribute("aria-expanded", "false");

  // The search filter is renderer presentation state, so every match renders
  // and the control disappears until the filter is cleared.
  await page.getByLabel(t("roomList.filterConversationsPlaceholder")).fill("Bound room");
  await expect(rows).toHaveCount(7);
  await expect(control).toHaveCount(0);
  await page.getByLabel(t("roomList.filterConversationsPlaceholder")).fill("");
  await expect(rows).toHaveCount(5);
  await expect(control).toHaveAttribute("aria-expanded", "false");
});
