import { expect, test, type Page } from "@playwright/test";
import { gotoReadyShell, invocationCount } from "./support/basicOperations";

/**
 * #1416 fixture: Rust previews an unedited address (`aliasLocalpart === null`)
 * as the create session's random default, never from the room name.
 */
async function mockAddressPreview(page: Page, defaultLocalpart: string) {
  await page.evaluate((fallback) => {
    window.__harness.setCommandResponse("preview_room_address", ({ aliasLocalpart }) => {
      const localpart = (aliasLocalpart ?? fallback).trim();
      const invalid = localpart.startsWith("#");
      return {
        localpart,
        full_alias: localpart && !invalid ? `#${localpart}:example.invalid` : null,
        error: invalid ? "invalid" : localpart ? null : "empty",
        server_name: "example.invalid"
      };
    });
  }, defaultLocalpart);
}

/** Publish the Rust advisory result for an address, as a snapshot. */
async function pushAvailability(page: Page, availability: unknown) {
  await page.evaluate((value) => {
    const next = structuredClone(window.__harness.currentSnapshot());
    next.state_generation = (next.state_generation ?? 0) + 1;
    next.state.ui.room_address_availability = value as typeof next.state.ui.room_address_availability;
    window.__harness.setSnapshot(next);
    window.__harness.pushStateUpdate();
  }, availability);
}

async function pushAvailable(page: Page, localpart: string) {
  await pushAvailability(page, {
    kind: "checked",
    request_id: 1,
    full_alias: `#${localpart}:example.invalid`,
    availability: "available",
    suggestion: null
  });
}

function createOptions(page: Page) {
  return page.evaluate(() =>
    window.__harness.invocationsOf("create_room").map((call) => call.args.options));
}

test("a public room starts with the random address, and Change keeps manual edits and collision drafts", async ({ page }) => {
  await gotoReadyShell(page);
  await mockAddressPreview(page, "room-3fa9c2");
  await page.evaluate(() => {
    window.__harness.setCommandResponse("create_room", () => { throw { kind: "aliasInUse" }; });
    window.__harness.clearInvocations();
  });
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  const name = page.getByRole("textbox", { name: "Room name" });
  await name.fill("Example Room");
  await page.locator('input[type="radio"][value="public"]').check();
  // The address is read-only until Change, and does not follow the name.
  await expect(page.locator("code", { hasText: "#room-3fa9c2:example.invalid" })).toBeVisible();
  await name.fill("Changed Name");
  await expect(page.locator("code", { hasText: "#room-3fa9c2:example.invalid" })).toBeVisible();
  const submit = page.getByRole("button", { name: "Submit create room" });
  // Create waits for the advisory check of the shown address.
  await expect(submit).toBeDisabled();
  await page.getByRole("button", { name: "Change room address" }).click();
  const address = page.getByRole("textbox", { name: "Room address" });
  await expect(address).toHaveValue("room-3fa9c2");
  await address.fill("manual");
  await page.locator('input[type="radio"][value="invite"]').check();
  await page.locator('input[type="radio"][value="public"]').check();
  await expect(address).toHaveValue("manual");
  await expect(page.getByRole("status").filter({ hasText: "Full address:" })).toHaveText("Full address: #manual:example.invalid");
  await pushAvailable(page, "manual");
  await expect(submit).toBeEnabled();
  await address.focus();
  await address.evaluate((input) => {
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", keyCode: 229, isComposing: true, bubbles: true }));
    input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
  expect(await invocationCount(page, "create_room")).toBe(0);
  await address.dispatchEvent("compositionend", { data: "manual" });
  await address.dispatchEvent("keyup", { key: "Enter", code: "Enter" });
  await submit.click();
  await expect(page.getByRole("alert").filter({ hasText: "already in use" })).toBeVisible();
  await expect(name).toHaveValue("Changed Name");
  await expect(address).toHaveValue("manual");
  await address.fill("available");
  await pushAvailable(page, "available");
  await page.evaluate(() => window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot()));
  await submit.click();
  await expect(name).toBeHidden();
  expect(await createOptions(page)).toEqual([
    expect.objectContaining({ name: "Changed Name", aliasLocalpart: "manual", visibility: "public" }),
    expect.objectContaining({ name: "Changed Name", aliasLocalpart: "available", visibility: "public" })
  ]);
});

test("private creation needs no alias even when a public address is invalid", async ({ page }) => {
  await gotoReadyShell(page);
  await mockAddressPreview(page, "#invalid");
  await page.evaluate(() => {
    window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot());
    window.__harness.clearInvocations();
  });
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  const name = page.getByRole("textbox", { name: "Room name" });
  await name.fill("Private discussion");
  await page.locator('input[type="radio"][value="public"]').check();
  // An invalid address opens the editor at once.
  await expect(page.getByRole("textbox", { name: "Room address" })).toHaveValue("#invalid");
  const submit = page.getByRole("button", { name: "Submit create room" });
  await expect(submit).toBeDisabled();
  await page.locator('input[type="radio"][value="invite"]').check();
  await expect(submit).toBeEnabled();
  await submit.click();
  await expect(name).toBeHidden();
  expect(await createOptions(page)).toEqual([expect.objectContaining({
    name: "Private discussion", visibility: "private", aliasLocalpart: null
  })]);
});

async function selectSpace(page: Page, spaceId: string, spaceName: string, joinRule: string | null) {
  await page.evaluate(({ spaceId, spaceName, joinRule }) => {
    const next = structuredClone(window.__harness.currentSnapshot());
    next.state_generation = (next.state_generation ?? 0) + 1;
    next.state.ui.navigation.active_space_id = spaceId;
    next.state.domain.spaces = [
      ...next.state.domain.spaces,
      {
        space_id: spaceId,
        raw_name: spaceName,
        display_name: spaceName,
        avatar: null,
        join_rule: joinRule,
        child_room_ids: []
      }
    ];
    next.sidebar.active_space_id = spaceId;
    next.sidebar.account_home.is_active = false;
    if (joinRule === "public") {
      next.sidebar.create_room_defaults = { visibility: "public", encrypted: true, invited_only: false };
    }
    next.sidebar.space_rail = [
      ...next.sidebar.space_rail.map((space) => ({ ...space, is_active: false })),
      {
        space_id: spaceId,
        display_name: spaceName,
        avatar: null,
        unread_count: 0,
        highlight_count: 0,
        is_active: true
      }
    ];
    window.__harness.setSnapshot(next);
    window.__harness.pushStateUpdate();
  }, { spaceId, spaceName, joinRule });
}

// #1006/#1416: from a Space, the default address starts with the Space name;
// a conflict names the attempted address and its server-wide scope at the
// address field without losing the draft or the Space.
test("a Space room conflict names the attempted address and keeps the draft in the Space", async ({ page }) => {
  await gotoReadyShell(page);
  await selectSpace(page, "!research:example.invalid", "research-group", null);
  await mockAddressPreview(page, "research-group-3fa9c2");
  await page.evaluate(() => {
    window.__harness.setCommandResponse("create_room", () => { throw { kind: "aliasInUse" }; });
    window.__harness.clearInvocations();
  });
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  const name = page.getByRole("textbox", { name: "Room name" });
  await name.fill("papers");
  await expect(page.getByRole("radio", { name: "Members of research-group" })).toBeVisible();
  await page.locator('input[type="radio"][value="public"]').check();
  await expect(page.locator("code", { hasText: "#research-group-3fa9c2:example.invalid" })).toBeVisible();
  await pushAvailable(page, "research-group-3fa9c2");

  await page.getByRole("button", { name: "Submit create room" }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "The address #research-group-3fa9c2:example.invalid is already in use. Addresses are shared across all Spaces on example.invalid. You can keep the room name ‘papers’; change only the room address, for example by adding a project name or number."
  );
  const address = page.getByRole("textbox", { name: "Room address" });
  await expect(address).toBeFocused();
  await expect(page.getByText(/must be unique on example\.invalid, across all Spaces/)).toBeVisible();
  await expect(name).toHaveValue("papers");
  expect(await createOptions(page)).toEqual([expect.objectContaining({
    name: "papers",
    aliasLocalpart: "research-group-3fa9c2",
    parentSpace: { spaceId: "!research:example.invalid" }
  })]);

  // Editing the address retires the conflict; the room name is still `papers`.
  await address.fill("research-group-papers");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await pushAvailable(page, "research-group-papers");
  await page.evaluate(() => window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot()));
  await page.getByRole("button", { name: "Submit create room" }).click();
  await expect(name).toBeHidden();
  expect(await createOptions(page)).toEqual([
    expect.objectContaining({ aliasLocalpart: "research-group-3fa9c2" }),
    expect.objectContaining({
      name: "papers",
      aliasLocalpart: "research-group-papers",
      parentSpace: { spaceId: "!research:example.invalid" }
    })
  ]);
});

// #1006: the advisory check is requested for the shown address after typing
// pauses, is rendered only from Rust snapshots for that address, and its
// alternative is used only on an explicit action.
test("advisory availability offers an unchecked alternative and follows the shown address", async ({ page }) => {
  await gotoReadyShell(page);
  await mockAddressPreview(page, "room-3fa9c2");
  await page.evaluate(() => window.__harness.clearInvocations());
  const checks = () => page.evaluate(() =>
    window.__harness.invocationsOf("check_room_address_availability").map((call) => call.args.aliasLocalpart));

  await page.getByRole("button", { name: "Create room", exact: true }).click();
  await page.locator('input[type="radio"][value="public"]').check();
  await expect.poll(checks).toEqual(["room-3fa9c2"]);
  await page.getByRole("button", { name: "Change room address" }).click();
  const address = page.getByRole("textbox", { name: "Room address" });
  await address.fill("papers");
  await expect.poll(checks).toEqual(["room-3fa9c2", "papers"]);

  await pushAvailability(page, { kind: "checking", request_id: 1, full_alias: "#papers:example.invalid" });
  await expect(page.getByText("Checking whether #papers:example.invalid is in use…")).toBeVisible();
  await pushAvailability(page, {
    kind: "checked", request_id: 1, full_alias: "#papers:example.invalid", availability: "inUse",
    suggestion: { localpart: "papers-2", full_alias: "#papers-2:example.invalid" }
  });
  await expect(page.getByText("#papers:example.invalid is already in use on example.invalid.", { exact: false })).toBeVisible();
  await expect(page.getByText("Suggestion (not checked yet): #papers-2:example.invalid")).toBeVisible();
  await expect(page.getByRole("button", { name: "Submit create room" })).toBeDisabled();
  // Nothing changes until the user asks for the suggestion.
  await expect(address).toHaveValue("papers");
  await page.getByRole("button", { name: "Use this address" }).click();
  await expect(address).toHaveValue("papers-2");
  await expect(page.getByRole("status").filter({ hasText: "Full address:" }))
    .toHaveText("Full address: #papers-2:example.invalid");
  // The old result no longer describes the shown address.
  await expect(page.getByText("Suggestion (not checked yet)", { exact: false })).toHaveCount(0);
  await expect.poll(checks).toEqual(["room-3fa9c2", "papers", "papers-2"]);

  // A late result for the earlier address is not shown for the new one.
  await pushAvailability(page, {
    kind: "checked", request_id: 1, full_alias: "#papers:example.invalid", availability: "available", suggestion: null
  });
  await expect(page.getByText(/was not in use when checked/)).toHaveCount(0);
  await pushAvailability(page, {
    kind: "checked", request_id: 2, full_alias: "#papers-2:example.invalid", availability: "available", suggestion: null
  });
  await expect(page.getByText(
    "#papers-2:example.invalid was not in use when checked. This does not reserve it; the server confirms the address when you create the room."
  )).toBeVisible();
  await expect(page.getByRole("button", { name: "Submit create room" })).toBeEnabled();

  await page.getByRole("button", { name: "Cancel create" }).click();
  await expect.poll(() => page.evaluate(() =>
    window.__harness.invocationsOf("clear_room_address_availability").length)).toBeGreaterThan(0);
});

// #1023/#1416: the dialog seeds its access choice from the Rust-projected
// `sidebar.create_room_defaults`; an unnamed public room uses its default
// address.
test("a public Space selects a public room and creates an unnamed room with the default address", async ({ page }) => {
  await gotoReadyShell(page);
  await selectPublicSpaceForCreate(page);
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  const name = page.getByRole("textbox", { name: "Room name" });
  await expect(name).toHaveValue("");
  await expect(page.locator('input[type="radio"][value="public"]')).toBeChecked();
  await expect(page.locator("code", { hasText: "#open-lab-3fa9c2:example.invalid" })).toBeVisible();
  await pushAvailable(page, "open-lab-3fa9c2");
  const submit = page.getByRole("button", { name: "Submit create room" });
  await expect(submit).toBeEnabled();
  await submit.click();
  await expect(name).toBeHidden();
  expect(await createOptions(page)).toEqual([expect.objectContaining({
    name: "",
    visibility: "public",
    aliasLocalpart: "open-lab-3fa9c2",
    encrypted: false,
    parentSpace: { spaceId: "!open:example.invalid" }
  })]);
});

async function selectPublicSpaceForCreate(page: Page) {
  await selectSpace(page, "!open:example.invalid", "open-lab", "public");
  await mockAddressPreview(page, "open-lab-3fa9c2");
  await page.evaluate(() => {
    window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot());
    window.__harness.clearInvocations();
  });
}

test("choosing invite in a public Space creates an encrypted private Space room", async ({ page }) => {
  await gotoReadyShell(page);
  await selectPublicSpaceForCreate(page);
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  await page.getByRole("textbox", { name: "Room name" }).fill("notes");
  await page.locator('input[type="radio"][value="invite"]').check();
  await expect(page.getByRole("checkbox", { name: "Encrypted room" })).toBeChecked();
  await page.getByRole("button", { name: "Submit create room" }).click();
  await expect(page.getByRole("textbox", { name: "Room name" })).toBeHidden();
  expect(await createOptions(page)).toEqual([expect.objectContaining({
    name: "notes",
    visibility: "private",
    aliasLocalpart: null,
    encrypted: true,
    parentSpace: { spaceId: "!open:example.invalid" }
  })]);
});

test("Home offers only invite or Anyone and an unnamed private room needs no name", async ({ page }) => {
  await gotoReadyShell(page);
  await page.evaluate(() => {
    window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot());
    window.__harness.clearInvocations();
  });
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  await expect(page.locator('input[type="radio"][value="invite"]')).toBeChecked();
  await expect(page.locator('input[type="radio"][value="restricted"]')).toHaveCount(0);
  await page.getByRole("button", { name: "Submit create room" }).click();
  await expect(page.getByRole("textbox", { name: "Room name" })).toBeHidden();
  expect(await createOptions(page))
    .toEqual([expect.objectContaining({ name: "", visibility: "private", encrypted: true, parentSpace: null })]);
});

test("a public room cannot be created with an emptied address", async ({ page }) => {
  await gotoReadyShell(page);
  await selectPublicSpaceForCreate(page);
  await page.evaluate(() => window.__harness.setCommandResponse("create_room", () => { throw { kind: "aliasInUse" }; }));
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  await page.getByRole("button", { name: "Change room address" }).click();
  const address = page.getByRole("textbox", { name: "Room address" });
  await address.fill("lobby");
  await pushAvailable(page, "lobby");
  const submit = page.getByRole("button", { name: "Submit create room" });
  await submit.click();
  await expect(page.getByRole("alert")).toHaveText(
    "The address #lobby:example.invalid is already in use. Addresses are shared across all Spaces on example.invalid. Change the room address, for example by adding a project name or number."
  );
  await expect(address).toBeFocused();
  await address.fill("");
  await expect(page.getByRole("status").filter({ hasText: "Enter an address" })).toBeVisible();
  await expect(submit).toBeDisabled();
  await address.fill("lobby-2");
  await pushAvailable(page, "lobby-2");
  await page.evaluate(() => window.__harness.setCommandResponse("create_room", () => window.__harness.currentSnapshot()));
  await submit.click();
  await expect(address).toBeHidden();
  expect(await createOptions(page)).toEqual([
    expect.objectContaining({ name: "", aliasLocalpart: "lobby", visibility: "public" }),
    expect.objectContaining({ name: "", aliasLocalpart: "lobby-2", visibility: "public" })
  ]);
});
