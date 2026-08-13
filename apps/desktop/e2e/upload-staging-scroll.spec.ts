/**
 * Headless geometry regression: the Upload attachments staging panel must
 * stay inside the application viewport at short window heights, with the
 * dialog header and Send attachments action visible while the preview body
 * scrolls (#515).
 *
 * Before the fix the staging dialog had no viewport-bounded max-height and no
 * internal scroll region, so with a tall portrait image the caption field,
 * output controls, or Send attachments action were clipped off the window with
 * no way to reach them. The same component is embedded in the main composer
 * and the thread composer, so both surfaces are covered.
 *
 * These tests drive the real attach flow through the harness and measure
 * rendered geometry (dialog bounds, scroll region overflow, scrollTop
 * movement), so they fail on the symptom rather than on a hard-coded height.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

/** Short viewport representative of the reported packaged window. */
const SHORT_VIEWPORT = { width: 900, height: 520 };

/** A small portrait image (240x640) so the preview keeps a tall aspect. */
const PORTRAIT_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAPAAAAKACAIAAAAtimItAAAGsUlEQVR4nO3SUQkAIBTAwBfbOAYzjCUEYRxcgH1s1j6QMd8L4CFDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphibF0KQYmhRDk2JoUgxNiqFJMTQphiblArUiJK23gEvYAAAAAElFTkSuQmCC",
  "base64"
);

async function gotoReadyShell(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Reply to message" }).first()).toBeVisible();
}

function stagedPortraitImage() {
  return {
    name: "portrait.png",
    mimeType: "image/png",
    buffer: PORTRAIT_PNG
  };
}

/** Rendered geometry of the staging dialog, its scroll list, and its actions. */
async function stagingGeometry(page: Page) {
  return page.evaluate(() => {
    const dialog = document.querySelector<HTMLElement>(".upload-staging-dialog");
    if (!dialog) {
      return null;
    }
    const box = dialog.getBoundingClientRect();
    const list = dialog.querySelector<HTMLElement>(".upload-staging-list");
    const actions = Array.from(
      dialog.querySelectorAll<HTMLElement>(".upload-staging-actions button")
    );
    const actionRects = actions.map((action) => action.getBoundingClientRect());
    return {
      viewportHeight: window.innerHeight,
      top: box.top,
      bottom: box.bottom,
      list: list
        ? {
            height: list.clientHeight,
            scrollHeight: list.scrollHeight,
            scrollTop: list.scrollTop,
            overflow: getComputedStyle(list).overflowY
          }
        : null,
      actions: actionRects.map((rect) => ({ top: rect.top, bottom: rect.bottom })),
      pageScrollY: window.scrollY
    };
  });
}

function expectStagingBounded(
  geometry: NonNullable<Awaited<ReturnType<typeof stagingGeometry>>>,
  label: string
) {
  const { viewportHeight, top, bottom, actions } = geometry;
  expect(top, `${label}: dialog top ${top} escapes the viewport`).toBeGreaterThanOrEqual(0);
  expect(
    bottom,
    `${label}: dialog bottom ${bottom} exceeds viewport height ${viewportHeight}`
  ).toBeLessThanOrEqual(viewportHeight + 1);
  for (const [index, action] of actions.entries()) {
    expect(
      action.bottom,
      `${label}: action ${index} bottom ${action.bottom} exceeds viewport height ${viewportHeight}`
    ).toBeLessThanOrEqual(viewportHeight + 1);
  }
}

test("main composer staging panel stays bounded and scrolls at a short viewport", async ({
  page
}) => {
  await page.setViewportSize(SHORT_VIEWPORT);
  await gotoReadyShell(page);
  await page.evaluate(() => {
    window.__harness.setCommandResponse("download_media", () => window.__harness.currentSnapshot());
    window.__harness.clearInvocations();
  });

  await page.getByRole("button", { name: "Attach file", exact: true }).click();
  await page
    .locator('input[type="file"][aria-label="Attach file input"]')
    .setInputFiles(stagedPortraitImage());

  const dialog = page.getByRole("dialog", { name: t("upload.dialogTitle") });
  await expect(dialog).toBeVisible();

  const geometry = await stagingGeometry(page);
  expect(geometry).not.toBeNull();
  expectStagingBounded(geometry!, "main staging");

  // The list owns vertical overflow: content overflows it and it scrolls.
  expect(geometry!.list).not.toBeNull();
  const list = geometry!.list!;
  expect(list.overflow).toBe("auto");
  expect(list.scrollHeight).toBeGreaterThan(list.height);

  // Header and Send attachments stay visible while the list scrolls.
  await expect(dialog.getByRole("heading", { name: t("upload.dialogTitle") })).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: t("upload.sendAttachments") })
  ).toBeVisible();

  // Wheel-scrolling inside the list moves only the list, never the page.
  const listLocator = page.locator(".upload-staging-list");
  await listLocator.hover();
  for (let index = 0; index < 4; index += 1) {
    await page.mouse.wheel(0, 120);
  }
  await expect
    .poll(async () => (await stagingGeometry(page))!.list!.scrollTop)
    .toBeGreaterThan(0);
  const after = await stagingGeometry(page);
  expect(after!.pageScrollY).toBe(0);
});

test("thread composer staging panel stays bounded and scrolls at a short viewport", async ({
  page
}) => {
  await page.setViewportSize(SHORT_VIEWPORT);
  await gotoReadyShell(page);
  await page.evaluate(() => {
    window.__harness.setCommandResponse("download_media", () => window.__harness.currentSnapshot());
    window.__harness.clearInvocations();
  });

  // Open the thread panel through the real user path.
  await page.getByRole("button", { name: /2 replies/ }).click();
  const contextPanel = page.locator('aside[aria-label="Context panel"]');
  const threadComposer = page.getByRole("textbox", { name: t("timeline.threadComposer") });
  await expect(threadComposer).toBeVisible();

  await contextPanel.getByRole("button", { name: "Attach file", exact: true }).click();
  await contextPanel
    .locator('input[type="file"][aria-label="Attach file input"]')
    .setInputFiles(stagedPortraitImage());

  const dialog = page.getByRole("dialog", { name: t("upload.dialogTitle") });
  await expect(dialog).toBeVisible();

  const geometry = await stagingGeometry(page);
  expect(geometry).not.toBeNull();
  expectStagingBounded(geometry!, "thread staging");

  expect(geometry!.list).not.toBeNull();
  const list = geometry!.list!;
  expect(list.overflow).toBe("auto");
  expect(list.scrollHeight).toBeGreaterThan(list.height);

  await expect(dialog.getByRole("heading", { name: t("upload.dialogTitle") })).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: t("upload.sendAttachments") })
  ).toBeVisible();

  const listLocator = contextPanel.locator(".upload-staging-list");
  await listLocator.hover();
  for (let index = 0; index < 4; index += 1) {
    await page.mouse.wheel(0, 120);
  }
  await expect
    .poll(async () => (await stagingGeometry(page))!.list!.scrollTop)
    .toBeGreaterThan(0);
  const after = await stagingGeometry(page);
  expect(after!.pageScrollY).toBe(0);
});
