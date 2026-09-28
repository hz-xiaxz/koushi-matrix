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

test("a failed emoji vocabulary chunk gates only the legacy emoji list (#1035)", async ({ page }) => {
  const emojiDataRoute = /\/src\/components\/emojiData\.ts(\?.*)?$/;
  await page.route(emojiDataRoute, (route) => route.abort());
  await page.addInitScript(() => {
    // Seed the legacy profile once; a reload models a later launch.
    if (sessionStorage.getItem("legacy-seeded") === "1") return;
    sessionStorage.setItem("legacy-seeded", "1");
    localStorage.setItem("koushi.homeSelection.v1", JSON.stringify({ kind: "activity" }));
    localStorage.setItem("koushi.displayDensity.v1", "compact");
    localStorage.setItem("koushi-recent-emojis", JSON.stringify(["😀"]));
    localStorage.setItem(
      "koushi.spaceLocalOverrides.v1",
      JSON.stringify({
        "!harness-space:example.invalid": { name: "Migrated Space", icon: "M" }
      })
    );
  });

  await page.goto("/appHarness.html");

  await expect(
    page
      .getByRole("navigation", { name: "Workspaces" })
      .getByRole("button", { name: "Migrated Space" })
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() => {
        const values = window.__harness.currentSnapshot().state.domain.settings.values;
        return {
          density: values.appearance.density,
          imported: values.legacy_frontend_preferences_imported
        };
      })
    )
    .toEqual({ density: "compact", imported: true });
  await expect
    .poll(() =>
      page.evaluate(() =>
        ["koushi.homeSelection.v1", "koushi.spaceLocalOverrides.v1", "koushi.displayDensity.v1"].map(
          (key) => localStorage.getItem(key)
        )
      )
    )
    .toEqual([null, null, null]);
  // The unvalidated list is neither imported nor discarded.
  expect(
    await page.evaluate(
      () => window.__harness.currentSnapshot().state.domain.settings.values.composer.recent_emojis
    )
  ).toEqual([]);
  expect(await page.evaluate(() => localStorage.getItem("koushi-recent-emojis"))).toBe(
    JSON.stringify(["😀"])
  );

  // A later launch: Rust already recorded the one-shot import, and the
  // emoji chunk now loads, so the kept list is imported through an ordinary
  // settings update and only then removed from browser storage.
  await page.unroute(emojiDataRoute);
  await page.goto("/appHarness.html?legacySettingsImported=1");
  await expect
    .poll(() =>
      page.evaluate(
        () => window.__harness.currentSnapshot().state.domain.settings.values.composer.recent_emojis
      )
    )
    .toEqual(["😀"]);
  expect(
    await page.evaluate(() => window.__harness.invocationsOf("import_legacy_settings").length)
  ).toBe(0);
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("koushi-recent-emojis")))
    .toBeNull();
});
