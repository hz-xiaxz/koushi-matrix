import { AlertTriangle } from "lucide-react";
import { useEffect, useState } from "react";

import { t, type MessageId } from "../i18n/messages";
import { ModalDialog } from "./ModalDialog";
import { ImeSafeForm } from "./ImeTextControl";
import type {
  RoomHistoryExportFailureKind,
  RoomHistoryExportRangeInput,
  RoomHistoryExportStart,
  RoomHistoryExportState,
  RoomSummary
} from "../domain/types";

/** Platform capabilities for the room-history export (#59). The Tauri adapter
 * owns the time zone, the save dialog, and the file; Rust owns the export. */
export interface RoomHistoryExportControls {
  loadTimeZone: () => Promise<string>;
  /** Opens the native save dialog and, unless it is dismissed, submits the
   * export. Resolves after the admitted snapshot has been applied. */
  start: (
    roomId: string,
    range: RoomHistoryExportRangeInput,
    dialogTitle: string,
    fileNameStem: string
  ) => Promise<RoomHistoryExportStart>;
  cancel: (requestId: number) => Promise<void>;
}

type RangeKind = RoomHistoryExportRangeInput["kind"];

const FAILURE_MESSAGES: Record<RoomHistoryExportFailureKind, MessageId> = {
  invalidRange: "roomHistoryExport.failedInvalidRange",
  roomNotFound: "roomHistoryExport.failedRoomNotFound",
  destinationUnavailable: "roomHistoryExport.failedDestination",
  write: "roomHistoryExport.failedWrite",
  network: "roomHistoryExport.failedNetwork",
  sdk: "roomHistoryExport.failedSdk"
};

/** Today's civil date in `timeZone`, as the `YYYY-MM-DD` a date input uses. */
function civilToday(timeZone: string): string {
  try {
    return new Intl.DateTimeFormat("en-CA", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" })
      .format(new Date());
  } catch {
    return "";
  }
}

/** The in-flight status line for `roomId`, or null when it is not exporting. */
export function roomHistoryExportStatus(state: RoomHistoryExportState, roomId: string): string | null {
  if (state.kind !== "exporting" || state.room_id !== roomId) return null;
  return state.cancel_requested
    ? t("roomHistoryExport.cancelling")
    : t("roomHistoryExport.exporting", {
        fetched: state.progress.fetched_events,
        exported: state.progress.exported_events
      });
}

/** Room info's summary of the latest export of `roomId`: progress while it
 * runs, then its Rust-reported outcome until the next export replaces it. */
export function roomHistoryExportSummary(state: RoomHistoryExportState, roomId: string): string[] {
  if (state.kind === "idle" || state.room_id !== roomId) return [];
  switch (state.kind) {
    case "exporting":
      return [roomHistoryExportStatus(state, roomId) ?? ""];
    case "cancelled":
      return [t("roomHistoryExport.cancelled")];
    case "failed":
      return [t("roomHistoryExport.failed"), t(FAILURE_MESSAGES[state.failure_kind])];
    case "completed":
      return [
        state.progress.exported_events > 0
          ? t("roomHistoryExport.completed", { exported: state.progress.exported_events })
          : t("roomHistoryExport.completedEmpty"),
        ...(state.progress.undecryptable_events > 0
          ? [t("roomHistoryExport.undecryptable", { count: state.progress.undecryptable_events })]
          : [])
      ];
  }
}

export function RoomHistoryExportDialog({
  room,
  exportState,
  controls,
  onClose
}: {
  room: RoomSummary;
  exportState: RoomHistoryExportState;
  controls: RoomHistoryExportControls;
  onClose: () => void;
}) {
  const [timeZone, setTimeZone] = useState<string | null>(null);
  const [rangeKind, setRangeKind] = useState<RangeKind>("allAvailable");
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  // Presentation only: the request this dialog started, so that its Rust
  // settlement is shown and an earlier export's result is not.
  const [submittedRequestId, setSubmittedRequestId] = useState<number | null>(null);
  const [starting, setStarting] = useState(false);
  const [startFailed, setStartFailed] = useState(false);

  useEffect(() => {
    let current = true;
    controls.loadTimeZone().then(
      (zone) => {
        if (!current) return;
        setTimeZone(zone);
        const today = civilToday(zone);
        setStartDate((value) => value || today);
        setEndDate((value) => value || today);
      },
      () => {
        if (current) setTimeZone(null);
      }
    );
    return () => {
      current = false;
    };
  }, [controls]);

  const roomId = room.room_id;
  const exportingHere = exportState.kind === "exporting" && exportState.room_id === roomId;
  const exportingElsewhere = exportState.kind === "exporting" && exportState.room_id !== roomId;
  const settledHere =
    submittedRequestId !== null &&
    exportState.kind !== "idle" &&
    exportState.kind !== "exporting" &&
    exportState.request_id === submittedRequestId
      ? exportState
      : null;
  // A rejected start leaves the Rust state unchanged, so the admitted snapshot
  // holds neither this request in flight nor its settlement.
  const notStarted =
    startFailed || (submittedRequestId !== null && !starting && !exportingHere && settledHere === null);
  const periodValid = startDate !== "" && endDate !== "" && startDate <= endDate;
  const canSave =
    !starting &&
    !exportingElsewhere &&
    (rangeKind === "allAvailable" || (timeZone !== null && periodValid));

  async function save() {
    if (!canSave) return;
    const range: RoomHistoryExportRangeInput =
      rangeKind === "period" && timeZone !== null
        ? { kind: "period", startDate, endDate, timeZone }
        : { kind: "allAvailable" };
    setStarting(true);
    setStartFailed(false);
    setSubmittedRequestId(null);
    try {
      const started = await controls.start(
        roomId,
        range,
        t("roomHistoryExport.saveDialogTitle"),
        t("roomHistoryExport.fileNameStem", { roomName: room.display_label })
      );
      if (started.kind === "submitted") setSubmittedRequestId(started.requestId);
    } catch {
      setStartFailed(true);
    } finally {
      setStarting(false);
    }
  }

  function startOver() {
    setSubmittedRequestId(null);
    setStartFailed(false);
  }

  const status = roomHistoryExportStatus(exportState, roomId);

  return (
    <ModalDialog title={t("roomHistoryExport.title")} className="room-history-export-modal" onClose={onClose}>
      <div className="room-history-export-content">
        {exportingHere && exportState.kind === "exporting" ? (
          <>
            <p role="status" data-testid="room-history-export-state">{status}</p>
            <p className="profile-settings-hint">{t("roomHistoryExport.continuesInBackground")}</p>
            <div className="dialog-actions">
              <button
                type="button"
                className="dialog-button"
                disabled={exportState.cancel_requested}
                onClick={() => void controls.cancel(exportState.request_id)}
              >
                {t("roomHistoryExport.stop")}
              </button>
            </div>
          </>
        ) : settledHere ? (
          <>
            <div role="status" data-testid="room-history-export-state" data-export-result={settledHere.kind}>
              {settledHere.kind === "completed" ? (
                <p>
                  {settledHere.progress.exported_events > 0
                    ? t("roomHistoryExport.completed", { exported: settledHere.progress.exported_events })
                    : t("roomHistoryExport.completedEmpty")}
                </p>
              ) : settledHere.kind === "cancelled" ? (
                <p>{t("roomHistoryExport.cancelled")}</p>
              ) : (
                <>
                  <p>{t("roomHistoryExport.failed")}</p>
                  <p>{t(FAILURE_MESSAGES[settledHere.failure_kind])}</p>
                </>
              )}
              {settledHere.kind === "completed" && settledHere.progress.undecryptable_events > 0 ? (
                <p className="room-history-export-warning">
                  <AlertTriangle size={14} aria-hidden="true" />
                  <span>
                    {t("roomHistoryExport.undecryptable", { count: settledHere.progress.undecryptable_events })}
                  </span>
                </p>
              ) : null}
            </div>
            <div className="dialog-actions">
              <button type="button" className="dialog-button" onClick={startOver}>
                {t("roomHistoryExport.again")}
              </button>
              <button type="button" className="dialog-button is-primary" onClick={onClose}>
                {t("action.done")}
              </button>
            </div>
          </>
        ) : (
          <ImeSafeForm
            className="room-history-export-form"
            aria-label={t("roomHistoryExport.title")}
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            <div className="create-room-visibility" role="radiogroup" aria-label={t("roomHistoryExport.range")}>
              <label className="create-room-option">
                <input
                  type="radio"
                  name="room-history-export-range"
                  checked={rangeKind === "allAvailable"}
                  onChange={() => setRangeKind("allAvailable")}
                />
                <span>{t("roomHistoryExport.rangeAll")}</span>
              </label>
              <label className="create-room-option">
                <input
                  type="radio"
                  name="room-history-export-range"
                  checked={rangeKind === "period"}
                  disabled={timeZone === null}
                  onChange={() => setRangeKind("period")}
                />
                <span>{t("roomHistoryExport.rangePeriod")}</span>
              </label>
            </div>
            {rangeKind === "period" && timeZone !== null ? (
              <div className="room-history-export-period">
                <label className="room-history-export-date">
                  <span>{t("roomHistoryExport.startDate")}</span>
                  <input
                    aria-label={t("roomHistoryExport.startDate")}
                    type="date"
                    value={startDate}
                    onChange={(event) => setStartDate(event.currentTarget.value)}
                  />
                </label>
                <label className="room-history-export-date">
                  <span>{t("roomHistoryExport.endDate")}</span>
                  <input
                    aria-label={t("roomHistoryExport.endDate")}
                    type="date"
                    value={endDate}
                    onChange={(event) => setEndDate(event.currentTarget.value)}
                  />
                </label>
                <p className="profile-settings-hint" data-testid="room-history-export-time-zone">
                  {t("roomHistoryExport.timeZone", { timeZone })}
                </p>
                {!periodValid ? (
                  <p className="profile-settings-hint error" role="alert">{t("roomHistoryExport.invalidPeriod")}</p>
                ) : null}
              </div>
            ) : null}
            <p className="profile-settings-hint">{t("roomHistoryExport.availability")}</p>
            <p className="profile-settings-hint">{t("roomHistoryExport.attachments")}</p>
            {room.is_encrypted ? (
              <p className="room-history-export-warning" role="note">
                <AlertTriangle size={14} aria-hidden="true" />
                <span>{t("roomHistoryExport.plaintextWarning")}</span>
              </p>
            ) : null}
            {exportingElsewhere ? (
              <p className="profile-settings-hint" role="status">{t("roomHistoryExport.busyOtherRoom")}</p>
            ) : null}
            {notStarted ? (
              <p className="profile-settings-hint error" role="alert">{t("roomHistoryExport.notStarted")}</p>
            ) : null}
            <div className="dialog-actions">
              <button type="button" className="dialog-button" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button type="submit" className="dialog-button is-primary" disabled={!canSave}>
                {t("roomHistoryExport.save")}
              </button>
            </div>
          </ImeSafeForm>
        )}
      </div>
    </ModalDialog>
  );
}
