/**
 * Headless spec: the right context panel must never be clipped off-screen (#452).
 *
 * `.app-grid`'s inline four-column layout has a hard minimum:
 *
 *   --shell-rail-width (72) + --sidebar-width (318)
 *     + timeline minmax(420px, 1fr) + --right-panel-width (390) = 1200px
 *
 * The floating-overlay fallback only engaged at `max-width: 1180px`, so between
 * 1181px and 1199px the inline grid stayed active while no longer fitting. With
 * `.desktop { overflow: hidden }` there is no scrollbar, so the panel's right
 * edge — including its close button — was cut off the window with no way back
 * except resizing.
 *
 * The reporter hit this through the Threads panel, but the fault is in the
 * shared `.app-grid` column and the shared `.thread-pane`, so any right-panel
 * content reproduces it. These tests open the Search panel because the harness
 * drives it from a real user action, and they measure rendered geometry, so they
 * fail on the symptom (an unreachable close control) rather than on a
 * breakpoint number.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

const HARNESS_ROOM_ID = "!harness-room:example.invalid";
/** Inside the reported dead band: past the overlay breakpoint, below the grid minimum. */
const DEAD_BAND_WIDTH = 1190;

async function gotoReadyShell(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
}

/** Open the right context panel through the Search route the harness supports. */
async function openRightPanel(page: Page): Promise<void> {
  await page.evaluate(
    ({ roomId }) => {
      window.__harness.setCommandResponse("submit_search", ({ query }: { query?: string }) => {
        const next = window.__harness.currentSnapshot();
        return {
          ...next,
          state: {
            ...next.state,
            domain: {
              ...next.state.domain,
              search: {
                kind: "results",
                request_id: 452,
                query: String(query ?? "layout"),
                scope: "currentRoom",
                results: [
                  {
                    room_id: roomId,
                    event_id: "$panel-clipping:example.invalid",
                    sender: "@harness-ada:example.invalid",
                    timestamp_ms: 1_800_000_004_000,
                    score_millis: 990,
                    snippet: "A result so the panel has content to clip.",
                    match_field: "messageBody",
                    highlights: [],
                    match_kind: "exact"
                  }
                ]
              }
            }
          }
        };
      });
    },
    { roomId: HARNESS_ROOM_ID }
  );

  await page.locator(".top-search input").fill("layout");
  await expect(page.locator(".thread-pane")).toBeVisible();
}

/** Rendered geometry of the right panel and its close control. */
async function dragResizer(page: Page, label: string, deltaX: number): Promise<void> {
  const resizer = page.getByRole("button", { name: label });
  const box = await resizer.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + 4);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2 + deltaX, box!.y + 4);
  await page.mouse.up();
}

async function panelGeometry(page: Page) {
  return page.evaluate(() => {
    const panel = document.querySelector<HTMLElement>(".thread-pane");
    if (!panel) {
      return null;
    }
    // Every control in the panel must be reachable — the reporter's escape
    // hatch was its close button, but any clipped control is the same fault.
    // Measured by role rather than class so the assertion does not depend on
    // which panel content happens to be open.
    const controls = Array.from(panel.querySelectorAll<HTMLElement>("button"));
    const panelRect = panel.getBoundingClientRect();
    return {
      viewportWidth: window.innerWidth,
      panelLeft: panelRect.left,
      panelRight: panelRect.right,
      controlCount: controls.length,
      widestControlRight: controls.reduce(
        (max, control) => Math.max(max, control.getBoundingClientRect().right),
        0
      )
    };
  });
}

test("the right context panel and its close control stay on-screen in the grid-minimum dead band", async ({
  page
}) => {
  await page.setViewportSize({ width: DEAD_BAND_WIDTH, height: 800 });
  await gotoReadyShell(page);
  await openRightPanel(page);

  const geometry = await panelGeometry(page);
  expect(geometry).not.toBeNull();
  const { viewportWidth, panelLeft, panelRight, controlCount, widestControlRight } = geometry!;

  expect(panelLeft).toBeGreaterThanOrEqual(0);
  expect(
    panelRight,
    `panel right edge ${panelRight} overflows the ${viewportWidth}px viewport`
  ).toBeLessThanOrEqual(viewportWidth + 1);
  expect(controlCount).toBeGreaterThan(0);
  expect(
    widestControlRight,
    `a panel control reaches ${widestControlRight}, outside the ${viewportWidth}px viewport`
  ).toBeLessThanOrEqual(viewportWidth + 1);
});

test("a widened right panel is fitted again when the window narrows", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 800 });
  await gotoReadyShell(page);
  await openRightPanel(page);

  const panel = page.locator(".thread-pane");
  const initialWidth = await panel.evaluate((element) => element.getBoundingClientRect().width);
  await dragResizer(page, t("workspace.resizeRightPanel"), -260);
  await expect
    .poll(() => panel.evaluate((element) => element.getBoundingClientRect().width))
    .toBeGreaterThan(initialWidth);
  const preferredWidth = await panel.evaluate((element) => element.getBoundingClientRect().width);

  await page.setViewportSize({ width: 1300, height: 800 });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
  await expect
    .poll(async () => {
      const geometry = await panelGeometry(page);
      return geometry ? geometry.panelRight - geometry.viewportWidth : Number.POSITIVE_INFINITY;
    })
    .toBeLessThanOrEqual(1);
  const geometry = await panelGeometry(page);
  expect(geometry).not.toBeNull();
  expect(geometry!.widestControlRight).toBeLessThanOrEqual(geometry!.viewportWidth + 1);

  await page.setViewportSize({ width: 1600, height: 800 });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
  await expect
    .poll(() => panel.evaluate((element) => element.getBoundingClientRect().width))
    .toBeCloseTo(preferredWidth, 0);
});

test("widening the sidebar cannot push the right panel off-screen", async ({ page }) => {
  await page.setViewportSize({ width: 1300, height: 800 });
  await gotoReadyShell(page);
  await openRightPanel(page);

  const sidebar = page.locator(".sidebar");
  const initialWidth = await sidebar.evaluate((element) => element.getBoundingClientRect().width);
  await dragResizer(page, t("workspace.resizeRoomList"), 120);
  await expect
    .poll(() => sidebar.evaluate((element) => element.getBoundingClientRect().width))
    .toBeGreaterThan(initialWidth);

  const geometry = await panelGeometry(page);
  expect(geometry).not.toBeNull();
  expect(geometry!.panelRight).toBeLessThanOrEqual(geometry!.viewportWidth + 1);
  expect(geometry!.widestControlRight).toBeLessThanOrEqual(geometry!.viewportWidth + 1);
});

test("the panel stays on-screen across the whole breakpoint boundary", async ({ page }) => {
  // The dead band was bounded by the overlay breakpoint below and the grid
  // minimum above, so walk both sides of it rather than trusting one width.
  await page.setViewportSize({ width: 1400, height: 800 });
  await gotoReadyShell(page);
  await openRightPanel(page);

  for (const width of [1400, 1200, 1199, 1190, 1181, 1180, 1100]) {
    await page.setViewportSize({ width, height: 800 });
    const geometry = await panelGeometry(page);
    expect(geometry).not.toBeNull();
    expect(
      geometry!.panelRight,
      `at ${width}px the panel right edge ${geometry!.panelRight} overflows the viewport`
    ).toBeLessThanOrEqual(width + 1);
    expect(
      geometry!.widestControlRight,
      `at ${width}px a panel control reaches ${geometry!.widestControlRight}`
    ).toBeLessThanOrEqual(width + 1);
  }
});
