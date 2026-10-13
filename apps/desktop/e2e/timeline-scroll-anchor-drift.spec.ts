/**
 * Real-browser regression for free-scroll stability.
 *
 * `.timeline-view` deliberately disables browser scroll anchoring
 * (`overflow-anchor: none`) so Koushi owns event-based viewport stability.
 * jsdom cannot prove real scroll positions or wheel behavior, so this spec uses
 * Playwright/Chromium and the real TimelineView harness.
 */

import { expect, test, type Page } from "@playwright/test";
import { roomTimelineKey } from "../src/domain/coreEvents";

const HARNESS_ROOM_ID = "!harness-room:example.invalid";
const HARNESS_ACCOUNT_KEY = "@harness-user:example.invalid";
const HARNESS_SEED_EVENT_ID = "$seed-event:example.invalid";
const ANCHOR_PIXEL_TOLERANCE = 3;

const TIMELINE_KEY = roomTimelineKey(HARNESS_ACCOUNT_KEY, HARNESS_ROOM_ID);

function makeEventItem(index: number, body?: string) {
  const eventId = `$scroll-anchor-item-${String(index).padStart(4, "0")}:example.invalid`;
  return {
    id: { Event: { event_id: eventId } },
    sender: HARNESS_ACCOUNT_KEY,
    sender_label: "Harness User",
    body: body ?? `scroll anchor message ${index}\nThis row has stable text height.`,
    timestamp_ms: 1_800_000_000_000 + index * 1000,
    in_reply_to_event_id: null,
    thread_root: null,
    can_react: false,
    is_redacted: false,
    is_hidden: false,
    can_redact: false,
    is_edited: false,
    can_edit: false,
    reactions: [],
    send_state: null,
    thread_summary: null
  };
}

async function gotoReadyApp(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Reply to message" }).first()).toBeVisible();
}

async function seedTimeline(page: Page, itemCount: number): Promise<void> {
  await seedTimelineWithItems(
    page,
    Array.from({ length: itemCount }, (_, index) => makeEventItem(index))
  );
}

async function seedTimelineWithItems(
  page: Page,
  items: ReturnType<typeof makeEventItem>[]
): Promise<void> {
  // appHarnessMain emits its own seed item during boot. Wait for it before
  // replacing the timeline, otherwise the boot retry loop can overwrite this
  // test's InitialItems between Playwright assertions.
  await expect(page.locator(`[data-event-id="${HARNESS_SEED_EVENT_ID}"]`)).toBeVisible();

  await expect
    .poll(
      async () =>
        page.evaluate(
          async ({ key, nextItems }) => {
            const itemDomIds = nextItems.flatMap((item) => {
              const eventId = (item as { id: { Event?: { event_id: string } } }).id.Event?.event_id;
              return eventId ? [eventId] : [];
            });
            await window.__harness.pushCoreEvent({
              kind: "Timeline",
              event: {
                InitialItems: {
                  request_id: null,
                  key,
                  generation: 2,
                  // eslint-disable-next-line @typescript-eslint/no-explicit-any
                  items: nextItems as any[]
                }
              }
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
            } as any);
            await new Promise((resolve) => setTimeout(resolve, 25));
            return itemDomIds.some((id) =>
              document.querySelector(`[data-item-id="${CSS.escape(id)}"]`)
            );
          },
          { key: TIMELINE_KEY, nextItems: items }
        ),
      { timeout: 10_000, intervals: [25, 50, 100, 250] }
    )
    .toBe(true);
}

async function waitAnimationFrames(page: Page, count: number): Promise<void> {
  for (let index = 0; index < count; index += 1) {
    await page.evaluate(
      () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
    );
  }
}

async function placeAnchorInViewport(
  page: Page,
  targetIndex: number
): Promise<{ anchorEventId: string; originalOffset: number }> {
  const anchorEventId = `$scroll-anchor-item-${String(targetIndex).padStart(4, "0")}:example.invalid`;

  await page.evaluate(() => {
    const container = document.querySelector<HTMLElement>("[data-testid=timeline-view]");
    if (!container) return;
    container.scrollTop = 0;
    container.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 120 }));
    container.dispatchEvent(new Event("scroll", { bubbles: true }));
  });

  const anchor = page.locator(`[data-event-id="${anchorEventId}"]`);
  // Move through the virtualized range in bounded, frame-separated steps. A
  // single estimated-height jump can mount the overscanned row while its
  // measured range still places it far outside the viewport; stepping lets the
  // production range model settle before the anchor oracle begins.
  for (let step = 0; step < 100; step += 1) {
    const inViewport = await page.evaluate((eventId) => {
      const row = document.querySelector<HTMLElement>(
        `[data-event-id="${CSS.escape(eventId)}"]`
      );
      const container = document.querySelector<HTMLElement>("[data-testid=timeline-view]");
      if (!row || !container) return false;
      const rowRect = row.getBoundingClientRect();
      const containerRect = container.getBoundingClientRect();
      return rowRect.bottom > containerRect.top && rowRect.top < containerRect.bottom;
    }, anchorEventId);
    if (inViewport) break;
    await page.evaluate(() => {
      const container = document.querySelector<HTMLElement>("[data-testid=timeline-view]");
      if (!container) return;
      container.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 500 }));
      container.scrollTop += 500;
      container.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await waitAnimationFrames(page, 2);
  }

  await expect(anchor).toBeVisible({ timeout: 5000 });
  await expect(anchor).toBeInViewport();

  const originalOffset = await anchorOffsetFromContainer(page, anchorEventId);
  expect(originalOffset).not.toBeNull();
  return { anchorEventId, originalOffset: originalOffset! };
}

async function anchorOffsetFromContainer(page: Page, eventId: string): Promise<number | null> {
  return page.evaluate(
    ({ targetEventId }) => {
      const anchor = document.querySelector<HTMLElement>(`[data-event-id="${targetEventId}"]`);
      const container = anchor?.closest<HTMLElement>("[data-testid=timeline-view]");
      if (!anchor || !container) return null;
      return anchor.getBoundingClientRect().top - container.getBoundingClientRect().top;
    },
    { targetEventId: eventId }
  );
}

async function growRenderedRowAboveViewport(page: Page): Promise<void> {
  await page.evaluate(() => {
    const container = document.querySelector<HTMLElement>("[data-testid=timeline-view]");
    if (!container) return;
    const containerTop = container.getBoundingClientRect().top;
    const rows = Array.from(
      container.querySelectorAll<HTMLElement>("[data-event-id]")
    ).filter((row) => row.getBoundingClientRect().bottom < containerTop);
    const target = rows.at(-1);
    if (!target) {
      throw new Error("No rendered row above viewport to grow");
    }
    target.style.paddingBlockEnd = "180px";
  });
  await page.waitForTimeout(100);
}

async function expectAnchorStayedPut(
  page: Page,
  eventId: string,
  originalOffset: number
): Promise<void> {
  await expect
    .poll(
      async () => {
        const nextOffset = await anchorOffsetFromContainer(page, eventId);
        if (nextOffset === null) return Number.POSITIVE_INFINITY;
        return Math.abs(nextOffset - originalOffset);
      },
      { timeout: 3000, intervals: [30, 50, 100, 200] }
    )
    .toBeLessThanOrEqual(ANCHOR_PIXEL_TOLERANCE);
}

async function timelineScrollTop(page: Page): Promise<number> {
  return page.evaluate(() => {
    const container = document.querySelector<HTMLElement>("[data-testid=timeline-view]");
    return container?.scrollTop ?? 0;
  });
}

test("Koushi-owned anchoring keeps virtualized free-scroll viewport stable", async ({
  page
}) => {
  await gotoReadyApp(page);
  await seedTimeline(page, 720);

  const { anchorEventId, originalOffset } = await placeAnchorInViewport(page, 220);
  await growRenderedRowAboveViewport(page);
  await expectAnchorStayedPut(page, anchorEventId, originalOffset);
});

test("a 147-row room keeps mounting bounded while scrolling and measuring rows", async ({
  page
}) => {
  await gotoReadyApp(page);
  await seedTimeline(page, 147);

  const timeline = page.locator("[data-testid=timeline-view]");
  await expect(timeline).toHaveAttribute("data-virtualized", "true");
  const { anchorEventId, originalOffset } = await placeAnchorInViewport(page, 70);
  expect(await timeline.locator("[data-item-id]").count()).toBeLessThanOrEqual(80);
  await growRenderedRowAboveViewport(page);
  await expectAnchorStayedPut(page, anchorEventId, originalOffset);
});

test("virtualized date dividers have the same modeled and physical height", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await gotoReadyApp(page);
  const items = Array.from({ length: 115 }, (_, index) => makeEventItem(index));
  for (let index = 17; index >= 0; index -= 1) {
    const timestamp = 1_800_000_000_000 + index * 86_400_000;
    items.splice(index * 6, 0, {
      ...makeEventItem(1000 + index),
      id: { Synthetic: { synthetic_id: `date-divider-${timestamp}` } },
      timestamp_ms: timestamp
    } as unknown as ReturnType<typeof makeEventItem>);
  }
  await seedTimelineWithItems(page, items);
  const timeline = page.locator("[data-testid=timeline-view]");
  await expect(timeline).toHaveAttribute("data-total-items", "133");
  await timeline.evaluate((node) => {
    node.scrollTop = 0;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  await waitAnimationFrames(page, 4);
  const mismatch = await page.evaluate(() => {
    const list = document.querySelector<HTMLElement>(".timeline-item-list");
    if (!list) throw new Error("Missing timeline list");
    const children = Array.from(list.children) as HTMLElement[];
    const sum = children.reduce((height, child) => height + child.getBoundingClientRect().height, 0);
    return Math.abs(list.getBoundingClientRect().height - sum);
  });
  expect(mismatch).toBeLessThanOrEqual(1);
  const { anchorEventId, originalOffset } = await placeAnchorInViewport(page, 70);
  for (let reversal = 0; reversal < 20; reversal += 1) {
    await timeline.evaluate((node) => {
      node.scrollTop += 400;
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await waitAnimationFrames(page, 2);
    await timeline.evaluate((node) => {
      node.scrollTop -= 400;
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await waitAnimationFrames(page, 2);
  }
  await expectAnchorStayedPut(page, anchorEventId, originalOffset);
  await timeline.hover();
  for (let reversal = 0; reversal < 60; reversal += 1) {
    await page.mouse.wheel(0, reversal % 2 === 0 ? 480 : -380);
    await waitAnimationFrames(page, 2);
    await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
    expect(await timeline.locator("[data-frame-item-id]").count()).toBeLessThanOrEqual(80);
  }
  expect(pageErrors).toEqual([]);
});

test("crossing the virtualization threshold keeps the reading row mounted", async ({ page }) => {
  await gotoReadyApp(page);
  await seedTimelineWithItems(
    page,
    Array.from({ length: 79 }, (_, index) => makeEventItem(index + 52))
  );

  const timeline = page.locator("[data-testid=timeline-view]");
  await expect(timeline).not.toHaveAttribute("data-virtualized", "true");
  const anchorEventId = "$scroll-anchor-item-0070:example.invalid";
  await timeline.evaluate((node) => {
    node.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: -500 }));
    node.scrollTop = 18 * 72;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  const anchor = page.locator(`[data-event-id="${anchorEventId}"]`);
  await expect(anchor).toBeInViewport();
  const originalOffset = await anchorOffsetFromContainer(page, anchorEventId);
  expect(originalOffset).not.toBeNull();
  await page.evaluate((eventId) => {
    const row = document.querySelector(`[data-event-id="${eventId}"]`);
    const list = document.querySelector(".timeline-item-list");
    if (!row || !list) throw new Error("missing initial anchor");
    document.documentElement.dataset["anchorRemovedDuringThresholdCrossing"] = "false";
    let mountedRows = list.querySelectorAll(".timeline-item-frame").length;
    let peakMountedRows = mountedRows;
    new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.removedNodes) {
          if (node === row || (node instanceof Element && node.contains(row))) {
            document.documentElement.dataset["anchorRemovedDuringThresholdCrossing"] = "true";
          }
          if (node instanceof Element && node.classList.contains("timeline-item-frame")) mountedRows -= 1;
        }
        for (const node of record.addedNodes) {
          if (node instanceof Element && node.classList.contains("timeline-item-frame")) mountedRows += 1;
        }
        peakMountedRows = Math.max(peakMountedRows, mountedRows);
      }
      document.documentElement.dataset["peakMountedRowsDuringThresholdCrossing"] = String(peakMountedRows);
    }).observe(list, { childList: true, subtree: true });
  }, anchorEventId);

  await page.evaluate(({ key, items }) => {
    window.__harness.pushCoreEvent({
      kind: "Timeline",
      event: {
        ItemsUpdated: {
          key,
          generation: 2,
          batch_id: 1,
          diffs: items.map((item) => ({ PushFront: { item } }))
        }
      }
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any);
  }, {
    key: TIMELINE_KEY,
    items: Array.from({ length: 52 }, (_, index) => makeEventItem(index)).reverse()
  });

  await expect(timeline).toHaveAttribute("data-total-items", "131");
  await expect(timeline).toHaveAttribute("data-virtualized", "true");
  await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.dataset["anchorRemovedDuringThresholdCrossing"])).toBe("false");
  expect(Number(await page.evaluate(() => document.documentElement.dataset["peakMountedRowsDuringThresholdCrossing"]))).toBeLessThanOrEqual(100);
  expect(await timeline.locator("[data-frame-item-id]").count()).toBeLessThanOrEqual(80);
  await expectAnchorStayedPut(page, anchorEventId, originalOffset!);

  // Backfill again after virtualization is already active. The previous
  // window's indices now refer to older rows, so this transition must also
  // preserve the reading row and its bounded DOM window.
  await page.evaluate(({ key, items }) => {
    window.__harness.pushCoreEvent({
      kind: "Timeline",
      event: {
        ItemsUpdated: {
          key,
          generation: 2,
          batch_id: 2,
          diffs: items.map((item) => ({ PushFront: { item } }))
        }
      }
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any);
  }, {
    key: TIMELINE_KEY,
    items: Array.from({ length: 52 }, (_, index) => makeEventItem(index - 52)).reverse()
  });
  await expect(timeline).toHaveAttribute("data-total-items", "183");
  expect(await page.evaluate(() => document.documentElement.dataset["anchorRemovedDuringThresholdCrossing"])).toBe("false");
  expect(Number(await page.evaluate(() => document.documentElement.dataset["peakMountedRowsDuringThresholdCrossing"]))).toBeLessThanOrEqual(100);
  expect(await timeline.locator("[data-frame-item-id]").count()).toBeLessThanOrEqual(80);
  await expectAnchorStayedPut(page, anchorEventId, originalOffset!);

  await timeline.hover();
  for (let reversal = 0; reversal < 48; reversal += 1) {
    await page.mouse.wheel(0, reversal % 2 === 0 ? -520 : 420);
    await waitAnimationFrames(page, 2);
    await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
    expect(await timeline.locator("[data-frame-item-id]").count()).toBeLessThanOrEqual(80);
  }
});

test("repeated scroll reversals across variable-height rows keep the application mounted", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await gotoReadyApp(page);
  await seedTimelineWithItems(
    page,
    Array.from({ length: 180 }, (_, index) =>
      makeEventItem(
        index,
        index % 7 === 0
          ? Array.from({ length: 12 }, (_, line) => `message ${index} line ${line} ${"long text ".repeat(12)}`).join("\n")
          : index % 3 === 0
            ? `message ${index} ${"medium text ".repeat(10)}`
            : `message ${index}`
      )
    )
  );

  const timeline = page.locator("[data-testid=timeline-view]");
  await expect(timeline).toHaveAttribute("data-virtualized", "true");
  await timeline.hover();
  for (let reversal = 0; reversal < 60; reversal += 1) {
    await page.mouse.wheel(0, reversal % 2 === 0 ? -680 : 540);
    await waitAnimationFrames(page, 2);
    await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
  }
  expect(pageErrors).toEqual([]);
});

test("virtualized free-scroll viewport stays stable while row measurements settle", async ({
  page
}) => {
  await gotoReadyApp(page);
  await seedTimelineWithItems(
    page,
    Array.from({ length: 720 }, (_, index) =>
      makeEventItem(
        index,
        Array.from(
          { length: index % 7 === 0 ? 9 : 2 },
          (_, line) => `scroll anchor message ${index} line ${line}`
        ).join("\n")
      )
    )
  );

  const { anchorEventId, originalOffset } = await placeAnchorInViewport(page, 220);
  await waitAnimationFrames(page, 8);
  await expectAnchorStayedPut(page, anchorEventId, originalOffset);
});

test("virtualized free-scroll viewport accepts small user wheel movement", async ({
  page
}) => {
  await gotoReadyApp(page);
  await seedTimeline(page, 720);
  await placeAnchorInViewport(page, 220);
  await waitAnimationFrames(page, 8);

  const timeline = page.locator("[data-testid=timeline-view]");
  await timeline.hover();
  const beforeScrollTop = await timelineScrollTop(page);
  await page.mouse.wheel(0, 180);
  await waitAnimationFrames(page, 8);
  const afterScrollTop = await timelineScrollTop(page);

  expect(afterScrollTop).toBeGreaterThan(beforeScrollTop + 20);
});

test("virtualized free-scroll viewport accepts small user wheel movement toward older messages", async ({
  page
}) => {
  await gotoReadyApp(page);
  await seedTimeline(page, 720);
  await placeAnchorInViewport(page, 220);
  await waitAnimationFrames(page, 8);

  const timeline = page.locator("[data-testid=timeline-view]");
  await timeline.hover();
  const beforeScrollTop = await timelineScrollTop(page);
  await page.mouse.wheel(0, -180);
  await waitAnimationFrames(page, 8);
  const afterScrollTop = await timelineScrollTop(page);

  expect(afterScrollTop).toBeLessThan(beforeScrollTop - 20);
});
