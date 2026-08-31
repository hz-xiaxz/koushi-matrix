import { expect, test } from "@playwright/test";

test("legacy Space presentation migrates through Rust-shaped navigation and clears confirmed keys", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("koushi.homeSelection.v1", JSON.stringify({ kind: "activity" }));
    localStorage.setItem("koushi.displayDensity.v1", "compact");
    localStorage.setItem("koushi.sidebarRoomCategory.v1", "dms");
    localStorage.setItem("koushi.sidebarRoomSort.v1", "name");
    localStorage.setItem(
      "koushi.roomSectionCollapsed.v1",
      JSON.stringify({ favourites: true, "low-priority": true, "not-joined": false })
    );
    localStorage.setItem("koushi-recent-emojis", JSON.stringify(["😀"]));
    localStorage.setItem(
      "koushi.spaceLocalOverrides.v1",
      JSON.stringify({
        "!harness-space:example.invalid": { name: "Migrated Space", icon: "M" }
      })
    );
  });

  await page.goto("/appHarness.html");

  const migrated = page
    .getByRole("navigation", { name: "Workspaces" })
    .getByRole("button", { name: "Migrated Space" });
  await expect(migrated).toBeVisible();
  await expect(migrated).toContainText("M");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const values = window.__harness.currentSnapshot().state.domain.settings.values;
        return {
          density: values.appearance.density,
          category: values.sidebar.category,
          sort: values.room_list_sort.kind,
          collapsed: values.sidebar.collapsed,
          recent: values.composer.recent_emojis
        };
      })
    )
    .toEqual({
      density: "compact",
      category: "people",
      sort: "normalLocale",
      collapsed: { favourites: true, low_priority: true, not_joined: false },
      recent: ["😀"]
    });
  await expect
    .poll(() =>
      page.evaluate(() => [
        "koushi.homeSelection.v1",
        "koushi.spaceLocalOverrides.v1",
        "koushi.displayDensity.v1",
        "koushi.sidebarRoomCategory.v1",
        "koushi.sidebarRoomSort.v1",
        "koushi.roomSectionCollapsed.v1",
        "koushi-recent-emojis"
      ].map((key) => localStorage.getItem(key)))
    )
    .toEqual([null, null, null, null, null, null, null]);
});
