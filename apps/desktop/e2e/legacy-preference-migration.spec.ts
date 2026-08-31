import { expect, test } from "@playwright/test";

test("legacy Space presentation migrates through Rust-shaped navigation and clears confirmed keys", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("koushi.homeSelection.v1", JSON.stringify({ kind: "activity" }));
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
      page.evaluate(() => ({
        home: localStorage.getItem("koushi.homeSelection.v1"),
        spaces: localStorage.getItem("koushi.spaceLocalOverrides.v1")
      }))
    )
    .toEqual({ home: null, spaces: null });
});
