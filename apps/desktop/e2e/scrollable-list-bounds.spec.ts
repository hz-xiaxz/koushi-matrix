/**
 * Headless spec: the three lists that were clipped instead of scrolling (#1385).
 *
 * One symptom (content disappears while a wheel event does nothing), two causes:
 *
 *  - The room media gallery and the in-room scheduled-messages card are direct
 *    flex children of `.main-pane`, a column flex container with no `overflow`.
 *    Their default `min-block-size: auto` let them take their whole content
 *    height, and `.timeline-scroll` (`flex: 1 1 0`, `min-height: 0`) gave up all
 *    of its space first. The timeline collapsed to zero height and the composer
 *    was pushed past the pane's bottom edge, where `.desktop { overflow: hidden }`
 *    clipped it. No ancestor offered a scrollbar.
 *  - The "Room index status" list carries `settings-detail-list
 *    crawler-room-status-list`. `.settings-detail-list { overflow: hidden }` is
 *    declared after the `crawler-room-status-list` rule at the same specificity,
 *    so it won and the list never scrolled.
 *
 * Every case drives the real UI, measures rectangles, and scrolls with a real
 * wheel event: `toBeVisible()` alone is not evidence here (engineering rules
 * "Right Panel And Composer Containment"). All content is synthetic.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";
import type { ScheduledSendItem, TimelineMediaGalleryItem } from "../src/domain/types";
import { HARNESS_ROOM_ID, gotoReadyShell } from "./support/basicOperations";

const SYNTHETIC_BODY = "Synthetic scheduled body for the scroll regression.";
const GALLERY_ITEM_COUNT = 80;
const SCHEDULED_ITEM_COUNT = 11;
/** Enough rows to overflow the 280px room-index scroll box. */
const CRAWLER_ROOM_COUNT = 8;
/** The floor the timeline keeps so the composer stays inside the pane. */
const TIMELINE_MIN_HEIGHT = 100;

const MAIN_GALLERY = ".main-pane > .room-media-gallery";
const MAIN_SCHEDULED = ".main-pane > .scheduled-messages";
const ROOM_INDEX_LIST = ".crawler-room-status-list";

interface Box {
  top: number;
  bottom: number;
  height: number;
  clientHeight: number;
  scrollHeight: number;
  scrollTop: number;
  overflowY: string;
}

async function boxOf(page: Page, selector: string): Promise<Box> {
  const box = await page.evaluate((sel) => {
    const element = document.querySelector<HTMLElement>(sel);
    if (!element) {
      return null;
    }
    const rect = element.getBoundingClientRect();
    return {
      top: rect.top,
      bottom: rect.bottom,
      height: rect.height,
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
      scrollTop: element.scrollTop,
      overflowY: getComputedStyle(element).overflowY
    };
  }, selector);
  if (!box) {
    throw new Error(`no element for ${selector}`);
  }
  return box;
}

async function scrollTopOf(page: Page, selector: string): Promise<number> {
  return page.evaluate(
    (sel) => document.querySelector<HTMLElement>(sel)?.scrollTop ?? -1,
    selector
  );
}

/** Scrolls the element under the pointer with a real wheel event. */
async function wheelOver(page: Page, selector: string, deltaY: number): Promise<void> {
  await page.locator(selector).hover();
  await page.mouse.wheel(0, deltaY);
}

/** Scrolls the element to its end with real wheel events and waits for it. */
async function wheelToEnd(page: Page, selector: string): Promise<void> {
  const box = await boxOf(page, selector);
  const target = box.scrollHeight - box.clientHeight;
  await wheelOver(page, selector, box.scrollHeight);
  await expect.poll(() => scrollTopOf(page, selector)).toBeGreaterThanOrEqual(target - 1);
}

/**
 * The pane's own scroll box clips a child the user must reach: the composer was
 * pushed below the pane's bottom edge and `.desktop { overflow: hidden }` hid it.
 */
async function expectPaneContains(page: Page, childSelector: string, label: string): Promise<void> {
  const pane = await boxOf(page, ".main-pane");
  const child = await boxOf(page, childSelector);
  expect(
    child.bottom,
    `${label}: bottom ${child.bottom} exceeds the pane bottom ${pane.bottom}`
  ).toBeLessThanOrEqual(pane.bottom + 1);
  expect(
    child.top,
    `${label}: top ${child.top} starts above the pane top ${pane.top}`
  ).toBeGreaterThanOrEqual(pane.top - 1);
}

/** The last row of a scroll box is fully inside it after the user scrolls. */
async function expectLastRowReachable(
  page: Page,
  listSelector: string,
  rowSelector: string
): Promise<void> {
  const row = page.locator(listSelector).locator(rowSelector).last();
  const overflowBefore = await row.evaluate((element) => {
    const list = element.parentElement!;
    return list.scrollHeight - list.clientHeight;
  });
  expect(overflowBefore, "the list does not overflow; the case cannot fail").toBeGreaterThan(0);
  await wheelToEnd(page, listSelector);
  const overflow = await row.evaluate((element) => {
    const list = element.parentElement!;
    const rowRect = element.getBoundingClientRect();
    const listRect = list.getBoundingClientRect();
    return {
      above: listRect.top - rowRect.top,
      below: rowRect.bottom - listRect.bottom
    };
  });
  expect(overflow.above, "the last row starts above the scroll box").toBeLessThanOrEqual(0);
  expect(overflow.below, "the last row still ends below the scroll box").toBeLessThanOrEqual(0);
}

function galleryItem(index: number): TimelineMediaGalleryItem {
  return {
    event_id: `$synthetic-gallery-${index}:example.invalid`,
    room_id: HARNESS_ROOM_ID,
    sender: "@harness-user:example.invalid",
    sender_label: "Harness User",
    timestamp_ms: 1_900_000_000_000 + index,
    media: {
      kind: "Image",
      filename: `synthetic-${index}.png`,
      source: {
        mxc_uri: `mxc://example.invalid/synthetic-${index}`,
        encrypted: false,
        encryption_version: null
      },
      mimetype: "image/png",
      size: 4096,
      width: 800,
      height: 600,
      thumbnail: null
    }
  };
}

function scheduledItem(index: number): ScheduledSendItem {
  return {
    scheduled_id: `synthetic-scroll-${index}`,
    room_id: HARNESS_ROOM_ID,
    body: `${SYNTHETIC_BODY} ${index}`,
    send_at_ms: 4_000_000_000_000,
    handle: { kind: "local" }
  };
}

test("the room media gallery scrolls inside its own box instead of hiding the composer", async ({
  page
}) => {
  await page.setViewportSize({ width: 1400, height: 700 });
  await gotoReadyShell(page);
  await page.evaluate((items) => {
    const snapshot = window.__harness.currentSnapshot();
    window.__harness.setSnapshot({
      ...snapshot,
      state: {
        ...snapshot.state,
        ui: {
          ...snapshot.state.ui,
          timeline: { ...snapshot.state.ui.timeline, media_gallery: items }
        }
      }
    });
    window.__harness.pushStateUpdate();
  }, Array.from({ length: GALLERY_ITEM_COUNT }, (_, index) => galleryItem(index)));

  await page.getByRole("button", { name: t("mediaGallery.open") }).click();
  await expect(page.getByRole("region", { name: t("mediaGallery.region") })).toBeVisible();

  const gallery = await boxOf(page, MAIN_GALLERY);
  // The gallery's content is taller than its box, so the box itself must scroll.
  expect(gallery.scrollHeight).toBeGreaterThan(gallery.clientHeight);
  expect(gallery.overflowY, "the gallery is not a scroll box").toBe("auto");
  expect(gallery.height).toBeLessThan((await boxOf(page, ".main-pane")).height);

  await wheelOver(page, MAIN_GALLERY, 400);
  await expect.poll(() => scrollTopOf(page, MAIN_GALLERY)).toBeGreaterThan(0);

  // The timeline keeps usable space and the composer stays on screen.
  const timeline = await boxOf(page, ".timeline-scroll");
  expect(timeline.height, "the gallery took the timeline's whole height").toBeGreaterThanOrEqual(
    TIMELINE_MIN_HEIGHT
  );
  await expectPaneContains(page, ".composer", "gallery open");

  // The media below the fold is reachable: its last item ends inside the box.
  await wheelToEnd(page, MAIN_GALLERY);
  const lastItem = page.getByRole("button", {
    name: t("mediaGallery.openItem", { filename: `synthetic-${GALLERY_ITEM_COUNT - 1}.png` })
  });
  await expect(lastItem).toBeInViewport();
  const reach = await lastItem.evaluate((element) => {
    const list = element.closest<HTMLElement>(".room-media-gallery")!;
    return element.getBoundingClientRect().bottom - list.getBoundingClientRect().bottom;
  });
  expect(reach, "the last media row ends below the gallery's own box").toBeLessThanOrEqual(0);
});

test("the in-room scheduled card scrolls inside its own box at the minimum window height", async ({
  page
}) => {
  // 620pt is the native minimum window height (apps/desktop/src-tauri/tauri.conf.json).
  await page.setViewportSize({ width: 1400, height: 620 });
  await gotoReadyShell(page);
  await page.evaluate((items) => {
    const snapshot = window.__harness.currentSnapshot();
    window.__harness.setSnapshot({
      ...snapshot,
      state: {
        ...snapshot.state,
        ui: {
          ...snapshot.state.ui,
          timeline: {
            ...snapshot.state.ui.timeline,
            scheduled_send_capability: "localFallback" as const,
            scheduled_sends: items
          }
        }
      }
    });
    window.__harness.pushStateUpdate();
  }, Array.from({ length: SCHEDULED_ITEM_COUNT }, (_, index) => scheduledItem(index)));

  await expect(page.locator(MAIN_SCHEDULED)).toBeVisible();

  const card = await boxOf(page, MAIN_SCHEDULED);
  expect(card.scrollHeight).toBeGreaterThan(card.clientHeight);
  expect(card.overflowY, "the in-room card is not a scroll box").toBe("auto");

  await wheelOver(page, MAIN_SCHEDULED, 400);
  await expect.poll(() => scrollTopOf(page, MAIN_SCHEDULED)).toBeGreaterThan(0);

  // The timeline keeps usable space and the composer — including its text
  // field — stays inside the pane.
  const timeline = await boxOf(page, ".timeline-scroll");
  expect(timeline.height, "the card took the timeline's whole height").toBeGreaterThanOrEqual(
    TIMELINE_MIN_HEIGHT
  );
  await expectPaneContains(page, ".composer", "scheduled card open");
  await expect(page.getByRole("textbox", { name: t("composer.messageComposer") })).toBeInViewport();

  // The last queued message stays reachable by wheel.
  await wheelToEnd(page, MAIN_SCHEDULED);
  await expect(page.locator(".main-pane .scheduled-message-item").last()).toBeInViewport();
});

test("the Settings room-index list scrolls instead of clipping its rows", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await gotoReadyShell(page);
  await page.evaluate((count) => {
    const snapshot = window.__harness.currentSnapshot();
    const base = snapshot.state.domain.rooms[0];
    const rooms = Array.from({ length: count }, (_, index) => ({
      ...base,
      room_id: `!synthetic-crawler-${index}:example.invalid`,
      display_label: `Synthetic Room ${index + 1}`
    }));
    const crawlerRooms = Object.fromEntries(
      rooms.map((room) => [room.room_id, { kind: "completed" as const, indexed: 3 }])
    );
    window.__harness.setSnapshot({
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          rooms: [...snapshot.state.domain.rooms, ...rooms],
          search_crawler: { ...snapshot.state.domain.search_crawler, rooms: crawlerRooms }
        }
      }
    });
    window.__harness.pushStateUpdate();
  }, CRAWLER_ROOM_COUNT);

  await page.getByRole("button", { name: t("workspace.userSettings") }).click();
  await page.getByRole("tab", { name: "Search history", exact: true }).click();
  const list = page.locator(ROOM_INDEX_LIST);
  await expect(list).toBeVisible();
  await expect(list.locator(".settings-detail-row")).toHaveCount(CRAWLER_ROOM_COUNT);

  // The page reaches its end with the list still clipped, so only the list's own
  // box can reach the rows below its 280px cap.
  const before = await boxOf(page, ROOM_INDEX_LIST);
  expect(before.scrollHeight).toBeGreaterThan(before.clientHeight);
  expect(before.overflowY, "the room-index list is not a scroll box").toBe("auto");

  await expectLastRowReachable(page, ROOM_INDEX_LIST, ".settings-detail-row");
  const after = await boxOf(page, ROOM_INDEX_LIST);
  expect(after.scrollTop).toBeLessThanOrEqual(after.scrollHeight - after.clientHeight);
});
