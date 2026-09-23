/**
 * Headless spec: room-history export (#59 Phase B).
 *
 * The real React app runs against the Tauri IPC mock. React renders the
 * Rust-owned `ui.room_history_export` slice and dispatches typed commands; the
 * specs push the Rust progress and settlement snapshots themselves. The save
 * dialog, civil-date resolution, and file writing belong to the Tauri adapter
 * and Core, so here they are only the command arguments.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";
import type { DesktopSnapshot, RoomHistoryExportState } from "../src/domain/types";

const HARNESS_ROOM_ID = "!harness-room:example.invalid";
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

async function pushExportState(page: Page, next: RoomHistoryExportState): Promise<void> {
  await page.evaluate((state) => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    const current = harness.currentSnapshot();
    harness.setSnapshot({
      ...current,
      state: { ...current.state, ui: { ...current.state.ui, room_history_export: state } }
    });
    harness.pushStateUpdate();
  }, next);
}

async function setHarnessRoomEncrypted(page: Page): Promise<void> {
  await page.evaluate((roomId) => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    const current = harness.currentSnapshot();
    harness.setSnapshot({
      ...current,
      state: {
        ...current.state,
        domain: {
          ...current.state.domain,
          rooms: current.state.domain.rooms.map((room) =>
            room.room_id === roomId ? { ...room, is_encrypted: true } : room
          )
        }
      }
    });
    harness.pushStateUpdate();
  }, HARNESS_ROOM_ID);
}

async function invocationArgs(page: Page, command: string): Promise<Array<Record<string, unknown>>> {
  return page.evaluate(
    (name) => (window as unknown as { __harness: Harness }).__harness.invocationsOf(name).map((call) => call.args),
    command
  );
}

async function openExportDialog(page: Page) {
  await page.getByRole("button", { name: t("room.roomInfo") }).click();
  const panel = page.getByRole("complementary", { name: t("panel.context") });
  await panel.getByRole("button", { name: t("roomHistoryExport.open"), exact: true }).click();
  const dialog = page.getByRole("dialog", { name: t("roomHistoryExport.title") });
  await expect(dialog).toBeVisible();
  return { panel, dialog };
}

const progress = (fetched: number, exported: number, undecryptable = 0) => ({
  fetched_events: fetched,
  exported_events: exported,
  undecryptable_events: undecryptable
});

test("a period export sends the displayed civil dates and zone, then renders Rust progress and the result", async ({
  page
}) => {
  await gotoReadyShell(page);
  await setHarnessRoomEncrypted(page);
  await page.evaluate(() => (window as unknown as { __harness: Harness }).__harness.clearInvocations());
  const { panel, dialog } = await openExportDialog(page);

  await expect(dialog.getByRole("note")).toHaveText(t("roomHistoryExport.plaintextWarning"));
  await dialog.getByRole("radio", { name: t("roomHistoryExport.rangePeriod") }).check();
  await expect(dialog.getByTestId("room-history-export-time-zone")).toHaveText(
    t("roomHistoryExport.timeZone", { timeZone: "Asia/Tokyo" })
  );
  await dialog.getByLabel(t("roomHistoryExport.startDate")).fill("2026-09-01");
  await dialog.getByLabel(t("roomHistoryExport.endDate")).fill("2026-09-30");
  await dialog.getByRole("button", { name: t("roomHistoryExport.save") }).click();

  await expect.poll(async () => (await invocationArgs(page, "export_room_history")).length).toBe(1);
  const [args] = await invocationArgs(page, "export_room_history");
  expect(args).toEqual({
    roomId: HARNESS_ROOM_ID,
    range: { kind: "period", startDate: "2026-09-01", endDate: "2026-09-30", timeZone: "Asia/Tokyo" },
    dialogTitle: t("roomHistoryExport.saveDialogTitle"),
    fileNameStem: expect.stringMatching(/ - Chat Export$/)
  });

  const state = dialog.getByTestId("room-history-export-state");
  await expect(state).toHaveText(t("roomHistoryExport.exporting", { fetched: 0, exported: 0 }));
  const range = { kind: "period" as const, start_ms: 1_788_188_400_000, end_exclusive_ms: 1_790_780_400_000, time_zone: "Asia/Tokyo" };
  await pushExportState(page, {
    kind: "exporting",
    request_id: REQUEST_ID,
    room_id: HARNESS_ROOM_ID,
    range,
    progress: progress(250, 240),
    cancel_requested: false
  });
  await expect(state).toHaveText(t("roomHistoryExport.exporting", { fetched: 250, exported: 240 }));
  await expect(panel.getByTestId("room-info-history-export-summary")).toHaveText(
    t("roomHistoryExport.exporting", { fetched: 250, exported: 240 })
  );

  await pushExportState(page, {
    kind: "completed",
    request_id: REQUEST_ID,
    room_id: HARNESS_ROOM_ID,
    range,
    progress: progress(262, 250, 2)
  });
  await expect(state).toHaveAttribute("data-export-result", "completed");
  await expect(state).toContainText(t("roomHistoryExport.completed", { exported: 250 }));
  await expect(state).toContainText(t("roomHistoryExport.undecryptable", { count: 2 }));
  await dialog.getByRole("button", { name: t("action.done") }).click();
  await expect(dialog).toBeHidden();
  await expect(panel.getByTestId("room-info-history-export-summary")).toContainText(
    t("roomHistoryExport.undecryptable", { count: 2 })
  );
});

test("Stop cancels the in-flight request and a cancelled export is not reported as saved", async ({ page }) => {
  await gotoReadyShell(page);
  const { dialog } = await openExportDialog(page);
  await dialog.getByRole("button", { name: t("roomHistoryExport.save") }).click();
  const state = dialog.getByTestId("room-history-export-state");
  await expect(state).toHaveText(t("roomHistoryExport.exporting", { fetched: 0, exported: 0 }));
  expect((await invocationArgs(page, "export_room_history"))[0]?.range).toEqual({ kind: "allAvailable" });

  await dialog.getByRole("button", { name: t("roomHistoryExport.stop") }).click();
  await expect.poll(() => invocationArgs(page, "cancel_room_history_export")).toEqual([
    { targetRequestId: REQUEST_ID }
  ]);
  await expect(state).toHaveText(t("roomHistoryExport.cancelling"));
  await expect(dialog.getByRole("button", { name: t("roomHistoryExport.stop") })).toBeDisabled();

  await pushExportState(page, {
    kind: "cancelled",
    request_id: REQUEST_ID,
    room_id: HARNESS_ROOM_ID,
    progress: progress(40, 40)
  });
  await expect(state).toHaveAttribute("data-export-result", "cancelled");
  await expect(state).toHaveText(t("roomHistoryExport.cancelled"));
});

test("a failed export shows its Rust failure kind and offers another attempt", async ({ page }) => {
  await gotoReadyShell(page);
  const { dialog } = await openExportDialog(page);
  await dialog.getByRole("button", { name: t("roomHistoryExport.save") }).click();
  const state = dialog.getByTestId("room-history-export-state");
  await expect(state).toHaveText(t("roomHistoryExport.exporting", { fetched: 0, exported: 0 }));
  await pushExportState(page, {
    kind: "failed",
    request_id: REQUEST_ID,
    room_id: HARNESS_ROOM_ID,
    progress: progress(10, 10),
    failure_kind: "write"
  });
  await expect(state).toHaveAttribute("data-export-result", "failed");
  await expect(state).toContainText(t("roomHistoryExport.failed"));
  await expect(state).toContainText(t("roomHistoryExport.failedWrite"));
  await dialog.getByRole("button", { name: t("roomHistoryExport.again") }).click();
  await expect(dialog.getByRole("form", { name: t("roomHistoryExport.title") })).toBeVisible();
});

test("a dismissed save dialog submits nothing and keeps the form", async ({ page }) => {
  await gotoReadyShell(page);
  await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.setCommandResponse("export_room_history", {
      kind: "dismissed"
    })
  );
  const { dialog } = await openExportDialog(page);
  await dialog.getByRole("button", { name: t("roomHistoryExport.save") }).click();
  await expect.poll(async () => (await invocationArgs(page, "export_room_history")).length).toBe(1);
  await expect(dialog.getByRole("form", { name: t("roomHistoryExport.title") })).toBeVisible();
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  await expect(dialog.getByTestId("room-history-export-state")).toHaveCount(0);
});
