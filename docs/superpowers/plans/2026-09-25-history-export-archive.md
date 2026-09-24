# History Export Archive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the JSON-only room-history download with a resumable archive
folder (HTML + `messages.json` + `events.jsonl` + attachments + thumbnails)
for one room or every joined non-DM room of a Space.

**Architecture:** Core runs a per-room three-stage pipeline (fetch → attachments
→ HTML) over a filesystem port, orchestrated room by room under a Rust-owned
two-level state machine (`AppState.history_export`). HTML is a pure function
of `events.jsonl` + `room.json` + `attachments.json`. The directory is a
native artifact; paths never enter commands, state, or logs.

**Tech Stack:** Rust (koushi-state, koushi-protocol, koushi-core, koushi-sdk,
matrix-sdk media API, `image` 0.25, `jiff` 0.2, ruma HTML sanitizer), Tauri 2
(`tauri-plugin-dialog` folder picker), React + vitest + Playwright, vendored
KaTeX 0.18.

**Spec:** `docs/superpowers/specs/2026-09-25-history-export-archive-design.md`

## Global Constraints

- One pull request; every commit leaves `cargo check --workspace` and
  `npm run typecheck` green.
- Rust owns every export transition, guard, room selection, resume decision,
  paging, download, thumbnailing and HTML generation. React renders
  `ui.history_export` and dispatches typed commands only.
- The export directory is `NativeArtifactKind::HistoryExportDirectory`. No
  path, file name, message body, or room name in commands, `AppState`, Debug
  output, diagnostics, or QA tokens.
- `koushi-state` and `koushi-protocol` stay filesystem-free (wasm check:
  `npm run qa:wasm-check`).
- `messages.json` bytes stay identical to today's Element-compatible output
  (the existing fixture test keeps passing unchanged).
- Attachments: `client.media().get_media_content(&request, false)` (no SDK
  cache write), under `AccountWorkKind::SearchCrawl` background permits, one
  file in memory at a time.
- Product text lives in `apps/desktop/src/i18n/messages.ts` (en + ja,
  complete). HTML strings reach Rust as `HistoryExportLabels` in the command.
- Room folder stem and attachment name stem: at most 120 characters.
- Rust test modules follow `scripts/check-rust-test-structure.mjs`: inline
  `mod tests` ≤ 200 lines, otherwise a sibling `#[cfg(test)] mod <name>_tests;`
  file (pattern: `room_history_export/sdk_source_tests.rs`). Any
  `include_bytes!`/`include_str!` of a non-Rust file is added to
  `ALLOWED_NON_RUST_TARGETS` in that script.
- Reproduce-first: every behavior task writes its failing test before code.

## Review Focus

1. A Space whose hierarchy contains a cycle (A ⊃ B ⊃ A) or lists the same
   room under two subspaces → each room exported once, no infinite loop.
   (Task 6 test `selection_visits_each_room_once_through_cycles`.)
2. Hostile `formatted_body` (`<script>`, `onerror=`, `<img src=mxc://…>`,
   `javascript:` links) and hostile file names (`../../x`, `CON`, 300 chars,
   `a/b\c:d`) → sanitized page, contained paths. (Task 3 and Task 2 tests.)
3. Stop pressed during attachment download of a room, then Start again on the
   same folder → completed rooms untouched, the interrupted room redone from
   scratch, no `.partial` left. (Task 7 test
   `stop_then_resume_redoes_only_the_interrupted_room`.)
4. The generated page opened over `file://` in Chromium under its CSP → math
   renders (`.katex` present), no console CSP violation. (Task 9 Playwright
   `history-export-page.spec.ts`.)
5. Phone photo with EXIF orientation 6 and a corrupt image → thumbnail
   rotated upright; corrupt image gets a link only, room still completes.
   (Task 4 tests.)

---

## File Structure

New Core modules (all under `crates/koushi-core/src/room_history_export/`):

| File | Responsibility |
| --- | --- |
| `fs.rs` | `HistoryExportFilesystem` port + `NativeHistoryExportFilesystem`; replaces `sink.rs` |
| `fs_fake.rs` (cfg(test)) | In-memory filesystem for tests |
| `layout.rs` | Folder/file naming: room folder stem + hash, attachment names, caps |
| `manifest.rs` | `koushi-export.json` schema, read/match/update |
| `attachments.rs` | Attachment extraction from event JSON, `AttachmentFetcher` port, retry, `attachments.json` |
| `thumbnail.rs` | Decode + EXIF orientation + resize to JPEG |
| `html/mod.rs`, `html/room_page.rs`, `html/index_page.rs`, `html/style.css`, `html/koushi-math.js` | Pure renderers |
| `room_meta.rs` | `room.json` (name, topic, sender display names, export metadata) |
| `space_selection.rs` | Recursive Space room selection over a `SpaceChildSource` port |
| `archive.rs` | Orchestrator: room loop, stop flag, manifest updates, index regeneration |
| `crates/koushi-core/assets/katex/` | Vendored KaTeX dist (min js/css, woff2 fonts, LICENSE, VERSION) |

Shared sanitizer: extract from `crates/koushi-core/src/timeline/item_projection.rs`
(around line 3164) into `crates/koushi-core/src/timeline/html_sanitize.rs`
as `pub(crate) fn sanitize_matrix_html(html: &str) -> Option<String>`.

State/protocol renames: `RoomHistoryExport*` → `HistoryExport*`,
`AppState.room_history_export` → `AppState.history_export`, file
`crates/koushi-state/src/state/room_history_export.rs` →
`state/history_export.rs`, reducer likewise, test
`crates/koushi-state/tests/room_history_export_state.rs` →
`tests/history_export_state.rs`.

---

### Task 1: Vendor KaTeX into Core

**Files:**
- Create: `crates/koushi-core/assets/katex/{katex.min.js,katex.min.css,LICENSE,VERSION}`, `crates/koushi-core/assets/katex/fonts/*.woff2`
- Create: `crates/koushi-core/src/room_history_export/katex_assets.rs`
- Create: `apps/desktop/src/i18n/katexVendor.test.ts` (version pin test; lives beside other vitest units)
- Modify: `THIRD_PARTY_NOTICES.md`, `scripts/check-rust-test-structure.mjs` (allowlist)

**Interfaces:**
- Produces: `pub(crate) struct AssetFile { pub path: &'static str, pub bytes: &'static [u8] }` and `pub(crate) fn katex_assets() -> &'static [AssetFile]` (paths relative to `assets/katex/`, e.g. `katex.min.js`, `fonts/KaTeX_Main-Regular.woff2`).

- [ ] **Step 1:** Copy from `apps/desktop/node_modules/katex/dist/`: `katex.min.js`, `katex.min.css`, all `fonts/*.woff2` (20 files, ~296 KB), and `node_modules/katex/LICENSE`. Write `VERSION` containing the exact installed version (`node -p "require('./apps/desktop/node_modules/katex/package.json').version"`). Strip `url(...woff)` and `url(...ttf)` fallbacks from the CSS `src:` lists with a sed that keeps only the woff2 entry, so the CSS references only shipped files.
- [ ] **Step 2: Failing test** `katexVendor.test.ts`:

```ts
import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
describe("vendored KaTeX", () => {
  test("matches the npm dependency version", () => {
    const vendored = readFileSync("../../crates/koushi-core/assets/katex/VERSION", "utf8").trim();
    const installed = JSON.parse(readFileSync("node_modules/katex/package.json", "utf8")).version;
    expect(vendored).toBe(installed);
  });
  test("CSS references only woff2 fonts that are vendored", () => {
    const css = readFileSync("../../crates/koushi-core/assets/katex/katex.min.css", "utf8");
    const urls = [...css.matchAll(/url\(([^)]+)\)/g)].map((m) => m[1].replace(/["']/g, ""));
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) {
      expect(url.endsWith(".woff2")).toBe(true);
      expect(() => readFileSync(`../../crates/koushi-core/assets/katex/${url}`)).not.toThrow();
    }
  });
});
```

Run: `cd apps/desktop && npx vitest run src/i18n/katexVendor.test.ts` → FAIL before files exist, PASS after Step 1.
- [ ] **Step 3:** `katex_assets.rs` with one `AssetFile { path, bytes: include_bytes!("../../assets/katex/<file>") }` per file, plus an inline test asserting every entry is non-empty and the list contains `katex.min.js`, `katex.min.css`, and 20 `.woff2` entries. Add every path to `ALLOWED_NON_RUST_TARGETS`. Run `cargo test -p koushi-core --lib katex_assets` and `node scripts/check-rust-test-structure.mjs`.
- [ ] **Step 4:** `THIRD_PARTY_NOTICES.md` entry (Project KaTeX, Repository https://github.com/KaTeX/KaTeX, Upstream `katex@<VERSION>` npm package, Source `dist/`, Local `crates/koushi-core/assets/katex`, License MIT, Copyright "Copyright (c) 2013-2020 Khan Academy and other contributors", Notes: copied into exported history folders).
- [ ] **Step 5:** Commit `feat(history-export): vendor KaTeX for exported pages`.

### Task 2: Filesystem port, layout, manifest

**Files:**
- Create: `room_history_export/fs.rs`, `fs_fake.rs`, `layout.rs`, `layout_tests.rs`, `manifest.rs`, `manifest_tests.rs`
- Modify: `room_history_export.rs` (mods/exports); keep `sink.rs` until Task 7 removes it

**Interfaces:**
- Produces:

```rust
pub trait HistoryExportFilesystem: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError>;
    fn read(&self, path: &Path) -> Result<Vec<u8>, HistoryExportFsError>;
    /// Staged file; bytes reach `path` only on commit, dropped = deleted.
    fn create_file(&self, path: &Path) -> Result<Box<dyn RoomHistoryExportFile>, HistoryExportFsError>;
    /// temp + fsync + rename.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), HistoryExportFsError>;
    fn rename(&self, from: &Path, to: &Path) -> Result<(), HistoryExportFsError>;
    fn remove_dir_all(&self, path: &Path) -> Result<(), HistoryExportFsError>;
    fn list_dir(&self, path: &Path) -> Result<Vec<String>, HistoryExportFsError>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HistoryExportFsError { #[error("not found")] NotFound, #[error("no space")] NoSpace, #[error("permission denied")] PermissionDenied, #[error("io failure")] Io }

pub(crate) fn room_folder_name(display_name: &str, room_id: &str) -> String; // "<stem ≤120> (<8 hex of sha256(room_id)>)"
pub(crate) fn partial_folder_name(final_name: &str) -> String;               // ".<final>.partial"
pub(crate) fn attachment_file_name(sequence: u32, original: Option<&str>, mimetype: Option<&str>) -> String; // "0001_<stem ≤120>.<ext>"
pub(crate) fn export_folder_name(stem: &str, civil_date: &str) -> String;    // "<stem ≤120> - Export YYYY-MM-DD"

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ExportManifest {
    pub format: String,            // "koushi-history-export"
    pub version: u32,              // 1
    pub scope: ManifestScope,      // { kind: "room"|"space", id }
    pub range: koushi_state::HistoryExportRange,
    pub title: String,             // Space or room display name at first run
    pub rooms: Vec<ManifestRoom>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ManifestRoom { pub room_id: String, pub folder: String, pub status: ManifestRoomStatus, pub counts: koushi_state::HistoryExportRoomCounts }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManifestRoomStatus { Pending, Completed, Skipped, Failed }
pub(crate) enum ManifestMatch { Fresh, Resume(ExportManifest), Mismatch }
pub(crate) fn match_manifest(existing: Option<&[u8]>, scope: &ManifestScope, range: &HistoryExportRange) -> ManifestMatch;
```

The 8-hex suffix is the first 8 characters of `sanitize_matrix_id_for_path(room_id)` (`timeline/item_projection.rs:3603`, already a path-safe hash of Matrix ids; widen it to `pub(crate)`). No new hash dependency.

- [ ] **Step 1: Failing tests** (`layout_tests.rs`):

```rust
#[test] fn room_folder_is_sanitized_capped_and_hash_suffixed() {
    let name = room_folder_name(&"a/b\\c:d*".repeat(40), "!room:example.org");
    assert!(!name.contains(['/', '\\', ':', '*']));
    let (stem, suffix) = name.rsplit_once(" (").unwrap();
    assert!(stem.chars().count() <= 120);
    assert_eq!(suffix.len(), 9); // 8 hex + ')'
    assert_eq!(name, room_folder_name(&"a/b\\c:d*".repeat(40), "!room:example.org"));
    assert_ne!(name, room_folder_name(&"a/b\\c:d*".repeat(40), "!other:example.org"));
}
#[test] fn attachment_names_are_sequenced_and_contained() {
    assert_eq!(attachment_file_name(1, Some("report.pdf"), None), "0001_report.pdf");
    let hostile = attachment_file_name(12, Some("../../etc/passwd"), None);
    assert!(!hostile.contains('/') && !hostile.contains(".."));
    assert!(attachment_file_name(3, None, Some("image/png")).ends_with(".png"));
    let long = attachment_file_name(4, Some(&format!("{}.jpg", "x".repeat(400))), None);
    assert!(long.chars().count() <= 5 + 120 + 4);
}
#[test] fn empty_or_dot_names_fall_back() {
    assert_eq!(room_folder_name("", "!r:x").split(" (").next().unwrap(), "room");
    assert_eq!(attachment_file_name(2, Some(".."), None), "0002_file");
}
```

`manifest_tests.rs`: `fresh_when_no_manifest`, `resume_when_scope_and_range_match` (round-trip through `serde_json`), `mismatch_on_other_space_or_range`, `mismatch_on_unknown_format_or_version`, `mismatch_on_corrupt_json`. Fake FS tests in `fs_fake.rs` inline (≤200 lines): staged file invisible until commit, dropped staged file leaves nothing, `write_atomic` replaces.
- [ ] **Step 2:** Run `cargo test -p koushi-core --lib room_history_export::layout room_history_export::manifest` → FAIL (unresolved).
- [ ] **Step 3:** Implement. Native FS: `create_file` reuses the current `NativeRoomHistoryExportFile` staging code (move it from `sink.rs`); `write_atomic` = `tempfile::NamedTempFile::new_in(parent)` + write + `sync_all` + `persist`; map `io::ErrorKind::StorageFull` → `NoSpace`, `PermissionDenied` → `PermissionDenied`, `NotFound` → `NotFound`, else `Io`. File naming reuses `crate::media_save::safe_media_save_filename` then trims leading dots/spaces, caps by chars, falls back to `room`/`file`.
- [ ] **Step 4:** Tests PASS; `node scripts/check-rust-test-structure.mjs` PASS.
- [ ] **Step 5:** Commit `feat(history-export): filesystem port, folder layout and resume manifest`.

### Task 3: Shared sanitizer and pure HTML renderers

**Files:**
- Create: `crates/koushi-core/src/timeline/html_sanitize.rs`; modify `timeline/item_projection.rs` (~3160-3175) and `timeline.rs` (mod)
- Create: `room_history_export/html/{mod.rs,room_page.rs,index_page.rs,labels.rs,style.css,koushi-math.js}`, `room_history_export/html/room_page_tests.rs`, `html/index_page_tests.rs`
- Create: `room_history_export/room_meta.rs`
- Modify: `crates/koushi-core/Cargo.toml` (`jiff = { workspace = true }` — add `jiff = "0.2"` to root `[workspace.dependencies]` and switch `apps/desktop/src-tauri/Cargo.toml` to `jiff.workspace = true`)
- Modify: `crates/koushi-protocol/src/command/account.rs` (add `HistoryExportLabels`, used by Task 7; defined here so the renderer compiles)

**Interfaces:**
- Consumes: `AttachmentRecord` (defined here, filled by Task 4):

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct AttachmentRecord {
    pub event_id: String, pub kind: AttachmentKind,        // Image|Sticker|Video|Audio|File
    pub name: String, pub size: Option<u64>, pub mimetype: Option<String>,
    pub file: Option<String>,   // "files/0001_x.pdf" when retrieved
    pub thumb: Option<String>,  // "thumbs/0001.jpg" when generated
    pub status: AttachmentStatus, // Retrieved | Failed
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct AttachmentIndex { pub attachments: Vec<AttachmentRecord> }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct RoomMeta { pub room_id: String, pub name: String, pub topic: String,
    pub senders: BTreeMap<String, String>, pub exported_at_ms: u64, pub time_zone: String }
```

- Produces:

```rust
// koushi-protocol
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryExportLabels {
    pub edited: String, pub in_reply_to: String /* "{name}" */, pub reply_unavailable: String,
    pub thread_reply: String, pub thread_root_link: String, pub redacted: String,
    pub undecryptable: String, pub not_retrieved: String, pub open_original: String,
    pub reactions: String, pub times_in_zone: String /* "{timeZone}" */,
    pub exported_at: String /* "{date}" */, pub index_title: String, pub range_all: String,
    pub range_period: String /* "{start}" "{end}" */, pub status_completed: String,
    pub status_skipped: String, pub status_failed: String, pub status_pending: String,
    pub events_count: String /* "{count}" */, pub attachments_count: String /* "{count}" */,
    pub failed_attachments_count: String /* "{count}" */, pub skipped_rooms_heading: String,
    pub state_joined: String, pub state_left: String, pub state_invited: String,
    pub state_removed: String, pub state_banned: String, pub state_renamed: String,
    pub state_topic: String, pub state_avatar: String, pub state_other: String,
    /* state_* use "{name}", "{target}", "{value}", "{type}" */
}
// renderer
pub(crate) fn render_room_page(events_jsonl: &[u8], meta: &RoomMeta, attachments: &AttachmentIndex,
    labels: &HistoryExportLabels) -> Result<Vec<u8>, RenderError>;
pub(crate) fn render_index_page(manifest: &ExportManifest, labels: &HistoryExportLabels,
    exported_at_ms: u64, time_zone: &str) -> Vec<u8>;
pub(crate) fn sanitize_matrix_html(html: &str) -> Option<String>; // timeline::html_sanitize
```

Rendering rules (spec §2): two passes over `events_jsonl` lines — pass 1 builds `HashMap<event_id, Index>` (reactions by key count from `m.reaction` `m.relates_to.rel_type = "m.annotation"`; latest `m.replace` content by `origin_server_ts`; thread root ids from `rel_type = "m.thread"`; reply excerpt = sender + first 80 chars of `body`); pass 2 writes one `<article id="e-<hex(event_id)>">` per displayable event, skipping `m.reaction` and `m.replace` events. Math: sanitized HTML keeps `data-mx-maths`; nothing else to do in Rust. Page head:

```html
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' file:; style-src 'self' file: 'unsafe-inline'; font-src 'self' file:; img-src 'self' file: data:">
<link rel="stylesheet" href="../../assets/katex/katex.min.css">
<script defer src="../../assets/katex/katex.min.js"></script>
<script defer src="../../assets/koushi-math.js"></script>
```

`koushi-math.js` (asset file, written by Task 7): on `DOMContentLoaded`, for each `[data-mx-maths]`, `katex.render(el.dataset.mxMaths, el, { displayMode: el.tagName === "DIV", throwOnError: false, maxSize: 20, maxExpand: 1000 })` skipping sources longer than 1024 chars (same limit as `TimelineMessageBody.tsx`; read its constants and copy them). Times: `jiff` `Timestamp::from_millisecond(ts).to_zoned(tz)` formatted `%Y-%m-%d` / `%H:%M`; unknown zone → UTC. Every interpolated string is HTML-escaped.

- [ ] **Step 1: Failing tests** (`room_page_tests.rs`), each building `events.jsonl` from `serde_json::json!` lines:
  - `plain_body_is_escaped_with_line_breaks` (`"a<b>\nc"` → `a&lt;b&gt;<br>c`).
  - `hostile_formatted_body_is_sanitized` — input `<script>x()</script><img src="mxc://h/m" onerror="y()"><a href="javascript:z()">l</a><b>ok</b>`; assert output contains `<b>ok</b>` and contains none of `<script`, `onerror`, `mxc://`, `javascript:`.
  - `math_source_survives` — `<span data-mx-maths="E=mc^2">E=mc^2</span>` → output contains `data-mx-maths="E=mc^2"`; CSP meta and both script tags present; no inline `<script>` without `src`.
  - `reactions_are_counted_under_their_target` — two `👍` and one `🎉` annotations → `👍 2`, `🎉 1`; reaction events produce no article.
  - `latest_edit_is_shown_and_marked` — original "v1", two `m.replace` with `m.new_content` "v2" then "v3" → shows `v3` and the `edited` label, not `v1`.
  - `reply_links_to_target_or_marks_unavailable`.
  - `thread_reply_links_to_root`.
  - `redacted_and_undecryptable_placeholders` (`m.bad.encrypted` msgtype and `unsigned.redacted_because`).
  - `image_with_thumb_links_original`, `failed_attachment_shows_not_retrieved`.
  - `member_and_name_state_lines` using `room.json` sender names.
  - `date_separator_and_zone_note` in `Asia/Tokyo` (ts crossing midnight JST).
  `index_page_tests.rs`: lists completed/skipped/failed rooms with counts; skipped rooms have no link; room links are percent-encoded relative paths `rooms/<folder>/index.html`.
  Sanitizer: move the existing timeline sanitizer tests if any reference the inline code; add `sanitize_matrix_html_is_shared_by_timeline` asserting `item_projection` calls it (behavioral: an existing timeline projection test still passes).
- [ ] **Step 2:** `cargo test -p koushi-core --lib room_history_export::html` → FAIL.
- [ ] **Step 3:** Implement. Extract the sanitizer body verbatim from `item_projection.rs` into `sanitize_matrix_html`, call it from the old site. Renderer uses a `String` builder with an `escape(&str)` helper; no template engine.
- [ ] **Step 4:** Tests PASS, then `cargo test -p koushi-core --lib timeline` PASS (sanitizer move is behavior-neutral).
- [ ] **Step 5:** Commit `feat(history-export): pure room and index page renderers`.

### Task 4: Attachments and thumbnails

**Files:**
- Create: `room_history_export/attachments.rs`, `attachments_tests.rs`, `thumbnail.rs`, `thumbnail_tests.rs`
- Test fixtures: generate images in tests with `image` (no binary fixtures)

**Interfaces:**
- Consumes: `AttachmentRecord`, `AttachmentIndex` (Task 3), `attachment_file_name` (Task 2), `HistoryExportFilesystem`.
- Produces:

```rust
pub(crate) struct AttachmentRef { pub event_id: String, pub kind: AttachmentKind, pub name: String,
    pub size: Option<u64>, pub mimetype: Option<String>, pub source: matrix_sdk::ruma::events::room::MediaSource }
/// From one effective event JSON: m.room.message (m.image/m.file/m.video/m.audio) and m.sticker,
/// using the latest bundled or jsonl-seen edit is NOT applied (originals only).
pub(crate) fn attachment_ref(event: &serde_json::Value) -> Option<AttachmentRef>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub(crate) enum FetchError { Transient, Permanent }
pub(crate) trait AttachmentFetcher: Send + Sync {
    fn fetch(&self, source: &MediaSource) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}
pub(crate) struct SdkAttachmentFetcher { client: matrix_sdk::Client, account_work: AccountWorkScheduler }
pub(crate) async fn download_attachments<F: AttachmentFetcher>(fetcher: &F, fs: &dyn HistoryExportFilesystem,
    room_dir: &Path, refs: Vec<AttachmentRef>, stop: &StopFlag,
    on_progress: impl FnMut(u64 /*done*/, u64 /*total*/, u64 /*failed*/)) -> Result<AttachmentIndex, ArchiveStepError>;
pub(crate) fn thumbnail_jpeg(bytes: &[u8], max_edge: u32) -> Option<Vec<u8>>; // None = not reducible
pub(crate) const THUMB_MAX_EDGE: u32 = 480;
pub(crate) const FETCH_ATTEMPTS: u32 = 3; // backoff 1s, 4s
```

`StopFlag` = `Arc<AtomicBool>` newtype defined in `archive.rs` (Task 7) — define it here in `attachments.rs` as `pub(crate) struct StopFlag(Arc<AtomicBool>)` with `is_set()`/`set()` and re-export from `archive.rs`. `ArchiveStepError { Stopped, Write(HistoryExportFsError) }`.

`SdkAttachmentFetcher::fetch` = `background(&account_work, || client.media().get_media_content(&MediaRequestParameters { source: source.clone(), format: MediaFormat::File }, false))` with `MEDIA_DOWNLOAD_TIMEOUT` from `timeline/media.rs` (make it `pub(crate)`); classify with the existing `classify_media_download_error` — `Network`/timeout → `Transient`, else `Permanent`. Move `background()` from `sdk_source.rs` to `room_history_export.rs` as `pub(super)` so both use it.

Thumbnail: `image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()`, `into_decoder()`, read `decoder.orientation()`, `DynamicImage::from_decoder`, `apply_orientation`, `thumbnail(max,max)` only when larger, encode JPEG quality 80. Set `image::Limits { max_alloc: Some(512 << 20), .. }`. GIF/SVG/unknown or decode error → `None`.

- [ ] **Step 1: Failing tests**:
  - `attachment_ref_reads_plain_and_encrypted_sources` (`url` and `file` with `v2` EncryptedFile JSON; name from `filename` then `body`).
  - `attachment_ref_ignores_text_and_redacted`.
  - `download_retries_transient_then_records_failure` — fake fetcher failing `Transient` 3× → record `Failed`, no file written, `failed == 1`; tokio `start_paused = true` to skip backoff sleeps.
  - `download_writes_sequenced_files_and_thumbs` — two images and a pdf → `files/0001_a.png`, `thumbs/0001.jpg`, `files/0003_c.pdf`, no thumb for pdf.
  - `corrupt_image_keeps_file_without_thumb`.
  - `stop_flag_interrupts_between_files` → `Err(Stopped)` after the first file.
  - `thumbnail_applies_exif_orientation` — build a 40×20 JPEG, inject an APP1 Exif segment with Orientation=6 (hand-built 30-byte TIFF header in the test), assert thumbnail is 20×40.
  - `thumbnail_skips_small_images_resize_but_reencodes` (10×10 → 10×10 JPEG).
- [ ] **Step 2:** Run `cargo test -p koushi-core --lib room_history_export::attachments room_history_export::thumbnail` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(history-export): download attachments and build thumbnails`.

### Task 5: Fetch stage writes `messages.json`, `events.jsonl`, `room.json`

**Files:**
- Modify: `room_history_export/driver.rs`, `room_history_export/tests.rs`, `room_history_export/sdk_source.rs` (header + sender names), `room_meta.rs`

**Interfaces:**
- Produces:

```rust
pub(crate) struct FetchOutputs { pub messages: Box<dyn RoomHistoryExportFile>, pub events: Box<dyn RoomHistoryExportFile> }
pub(crate) struct FetchResult { pub senders: BTreeSet<String>, pub attachments: Vec<AttachmentRef> }
pub(crate) async fn run_fetch<S: HistoryPageSource, P: AsyncProgress>(source: &mut S, outputs: FetchOutputs,
    header: &ExportHeader, range: &HistoryExportRange, own_user_id: &str,
    counters: &Arc<ExportCounters>, stop: &StopFlag, on_page: P) -> Result<FetchResult, HistoryExportRoomFailureKind>;
```

`run_export` becomes `run_fetch`: the in-range test happens before the Element filter; every in-range distinct event is appended to `events` as `serde_json::to_vec(&event.json)` + `\n`; Element-rendered ones still go to `messages` byte-identically; `attachment_ref` is collected in walk order from in-range events that Element renders; senders collected from every in-range event. `stop.is_set()` checked after each page → `Err(Stopped)` (new failure kind variant consumed only internally; see Task 7). Both files commit at the end.

- [ ] **Step 1: Failing tests** in `tests.rs`: `events_jsonl_keeps_reactions_and_edits_messages_json_unchanged` (reuse the existing fixture source; assert `messages` bytes equal `element_expected.json` as the existing test does, and `events` has one line per in-range distinct event including `m.reaction`); `fetch_collects_attachments_in_order`; `fetch_stops_between_pages`. Update existing tests to the new signature (they keep their assertions).
- [ ] **Step 2:** `cargo test -p koushi-core --lib room_history_export::tests` → FAIL.
- [ ] **Step 3:** Implement; `room_meta` built after fetch by resolving each sender with `room.get_member_no_sync(user_id)` display name (fallback: user id) in a `pub(crate) async fn room_meta(room: &Room, senders, now_ms, time_zone) -> RoomMeta`.
- [ ] **Step 4:** PASS, including `cargo test -p koushi-core --lib room_history_export` (whole module).
- [ ] **Step 5:** Commit `feat(history-export): write the lossless events.jsonl beside messages.json`.

### Task 6: Space room selection

**Files:**
- Create: `room_history_export/space_selection.rs`, `space_selection_tests.rs`
- Modify: `crates/koushi-sdk/src/room_projection.rs` (extract `pub async fn matrix_room_is_dm(room: &matrix_sdk::Room, direct_targets_by_room: Option<&MatrixDirectTargetsByRoom>) -> bool` from lines ~2228-2237 and call it there), `crates/koushi-sdk/src/lib.rs` (export)

**Interfaces:**
- Produces:

```rust
pub(crate) struct ChildEntry { pub room_id: String, pub display_name: String, pub joined: bool, pub is_space: bool }
pub(crate) trait SpaceChildSource: Send {
    fn children(&mut self, space_id: &str) -> impl Future<Output = Result<Vec<ChildEntry>, SelectionError>> + Send;
    fn is_dm(&mut self, room_id: &str) -> impl Future<Output = bool> + Send;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectedRoom { pub room_id: String, pub display_name: String, pub target: bool } // target=false → Skipped(NotJoined)
pub(crate) async fn select_space_rooms<S: SpaceChildSource>(source: &mut S, space_id: &str) -> Result<Vec<SelectedRoom>, SelectionError>;
pub(crate) struct SdkSpaceChildSource { session: Arc<MatrixClientSession> }
```

Rules: BFS from `space_id` with a `visited` set; child `is_space && joined` → enqueue (not listed); child `is_space && !joined` → listed as skipped; room `joined` → listed as target unless `is_dm`; room `!joined` → skipped; a room reached twice is listed once (first occurrence order). Unjoined subspaces are not traversed. `SdkSpaceChildSource::children` maps `koushi_sdk::matrix_space_children_projection` entries (`membership == Joined`); `is_dm` gets `client.get_room(id)` and calls `matrix_room_is_dm(&room, None)`.

- [ ] **Step 1: Failing tests**: `selection_recurses_joined_subspaces_and_skips_unjoined`, `selection_excludes_dms`, `selection_visits_each_room_once_through_cycles` (A→B→A, R listed under A and B), `selection_error_on_root_failure` (root `children` error → `Err`), `subspace_failure_marks_nothing_and_continues` (a subspace `children` error skips that subspace only).
- [ ] **Step 2:** `cargo test -p koushi-core --lib room_history_export::space_selection` → FAIL.
- [ ] **Step 3:** Implement; `cargo test -p koushi-sdk --lib` still PASS after extraction.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(history-export): select a Space's joined non-DM rooms recursively`.

### Task 7: State machine, protocol, actor, orchestrator (the switch)

**Files:**
- Rename/modify (koushi-state): `state/room_history_export.rs` → `state/history_export.rs`; `reducer/room_history_export.rs` → `reducer/history_export.rs`; `state/mod.rs`, `lib.rs`, `action.rs`, `effect.rs` (`UiEvent::HistoryExportChanged`), `reducer/mod.rs` (dispatch + session reset at ~1874/1906/1994); `tests/room_history_export_state.rs` → `tests/history_export_state.rs`
- Modify (koushi-protocol): `command/account.rs` (`HistoryExportRequest`, `ExportHistory`, `StopHistoryExport`, `RetryHistoryExport`), `command.rs`/re-exports, `state_update.rs`
- Modify (koushi-core): `native_artifact.rs` (`HistoryExportDirectory` replaces `RoomHistoryExportDestination`), `command_policy.rs`, `runtime.rs` (~2187, ~2212, ~4784, ~4948), `account/actor.rs` (message, fields ~1003/1285, routing ~1632/2622/2640), `account/history_export.rs` (rewrite), `account/runtime_children.rs`, `account/local_data_cleanup/tests.rs`, `state_delta.rs`, `lib.rs`; create `room_history_export/archive.rs`, `archive_tests.rs`; delete `room_history_export/sink.rs`; `tests/native_artifact_boundary.rs`
- Modify: `apps/desktop/src-tauri/src/dto.rs` (+ `dto/tests.rs`, golden `tests/golden/frontend_app_state.json`) — rename field to `history_export` so the workspace compiles; Tauri commands are reworked in Task 8
- Modify: `docs/architecture/state-machine.md` (rewrite section "Room History Export" → "History Export")

**Interfaces:**

```rust
// koushi-state
pub enum HistoryExportScope { Room { room_id: String }, Space { space_id: String } } // serde tag "kind", camelCase; Debug redacts ids
pub type HistoryExportRange = /* renamed RoomHistoryExportRange, unchanged */;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryExportRoomCounts { pub fetched_events: u64, pub exported_events: u64, pub undecryptable_events: u64,
    pub attachments_total: u64, pub attachments_done: u64, pub attachments_failed: u64 }
pub enum HistoryExportRoomPhase { Pending, Fetching, Attachments, Rendering, Completed, Skipped, Failed } // serde camelCase
pub enum HistoryExportRoomSkipReason { NotJoined }
pub enum HistoryExportRoomFailureKind { Network, Sdk, Write, Stopped }
pub struct HistoryExportRoom { pub room_id: String, pub display_name: String, pub phase: HistoryExportRoomPhase,
    pub counts: HistoryExportRoomCounts, pub skip_reason: Option<HistoryExportRoomSkipReason>,
    pub failure_kind: Option<HistoryExportRoomFailureKind> } // Debug redacts room_id and display_name
pub enum HistoryExportFailureKind { InvalidRange, RoomNotFound, SpaceNotFound, DestinationUnavailable,
    ManifestMismatch, Write, NoSpace, Network, Sdk }
pub enum HistoryExportState {
    #[default] Idle,
    Preparing { request_id: u64, scope: HistoryExportScope, range: HistoryExportRange, stop_requested: bool },
    Running   { request_id: u64, scope: HistoryExportScope, range: HistoryExportRange, rooms: Vec<HistoryExportRoom>, stop_requested: bool },
    Completed { request_id: u64, scope: HistoryExportScope, range: HistoryExportRange, rooms: Vec<HistoryExportRoom> },
    Stopped   { request_id: u64, scope: HistoryExportScope, range: HistoryExportRange, rooms: Vec<HistoryExportRoom> },
    Failed    { request_id: u64, scope: HistoryExportScope, range: HistoryExportRange, rooms: Vec<HistoryExportRoom>, failure_kind: HistoryExportFailureKind },
}
// actions (AppAction)
HistoryExportRequested { request_id, scope, range }            // admission: Idle|terminal → Preparing
HistoryExportRetryRequested { request_id, target_request_id }  // terminal with target id → Preparing (same scope/range)
HistoryExportPrepared { request_id, rooms: Vec<HistoryExportRoom> }   // Preparing → Running
HistoryExportRoomProgressed { request_id, room_id, phase, counts }    // Running only, room must exist, phase monotone
HistoryExportRoomSettled { request_id, room_id, phase /*Completed|Skipped|Failed*/, counts, failure_kind: Option<..> }
HistoryExportStopRequested { request_id }                      // Preparing|Running, once
HistoryExportCompleted { request_id } | HistoryExportStopped { request_id } | HistoryExportFailed { request_id, kind }
// protocol
pub struct HistoryExportRequest { pub scope: HistoryExportScope, pub range: HistoryExportRange,
    pub display_time_zone: String, pub export_date_utc_offset_minutes: i32, pub labels: HistoryExportLabels }
AccountCommand::ExportHistory { request_id, request }            // native artifact HistoryExportDirectory
AccountCommand::StopHistoryExport { request_id, target_request_id }
AccountCommand::RetryHistoryExport { request_id, target_request_id }
```

Guards (write them into state-machine.md): start requires Ready session, valid range, no export in `Preparing|Running`, and for `Room` scope a known room, for `Space` scope a known Space; stale `request_id` dropped everywhere; `RoomProgressed` ignored for unknown room or a phase that moves backward; Retry requires terminal state whose `request_id == target_request_id`; logout/lock/switch/session gate → `Idle` (the actor aborts the task; the manifest remains).

Orchestrator (`archive.rs`):

```rust
pub(crate) struct ArchiveContext<'a, F, M, S> { fs: &'a dyn HistoryExportFilesystem, fetcher: &'a F,
    rooms: &'a mut M /* RoomPipelineSource: per-room page source + header + room_meta */, selection: &'a mut S,
    labels: &'a HistoryExportLabels, stop: StopFlag, reporter: &'a dyn ArchiveReporter }
pub(crate) enum ExportTarget { Fresh { parent: PathBuf, folder_name: String }, Existing { dir: PathBuf } }
pub(crate) async fn run_archive(ctx, target: ExportTarget, scope, range, time_zone, now_ms) -> Result<ArchiveOutcome, HistoryExportFailureKind>;
pub(crate) enum ArchiveOutcome { Completed, Stopped }
```

Flow: resolve target (`dir/koushi-export.json` exists → `Existing`; `match_manifest` → `Mismatch` ⇒ `Err(ManifestMismatch)`); select rooms (room scope → one target; space scope → `select_space_rooms`); merge with manifest (completed rooms keep `Completed` and are skipped; new rooms appended `Pending`); write assets (`assets/katex/*`, `assets/koushi-math.js`) if missing; `reporter.prepared(rooms)`; delete every `rooms/.*.partial`; for each target room not completed: create `.partial`, fetch (Task 5) → `room.json` → attachments (Task 4) → HTML (Task 3) → `attachments.json`, `index.html` → `rename(.partial, final)` (if `final` exists from an older failed run, `remove_dir_all` it first) → manifest `Completed` → `write_atomic(manifest)` → `reporter.room_settled`. A room fetch `Network|Sdk` failure → remove `.partial`, manifest `Failed`, continue. `HistoryExportFsError::{NoSpace,PermissionDenied}` anywhere → regenerate index best-effort and `Err(NoSpace|Write)`. `stop.is_set()` → remove current `.partial`, write manifest + index, `Ok(Stopped)`. Always regenerate top-level `index.html` at the end.

Actor (`account/history_export.rs`): `ActiveHistoryExport { request_id, stop: StopFlag, settlement, task, export_dir: Arc<Mutex<Option<PathBuf>>> }`; after settlement keep `last_export: Option<(RequestId, PathBuf, HistoryExportRequest)>` for Retry. Stop command → `stop.set()` (cooperative); teardown → `task.abort()` + settle `Stopped` from recorded settlement like today. Room-scope start with a registered dir lacking a manifest → `ExportTarget::Fresh { parent: dir, folder_name: export_folder_name(<room name>, <local date from export_date_utc_offset_minutes>) }`.

- [ ] **Step 1: Failing state tests** (`tests/history_export_state.rs`, port every existing test to the new names, then add): `space_start_requires_known_space`, `prepared_moves_to_running_with_rooms`, `room_progress_ignores_unknown_room_and_backward_phase`, `stop_accepted_once_in_preparing_and_running`, `retry_requires_matching_terminal_request`, `session_teardown_resets_to_idle`, `debug_redacts_room_ids_and_names`. Run `cargo test -p koushi-state --test history_export_state` → FAIL.
- [ ] **Step 2:** Implement state/actions/reducer + renames across the workspace (compiler-driven: `cargo check --workspace`; also `rg -n "room_history_export|RoomHistoryExport" crates apps/desktop/src-tauri` must only show QA/test names intentionally kept until Task 10). PASS state tests; `npm run qa:wasm-check` PASS.
- [ ] **Step 3: Failing orchestrator tests** (`archive_tests.rs`, fake FS + fake page source + fake fetcher + fake selection + recording reporter): `room_export_writes_full_layout` (asserts all files of spec §1 exist and `koushi-export.json` has one `completed` room), `space_export_skips_unjoined_and_lists_them_in_index`, `stop_then_resume_redoes_only_the_interrupted_room`, `room_fetch_failure_continues_with_next_room`, `no_space_fails_whole_export_and_keeps_completed_rooms`, `manifest_mismatch_refuses_resume`, `stale_partial_folders_are_removed_on_start`, `new_space_room_is_added_on_resume`, `retry_after_failure_redoes_failed_rooms_only`. Run `cargo test -p koushi-core --lib room_history_export::archive` → FAIL.
- [ ] **Step 4:** Implement `archive.rs`, rewire actor/runtime/command policy, delete `sink.rs`, update `tests/native_artifact_boundary.rs` (kind rename, Stop/Retry correlation). PASS: `cargo test -p koushi-core --lib room_history_export`, `cargo test -p koushi-core --test native_artifact_boundary`, `cargo test -p koushi-core --lib account`.
- [ ] **Step 5:** DTO rename in `dto.rs`, update `dto/tests.rs` and regenerate the golden per the instructions in `docs/agents/state-ownership.md` (Snapshot and wire contract mirrors). `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib` PASS (Tauri command bodies may temporarily call the new command with a Room scope; Task 8 finishes them).
- [ ] **Step 6:** Rewrite `docs/architecture/state-machine.md` section with the mermaid diagram for both levels and the guard list above.
- [ ] **Step 7:** Commit `feat(history-export): Rust-owned archive export state machine and orchestrator`.

### Task 8: Tauri adapter

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/history_export.rs`, `commands/mod.rs`, `lib.rs` (handler list), `capabilities/*` only if the folder picker needs a permission (check `dialog:allow-open` presence)

**Interfaces:**
- Produces Tauri commands (camelCase args from JS):
  - `history_export_time_zone() -> String` (renamed from `room_history_export_time_zone`)
  - `export_history(scope: HistoryExportScopeInput, range: RangeInput, labels: HistoryExportLabels, dialogTitle: String, folderNameStem: String) -> FrontendHistoryExportStart` — opens `app.dialog().file().pick_folder(...)` (title, `set_can_create_directories(true)`, default Downloads). QA: `KOUSHI_QA_HISTORY_EXPORT_DIR` is used as the chosen directory. The adapter does **not** compute the folder name itself; it passes `folderNameStem` inside the request (`HistoryExportRequest` gains `folder_name_stem: String`, sanitized in Core with `export_folder_name`).
  - `stop_history_export(targetRequestId: u64) -> FrontendCommandAdmission`
  - `retry_history_export(targetRequestId: u64) -> FrontendCommandAdmission`
- Keep: range resolution (`resolve_range`), `platform_time_zone_name` for `display_time_zone`.

- [ ] **Step 1: Failing unit tests** in the existing test module: command wire shapes for `ExportHistory` (Room and Space scope, labels pass-through), `StopHistoryExport`/`RetryHistoryExport` correlation on the adapter connection, Debug of the built command shows no ids/labels paths. Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib history_export` → FAIL.
- [ ] **Step 2:** Implement; add `folder_name_stem` to `HistoryExportRequest` (protocol) and use it in the actor for `Fresh` targets.
- [ ] **Step 3:** PASS; `npm run lint:tauri-boundary` PASS.
- [ ] **Step 4:** Commit `feat(history-export): folder picker and stop/retry commands in the Tauri adapter`.

### Task 9: React UI, catalog, browser tests

**Files:**
- Rename/modify: `apps/desktop/src/components/RoomHistoryExportDialog.tsx` → `HistoryExportDialog.tsx` (+ `.test.tsx`), `RoomInfoPanel.tsx`, `rightPanel.tsx`, `SpaceAccessSection.tsx`'s parent Space info panel (find with `rg -n "SpaceAccessSection" apps/desktop/src/components`), `App.tsx`, `styles.css`
- Modify: `apps/desktop/src/domain/types.ts` (mirror Rust types exactly), `backend/desktopApi.ts`, `backend/client.ts`, `backend/desktopApi.contract.test.ts`, `test/tauriIpcMock.ts`, `test/appHarnessMain.tsx`, `domain/appStore.test.ts`, `components/TimelinePane.renderIsolation.test.tsx`, `domain/rightPanel.test.ts`
- Modify: `apps/desktop/src/i18n/messages.ts` (en + ja): replace `roomHistoryExport.*` with `historyExport.*` dialog keys and add `historyExport.page.*` keys for every `HistoryExportLabels` field; `historyExport.attachments` now says every attachment is downloaded
- Create: `apps/desktop/src/domain/historyExportLabels.ts` (`export function historyExportLabels(t): HistoryExportLabels` — one place mapping catalog → labels)
- Rename/modify: `apps/desktop/e2e/room-history-export.spec.ts` → `e2e/history-export.spec.ts`; create `e2e/history-export-page.spec.ts`
- Create: `apps/desktop/e2e/fixtures/history-export-page/` generated by a Rust test (Step 4), never hand-edited

**Behavior:**
- Start dialog (both scopes): range (existing controls), warnings (time/disk, plaintext incl. attachments, all attachments downloaded), target counts for Space scope come from Rust only after `Preparing` → `Running` (the dialog shows "Preparing…" until then; no React-side counting), Save → `exportHistory(...)`.
- Progress view: `"{done} / {total} rooms completed"` from `rooms` (done = Completed+Skipped+Failed; total = rooms.length), per-room rows with phase, counts, and failure kind; Stop button (disabled once `stop_requested`); terminal: summary + "Retry failed" when any room `Failed` or state `Stopped|Failed`.
- Room info keeps its section (label "Download history"); Space info gains "Download Space history" section.
- The dialog tracks its own request id exactly like today's (`RoomHistoryExportDialog.tsx`) so earlier exports' settlements are not shown as this one's.

- [ ] **Step 1: Failing vitest** (`HistoryExportDialog.test.tsx`, port all existing cases, then add): Space scope submission sends `{kind:"space", spaceId}` with labels from the catalog (assert a couple of label values in en and ja); progress list renders per-room phases and `3 / 5` count; Stop disabled after `stop_requested`; Retry shown after a failed room and calls `retryHistoryExport(requestId)`; skipped rooms show the not-joined reason. Run `cd apps/desktop && npx vitest run src/components/HistoryExportDialog.test.tsx` → FAIL.
- [ ] **Step 2:** Implement types, API, dialog, panels, catalog. `npx vitest run` (all) and `npm run typecheck` and `npm run lint` PASS (lint includes IME and catalog checks).
- [ ] **Step 3: Playwright** `history-export.spec.ts`: port the existing spec to the new commands and state shape; add a Space flow driving `Preparing → Running(rooms) → per-room progress → Completed`, Stop, and Retry via the IPC mock. Run `npx playwright test e2e/history-export.spec.ts --workers=1` PASS.
- [ ] **Step 4: Committed page fixture.** CI's browser job has no Rust toolchain, so the generated page is committed. Add Rust test `archive_tests::browser_fixture_matches_renderer`: it runs a room export through the native filesystem into a temp dir (one message with inline math `<span data-mx-maths="a^2">`, one with display math `<div data-mx-maths="\\int_0^1 x\\,dx">`, one small PNG image attachment via the fake fetcher, fixed `now_ms` and `Asia/Tokyo`), then compares every file byte-for-byte with `apps/desktop/e2e/fixtures/history-export-page/`. With env `KOUSHI_UPDATE_HISTORY_EXPORT_FIXTURE=1` it rewrites the fixture instead. Add the fixture directory to `ALLOWED_NON_RUST_TARGETS` only if the test uses `include_*` (prefer `std::fs` reads from `CARGO_MANIFEST_DIR`, which needs no allowlist). Generate: `KOUSHI_UPDATE_HISTORY_EXPORT_FIXTURE=1 cargo test -p koushi-core --lib browser_fixture_matches_renderer`, then run it again without the env var → PASS.
- [ ] **Step 5: Failing Playwright** `history-export-page.spec.ts`: `page.goto(pathToFileURL(<fixture>/rooms/<folder>/index.html).href)`, collect `console` messages and `pageerror`, assert `.katex` count ≥ 2 (one inside a display block), the thumbnail `img` has `naturalWidth > 0`, and no console message mentions "Content Security Policy". Run `npx playwright test e2e/history-export-page.spec.ts --workers=1` → FAIL if the bootstrap or CSP is wrong, PASS once correct. If Chromium rejects `'self'` for `file:` pages, adjust the CSP in the renderer (keep `default-src 'none'` and no inline script), regenerate the fixture, and rerun.
- [ ] **Step 6:** Commit `feat(history-export): Space and room download dialogs with per-room progress`.

### Task 10: Headless core QA and Linux GUI lane

**Files:**
- Modify: `crates/koushi-qa/src/bin/headless_core_qa/scenarios/history_export.rs`, `registry.rs`, `registry_tests.rs`, `orchestrator.rs` (if it names the scenario), `scripts/lib/qa-token-contract.mjs`, `scripts/desktop-headless-local-qa.mjs`, `scripts/desktop-linux-gui-qa/scenarios/rooms-timeline.mjs`, `scripts/desktop-linux-gui-qa/registry.mjs`, `apps/desktop/src/scripts/linuxGuiQa.test.ts`, `scripts/desktop-release-gate-check.mjs`, `docs/qa/headless-basic-operations.md`

- [ ] **Step 1:** Headless core scenario (keep scenario name `room_history_export` to avoid lane churn; add Space coverage): seed a Space with two joined rooms (one encrypted with an image upload), one room B has not joined, and a DM added as a Space child. Export the Space into a temp dir via the native artifact registry; assert from `AppStateSnapshot`: `Completed`, rooms = 2 targets + 1 skipped, DM absent; assert on disk: `koushi-export.json` statuses, each room has `messages.json` (parse, Element top-level keys), `events.jsonl`, `index.html`, and the encrypted room's `files/0001_*` bytes equal the uploaded bytes and `thumbs/0001.jpg` decodes. Then Stop mid-run (large synthetic history) and Start again → resume completes with the first room untouched (compare mtime/inode or a sentinel). New tokens: `history_export_space=ok`, `history_export_attachments=ok`, `history_export_resume=ok`; keep `history_export_full`, `history_export_period`, `history_export_utd_counted`, `history_export_cancel` (rename its meaning to Stop), `room_history_export=ok`. Register tokens in `qa-token-contract.mjs`. Run `cargo test -p koushi-qa --features qa-bin --bin headless-core-qa` PASS, then `cd apps/desktop && npm run qa:headless-local -- --server=tuwunel --core --scenario=room_history_export` PASS.
- [ ] **Step 2:** Linux GUI scenario `local-room-history-export`: point at the folder output (`rooms/*/messages.json`, `index.html` exists), keep its three checks; update `linuxGuiQa.test.ts`. Run `npx vitest run src/scripts/linuxGuiQa.test.ts` PASS; run the lane `npm run qa:linux-gui -- --scenario=local-room-history-export --server=tuwunel` once at the end (Task 11).
- [ ] **Step 3:** Commit `test(history-export): headless core and Linux GUI coverage for archive exports`.

### Task 11: Docs, gates, PR

**Files:**
- Modify: `docs/agents/state-ownership.md`, `docs/architecture/overview.md` (port list: `HistoryExportFilesystem`, `AttachmentFetcher`), `docs/agents/qa-lanes.md`, `docs/help/rooms-and-spaces.md` ("Download room history" → room + Space, folder contents, resume), `docs/help/security-and-recovery.md` (plaintext attachments), `docs/agents/plans.md` (link this plan and the spec)

- [ ] **Step 1:** Update docs; `node scripts/check-agents-docs.mjs` and `git diff --check` PASS.
- [ ] **Step 2: Local gates** (each with `> log 2>&1; echo EXIT=$?`):
  - `cargo test -p koushi-state`, `cargo test -p koushi-core`, `cargo test -p koushi-sdk --lib`, `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `cargo test -p koushi-qa --features qa-bin --bin headless-core-qa`
  - `cd apps/desktop && npm run typecheck && npx vitest run && npm run lint && npm run lint:tauri-boundary && npm run lint:domain-deps && npm run lint:rust-test-structure && npm run qa:wasm-check && npx playwright test`
  - `npm run qa:headless-local -- --server=both` (merge gate), plus the Linux GUI lane from Task 10.
- [ ] **Step 3:** Preflight checklist `~/.agents/skills/preflight-review/SKILL.md`; fix findings. Whole-branch review (`/code-review high`); fix confirmed findings.
- [ ] **Step 4:** Push, open the PR (body: summary, spec/plan links, gates with EXIT codes, closes nothing — reference #59), then `babysit-pr` until merged.
