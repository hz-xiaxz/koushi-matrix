# Issue #59 Phase A: Element-compatible room-history export

Status: Phase A implementation record. Phase B (GUI entry point, period
dialog, Tauri save dialog, browser-headless and Linux GUI QA) is not part of
this change.

## Upstream behaviour

- **Element Web** (Room Info → Export Chat) exports the live timeline, the
  whole room, or the last N events as HTML, plain text, or JSON. It has no
  date-range option: `ExportType`'s start-date variant and the fetch loop's
  start-date check are commented out. The JSON output is `room_name`,
  `room_creator`, `topic`, `export_date`, `exported_by`, and `messages`.
  `messages` holds `getEffectiveEvent()` for each fetched event that has a
  renderer, `haveRendererForEvent(event, client, false)`.
  Baseline revisions: element-web `c9cff69c74d5faa4606167863e066ef02c0bae0d`
  and matrix-js-sdk `b08a603df74fbb7e97cbfe83097b004ff4122b93`.
- **Element X Android and iOS** have no chat-export flow. As of 2026-09-23,
  their issue trackers have no export request other than room-key export
  (element-x-android #4610).

Intentional divergences from Element Web:

- The export offers a period range (inclusive civil start and end dates in a
  named time zone) and all available history. It does not offer the loaded
  timeline, last-N, HTML, or plain text. The JSON structure is unchanged.
- Attachments keep their event references only. Koushi does not download
  files or build a ZIP.

Observed upstream behaviour that Koushi reproduces rather than "fixes":

- Element maps freshly fetched `/messages` events. Edits are therefore not
  applied to their originals, and `m.replace` events have no renderer. The
  exception is an event that happens to be loaded in Element's in-memory
  timeline, which can show edited content. That depends on UI state and is not
  reproduced.
- An undecryptable event becomes `m.room.message` with `m.bad.encrypted`
  content. A redacted encrypted event stays the pruned `m.room.encrypted` wire
  event.

## Contract

- Commands: `AccountCommand::ExportRoomHistory { request_id, request }` and
  `AccountCommand::CancelRoomHistoryExport { request_id, target_request_id }`.
  `RoomHistoryExportRequest` carries `room_id`, a `RoomHistoryExportRange`, and
  the platform's current UTC offset, which is used only for `export_date`. The
  destination is a native artifact (`RoomHistoryExportDestination`)
  registered for the same request. It never appears in commands, state, or
  logs.
- State: `AppState.room_history_export`. The reducer, guards, and diagram are
  in `docs/architecture/state-machine.md`, section "Room History Export".
  The Tauri DTO carries it as `ui.room_history_export`.
- Core: the account actor admits the export, spawns the task, and retains its
  handle. Cancel and session teardown abort the task and await it.
  `room_history_export::driver` pages `Room::messages` forward from the first
  visible event, 250 events per page, under the account work scheduler's
  background band. It ends on a missing or repeated `end` token, or after 32
  consecutive empty pages. It deduplicates by event id and writes each
  selected event as soon as it is read, so only the event-id set grows with the
  room. `room_history_export::element` contains the effective-event mapping,
  the ported renderer filter, the header, and the `JSON.stringify(…, null, 2)`
  layout. It does no I/O.
- Period exports walk the whole visible history. Topological order does not
  follow `origin_server_ts`, so stopping at the first event past either
  boundary could drop in-range events. An early-termination optimization
  would need its own correctness argument.
- Output: `RoomHistoryExportSink` stages bytes in a hidden temporary file next
  to the destination (created with owner-only permissions). It fsyncs the
  file and renames it over the destination on commit. A staged file that is
  dropped is deleted. The native sink lives in Core behind the port, because
  both the desktop adapter and `koushi-qa` need the same behaviour.
  `koushi-protocol` and `koushi-state` stay filesystem-free.
- `export_date` follows Element's
  `Intl.DateTimeFormat(locale, { year, month, day: "numeric" })`: `M/D/YYYY`
  for `en` and `YYYY/M/D` for `ja`. The date is taken in the adapter-supplied
  offset, and the locale is the Rust-resolved catalog locale.

## Canon consulted

`REPOSITORY_RULES.md` (architecture, state machines, security, tests, QA),
`docs/architecture/state-machine.md`, `docs/architecture/overview.md`
(platform portability), `docs/policies/engineering-rules.md` (build and QA
gates), `docs/agents/state-ownership.md` (snapshot mirrors),
`docs/agents/verification.md`, and `docs/agents/qa-lanes.md`. The canon
amendments in this change are the new state-machine section, the
state-ownership entry, the overview port list, and the QA lane row. They were
reviewed with the strongest available model before implementation.

## Verification

- `cargo test -p koushi-state --test room_history_export_state`: start guard,
  progress, cancel, completion, failure, stale and duplicate settlements,
  logout and lock reset, range boundaries, and Debug redaction.
- `cargo test -p koushi-core --lib room_history_export`: Element fixture
  equality across pages with duplicates and an empty page, the exact
  `JSON.stringify` layout, an empty export, period boundaries, out-of-order
  timestamps, token and empty-page termination, page and write failures,
  `export_date`, and native sink commit and discard. Fixtures are under
  `crates/koushi-core/tests/fixtures/room_history_export/`, and their README
  records the baseline and what comparisons normalize.
- `cargo test -p koushi-core --test room_history_export_admission`: command
  correlation, ready gating, Debug redaction, and release of a rejected
  export's destination registration.
- `qa:headless-local -- --core --scenario=room_history_export` proves the
  flow on a local homeserver. Tokens: `history_export_full=ok`,
  `history_export_period=ok`, `history_export_utd_counted=ok`,
  `history_export_cancel=ok`, and `room_history_export=ok`.

## Remaining for Phase B

The Room Info entry point and period dialog, time-zone display, and the Tauri
save dialog plus native-artifact registration command. Also: progress,
cancel, and result presentation, including the warning that encrypted content
is saved as plaintext; catalog entries; user-guide updates; and
browser-headless and Linux GUI QA.
