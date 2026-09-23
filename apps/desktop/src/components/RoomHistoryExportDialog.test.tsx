// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import {
  RoomHistoryExportDialog,
  roomHistoryExportSummary,
  type RoomHistoryExportControls
} from "./RoomHistoryExportDialog";
import { RoomInfoPanel } from "./RoomInfoPanel";
import { setActiveLocaleProfile, t } from "../i18n/messages";
import type {
  RoomHistoryExportProgress,
  RoomHistoryExportRangeInput,
  RoomHistoryExportStart,
  RoomHistoryExportState,
  RoomSummary
} from "../domain/types";

const room: RoomSummary = {
  room_id: "!history:example.invalid",
  display_name: "Synthetic History",
  display_label: "Synthetic History",
  original_display_label: "Synthetic History",
  avatar: null,
  is_dm: false,
  dm_user_ids: [],
  tags: { favourite: null, low_priority: null },
  parent_space_ids: [],
  dm_space_ids: [],
  is_encrypted: true,
  unread_count: 0
};

const progress = (fetched: number, exported: number, undecryptable = 0): RoomHistoryExportProgress => ({
  fetched_events: fetched,
  exported_events: exported,
  undecryptable_events: undecryptable
});

const idle: RoomHistoryExportState = { kind: "idle" };

function controls(started: RoomHistoryExportStart = { kind: "dismissed" }) {
  return {
    loadTimeZone: vi.fn(async () => "Asia/Tokyo"),
    start: vi.fn(
      async (_roomId: string, _range: RoomHistoryExportRangeInput, _title: string, _stem: string) => started
    ),
    cancel: vi.fn(async () => undefined)
  } satisfies RoomHistoryExportControls;
}

function renderDialog(exportState: RoomHistoryExportState, ports: RoomHistoryExportControls) {
  const onClose = vi.fn();
  const view = render(
    <RoomHistoryExportDialog room={room} exportState={exportState} controls={ports} onClose={onClose} />
  );
  return {
    onClose,
    rerender: (next: RoomHistoryExportState) =>
      view.rerender(<RoomHistoryExportDialog room={room} exportState={next} controls={ports} onClose={onClose} />)
  };
}

afterEach(() => {
  cleanup();
  setActiveLocaleProfile("en", "none");
});

describe.each(["en", "ja"] as const)("room-history export dialog in %s", (locale) => {
  test("sends the displayed civil period and time zone, with the plaintext warning", async () => {
    setActiveLocaleProfile(locale, "none");
    const ports = controls();
    renderDialog(idle, ports);
    expect(screen.getByRole("note").textContent).toBe(t("roomHistoryExport.plaintextWarning"));
    const period = screen.getByRole("radio", { name: t("roomHistoryExport.rangePeriod") });
    await waitFor(() => expect((period as HTMLInputElement).disabled).toBe(false));
    fireEvent.click(period);
    expect(screen.getByTestId("room-history-export-time-zone").textContent).toBe(
      t("roomHistoryExport.timeZone", { timeZone: "Asia/Tokyo" })
    );
    fireEvent.change(screen.getByLabelText(t("roomHistoryExport.startDate")), { target: { value: "2026-09-01" } });
    fireEvent.change(screen.getByLabelText(t("roomHistoryExport.endDate")), { target: { value: "2026-09-30" } });
    fireEvent.click(screen.getByRole("button", { name: t("roomHistoryExport.save") }));
    await waitFor(() => expect(ports.start).toHaveBeenCalledTimes(1));
    expect(ports.start).toHaveBeenCalledWith(
      room.room_id,
      { kind: "period", startDate: "2026-09-01", endDate: "2026-09-30", timeZone: "Asia/Tokyo" },
      t("roomHistoryExport.saveDialogTitle"),
      t("roomHistoryExport.fileNameStem", { roomName: "Synthetic History" })
    );
  });
});

test("an end date before the start date blocks Save", async () => {
  const ports = controls();
  renderDialog(idle, ports);
  const period = screen.getByRole("radio", { name: "Period" });
  await waitFor(() => expect((period as HTMLInputElement).disabled).toBe(false));
  fireEvent.click(period);
  fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-09-10" } });
  fireEvent.change(screen.getByLabelText("End date"), { target: { value: "2026-09-01" } });
  expect(screen.getByRole("alert").textContent).toBe(t("roomHistoryExport.invalidPeriod"));
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.submit(screen.getByRole("form", { name: "Download history" }));
  expect(ports.start).not.toHaveBeenCalled();
});

test("dates before 1970 are rejected in the dialog with their own hint", async () => {
  const ports = controls();
  renderDialog(idle, ports);
  const period = screen.getByRole("radio", { name: "Period" });
  await waitFor(() => expect((period as HTMLInputElement).disabled).toBe(false));
  fireEvent.click(period);
  fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "1969-12-31" } });
  expect(screen.getByRole("alert").textContent).toBe(t("roomHistoryExport.periodTooEarly"));
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a failed Stop submit is contained and leaves the Rust view", async () => {
  const ports = controls();
  ports.cancel.mockRejectedValueOnce(new Error("submit timed out"));
  renderDialog(
    {
      kind: "exporting",
      request_id: 41,
      room_id: room.room_id,
      range: { kind: "allAvailable" },
      progress: progress(1, 1),
      cancel_requested: false
    },
    ports
  );
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => expect(ports.cancel).toHaveBeenCalledTimes(1));
  expect((screen.getByRole("button", { name: "Stop" }) as HTMLButtonElement).disabled).toBe(false);
});

test("all available history is the default and needs no dates", async () => {
  const ports = controls();
  renderDialog(idle, ports);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(ports.start).toHaveBeenCalledTimes(1));
  expect(ports.start.mock.calls[0]?.[1]).toEqual({ kind: "allAvailable" });
});

test("renders Rust progress and dispatches Stop for the in-flight request", () => {
  const ports = controls();
  const view = renderDialog(
    {
      kind: "exporting",
      request_id: 41,
      room_id: room.room_id,
      range: { kind: "allAvailable" },
      progress: progress(500, 480),
      cancel_requested: false
    },
    ports
  );
  expect(screen.getByTestId("room-history-export-state").textContent).toBe(
    t("roomHistoryExport.exporting", { fetched: 500, exported: 480 })
  );
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(ports.cancel).toHaveBeenCalledExactlyOnceWith(41);
  view.rerender({
    kind: "exporting",
    request_id: 41,
    room_id: room.room_id,
    range: { kind: "allAvailable" },
    progress: progress(500, 480),
    cancel_requested: true
  });
  expect(screen.getByTestId("room-history-export-state").textContent).toBe(t("roomHistoryExport.cancelling"));
  expect((screen.getByRole("button", { name: "Stop" }) as HTMLButtonElement).disabled).toBe(true);
});

test("shows the settlement of its own request, with undecryptable counts", async () => {
  const ports = controls({ kind: "submitted", requestId: 7, admission: { protocolVersion: 1, admittedGeneration: 3 } });
  const earlier: RoomHistoryExportState = {
    kind: "completed",
    request_id: 6,
    room_id: room.room_id,
    range: { kind: "allAvailable" },
    progress: progress(9, 9)
  };
  const view = renderDialog(earlier, ports);
  // An earlier export's result is not this dialog's result.
  expect(screen.queryByTestId("room-history-export-state")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await act(async () => {
    view.rerender({
      kind: "exporting",
      request_id: 7,
      room_id: room.room_id,
      range: { kind: "allAvailable" },
      progress: progress(0, 0),
      cancel_requested: false
    });
  });
  view.rerender({
    kind: "completed",
    request_id: 7,
    room_id: room.room_id,
    range: { kind: "allAvailable" },
    progress: progress(12, 10, 2)
  });
  const result = screen.getByTestId("room-history-export-state");
  expect(result.getAttribute("data-export-result")).toBe("completed");
  expect(result.textContent).toContain(t("roomHistoryExport.completed", { exported: 10 }));
  expect(result.textContent).toContain(t("roomHistoryExport.undecryptable", { count: 2 }));
  fireEvent.click(screen.getByRole("button", { name: "Download again" }));
  expect(screen.getByRole("form", { name: "Download history" })).toBeTruthy();
});

test.each([
  ["cancelled", { kind: "cancelled", request_id: 7, room_id: room.room_id, progress: progress(3, 3) }, ["roomHistoryExport.cancelled"]],
  [
    "failed",
    { kind: "failed", request_id: 7, room_id: room.room_id, progress: progress(3, 3), failure_kind: "network" },
    ["roomHistoryExport.failed", "roomHistoryExport.failedNetwork"]
  ]
] as const)("a %s export is never reported as saved", async (_kind, settled, messages) => {
  const ports = controls({ kind: "submitted", requestId: 7, admission: { protocolVersion: 1, admittedGeneration: 3 } });
  const view = renderDialog(idle, ports);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(ports.start).toHaveBeenCalled());
  await act(async () => view.rerender(settled as RoomHistoryExportState));
  const result = screen.getByTestId("room-history-export-state");
  for (const id of messages) expect(result.textContent).toContain(t(id));
  expect(result.textContent).not.toContain(t("roomHistoryExport.completed", { exported: 3 }));
});

test("a rejected start or a transport error reports that nothing started", async () => {
  const rejected = controls({ kind: "submitted", requestId: 9, admission: { protocolVersion: 1, admittedGeneration: 3 } });
  renderDialog(idle, rejected);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", t("roomHistoryExport.notStarted"));
  cleanup();

  const failing = controls();
  failing.start.mockRejectedValueOnce(new Error("transport"));
  renderDialog(idle, failing);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", t("roomHistoryExport.notStarted"));
});

test("a dismissed save dialog leaves the form without an error", async () => {
  const ports = controls({ kind: "dismissed" });
  renderDialog(idle, ports);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(ports.start).toHaveBeenCalled());
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("form", { name: "Download history" })).toBeTruthy();
});

test("another room's export in flight blocks a new one", () => {
  renderDialog(
    {
      kind: "exporting",
      request_id: 2,
      room_id: "!other:example.invalid",
      range: { kind: "allAvailable" },
      progress: progress(1, 1),
      cancel_requested: false
    },
    controls()
  );
  expect(screen.getByText(t("roomHistoryExport.busyOtherRoom"))).toBeTruthy();
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
});

test("an unencrypted room shows no plaintext warning", () => {
  render(
    <RoomHistoryExportDialog
      room={{ ...room, is_encrypted: false }}
      exportState={idle}
      controls={controls()}
      onClose={vi.fn()}
    />
  );
  expect(screen.queryByRole("note")).toBeNull();
});

test("Room info opens the dialog and summarizes the latest Rust outcome for its room", () => {
  const completed: RoomHistoryExportState = {
    kind: "completed",
    request_id: 5,
    room_id: room.room_id,
    range: { kind: "allAvailable" },
    progress: progress(4, 4, 1)
  };
  render(
    <RoomInfoPanel
      room={room}
      roomNotificationSettings={undefined}
      spaces={[]}
      roomHistoryExport={completed}
      roomHistoryExportControls={controls()}
    />
  );
  expect(screen.getByTestId("room-info-history-export-summary").textContent).toBe(
    t("roomHistoryExport.completed", { exported: 4 }) + t("roomHistoryExport.undecryptable", { count: 1 })
  );
  fireEvent.click(screen.getByRole("button", { name: "Download" }));
  expect(screen.getByRole("dialog", { name: "Download history" })).toBeTruthy();
  expect(roomHistoryExportSummary(completed, "!other:example.invalid")).toEqual([]);
  expect(roomHistoryExportSummary(idle, room.room_id)).toEqual([]);
});
