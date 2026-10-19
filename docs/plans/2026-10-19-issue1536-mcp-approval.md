# #1536 - Bundled MCP launcher and in-app approval

Status: implementation plan. Canon: `docs/architecture/overview.md` "External
MCP Bridge"; `docs/architecture/state-machine.md` "Settings" and "Desktop
Application Updates" (the process-wide lifecycle this follows);
`docs/agents/state-ownership.md` "The boundary"; engineering rules "Product
Text And Localization". Predecessors: [#1520](2026-10-18-issue1520-mcp-bridge.md),
[#1524](2026-10-18-mcp-ui-isolation.md). Mockups are in #1536. Element parity:
not applicable (Element has no local assistant bridge).

## Problem

The bridge works, but only for technical users: the `koushi-mcp` launcher is
built from source, and access is granted by hand-editing
`<data_dir>/mcp/connections.json`. An assistant without a grant gets
`consent_required` and nothing happens in the app.

## Decisions (user)

1. The bridge is **on by default**; the approval dialog is the protection. A
   switch in App Settings turns it off.
2. Approval is **per assistant x per account**, asked the first time an
   assistant touches that account.
3. Permissions (read, send, attachments) are **asked on demand**.
4. Once send is allowed, later sends need **no confirmation**. Revocation lives
   in the settings page.
5. Registration is **copy-and-paste**; Koushi never edits another tool's
   config. (Extended by
   [one-click registration](2026-10-19-mcp-one-click-register.md): Koushi may
   run a client's own CLI on request, still writing no other tool's files.) Clients: Claude Code, Codex, pi (1.0+), DeepSeek Harness, other.
6. The launcher ships inside every release package.

## Placement

The consent broker lives in `koushi-mcp-host`, not in `koushi-state` `AppState`
and not in the Tauri adapter. Consent is not Matrix semantics, the crate is
toolkit-independent and shared with the headless QA host, and the state is
process-wide rather than per account, as with desktop updates. The Tauri
adapter only forwards the broker's state and the user's answers.

## Grant store (`koushi-mcp-host::grants`)

`connections.json` stays the single store, now written by the app. Settings
cannot hold it: settings values must not contain user IDs, and account ids
embed them.

Schema 2 makes permissions per account:

```json
{ "schema": 2, "connections": [
  { "client_name": "claude-code",
    "accounts": [ { "account_id": "account:@alice:example.invalid",
                    "read": true, "send": true, "attachments": false } ] } ] }
```

- `load` returns a typed status: `Missing`, `Ok`, `Malformed`, `Unreadable`,
  `UnsupportedSchema`. A missing `schema` means 1; anything above 2 is
  unsupported.
- Schema 1 is read with today's effective rule (entries without a name or
  accounts are dropped; the first entry for a client wins; each listed account
  gets that entry's global flags). The next write upgrades the file.
- Unknown fields on the file and on entries are preserved on rewrite.
- The app writes only when the status is `Missing` or `Ok`. Writes re-read the
  file under a single async writer lock, then atomically replace it (`0600`,
  directory `0700`). Calls still re-read the file, so hand edits and
  revocations apply on the next call.
- Room scope and per-connection poll limits from the #1520 plan are dropped:
  neither was implemented, and the poll wait stays a fixed 50 s bound.

## Consent broker (`koushi-mcp-host::consent`)

Two modes, chosen at construction: `Interactive` (desktop) and `FailClosed`
(headless QA and any host without a consent surface). `FailClosed` answers a
missing permission with `consent_required` immediately and creates nothing.

`authorize(client, account, need, context)` in `Interactive`:

1. The account must resolve to a live account first; otherwise
   `account_unavailable`, no dialog.
2. Granted in the store: `Ok`. `list_accounts` reports per-permission state.
3. Denied earlier for the same key `(client, account, need)`: `not_permitted`
   immediately. Denial memory is cleared by a grant from settings, Remove
   access, turning the bridge off, and restart.
4. Otherwise join or create the pending request for that key and wait up to
   **40 s**:
   - Allow: write the store; on success, update state, publish, then wake
     waiters with `Ok`. On write failure the request stays pending, waiters get
     `internal` ("Koushi could not save the approval"), and the state reports
     the store error.
   - Deny: waiters get `not_permitted`; the key is remembered.
   - Timeout: the waiter gets `consent_required` ("approve it in Koushi, then
     retry"). The request stays pending so a later Allow records the grant.
   - Client disconnect: the waiter is dropped; the tool never runs.
   - Bridge off or stopped: waiters get `koushi_unavailable`; pending requests
     and denial memory are cleared.
5. A pending request expires 5 minutes after creation, closing its dialog.
   When its last waiter leaves, its message preview is cleared.
6. Caps: 4 pending keys per client name, 16 in total; beyond that `busy`.

Answers carry the `request_id` (monotonic). An unknown, expired, or settled id
is a no-op reported as `stale`.

Lock discipline follows `app_updates.rs` `transition()`: one
`std::sync::Mutex` for broker state, no await and no file I/O under it, and the
watch publication ordered with each mutation.

### Dialog content

- The account is rendered from Koushi's own account tabs, never from request
  text.
- `client_name` is validated at hello: 1-64 characters from
  `[A-Za-z0-9._-]`. Anything else is rejected before a connection exists. The
  client version from hello is shown next to the name. The dialog states that
  the name is self-declared.
- Send context: the room label only if the room resolves to a joined room of
  that account, and the first 280 characters of the message with control and
  bidirectional characters removed, labelled as context. The button names the
  standing permission ("Allow sending from Work"). An unresolved room gives the
  same consent behavior as a resolved one, so consent cannot probe membership;
  the room is checked after consent as today.
- Allow has no default focus and ignores activation for 1 s after the dialog
  appears.
- `Debug` and diagnostics redact client names, account ids, room labels, and
  previews; only counts and closed codes are recorded.

### Attention

On a new pending request the adapter requests user attention (dock bounce /
taskbar flash). If the main window is hidden in the tray it is shown and
unminimized, but focus is not forced. No native notification (it would need
Rust-side product text).

## Transport (`koushi-desktop://mcp`)

`McpAssistantsState`:

- `bridge`: `disabled | running | start_failed`
- `store`: `ok | missing | malformed | unreadable | unsupported_schema |
  write_failed`
- `launcher`: `{ path } | { unavailable: not_bundled | translocated |
  ephemeral }`
- `pending`: `[{ request_id, client_name, client_version, account_id, need,
  room_label?, message_preview?, waiting }]`
- `assistants`: `[{ client_name, last_used_ms?, accounts: [{ account_id, read,
  send, attachments, denied: [need] }] }]` - store entries plus clients denied
  this run, so a mistaken Deny can be undone by turning the permission on.
- `registrations`: `[{ client, client_name, kind, text, config_hint? }]`

Commands: `get_mcp_assistants_state` (re-reads the store),
`answer_mcp_approval(request_id, allow)`,
`set_mcp_permission(client_name, account_id, need, allowed)`,
`remove_mcp_assistant(client_name)`. All `pub fn`, registered in
`generate_handler!`.

## Bridge lifecycle and on/off

`SettingsValues.mcp.enabled` and `AppSettingsValues.mcp.enabled` (app scope,
default `true`) through the ordinary settings reducer and store.

- The desktop starts the bridge only after the first settings load completes.
  A missing settings file means defaults (on); a load error means off.
- Start and stop are serialized: stop joins the listener and every connection,
  removes the endpoint file and socket, then a later start creates new ones.
- `serve_connection` watches the shutdown signal, so turning the bridge off
  closes live connections: the next call from an already connected launcher
  fails with `koushi_unavailable`.
- While a request waits for approval, the connection also watches for EOF.

## Timing

Approval waits at most 40 s. `wait_for_events` counts the approval wait against
its own deadline so the whole call stays under 60 s (Codex's default tool
timeout; the launcher's own limit is 70 s). A send that times out and is
retried by the assistant reuses its `client_request_id`, which already
deduplicates at the core.

## Launcher packaging

- Every desktop build script stages
  `apps/desktop/src-tauri/binaries/koushi-mcp-<T>[.exe]` (built with
  `cargo build --release -p koushi-mcp --target <T>`) and passes one merged
  `--config` with `bundle.externalBin: ["binaries/koushi-mcp"]` in every mode.
  `tauri-build` copies `externalBin` from `TAURI_CONFIG` during `build.rs`, so
  plain `cargo build/test` of `koushi-desktop` without that config is
  unaffected. `binaries/` is git-ignored.
- macOS: `Contents/MacOS/koushi-mcp`, covered by the existing
  `codesign --deep --strict`, notarization, and `spctl` gates.
- Windows: next to `koushi-desktop.exe`, unsigned like the installer. The
  future `--signed-input` flow must sign it too; recorded in the release
  runbook.
- Linux deb/rpm: next to the main binary. AppImage (`APPIMAGE` set): copy the
  sidecar to `<data_dir>/mcp/bin/koushi-mcp` through a temp file, set the
  executable bit, and rename, when the content differs; report that path.
- macOS paths under `/Volumes/` or containing `/AppTranslocation/` report
  `translocated`, and the page asks the user to move Koushi to Applications.
- Pinning a grant to a launcher path (#1520 open question) is deferred.

## Registration snippets (`koushi-mcp-host::registration`)

POSIX paths in single quotes (`'` written as `'\''`); Windows paths in double
quotes. JSON via `serde_json`; YAML in single-quoted scalars.

| Client | Text |
| --- | --- |
| Claude Code | `claude mcp add --scope user koushi -- <path> --client-name claude-code` |
| Codex | `codex mcp add koushi -- <path> --client-name codex` |
| pi | `pi mcp add koushi -- <path> --client-name pi` |
| DeepSeek Harness | `@deepseek-ai/dsh-mcp-client` YAML entry; hint `~/.dsh/profiles/<profile>/cordis.patch.yml` |
| Other | `mcpServers` JSON with `--client-name my-client` |

`pi mcp add` syntax is from pi's MCP docs (name, `--`, command, args). The
DeepSeek entry follows the package README; the help page keeps its caveat that
some harness setups cannot start local processes.

## UI

- App-level `McpApprovalDialog` beside `DesktopUpdates`.
- App Settings -> **AI assistants**: on/off switch, store and launcher status,
  assistants with per-account switches and Remove access, Connect tabs with
  snippet and Copy.
- All text through the message catalog in five locales.

## Testing

- `koushi-mcp-host` (RED first): schema 1 read and upgrade, unknown fields
  kept, malformed/unsupported never overwritten, duplicate schema 1 entries;
  allow records exactly one permission; write failure keeps the request
  pending; deny memory and its clearing; timeout then later allow; key join;
  stale answer; caps; expiry; `FailClosed` mode; disable closes live
  connections; disconnect mid-wait means the tool never runs; client name
  validation; snippet quoting.
- Settings: `mcp.enabled` patch, persistence, goldens.
- Tauri: commands registered; DTO serialization.
- React: vitest for the dialog (no default focus, activation delay) and the
  settings page; Playwright harness spec for approval and switches.
- Scripts: release configuration test for `externalBin` in every mode.
- The agent MCP scenario keeps a schema 1 grant in `FailClosed` mode, proving
  compatibility. Consent adds no Matrix behavior, so no new homeserver QA
  scenario.
- One local Linux bundle build to confirm the sidecar is packaged.

## Docs in the same change

Overview "External MCP Bridge" (grant store, broker placement, consent
exception to "No GUI coupling", poll limit); state-machine sections for the
approval lifecycle, bridge lifecycle, and `mcp.enabled`; state-ownership mirror
entry; `docs/help/ai-assistants.md` and `docs/help/settings.md`; plan index;
release runbook note.

## Out of scope

Native notification text, OAuth/HTTP transport, automatic config editing,
per-room scopes, a persisted activity log, launcher path pinning.

## Review record

- Design review (independent subagent, 2026-10-19): approve with changes. All
  findings adopted above: live-connection shutdown, disconnect cancellation,
  account resolution and client-name validation before any dialog, expiry and
  per-client caps, preview labelling and redaction, typed store status and
  write ordering, canon edits, fail-closed startup, timing budget, lock
  discipline, packaging modes, AppImage/translocation paths, tests and docs.
- Integrated-diff review (independent subagent, 2026-10-19): approve with
  changes; no path grants access without the user. Adopted: a joining call
  updates the dialog's context and is counted; a store generation keeps a stale
  read from undoing a revocation; only live approvals are joined; Allow settles
  every approval for its key; Deny is serialized with writes; the broker resets
  after connections drain; the listener is dropped at once and a stuck drain is
  aborted; exit stop no longer depends on an async lock; settings are
  subscribed before loading; the `seen` map is bounded; invisible and tag
  characters are stripped; `ConsentContext` Debug is redacted; open approvals
  are answerable from the settings page; ARIA fixes; canon wording on publish
  order, shutdown, the 60 s budget, focus, and the `list_accounts` account-id
  disclosure (pre-existing, now documented rather than changed).
- Verified locally: a debug `.deb` built with the staged sidecar contains
  `usr/bin/koushi-mcp` beside `usr/bin/koushi-desktop`.
