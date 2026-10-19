# One-click MCP registration (follow-up to #1536)

Status: implementation plan. Canon: `docs/architecture/overview.md` "External
MCP Bridge"; [#1536 plan](2026-10-19-issue1536-mcp-approval.md). Element parity:
not applicable.

## Problem

The AI assistants page shows registration text to copy. Users who are not at
home in a terminal stall there. A user report also showed that people try the
bridge before they know where the launcher is.

## Decisions (user)

1. Clients with an official registration CLI (Claude Code, Codex, pi): Koushi
   runs that CLI on the user's request. Koushi still never edits another tool's
   files; the client's own CLI does.
2. Clients without a CLI (DeepSeek Harness, other): copy and paste, as now.
   They are used by experts.
3. Claude Desktop: use its official extension format, MCPB (`.mcpb`).

## Design

### Run the client's CLI

- Each command registration gains an `action: run_command`. The settings page
  shows **Register** next to **Copy**. Register opens a confirmation dialog
  with the exact command text; **Run** calls
  `register_mcp_client(client)`.
- The webview sends only the client enum. Rust rebuilds the argument vector
  from the broker's launcher path (`registration::command_argv`), so the
  webview cannot choose what runs.
- Program lookup (desktop adapter): a GUI app does not inherit the shell
  `PATH`, and these CLIs are often Node scripts that also need `node` on
  `PATH`. On macOS and Linux Koushi asks the user's login shell for its `PATH`
  once per request (`$SHELL -ilc` printing `PATH` between markers, stdin
  closed, 5 s timeout), falling back to the app's own `PATH`. On Windows the
  app's `PATH` is used. The program is resolved in that `PATH`, and the child
  runs with it.
- Execution: no shell, arguments as a vector, stdin closed, 60 s timeout with
  kill, stdout/stderr captured and capped (4 KiB). Nothing is logged; the
  diagnostic records only the outcome code.
- Outcomes: `registered`, `not_found` (the page says to install the client or
  copy the command), `failed` (exit code plus the client's own trimmed output,
  shown verbatim in a monospace box, for example "already exists"),
  `launcher_unavailable`.
- No automatic replacement of an existing `koushi` entry; the client's own
  error says so, and copying remains available.

### Claude Desktop extension (macOS and Windows)

- A `claude_desktop` registration with `action: create_extension` and, as the
  copy fallback, the `claude_desktop_config.json` `mcpServers` entry.
- **Create extension** calls `create_claude_desktop_extension()`, which writes
  `Koushi.mcpb` to the Downloads folder and opens it, so Claude Desktop shows
  its own install review.
- Bundle contents: `manifest.json` (manifest_version 0.3, `server.type:
  binary`, `entry_point: server/koushi-mcp[.exe]`, `mcp_config.command:
  ${__dirname}/server/koushi-mcp[.exe]`, args `--client-name claude-desktop`,
  `compatibility.platforms` of the current platform, tool list, version equal
  to Koushi's), `icon.png` (the 512 px app icon), and a copy of the bundled
  launcher with mode 0755. The spec's examples keep the executable inside the
  bundle; pointing at the app's own launcher by absolute path is not
  documented, so it is not used. After a Koushi update that changes the bridge
  protocol the extension must be recreated; the page says so.
- Manifest prose is English, like the registration snippets: it is an
  artifact for another app, not Koushi product text.
- Outcomes: `opened`, `saved` (could not open; the page shows the path and says
  to drag it into Claude Desktop), `unsupported_platform`, `launcher_unavailable`,
  `write_failed`.

### UI

- Register button and confirmation dialog for `run_command`; result line
  under the snippet. Create button for `create_extension`.
- Copy stays for every client.
- Text in the five catalogs.

## Security

- The webview cannot name a program, an argument, or a path: commands are
  rebuilt in Rust from a closed client enum and the launcher path the app
  resolved itself.
- The login shell is run once with a fixed script; its output is parsed only
  for `PATH` between unique markers. Shell startup files are the user's own.
- Child processes get the app environment plus that `PATH`; stdin is closed.
- The extension contains no token or account data: only the launcher, a
  manifest, and an icon. The token is still read from the endpoint file at
  runtime.

## Testing

- `koushi-mcp-host`: argv per client equals the displayed command; the Claude
  Desktop manifest fields; registrations list Claude Desktop only on macOS and
  Windows.
- Desktop: `PATH` parsing from shell output with noise; program resolution in
  a synthetic `PATH`; the runner against a fake CLI script (success with the
  expected argv, failure with captured output, timeout); the bundle zip
  contains the manifest, icon, and an executable launcher.
- React: confirmation dialog, outcomes, extension button; Playwright spec.
- Docs: help "Step 1", settings row, overview wording.

## Out of scope

Editing configuration files directly, deep links for Cursor or VS Code,
replacing an existing registration, Linux Claude Desktop (unsupported by it).

## Review record

Design review (independent subagent, 2026-10-19): approve with changes.
Adopted:

- Windows: resolve the program over `PATHEXT` (preferring `.exe`), let Rust's
  batch-file escaping run `.cmd` shims and map its refusal to an
  `unsafe_arguments` outcome, hide the console window, and read the user and
  system `Path` from the registry so a CLI installed after Koushi started is
  found. The Windows CI job runs the MCP module's tests.
- Login-shell probe: `$SHELL`, else the passwd entry; `printenv PATH` between
  markers (fish prints `$PATH` space-separated); own process group, killed as
  a group on timeout and reaped; stdout read with a byte cap under the same
  timeout. Fallback install directories when the probe fails.
- CLI runner: own process group (Unix) or `taskkill /T` (Windows) on timeout;
  streamed output with a byte cap, cut on a UTF-8 boundary, ANSI and control
  characters removed; only absolute `PATH` entries; working directory is the
  home directory; AppImage variables removed and `NO_COLOR`/`TERM=dumb` set;
  stdin closed so first-run prompts fail rather than hang.
- One registration at a time (`in_progress`), and the button is disabled while
  it runs.
- A launcher that speaks an older bridge protocol is recorded by the broker
  and the page says to register again or recreate the extension.
- The displayed command is derived from the executed vector, which is the only
  source; the client enum is closed and non-command clients are refused.
- The extension is written through a temporary file and a rename in the
  Downloads folder (falling back to Koushi's data folder), and opened from
  Rust only. The manifest's tool list is tested against the launcher's tools.
- The client's output is shown on screen only, never in state `Debug` or
  diagnostics.

Integrated-diff review (independent subagent): request changes, adopted:
`OsString` import gated for Windows clippy; the single-flight flag is cleared
by a drop guard; after the CLI exits the rest of its process group is killed
and each output reader is stopped at its own deadline, keeping what it read;
the outdated notice clears on a current-protocol handshake and tells users to
recreate the extension (CLI registrations follow the in-app launcher);
`taskkill` and `reg` run by full System32 path; the locked-file case gets a
hint; the Windows shim test requires success. Noted: the login-shell probe
runs the user's own rc files interactively (intended), and a shell that
rejects `-l -i -c` (tcsh) falls back to the usual install directories.

Not adopted: building the `.mcpb` in release CI with the official CLI. The
bundled launcher is copied byte for byte, so its signature is unchanged, and
the release pipeline gains no Node tooling. Instead, one generated bundle is
checked locally with the official `mcpb validate`, and launching it from a
notarized build is a manual check.
