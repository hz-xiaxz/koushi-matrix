# 2026-10-20 bug burndown (#1485 #1487 #1509 #1518 #1533 #1540 #1542 #1544 #1545 #1546 #1551 #1552 #1553 #1557 #1559 #1476)

One consolidated branch for the bug reports filed 2026-10-16..20. Each fix was
reproduced with a failing test before the change (Rust unit/integration tests,
vitest, or the Playwright DOM harness).

## Canon consulted and changed

- `docs/agents/state-ownership.md` (Not joined lane): a joined sub-space is a
  joined child and never appears in the lane (#1559). Clarification only.
- `docs/architecture/state-machine.md` (Basic Operations): starting a direct
  message is a tracked basic operation. Rust marks it pending before the server
  call and clears it on every outcome (`DirectMessageStarted`,
  `DirectMessageMissing`, failure) before the outcome event, so the Profile
  **Send message** button is disabled from a Rust snapshot instead of the shared
  React `isBusy` guard (#1518). It shares the existing basic-operation slot, so
  the create-room and add-existing-room dialogs also read busy for that window.
- `docs/architecture/i18n.md`: the "Known gap" for Rust-rendered notification
  text is replaced by the `koushi_state::notification_text` catalog rule. OS
  banners and delivery digests read the locale at render time (#1476).
- `docs/architecture/overview.md`: MCP attachments go through Core local-upload
  preparation and `UploadAndSendMedia`, never through composer staging (#1533);
  `wait_for_events` is documented as a sampled, bounded, per-run feed with
  opaque account- and run-bound cursors and a typed `resync_required` error
  (#1551). `docs/plans/2026-10-18-issue1520-mcp-bridge.md` still describes the
  older staging route; the canon wins.

## Verification

Per-lane targeted gates are recorded in the PR description. On the merged
branch: `cargo fmt --check`; clippy `-D warnings` for koushi-state, koushi-core,
koushi-mcp, koushi-mcp-host and koushi-desktop; tests for koushi-state,
koushi-core (lib and integration), koushi-mcp, koushi-mcp-host and
koushi-desktop lib; frontend typecheck, lint and the full vitest suite; and the
full Playwright DOM suite (chromium + webkit), all run on the merged branch
after merging current main.

## Not covered

- #1533: no live encrypted upload against a disposable homeserver yet
  (`scripts/mcp-agent-scenario.mjs` can be extended).
- #1551: no test that invites stay out of the sampled feed.
- #1540: members never seen in the timeline still render initials (the avatar
  gap tracked since #839).
- #1509: drag-reorder autoscroll along a long rail needs a manual WebKit check.
- `timeline-scroll-anchor-drift.spec.ts` assumed 72 px rows; on hosts whose
  default sans-serif is Noto Sans CJK the rows are ~97 px and the test failed on
  main. The test now measures the row instead of assuming its height.
