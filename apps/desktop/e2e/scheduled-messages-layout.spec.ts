/**
 * Headless spec: the scheduled-message surfaces' rendered layout.
 *
 * Three defects are pinned here, all measured from the DOM the user sees:
 *
 *  - the right panel's card was the middle `minmax(0, 1fr)` row of the panel
 *    shell, so its `auto` grid rows stretched across the whole pane: the
 *    heading, the local-fallback note, and the item drifted hundreds of pixels
 *    apart instead of keeping the card's own 8px rhythm;
 *  - `.scheduled-message-item` used an implicit `auto` action column, so the
 *    buttons took their intrinsic width and left the preview (a few characters
 *    wide in the 320px panel) or the edit form crushed beside it. The
 *    containment contract in engineering rules "Right Panel And Composer
 *    Containment" requires explicit `minmax(0, 1fr)` tracks;
 *  - the composer's send-later clock showed no sign that the room already had
 *    messages waiting.
 *
 * Every case seeds Rust-shaped transport values (`state.ui.scheduled_sends` and
 * `state.ui.scheduled_sends_list`), drives real pointer clicks, measures
 * rectangles, and hit-tests the panel's controls with `elementFromPoint`. All
 * content is synthetic.
 */

import { expect, test, type Page } from "@playwright/test";
import type { ScheduledSendItem } from "../src/domain/types";
import { t } from "../src/i18n/messages";
import { HARNESS_ROOM_ID, gotoReadyShell, invocationCount } from "./support/basicOperations";
import {
  RIGHT_PANEL_SELECTOR,
  containmentReport,
  dragResizer,
  expectContained,
  type ContainmentProbe
} from "./support/panelGeometry";

const HARNESS_SPACE_ID = "!harness-space:example.invalid";
const SYNTHETIC_BODY = "Synthetic scheduled body for the layout regression.";
const MIN_PANEL_WIDTH = 320;

const PANEL_IDLE_PROBE: ContainmentProbe = {
  root: RIGHT_PANEL_SELECTOR,
  controls: {
    close: ".thread-header button",
    edit: ".scheduled-message-actions button"
  },
  rows: [".scheduled-messages-heading", ".scheduled-message-item", ".scheduled-message-main"]
};

const PANEL_EDIT_PROBE: ContainmentProbe = {
  root: RIGHT_PANEL_SELECTOR,
  controls: {
    close: ".thread-header button",
    save: ".scheduled-message-edit .scheduled-message-actions button:last-child"
  },
  rows: [".scheduled-message-item", ".scheduled-message-edit", ".scheduled-send-adjustments"]
};

function scheduledItem(overrides: Partial<ScheduledSendItem> = {}): ScheduledSendItem {
  return {
    scheduled_id: "synthetic-layout-1",
    room_id: HARNESS_ROOM_ID,
    body: SYNTHETIC_BODY,
    send_at_ms: 4_000_000_000_000,
    handle: { kind: "local" },
    ...overrides
  };
}

/** Publishes the account queue the composer reads for its clock badge. */
async function seedRoomQueue(page: Page, items: ScheduledSendItem[]): Promise<void> {
  await page.evaluate((scheduledSends) => {
    const current = window.__harness.currentSnapshot();
    window.__harness.setSnapshot({
      ...current,
      state: {
        ...current.state,
        ui: {
          ...current.state.ui,
          timeline: {
            ...current.state.ui.timeline,
            scheduled_send_capability: "localFallback" as const,
            scheduled_sends: scheduledSends
          }
        }
      }
    });
    window.__harness.pushStateUpdate();
  }, items);
}

/** Registers the Rust-shaped Open projection and opens the panel by toolbar click. */
async function openScheduledPanel(page: Page, items: ScheduledSendItem[]): Promise<void> {
  await page.evaluate(
    ({ scheduledItems, spaceId }) => {
      window.__harness.setCommandResponse("open_scheduled_sends_list", () => {
        const current = window.__harness.currentSnapshot();
        const next = {
          ...current,
          state: {
            ...current.state,
            ui: {
              ...current.state.ui,
              scheduled_sends_list: {
                kind: "open" as const,
                scope: { kind: "space" as const, space_id: spaceId },
                capability: "localFallback" as const,
                items: scheduledItems
              }
            }
          }
        };
        window.__harness.setSnapshot(next);
        return next;
      });
    },
    { scheduledItems: items, spaceId: HARNESS_SPACE_ID }
  );
  await page.locator('[data-header-action="scheduled"]').click();
  await expect(page.locator(`${RIGHT_PANEL_SELECTOR} .scheduled-messages`)).toBeVisible();
}

/** Minimizes the shared right panel to its 320px floor (dragging its inline-start edge end-wards). */
async function minimizeRightPanel(page: Page): Promise<void> {
  await dragResizer(page, t("workspace.resizeRightPanel"), 600);
  await expect
    .poll(() =>
      page.locator(RIGHT_PANEL_SELECTOR).evaluate((element) => element.getBoundingClientRect().width)
    )
    .toBe(MIN_PANEL_WIDTH);
}

test("the panel keeps its own row rhythm instead of stretching across the pane", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await gotoReadyShell(page);
  await openScheduledPanel(page, [scheduledItem()]);

  const report = await page.evaluate(() => {
    const box = (selector: string) => {
      const element = document.querySelector<HTMLElement>(selector);
      if (!element) {
        return null;
      }
      const rect = element.getBoundingClientRect();
      return { top: rect.top, bottom: rect.bottom, height: rect.height };
    };
    return {
      pane: box(".thread-pane")!,
      card: box(".thread-pane .scheduled-messages")!,
      heading: box(".thread-pane .scheduled-messages-heading")!,
      note: box(".thread-pane .scheduled-messages-note")!,
      item: box(".thread-pane .scheduled-message-item")!
    };
  });

  // The card's own 8px gap, not the pane's leftover height, separates the rows.
  expect(report.note.top - report.heading.bottom).toBeLessThanOrEqual(16);
  expect(report.item.top - report.note.bottom).toBeLessThanOrEqual(16);
  // One short item and its actions are content-sized, and the card hugs them.
  expect(report.item.height).toBeLessThan(200);
  expect(report.card.height).toBeLessThan(report.pane.height - 200);
});

test("the panel's scheduled list stays contained at the minimum panel width", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await gotoReadyShell(page);
  await openScheduledPanel(page, [scheduledItem()]);
  await minimizeRightPanel(page);

  await expectContained(page, PANEL_IDLE_PROBE, "idle scheduled panel escapes its own shell");

  // The preview keeps a readable measure: the actions stack under it instead of
  // taking an intrinsic-width column from it.
  const idle = await page.evaluate(() => {
    const measure = (selector: string) => {
      const element = document.querySelector<HTMLElement>(selector)!;
      const rect = element.getBoundingClientRect();
      return { width: rect.width, clientWidth: element.clientWidth, height: rect.height };
    };
    return {
      item: measure(".scheduled-message-item"),
      main: measure(".scheduled-message-main"),
      body: measure(".scheduled-message-body"),
      actions: measure(".scheduled-message-actions")
    };
  });
  expect(idle.main.width).toBeGreaterThanOrEqual(idle.item.width - 1);
  expect(idle.body.width).toBeGreaterThanOrEqual(200);

  // A real click opens the edit form; it must take the whole item, not a column.
  await page.getByRole("button", { name: t("scheduled.edit") }).click();
  await expect(page.locator(".scheduled-message-edit")).toBeVisible();

  const editing = await page.evaluate(() => {
    const element = document.querySelector<HTMLElement>(".scheduled-message-edit")!;
    const item = document.querySelector<HTMLElement>(".scheduled-message-item")!;
    const rect = element.getBoundingClientRect();
    return {
      editWidth: rect.width,
      editTop: rect.top,
      itemWidth: item.getBoundingClientRect().width,
      mainBottom: document
        .querySelector<HTMLElement>(".scheduled-message-main")!
        .getBoundingClientRect().bottom
    };
  });
  expect(editing.editWidth).toBeGreaterThanOrEqual(editing.itemWidth - 1);
  // Stacked below the preview it edits, never beside it.
  expect(editing.editTop).toBeGreaterThanOrEqual(editing.mainBottom - 1);

  await expectContained(page, PANEL_EDIT_PROBE, "scheduled edit form escapes its own shell");

  // The edit form's own save action still dispatches the typed command.
  await page.getByRole("button", { name: t("scheduled.save") }).click();
  await expect.poll(() => invocationCount(page, "reschedule_scheduled_send")).toBe(1);
});

test("a queue longer than the pane row scrolls inside the card", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 700 });
  await gotoReadyShell(page);
  const queue = Array.from({ length: 12 }, (_, index) =>
    scheduledItem({ scheduled_id: `synthetic-layout-${index}`, body: `${SYNTHETIC_BODY} ${index}` })
  );
  await openScheduledPanel(page, queue);
  await minimizeRightPanel(page);

  const report = await page.evaluate(() => {
    const card = document.querySelector<HTMLElement>(".thread-pane .scheduled-messages")!;
    const pane = document.querySelector<HTMLElement>(".thread-pane")!;
    const cardRect = card.getBoundingClientRect();
    const paneRect = pane.getBoundingClientRect();
    const items = Array.from(card.querySelectorAll<HTMLElement>(".scheduled-message-item"));
    return {
      cardHeight: cardRect.height,
      cardBottom: cardRect.bottom,
      paneHeight: paneRect.height,
      paneBottom: paneRect.bottom,
      scrollHeight: card.scrollHeight,
      clientHeight: card.clientHeight,
      lastItemBottom: items[items.length - 1]!.getBoundingClientRect().bottom
    };
  });

  // Capped by the pane row (never past the pane) and scrollable inside the card.
  expect(report.cardBottom).toBeLessThanOrEqual(report.paneBottom + 1);
  expect(report.cardHeight).toBeLessThan(report.paneHeight);
  expect(report.scrollHeight).toBeGreaterThan(report.clientHeight);
  await expectContained(page, PANEL_IDLE_PROBE, "a long scheduled queue escapes its own shell");
});

test("the timeline card's edit form spans the item instead of a side column", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await gotoReadyShell(page);
  await seedRoomQueue(page, [scheduledItem()]);

  const card = page.locator(".main-pane .scheduled-messages");
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: t("scheduled.edit") }).click();
  await expect(page.locator(".main-pane .scheduled-message-edit")).toBeVisible();

  const report = await page.evaluate(() => {
    const pane = document.querySelector<HTMLElement>(".main-pane")!;
    const item = document.querySelector<HTMLElement>(".main-pane .scheduled-message-item")!;
    const edit = document.querySelector<HTMLElement>(".main-pane .scheduled-message-edit")!;
    const composer = edit.querySelector<HTMLElement>(".composer")!;
    const composerRect = composer.getBoundingClientRect();
    return {
      paneWidth: pane.getBoundingClientRect().width,
      itemWidth: item.getBoundingClientRect().width,
      editWidth: edit.getBoundingClientRect().width,
      composerRight: composerRect.right,
      paneRight: pane.getBoundingClientRect().right
    };
  });

  expect(report.editWidth).toBeGreaterThanOrEqual(report.itemWidth - 1);
  expect(report.editWidth).toBeGreaterThan(report.itemWidth * 0.9);
  expect(report.composerRight).toBeLessThanOrEqual(report.paneRight + 1);
});

test("the composer's clock badges this room's pending reservations", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await gotoReadyShell(page);

  const clock = page.locator(".main-pane .composer-schedule-button");
  await expect(clock).toHaveAccessibleName(t("scheduled.sendLater"));
  await expect(page.locator(".main-pane .composer-schedule-badge")).toHaveCount(0);

  await seedRoomQueue(page, [scheduledItem(), scheduledItem({ scheduled_id: "synthetic-layout-2" })]);

  await expect(clock).toHaveAccessibleName(t("scheduled.sendLaterWithCount", { count: 2 }));
  await expect(clock).toHaveAttribute("data-count", "2");
  await expect(page.locator(".main-pane .composer-schedule-badge")).toHaveText("2");

  // The badge is the room's queue, so an empty queue retires it again.
  await seedRoomQueue(page, []);
  await expect(clock).toHaveAccessibleName(t("scheduled.sendLater"));
  await expect(page.locator(".main-pane .composer-schedule-badge")).toHaveCount(0);

  // The control still opens the schedule form after a count has come and gone.
  await page.getByRole("textbox", { name: t("composer.messageComposer") }).fill("Synthetic body");
  await clock.click();
  await expect(page.locator(".main-pane .scheduled-send-form")).toBeVisible();
});
