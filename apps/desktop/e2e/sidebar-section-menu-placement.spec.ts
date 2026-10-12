/**
 * Headless spec: the sidebar section options menu floats in the shared layer (#1369).
 *
 * The menu used to be `position: absolute` inside `.section-menu-wrap`, which
 * lives inside the scrolling `.sidebar-scroll`. Two consequences, both pinned
 * here:
 *
 *  1. A menu opened from a section near the bottom of a scrolled sidebar was cut
 *     off at the scroll viewport (and stretched the sidebar's own scroll extent,
 *     moving the scroll position under the pointer). The menu now renders
 *     `fixed` in the shared body-level floating layer, bounded by the sidebar
 *     scroll viewport.
 *  2. A menu opened mid-list overlays the next section's header, so the covered
 *     `⋯` trigger is not pointer-reachable while the menu is open.
 *
 * #1369 recorded this decision for the overlap: an open menu MAY cover the next
 * section's header. It is a floating overlay like every other popup in the
 * shell, the alternative placements only cover a different control, and a
 * click-guard backdrop is dismissal behaviour that #1368 owns — that owner
 * deliberately does not swallow the press, and #1368's own coverage asserts the
 * same press still reaches the conversation underneath. What the decision pins:
 * the covered trigger cannot be activated by accident, stays reachable by
 * keyboard, and is pointer-reachable again once the menu closes.
 */

import { expect, test, type Page } from "@playwright/test";

import type { StateUpdateEnvelope } from "../src/domain/coreEvents";
import type { DesktopSnapshot, RoomListItem } from "../src/domain/types";
import { t } from "../src/i18n/messages";
import { pushSnapshot } from "./support/stateUpdates";

interface Harness {
  currentSnapshot(): DesktopSnapshot;
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

const dm = row("!dm-placement:example.invalid", "Synthetic DM");

function section(page: Page, label: string) {
  return page.getByRole("region", { name: label, exact: true });
}

function optionsTrigger(page: Page, label: string) {
  return section(page, label).getByRole("button", {
    name: t("roomList.sectionOptions", { section: label }), exact: true
  });
}

/**
 * Replace the sidebar with `roomCount` synthetic rooms and one DM, and wait
 * until the rows the caller names are on screen. `section_bounds` is dropped so
 * the fixture renders every seeded row: this spec measures scroll geometry, and
 * a bounded section (#1363) could hide the rows that make the sidebar scroll.
 */
async function seedSidebar(page: Page, roomCount: number): Promise<void> {
  const base = await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.currentSnapshot());
  await pushSnapshot(page, {
    ...base,
    sidebar: {
      ...base.sidebar,
      section_bounds: null,
      global_dms: [dm],
      sections: {
        ...base.sidebar.sections,
        rooms: Array.from({ length: roomCount }, (_, index) =>
          row(`!room-${index}:example.invalid`, `Synthetic room ${index}`)),
        people: [dm]
      }
    }
  } satisfies DesktopSnapshot);
}

function scrollerGeometry(page: Page) {
  return page.evaluate(() => {
    const scroller = document.querySelector(".sidebar-scroll")!;
    return { scrollHeight: scroller.scrollHeight, scrollTop: scroller.scrollTop };
  });
}

function menuGeometry(page: Page, label: string) {
  return page.evaluate((sectionLabel) => {
    const scroller = document.querySelector(".sidebar-scroll")!;
    const scrollerRect = scroller.getBoundingClientRect();
    const menu = document.querySelector(".section-menu")!;
    const menuRect = menu.getBoundingClientRect();
    const trigger = document
      .querySelector(`section[aria-label="${sectionLabel}"]`)!
      .querySelector<HTMLElement>('button[aria-label^="Options for"]')!;
    const triggerRect = trigger.getBoundingClientRect();
    const point = {
      x: triggerRect.left + triggerRect.width / 2,
      y: triggerRect.top + triggerRect.height / 2
    };
    const hit = document.elementFromPoint(point.x, point.y) as HTMLElement | null;
    return {
      menuRect: menuRect.toJSON(),
      scrollerRect: scrollerRect.toJSON(),
      // #1369: the menu belongs to the body-level floating layer, so no scroll
      // viewport can clip it and it cannot extend one either.
      insideSidebarScroll: scroller.contains(menu),
      point,
      hitInsideMenu: hit?.closest(".section-menu") != null,
      hitRole: hit?.closest<HTMLElement>('[role^="menuitem"]')?.getAttribute("role") ?? null,
      hitIsTrigger: hit != null && trigger.contains(hit)
    };
  }, label);
}

function menuTriggerGap(page: Page, label: string): Promise<number> {
  return page.evaluate((sectionLabel) => {
    const menuRect = document.querySelector(".section-menu")!.getBoundingClientRect();
    const trigger = document
      .querySelector(`section[aria-label="${sectionLabel}"]`)!
      .querySelector<HTMLElement>('button[aria-label^="Options for"]')!;
    return Math.abs(trigger.getBoundingClientRect().top - menuRect.bottom);
  }, label);
}

test("a menu near the bottom of a scrolled sidebar stays inside the sidebar", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedSidebar(page, 24);
  await expect(section(page, labels.rooms)
    .getByRole("button", { name: "Synthetic room 0" })).toBeVisible();

  // Scroll to the last section, so a menu hanging below its header would leave
  // the scroll viewport.
  await page.evaluate(() => {
    const scroller = document.querySelector(".sidebar-scroll")!;
    scroller.scrollTop = scroller.scrollHeight;
  });
  const before = await scrollerGeometry(page);
  // The premise of this spec: the sidebar really is scrolled, so a menu below
  // the trigger has nowhere to go inside the scroll viewport.
  expect(before.scrollTop).toBeGreaterThan(0);

  await optionsTrigger(page, labels.dms).click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  const { menuRect, scrollerRect, insideSidebarScroll, hitIsTrigger } =
    await menuGeometry(page, labels.dms);
  const after = await scrollerGeometry(page);

  // The menu owns the shared floating layer instead of the scroll viewport.
  expect(insideSidebarScroll).toBe(false);
  // Fully visible: entirely inside the sidebar's scroll viewport.
  expect(menuRect.top).toBeGreaterThanOrEqual(scrollerRect.top);
  expect(menuRect.left).toBeGreaterThanOrEqual(scrollerRect.left);
  expect(menuRect.right).toBeLessThanOrEqual(scrollerRect.right);
  expect(menuRect.bottom).toBeLessThanOrEqual(scrollerRect.bottom);
  // Opening it neither stretches the sidebar nor moves what the user scrolls to.
  expect(after.scrollHeight).toBe(before.scrollHeight);
  expect(after.scrollTop).toBe(before.scrollTop);
  // This menu flipped above its trigger, so the trigger stayed reachable.
  expect(hitIsTrigger).toBe(true);
});

test("an open menu covers the next section's trigger without activating it", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedSidebar(page, 1);

  await optionsTrigger(page, labels.rooms).click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  const { point, hitInsideMenu, hitRole, insideSidebarScroll } = await menuGeometry(page, labels.dms);
  // Recorded decision: the open menu may cover the next section's header, and
  // the point aimed at the covered trigger belongs to the menu, not the trigger.
  expect(insideSidebarScroll).toBe(false);
  expect(hitInsideMenu).toBe(true);
  // The covered point falls on an actionable menu row. That is the honest
  // precondition of the press below: a press on a visible row runs that row and
  // closes the menu, a press on non-actionable menu chrome (title, separator,
  // padding) is inside the popup and therefore belongs to no dismissal owner —
  // #1368 deliberately keeps the press that dismisses outside the menu. If the
  // layout ever moves this point off a row, this assertion fails first and the
  // covered-press contract has to be decided again rather than silently lost.
  expect(hitRole).toMatch(/^menuitem/);

  // The press dismisses the open menu and does not activate the covered trigger:
  // no second menu, and the covered section stays collapsed.
  await page.mouse.click(point.x, point.y);
  await expect(page.locator(".section-menu")).toHaveCount(0);
  await expect(optionsTrigger(page, labels.dms)).toHaveAttribute("aria-expanded", "false");

  // The coverage was transient: the trigger is pointer-reachable again.
  await optionsTrigger(page, labels.dms).click();
  await expect(page.locator("[role='menu']")).toHaveCount(1);
  await expect(optionsTrigger(page, labels.dms)).toHaveAttribute("aria-expanded", "true");
});

test("focus enters the menu on both the pointer and the keyboard path", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedSidebar(page, 1);

  // The layer hides the menu until it has measured and placed it, and a hidden
  // element cannot take focus, so focus must wait for the placement.
  await optionsTrigger(page, labels.rooms).focus();
  await page.keyboard.press("Enter");
  const first = page.locator(".section-menu [role='menuitemradio']").first();
  await expect(first).toBeFocused();

  await page.keyboard.press("ArrowDown");
  await expect(page.locator(".section-menu [role='menuitemradio']").nth(1)).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(optionsTrigger(page, labels.rooms)).toBeFocused();

  await optionsTrigger(page, labels.rooms).click();
  await expect(page.locator(".section-menu [role='menuitemradio']").first()).toBeFocused();
});

test("the menu keeps following its trigger while the sidebar scrolls", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedSidebar(page, 24);
  await expect(section(page, labels.rooms)
    .getByRole("button", { name: "Synthetic room 0" })).toBeVisible();

  await page.evaluate(() => {
    const scroller = document.querySelector(".sidebar-scroll")!;
    scroller.scrollTop = scroller.scrollHeight;
  });
  await optionsTrigger(page, labels.dms).click();
  await expect(page.locator(".section-menu")).toHaveCount(1);

  const gapBefore = await menuTriggerGap(page, labels.dms);
  // A small scroll keeps the trigger inside the scrollport, so the menu must
  // move with it: the layer re-resolves measured coordinates because a `fixed`
  // menu no longer travels with the scrolled content.
  await page.evaluate(() => {
    document.querySelector(".sidebar-scroll")!.scrollTop -= 40;
  });
  await expect.poll(() => menuTriggerGap(page, labels.dms)).toBeCloseTo(gapBefore, 1);

  const { menuRect, scrollerRect } = await menuGeometry(page, labels.dms);
  expect(menuRect.top).toBeGreaterThanOrEqual(scrollerRect.top);
  expect(menuRect.bottom).toBeLessThanOrEqual(scrollerRect.bottom);
});

test("the menu keeps the sidebar header's typography out of the sidebar", async ({ page }) => {
  await page.goto("/appHarness.html");
  await expect(section(page, labels.rooms)).toBeVisible();
  await seedSidebar(page, 1);

  await optionsTrigger(page, labels.rooms).click();
  const item = page.locator(".section-menu-item").first();
  await expect(item).toBeVisible();
  // Portaled out of `.section-title`, so the inherited header font is gone
  // unless the menu states it itself: a body-level menu must not re-typograph.
  expect(await item.evaluate((element) => getComputedStyle(element).fontSize)).toBe("13px");
  expect(await item.evaluate((element) => getComputedStyle(element).fontWeight)).toBe("800");
});
