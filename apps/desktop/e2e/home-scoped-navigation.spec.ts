import { expect, test } from "@playwright/test";
import { t } from "../src/i18n/messages";

/**
 * #330: account-global actions belong to Home. A sidebar entry's location has to
 * explain its scope, so Explore and Invites are Home-only and a space sidebar is
 * the room list for that space.
 */

function rail(page: import("@playwright/test").Page) {
  return page.getByRole("navigation", { name: t("workspace.workspaces") });
}

async function selectHome(page: import("@playwright/test").Page) {
  await rail(page).getByRole("button", { name: "Home" }).click();
}

async function selectSpace(page: import("@playwright/test").Page) {
  await rail(page).getByRole("button", { name: "Synthetic Lab" }).click();
}

test("Home owns Explore and Invites; a selected space shows neither", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("complementary", { name: t("workspace.rooms") })).toBeVisible();

  await selectHome(page);
  await expect(page.getByRole("button", { name: t("workspace.explore"), exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: t("workspace.invites"), exact: true })).toBeVisible();

  await selectSpace(page);
  await expect(page.getByRole("button", { name: t("workspace.explore"), exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: t("workspace.invites"), exact: true })).toHaveCount(0);
});

test("a selected space shows no Threads nav entry", async ({ page }) => {
  // The sidebar entry opened the *selected room's* threads, which reads as
  // space-scoped from where it sat. The room header owns that entry point now.
  await page.goto("/");
  await expect(page.getByRole("complementary", { name: t("workspace.rooms") })).toBeVisible();

  await selectSpace(page);

  const sidebar = page.getByRole("complementary", { name: t("workspace.rooms") });
  await expect(sidebar.getByRole("button", { name: t("workspace.threads") })).toHaveCount(0);
});

test("the room header exposes Threads with no unread thread activity", async ({ page }) => {
  // Previously the header button appeared only when a thread had unread
  // attention, so a quiet room's threads were unreachable once the sidebar
  // entry went away.
  await page.goto("/");
  const header = page.getByRole("main", { name: t("timeline.conversation") });
  await expect(header.getByRole("button", { name: t("workspace.threads") })).toBeVisible();
});

test("the Home rail badge totals unread messages and invites separately in its label", async ({
  page
}) => {
  await page.goto("/");
  await expect(page.getByRole("complementary", { name: t("workspace.rooms") })).toBeVisible();

  const homeButton = rail(page).getByRole("button", { name: "Home" });
  const label = await homeButton.getAttribute("aria-label");
  const badge = await homeButton.getAttribute("data-count");

  expect(label).not.toBeNull();
  const counts = label!.match(/(\d+)\D+(\d+)/);
  expect(
    counts,
    `Home label should name unread messages and invites separately, got ${label}`
  ).not.toBeNull();

  const unread = Number(counts![1]);
  const invites = Number(counts![2]);
  expect(invites).toBeGreaterThan(0);
  expect(
    Number(badge),
    "the badge shows the Rust-owned total of unread messages plus invites"
  ).toBe(unread + invites);
});
