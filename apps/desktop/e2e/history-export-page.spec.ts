/**
 * Headless spec: an exported history page opened straight from disk.
 *
 * The fixture under e2e/fixtures/history-export-page is rendered by Rust
 * (`archive_tests::browser_fixture_matches_renderer` keeps it current). This
 * spec adds the vendored KaTeX assets exactly as the exporter writes them and
 * opens the room page over `file://`, proving that math renders and the
 * thumbnail loads under the page's Content Security Policy.
 */

import { cpSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { expect, test } from "@playwright/test";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.resolve(HERE, "fixtures/history-export-page");
const CORE_ASSETS = path.resolve(HERE, "../../../crates/koushi-core/assets");

let exportDir = "";

test.beforeAll(() => {
  exportDir = mkdtempSync(path.join(tmpdir(), "koushi-history-export-"));
  cpSync(FIXTURE, exportDir, { recursive: true });
  cpSync(path.join(CORE_ASSETS, "katex"), path.join(exportDir, "assets/katex"), { recursive: true });
  cpSync(path.join(CORE_ASSETS, "koushi-math.js"), path.join(exportDir, "assets/koushi-math.js"));
});

test.afterAll(() => {
  if (exportDir) rmSync(exportDir, { recursive: true, force: true });
});

function roomPage(): string {
  const [folder] = readdirSync(path.join(exportDir, "rooms"));
  return pathToFileURL(path.join(exportDir, "rooms", folder, "index.html")).href;
}

test("an exported room page renders math and its thumbnail offline under its CSP", async ({ page }) => {
  const problems: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || /Content Security Policy/i.test(message.text())) problems.push(message.text());
  });
  page.on("pageerror", (error) => problems.push(error.message));
  const external: string[] = [];
  page.on("request", (request) => {
    if (!request.url().startsWith("file:")) external.push(request.url());
  });

  await page.goto(roomPage());
  await expect(page.locator("span[data-mx-maths] .katex")).toHaveCount(1);
  await expect(page.locator("div[data-mx-maths] .katex-display")).toHaveCount(1);
  const thumbnail = page.locator(".attachment img");
  await expect(thumbnail).toHaveCount(1);
  await expect.poll(() => thumbnail.evaluate((image: HTMLImageElement) => image.naturalWidth)).toBeGreaterThan(0);
  expect(problems).toEqual([]);
  expect(external).toEqual([]);
});

test("the table of contents links the exported room", async ({ page }) => {
  await page.goto(pathToFileURL(path.join(exportDir, "index.html")).href);
  const link = page.getByRole("link").first();
  await link.click();
  await expect(page.locator("article")).toHaveCount(2);
});
