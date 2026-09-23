# Issue #59 Phase B: room-history export GUI and file saving

Status: Phase B implementation record. The Rust contract, paging, Element
mapping, and headless QA are in the
[Phase A plan](2026-09-23-issue59-room-history-export-phase-a.md); no reducer,
protocol, or Core behavior changes here.

## Upstream behaviour

Element Web opens its export from Room Info → Export Chat, in a modal with the
format, range, size limit, and an attachments checkbox, and shows a progress
line with a Cancel button while it runs. Element X Android and iOS have no
chat-export flow (see the Phase A plan). Koushi follows Element Web's entry
point and modal-with-progress shape. The deliberate differences are the ones
recorded in Phase A: JSON only, a period or all available history, and no
attachment bodies. Koushi also shows the time zone used for the dates and a
warning that encrypted content is saved as plain text.

## Ownership

- **Rust Core** (Phase A) owns the export: admission, paging, period
  inclusion, decryption outcomes, event selection, JSON, the staged file and
  its atomic commit, and `AppState.room_history_export`.
- **Tauri adapter** (`apps/desktop/src-tauri/src/commands/history_export.rs`):
  - `room_history_export_time_zone` names the platform IANA zone (`jiff`'s
    system zone; `UTC` when the platform names none).
  - `export_room_history(roomId, range, dialogTitle, fileNameStem)` resolves
    the range, opens the native save dialog, registers the chosen path as the
    `RoomHistoryExportDestination` native artifact, and submits
    `AccountCommand::ExportRoomHistory`. It answers `dismissed` when the dialog
    is closed, or `submitted` with the request id and its admission. The path
    never reaches the WebView. `export_date_utc_offset_minutes` is the system
    zone's current offset.
  - A period arrives as inclusive civil dates (`YYYY-MM-DD`) with the zone
    name the dialog displayed. The adapter resolves `start_ms` as 00:00 of the
    start day and `end_exclusive_ms` as 00:00 of the day after the end day, in
    that zone. A midnight inside a DST gap resolves to the first instant after
    the gap. Unknown zones, malformed or impossible dates, and dates before
    1970 are rejected before any dialog opens. An end before the start
    resolves to a range Core rejects as `InvalidRange`.
  - `cancel_room_history_export(targetRequestId)` rebuilds the target
    `RequestId` on the adapter's own connection and submits
    `AccountCommand::CancelRoomHistoryExport`.
- **React** renders `ui.room_history_export` and owns presentation state only:
  whether the dialog is open, the unsent range and dates, and the request id
  it started. That id keeps an earlier export's settlement from being shown as
  this dialog's result. An admitted snapshot that holds neither this request in
  flight nor its settlement means Rust rejected the start. Room info shows the
  Rust state for its room: progress while the export runs, then the latest
  outcome until the next export replaces it.

The default file name is `<room name> - Chat Export - YYYY-MM-DD.json`. The
catalog supplies the stem, the adapter replaces characters that are not
portable in file names and appends the platform-local date.

## QA destination override

WebDriver cannot drive a native file dialog. Debug and test builds honour
`KOUSHI_QA_HISTORY_EXPORT_DIR`: when it is set, the adapter writes to
`<dir>/<default file name>` instead of opening the dialog. The constant is
declared under `#[cfg(any(debug_assertions, test))]` and listed in
`scripts/desktop-release-gate-check.mjs`, so a release build cannot read it.

The existing `local-e2ee-key-management` Linux GUI lane types into
"Key export destination" inputs that the Security settings no longer render.
That lane is stale on `main`. It is outside this change and is not fixed here.

## Canon consulted

`REPOSITORY_RULES.md` (architecture and ownership, text input and IME, user
visible text, localization, security, QA gates, documentation),
`docs/architecture/overview.md`, `docs/architecture/state-machine.md`
(unchanged; no reducer change), `docs/architecture/i18n.md`,
`docs/agents/state-ownership.md` (amended for the adapter's Phase B role),
`docs/agents/verification.md`, and `docs/agents/qa-lanes.md` (new Linux GUI
row).

## Files

- Tauri: `commands/history_export.rs` (new), `commands/mod.rs`
  (`submit_core_command_with_native_artifact_path`), `lib.rs` (handler
  registration), `Cargo.toml` (`jiff`).
- React: `components/RoomHistoryExportDialog.tsx` (new), `RoomInfoPanel.tsx`,
  `rightPanel.tsx`, `App.tsx`, `styles.css`, `i18n/messages.ts` (en and ja),
  `backend/desktopApi.ts`, `backend/client.ts`, `domain/types.ts`.
- Tests: `history_export.rs` unit tests, `RoomHistoryExportDialog.test.tsx`,
  `e2e/room-history-export.spec.ts`, harness and IPC-mock transitions, the
  Linux GUI scenario `local-room-history-export`.
- Docs: `docs/help/rooms-and-spaces.md` ("Download room history"),
  `docs/help/security-and-recovery.md`, `docs/agents/qa-lanes.md`,
  `docs/agents/state-ownership.md`, `docs/agents/plans.md`.

## Verification

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
  history_export`: period resolution in Asia/Tokyo, a 23-hour DST day in
  America/New_York, rejected inputs, an end before the start, the wire
  shapes, the UTC offset, file-name sanitizing, command correlation, and
  Debug redaction of the room and zone.
- `npx vitest run src/components/RoomHistoryExportDialog.test.tsx`: en and ja
  period submission with the displayed zone and plaintext warning, invalid
  period, default range, progress and Stop, a settlement matched to its own
  request, cancelled and failed results never reported as saved, a rejected
  start, a dismissed dialog, another room in flight, and the Room info
  summary.
- `npx playwright test e2e/room-history-export.spec.ts --workers=1`: the real
  app against the IPC mock, from Room info through the command arguments,
  Rust progress, completion with undecryptable counts, Stop, failure, and a
  dismissed dialog.
- Linux GUI `--scenario=local-room-history-export --server=tuwunel`: a local
  Tuwunel room with synthetic messages, exported three times through the real
  dialog. It checks all available history, a period in 2000 that saves no
  messages, and today's period in the displayed zone. Each committed file is
  parsed for Element's top-level keys and the seeded messages, and its count
  is compared with the dialog's. Tokens: `gui_local_history_export_all=ok`,
  `gui_local_history_export_period=ok`, `gui_local_room_history_export=ok`.
