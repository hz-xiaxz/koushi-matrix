/**
 * Headless spec: history export (room and Space archive folders).
 *
 * The real React app runs against the Tauri IPC mock. React renders the
 * Rust-owned `ui.history_export` slice and dispatches typed commands; the
 * specs push the Rust progress and settlement snapshots themselves. The
 * folder dialog, civil-date resolution, and every file belong to the Tauri
 * adapter and Core, so here they are only the command arguments.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";
import type { DesktopSnapshot, HistoryExportRoom, HistoryExportState } from "../src/domain/types";

const HARNESS_ROOM_ID = "!harness-room:example.invalid";
const HARNESS_SPACE_ID = "!harness-space:example.invalid";
const REQUEST_ID = 9_300;

type Harness = {
  currentSnapshot(): DesktopSnapshot;
  setSnapshot(snapshot: DesktopSnapshot): void;
  pushStateUpdate(): void;
  clearInvocations(): void;
  setCommandResponse(command: string, response: unknown): void;
  invocationsOf(command: string): Array<{ args: Record<string, unknown> }>;
};

async function gotoReadyShell(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
}

async function pushExportState(page: Page, next: HistoryExportState): Promise<void> {
  await page.evaluate((state) => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    const current = harness.currentSnapshot();
    harness.setSnapshot({
      ...current,
      state: { ...current.state, ui: { ...current.state.ui, history_export: state } }
    });
    harness.pushStateUpdate();
  }, next);
}

async function invocationArgs(page: Page, command: string): Promise<Array<Record<string, unknown>>> {
  return page.evaluate(
    (name) => (window as unknown as { __harness: Harness }).__harness.invocationsOf(name).map((call) => call.args),
    command
  );
}

function contextPanel(page: Page) {
  return page.getByRole("complementary", { name: t("panel.context") });
}

async function openRoomExportDialog(page: Page) {
  await page.getByRole("button", { name: t("room.roomInfo") }).click();
  const panel = contextPanel(page);
  await panel
    .getByRole("region", { name: t("historyExport.section") })
    .getByRole("button", { name: t("historyExport.open"), exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: t("historyExport.title") });
  await expect(dialog).toBeVisible();
  return { panel, dialog };
}

async function openSpaceExportDialog(page: Page) {
  await page
    .getByRole("navigation", { name: t("workspace.workspaces") })
    .getByRole("button", { name: "Harness Space", exact: true })
    .click();
  await page.getByRole("button", { name: t("workspace.spaceInfoSettings") }).click();
  const panel = contextPanel(page);
  await expect(panel.getByText(t("panel.spaceInfo"), { exact: true })).toBeVisible();
  await panel
    .getByRole("region", { name: t("historyExport.spaceSection") })
    .getByRole("button", { name: t("historyExport.open"), exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: t("historyExport.spaceTitle") });
  await expect(dialog).toBeVisible();
  return { panel, dialog };
}

function counts(partial: Partial<HistoryExportRoom["counts"]> = {}): HistoryExportRoom["counts"] {
  return {
    fetched_events: 0,
    exported_events: 0,
    undecryptable_events: 0,
    attachments_total: 0,
    attachments_done: 0,
    attachments_failed: 0,
    ...partial
  };
}

function room(id: string, name: string, phase: HistoryExportRoom["phase"], partial: Partial<HistoryExportRoom> = {}): HistoryExportRoom {
  return {
    room_id: id,
    display_name: name,
    phase,
    counts: counts(),
    skip_reason: phase === "skipped" ? "notJoined" : null,
    failure_kind: null,
    ...partial
  };
}

const spaceScope = { kind: "space" as const, space_id: HARNESS_SPACE_ID };

test("a room export sends the displayed period, zone, labels, and room name", async ({ page }) => {
  await gotoReadyShell(page);
  await page.evaluate(() => (window as unknown as { __harness: Harness }).__harness.clearInvocations());
  const { panel, dialog } = await openRoomExportDialog(page);

  await expect(dialog.getByTestId("history-export-size-warning")).toHaveText(t("historyExport.attachments"));
  await dialog.getByRole("radio", { name: t("historyExport.rangePeriod") }).check();
  await expect(dialog.getByTestId("history-export-time-zone")).toHaveText(
    t("historyExport.timeZone", { timeZone: "Asia/Tokyo" })
  );
  await dialog.getByLabel(t("historyExport.startDate")).fill("2026-09-01");
  await dialog.getByLabel(t("historyExport.endDate")).fill("2026-09-30");
  await dialog.getByRole("button", { name: t("historyExport.save") }).click();

  await expect.poll(async () => (await invocationArgs(page, "export_history")).length).toBe(1);
  const [args] = await invocationArgs(page, "export_history");
  expect(args.scope).toEqual({ kind: "room", roomId: HARNESS_ROOM_ID });
  expect(args.range).toEqual({ kind: "period", startDate: "2026-09-01", endDate: "2026-09-30", timeZone: "Asia/Tokyo" });
  expect(args.dialogTitle).toBe(t("historyExport.folderDialogTitle"));
  expect(typeof args.folderNameStem).toBe("string");
  expect((args.labels as Record<string, string>).inReplyTo).toBe(t("historyExport.page.inReplyTo"));

  const state = dialog.getByTestId("history-export-state");
  await expect(state).toHaveText(t("historyExport.preparing"));
  await pushExportState(page, {
    kind: "running",
    request_id: REQUEST_ID,
    scope: { kind: "room", room_id: HARNESS_ROOM_ID },
    range: { kind: "allAvailable" },
    rooms: [room(HARNESS_ROOM_ID, "Harness Room", "attachments", { counts: counts({ attachments_total: 8, attachments_done: 3 }) })],
    stop_requested: false
  });
  await expect(state).toHaveText(t("historyExport.roomsProgress", { done: 0, total: 1 }));
  await expect(dialog.getByTestId("history-export-rooms")).toContainText(
    t("historyExport.phaseAttachments", { done: 3, total: 8 })
  );
  await expect(panel.getByTestId("history-export-summary")).toHaveText(
    t("historyExport.summaryRunning", { done: 0, total: 1 })
  );
  await pushExportState(page, {
    kind: "completed",
    request_id: REQUEST_ID,
    scope: { kind: "room", room_id: HARNESS_ROOM_ID },
    range: { kind: "allAvailable" },
    rooms: [room(HARNESS_ROOM_ID, "Harness Room", "completed", { counts: counts({ exported_events: 20, attachments_total: 8, attachments_done: 8 }) })]
  });
  await expect(state).toHaveAttribute("data-export-result", "completed");
  await expect(state).toHaveText(t("historyExport.completed"));
  await dialog.getByRole("button", { name: t("action.done") }).click();
  await expect(dialog).toBeHidden();
  await expect(panel.getByTestId("history-export-summary")).toHaveText(t("historyExport.completed"));
});

test("a Space export shows every room's progress, stops, and continues the same folder", async ({ page }) => {
  await gotoReadyShell(page);
  const { dialog } = await openSpaceExportDialog(page);
  await expect(dialog.getByTestId("history-export-plaintext-warning")).toBeVisible();
  await dialog.getByRole("button", { name: t("historyExport.save") }).click();
  await expect.poll(async () => (await invocationArgs(page, "export_history"))[0]?.scope).toEqual({
    kind: "space",
    spaceId: HARNESS_SPACE_ID
  });
  const state = dialog.getByTestId("history-export-state");
  await expect(state).toHaveText(t("historyExport.preparing"));

  const rooms = [
    room("!a:example.invalid", "General", "completed", { counts: counts({ exported_events: 40, attachments_total: 2, attachments_done: 2 }) }),
    room("!b:example.invalid", "Papers", "fetching", { counts: counts({ fetched_events: 500 }) }),
    room("!c:example.invalid", "Closed", "skipped")
  ];
  await pushExportState(page, { kind: "running", request_id: REQUEST_ID, scope: spaceScope, range: { kind: "allAvailable" }, rooms, stop_requested: false });
  await expect(state).toHaveText(t("historyExport.roomsProgress", { done: 2, total: 3 }));
  const list = dialog.getByTestId("history-export-rooms");
  await expect(list.locator("li")).toHaveCount(3);
  await expect(list.locator("li").nth(1)).toContainText(t("historyExport.phaseFetching", { fetched: 500 }));
  await expect(list.locator("li").nth(2)).toContainText(t("historyExport.phaseSkipped"));

  await dialog.getByRole("button", { name: t("historyExport.stop") }).click();
  await expect.poll(() => invocationArgs(page, "stop_history_export")).toEqual([{ targetRequestId: REQUEST_ID }]);
  await expect(state).toHaveText(t("historyExport.stopping"));
  await expect(dialog.getByRole("button", { name: t("historyExport.stop") })).toBeDisabled();

  await pushExportState(page, {
    kind: "stopped",
    request_id: REQUEST_ID,
    scope: spaceScope,
    range: { kind: "allAvailable" },
    rooms: [rooms[0], room("!b:example.invalid", "Papers", "pending"), rooms[2]]
  });
  await expect(state).toHaveAttribute("data-export-result", "stopped");
  await expect(state).toHaveText(t("historyExport.stopped"));
  await dialog.getByRole("button", { name: t("historyExport.resume") }).click();
  await expect.poll(() => invocationArgs(page, "retry_history_export")).toEqual([{ targetRequestId: REQUEST_ID }]);
  await expect(state).toHaveText(t("historyExport.preparing"));
});

test("a failed export shows its Rust failure kind and offers a retry", async ({ page }) => {
  await gotoReadyShell(page);
  const { dialog } = await openSpaceExportDialog(page);
  await dialog.getByRole("button", { name: t("historyExport.save") }).click();
  const state = dialog.getByTestId("history-export-state");
  await expect(state).toHaveText(t("historyExport.preparing"));
  await pushExportState(page, {
    kind: "failed",
    request_id: REQUEST_ID,
    scope: spaceScope,
    range: { kind: "allAvailable" },
    rooms: [room("!a:example.invalid", "General", "completed")],
    failure_kind: "noSpace"
  });
  await expect(state).toHaveAttribute("data-export-result", "failed");
  await expect(state).toContainText(t("historyExport.failed"));
  await expect(state).toContainText(t("historyExport.failedNoSpace"));
  await dialog.getByRole("button", { name: t("historyExport.retryFailed") }).click();
  await expect.poll(async () => (await invocationArgs(page, "retry_history_export")).length).toBe(1);
});

test("a dismissed folder dialog submits nothing and keeps the form", async ({ page }) => {
  await gotoReadyShell(page);
  await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.setCommandResponse("export_history", { kind: "dismissed" })
  );
  const { dialog } = await openRoomExportDialog(page);
  await dialog.getByRole("button", { name: t("historyExport.save") }).click();
  await expect.poll(async () => (await invocationArgs(page, "export_history")).length).toBe(1);
  await expect(dialog.getByRole("form", { name: t("historyExport.title") })).toBeVisible();
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  await expect(dialog.getByTestId("history-export-state")).toHaveCount(0);
});
