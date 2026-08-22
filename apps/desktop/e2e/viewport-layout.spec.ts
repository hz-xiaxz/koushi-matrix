import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";
import { gotoReadyShell } from "./support/basicOperations";

async function layoutGeometry(page: Page) {
  return page.evaluate(() => {
    const rect = (selector: string) => {
      const element = document.querySelector<HTMLElement>(selector);
      if (!element) return null;
      const box = element.getBoundingClientRect();
      return {
        top: box.top,
        left: box.left,
        right: box.right,
        bottom: box.bottom,
        width: box.width,
        height: box.height
      };
    };
    return {
      viewport: { width: window.innerWidth, height: window.innerHeight },
      document: {
        width: document.documentElement.clientWidth,
        height: document.documentElement.clientHeight
      },
      body: rect("body"),
      root: rect("#root"),
      desktop: rect(".desktop"),
      panelClose: rect(".thread-pane .thread-header button:last-child")
    };
  });
}

function expectRootAligned(geometry: Awaited<ReturnType<typeof layoutGeometry>>) {
  expect(geometry.body).not.toBeNull();
  expect(geometry.root).not.toBeNull();
  expect(geometry.desktop).not.toBeNull();
  for (const box of [geometry.body, geometry.root, geometry.desktop]) {
    expect(box!.top).toBeCloseTo(0, 0);
    expect(box!.left).toBeCloseTo(0, 0);
    expect(box!.width).toBeCloseTo(geometry.document.width, 0);
    expect(box!.height).toBeCloseTo(geometry.document.height, 0);
    expect(box!.right).toBeLessThanOrEqual(geometry.viewport.width + 1);
    expect(box!.bottom).toBeLessThanOrEqual(geometry.viewport.height + 1);
  }
}

test("density, browser resize, and right-panel resize preserve the root viewport", async ({
  page
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReadyShell(page);
  await page.getByRole("button", { name: t("workspace.userSettings"), exact: true }).click();
  await expect(page.locator(".thread-pane")).toBeVisible();

  for (const density of ["Compact", "Default", "Comfortable"] as const) {
    await page.getByRole("button", { name: density, exact: true }).click();
    await expect(page.locator(`.desktop[data-density="${density.toLowerCase()}"]`)).toBeVisible();
    expectRootAligned(await layoutGeometry(page));
  }

  expectRootAligned(await layoutGeometry(page));

  const resizer = page.getByRole("button", { name: t("workspace.resizeRightPanel") });
  const beforePanel = await page
    .locator(".thread-pane")
    .evaluate((element) => element.getBoundingClientRect().width);
  const resizerBox = await resizer.boundingBox();
  expect(resizerBox).not.toBeNull();
  await page.mouse.move(resizerBox!.x + resizerBox!.width / 2, resizerBox!.y + 4);
  await page.mouse.down();
  await page.mouse.move(resizerBox!.x - 80, resizerBox!.y + 4);
  await page.mouse.up();
  const afterPanel = await page
    .locator(".thread-pane")
    .evaluate((element) => element.getBoundingClientRect().width);
  expect(afterPanel).not.toBe(beforePanel);
  expectRootAligned(await layoutGeometry(page));

  const closePanel = page.getByRole("button", {
    name: t("action.close", { title: t("panel.userSettings") }),
    exact: true
  });
  await closePanel.click();
  await expect(page.locator(".app-grid")).toHaveClass(/(^|\s)thread-closed(\s|$)/);
  await expect(closePanel).toBeHidden();
  expectRootAligned(await layoutGeometry(page));

  await page.setViewportSize({ width: 1100, height: 720 });
  expectRootAligned(await layoutGeometry(page));
});
