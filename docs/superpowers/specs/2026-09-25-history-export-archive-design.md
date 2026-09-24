# Design — History export archive (room and Space, HTML + original JSON + attachments)

Status: approved in brainstorming on 2026-09-25; follows #59 (closed).

## Goal

Let a user download a room's history, or the history of every non-DM room in a
Space, as a self-contained folder that serves **migration**: the original
events are kept losslessly, every attachment is downloaded, and a human can
read the history offline in a browser.

- Output = human-readable HTML **plus** the Element-compatible JSON original.
- Images are embedded in the HTML as reduced previews that link to the
  full-size file.
- Math renders in the HTML and its LaTeX source survives in both outputs.
- All attachments are downloaded, with no size or type limit. The user is
  warned about time and disk use up front; the export may run for hours.
- A progress dialog shows which rooms of the Space are finished.
- An interrupted export resumes at room granularity.

Non-goals: import back into Koushi or another client; a JSON-only mode;
per-file resume inside a room; exporting rooms the user has not joined.

## Decisions

| Topic | Decision |
| --- | --- |
| Formats | HTML for reading + two originals: `messages.json` (current Element-compatible output, unchanged) and `events.jsonl` (every fetched in-range event verbatim, one per line) |
| Math in HTML | Keep `data-mx-maths` source verbatim; KaTeX assets bundled into the folder render it on open; LaTeX source is the no-JS fallback |
| Images | Thumbnails generated locally in Rust from the downloaded original; server thumbnails are not used |
| Attachments | All downloaded (encrypted ones decrypted); per-file failure is recorded, not fatal |
| Space scope | Recurse into subspaces; joined rooms only; DMs excluded; unjoined rooms listed as skipped |
| Single-room export | Replaced by the same folder format (one-room archive); the JSON-only file output is retired |
| Resume | Per room: work in `.partial`, rename on completion, skip completed rooms on rerun |
| Architecture | Per-room three-stage pipeline; HTML is a pure function of `events.jsonl` + `room.json` + `attachments.json` |
| State ownership | The whole download state machine lives in Rust; React renders and dispatches typed commands |

Rejected: HTML only (lossy — relations, raw content and math source would only
survive as rendered output); rendering HTML or KaTeX in React (breaks Rust
ownership of export semantics); Rust-side LaTeX→MathML (fidelity diverges from
the timeline's KaTeX); a single pass writing JSON, attachments and HTML
together (entangles rendering with paging, and HTML cannot be regenerated).

## 1. Folder layout and resume

The user chooses a parent directory. The export creates:

```
<Space name> - Export 2026-09-25/
  koushi-export.json        resume manifest
  index.html                table of contents
  assets/katex/…            KaTeX JS/CSS/fonts, embedded in the Rust binary
  rooms/
    <room name> (<short hash of room_id>)/
      messages.json         Element-compatible original (as today)
      events.jsonl          every fetched in-range event, verbatim, one per line
      room.json             room name, topic, sender display names, export metadata
      attachments.json      event_id → files/… and thumbs/… with fetch status
      index.html
      files/0001_report.pdf
      thumbs/0001.jpg
```

A single-room export produces the same layout with one room.

- Room folder names are the sanitized room name plus a short hash of the
  `room_id`, so same-named rooms do not collide and a renamed room is still
  found on resume. Sanitizing reuses `safe_media_save_filename`.
- Attachment files are named `<chronological sequence>_<sanitized original
  name>`; thumbnails `<sequence>.jpg`. Room folder stems and attachment name
  stems are capped (120 characters and 150 UTF-8 bytes) so names stay within
  the 255-byte file-name limit, including for CJK text.
- `messages.json` keeps Element's renderer filter, which drops reactions and
  edit events. `events.jsonl` is the lossless original: every distinct event
  the walk read inside the range, before that filter, one JSON object per
  line. Both are written by the same pass over the history.
- The chosen directory is a native artifact
  (`NativeArtifactKind::HistoryExportDirectory`). It never appears in
  commands, state, or logs. If the chosen directory contains
  `koushi-export.json`, it is the export directory and the start is a resume;
  otherwise it is the parent and a new export folder is created in it (with a
  ` (n)` suffix when the name is taken; a parent is never resumed).
- `koushi-export.json` records a format version, the scope (room or Space id),
  the range, and each room's folder and status. It is rewritten atomically
  (temp file + rename) after every room transition that changes it.
- A room is built in `rooms/.<folder>.partial/` and renamed to its final name
  only after all three stages succeed.
- Choosing an existing export directory whose manifest matches resumes:
  completed rooms are skipped, `.partial` folders are deleted and their rooms
  redone. A scope or range mismatch refuses resume and asks for a new folder.
  Rooms added to the Space since the previous run join as pending.
- The top-level `index.html` is regenerated at the end of every run, including
  stopped and resumed runs.
- The existing range choice (all available history, or a period) applies to
  every room of a Space export.

## 2. HTML rendering

`index.html` for a room is produced by a pure function of `events.jsonl`,
`room.json` and `attachments.json`, in two streaming passes: the first
collects reactions, edits, thread roots and reply excerpts by event id, the
second writes the page. It performs no network access, and the page loads
nothing from outside the folder.

Safety:

- `formatted_body` goes through the same Rust sanitization the timeline uses
  (ruma `SanitizerConfig::compat()` plus Koushi's post-processing in
  `timeline/item_projection.rs`). That code is extracted into a shared function
  called by both the timeline projection and the exporter. Scripts, event
  handlers and remote/`mxc:` images never reach the output.
- Each page carries a CSP that allows scripts only from files in the export
  folder (the bundled KaTeX and a small `koushi-math.js` bootstrap; no inline
  script) and no network connections. A browser test opens a generated page
  over `file://` and proves math renders under that CSP.
- Messages without `formatted_body` render `body` escaped with line breaks
  preserved.

Math: elements with `data-mx-maths` are emitted with their source intact; the
bundled KaTeX renders them on open using the timeline's inline/display
distinction and expression-size limit. Without JS the LaTeX source shows.

Attachments:

- Images and stickers: thumbnail from `thumbs/`, linking to the original in
  `files/`. Formats that cannot be reduced (animated GIF, SVG, corrupt data)
  show a file link only.
- Video, audio and files: name, size and a link into `files/`.
- Failed downloads: name and a "not retrieved" marker.

Structure:

- Sender display name and time; a separator at each date change. Times use the
  exporting device's time zone, which the page states.
- Replies: the replied-to sender and an excerpt, linking to that message's
  anchor on the page.
- Threads: kept in chronological order; thread replies carry a marker and a
  link to the thread root. No collapsing.
- Edits: like Element, the latest edit is shown when the original event bundles
  it, marked "edited".
- Reactions: per-key counts under the message.
- Redacted and undecryptable events: explicit placeholders.
- State events (joins, leaves, renames, …): one-line descriptions.

Fixed strings follow the app locale at export time and live in the message
catalog: React resolves them and the start command carries them to Rust as a
typed label set, the same way the file-name stem travels today. CSS is inline, supports light and dark, and prints acceptably.

The top-level `index.html` lists the Space name, export time, range, and every
room with its status (completed, skipped, failed), event count, attachment
count, failed-attachment count, and a link.

## 3. State machine, commands and progress UI

The download state machine is Rust-owned and extends the existing
`Room History Export` section of `docs/architecture/state-machine.md`.
`AppState.room_history_export` is replaced by `AppState.history_export`
covering both scopes.

Rust owns: every transition and guard; resume eligibility (reading and
matching the manifest); room selection (subspace recursion, DM exclusion,
unjoined skip — reusing the Space child/membership projection from #961 and
`direct_message_classification`); history paging; attachment download,
decryption and thumbnailing; HTML generation; success/failure decisions.

React renders the DTO and dispatches typed commands: `Start { scope, range,
labels }`, `Stop`, and `RetryFailed`. The directory picker runs in the Tauri
adapter, which registers the chosen directory as a native artifact for the
start request. The account actor keeps the resolved export directory after
settlement so `RetryFailed` can resume it without the path ever leaving
Core. A range picked for a resumed export must match the manifest's range.

```
HistoryExportState
  Idle
  Preparing { request_id, scope, range }
  Running   { request_id, scope, range, rooms, stop_requested }
  Completed | Stopped | Failed { request_id, scope, range, rooms, failure_kind? }

RoomExportState (elements of rooms[], advanced only while Running)
  Pending → Fetching{counts} → Attachments{done, total, failed} → Rendering → Completed{counts}
  Pending → Skipped{NotJoined}
  Fetching | Attachments | Rendering → Failed{kind}
  Completed may also be restored from the manifest on resume.
```

Guards:

- Every progress event must match the current `request_id` and `room_id`;
  anything stale is dropped.
- `Stop` is accepted once, while `Preparing` or `Running`. Stopping is
  cooperative: the task checks the request between pages and attachments,
  then removes the room's `.partial` folder, rewrites the manifest and the
  top-level `index.html`, and settles as `Stopped`.
- `RetryFailed` is accepted only from `Completed`, `Stopped` or `Failed`, and
  resumes the same export directory.
- Only one export runs at a time, across rooms and Spaces.
- Logout, session lock, account switch and the session gate return to `Idle`
  and cancel the work; the manifest remains, so the export can be resumed.

The state carries counts and failure kinds only; room names are resolved for
display from existing room projections. No message content or file names go
into QA output or logs.

UI:

- Entry points: Room info "Download history" (existing), and a new
  "Download Space history" in the Space settings/info panel next to the
  access controls from #935.
- Start dialog: range, parent directory, number of target rooms and of
  skipped unjoined rooms, and warnings that the export takes time and disk
  space, writes encrypted messages and attachments as plaintext, and downloads
  every attachment. Choosing an existing export directory resumes it.
- Progress dialog: overall "N / M rooms completed" and a per-room status list
  with live counts. Closing it does not stop the export; Room/Space info shows
  a summary. Stop finishes the current step, deletes the `.partial` folder,
  keeps completed rooms, and regenerates the top-level `index.html`.

Errors:

- Attachments are fetched with the SDK media API without writing them into
  the SDK media cache, under the account work scheduler's background band,
  one file in memory at a time. Thumbnails honor EXIF orientation.
- One attachment: transient failures retry a few times with backoff, then the
  attachment is recorded as failed and the room continues.
- One room (history fetch or room write): the room becomes `Failed` and the
  export moves to the next room.
- Export directory unusable (disk full, permission): the whole export stops as
  `Failed`; completed rooms remain and can be resumed after fixing the cause.
- Completion shows failed room and attachment counts and offers
  "Retry failed", which resumes over the incomplete rooms.

## 4. Testing and delivery

Tests (reproduce behavior with a failing headless check first):

- Rust unit: state machine transitions and guards (stale `request_id`, stop,
  resume, session teardown); room selection (recursion, DM exclusion,
  unjoined); manifest round-trip and resume matching (scope/range mismatch,
  `.partial` cleanup); attachment map and sequence naming; thumbnailing
  (corrupt and non-reducible images); HTML golden tests covering math,
  replies, threads, reactions, edits, redactions, undecryptable events, and
  hostile HTML being sanitized.
- Rust integration: a fake filesystem and fake SDK source drive a full Space
  export, stop-then-resume, and continue-after-attachment-failure.
- Headless GUI (Playwright): start and progress dialogs (per-room status,
  overall count, stop, retry) and the dispatched commands.
- QA: a disposable local homeserver (tuwunel/synapse) exporting a Space that
  includes an encrypted room; artifacts record counts and states only.

Delivery: one pull request, built as ordered commits:

1. Core: pure HTML renderer and the shared sanitization extraction.
2. Core: attachment download, decryption, thumbnailing and `attachments.json`.
3. Core: folder layout, manifest and resume; single-room export switches to
   the archive format.
4. Core: Space export — room selection and the two-level state machine.
5. GUI: start and progress dialogs, the Space entry point, catalog strings.

## Security notes

- Decrypted messages and attachments are written as plaintext; the start
  dialog says so.
- Because `messages.json` keeps event content verbatim (as Element does),
  encrypted attachments' decryption keys (`content.file.key`) are already
  present in the JSON today. The archive keeps this behavior; the plaintext
  warning covers it.
- KaTeX (MIT) is vendored under `crates/koushi-core/assets/katex/` (the
  minified JS, CSS and woff2 fonts of the version `apps/desktop/package.json`
  pins) so the Rust-only gates build without `node_modules`. It gets a
  `THIRD_PARTY_NOTICES.md` entry, and a test keeps the vendored version equal
  to the npm dependency.
