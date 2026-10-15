# Post-merge housekeeping after #1413, #1422, #1423 and #1425

Canon consulted: `REPOSITORY_RULES.md` ("QA Gates And Cleanup", "Review And
Audit", "User-Visible Text", "Documentation And Work Records"), `AGENTS.md`
("Keeping these notes maintainable"), engineering rules "Build, Dependencies,
QA Gates" 4 and the QA-lane contract in `docs/agents/qa-lanes.md`.

Four PRs merged on 2026-10-15 (#1423, #1422, #1413, #1425). Reviewing their
finished state left four decisions. Each is recorded here with the evidence,
so the next reader does not have to re-derive it.

## 1. Browser-tier WebKit wheel-scroll flake — keep, but make it diagnosable

`[webkit] e2e/timeline-scroll-anchor-drift.spec.ts` failed the required
browser-headless gate twice on #1425 (both attempts: `accepts small user wheel
movement`, `19646` against `> 19666`, then both wheel cases) and passed on the
third with no change to the branch.

Evidence that it is not this repository's product code:

- The failing assertion measures `scrollTop` after a wheel event and reads the
  pre-wheel value exactly, i.e. no input arrived; the spec's own comment already
  allows for WebKit delivering compositor scroll late "on a busy CI runner".
- The same spec passes locally in WebKit 3/3 (the neighbouring
  `crossing the virtualization threshold` case fails on this machine on
  `origin/main` too, so local WebKit is its own environment).
- #1425's only browser-tier-relevant change is which row id carries the unread
  divider, and the e2e harness sets no `first_unread_*` field at all, so the
  tier cannot observe the change. Its Rust changes never execute under the
  harness.
- The identical symptom is already recorded against #1427 in
  `docs/worklogs/2026-10-15-issue1427-device-local-delivery.md`.

Decision: do not touch the spec or the app. Make the next occurrence
diagnosable instead — CI uploads `apps/desktop/test-results/` on failure, and a
dispatched, non-required run can record a Playwright trace for the WebKit
project (`browser_trace` input, `KOUSHI_PLAYWRIGHT_TRACE`).

**Correction (same day).** The first version of this change enabled
`trace: "retain-on-failure"` for the WebKit project in the required run. The
next `main` CI failed with three WebKit failures in this spec, and the durations
show why: date dividers 18.6s -> 30.5s, threshold crossing 15.0s -> 30.2s, wheel
6.5s -> 16.6s against the previous green run. Tracing perturbs the very
timing-sensitive assertions it was meant to explain, so the required run is
trace-free again and tracing is opt-in per dispatched run. See
[docs/agents/verification.md](../agents/verification.md#browser-tier-trace-collection).

## 2. Retired real-homeserver QA — quarantine finished, tombstone deferred

#1413 removed the entry points and the live docs, but the "Retired QA
vocabulary" table in `docs/agents/history.md` never named them, and
`crates/koushi-qa/src/bin/real_homeserver_qa/tests.rs` still pointed at the
deleted `qa:headless-basic:real` package script. Both are fixed here.

The tombstone itself (the fail-closed `real-homeserver-qa` binary, its ~3.6k
line module, and `scripts/desktop-real-homeserver-qa.mjs`) stays. Two live
gates still require it — `scripts/check-protocol-qa-boundaries.mjs` demands the
binary in the `koushi-qa` manifest, and `scripts/check-agents-docs.mjs` extracts
scenario names from its sources — so deleting it is a design change to those
gates, tracked as #1435 with the options and acceptance criteria.
`scripts/check-agents-docs.mjs` keeps its own hard-coded retired-flag list;
adding these entry points to it is part of #1435 rather than this change.

## 3. Invite-dialog history wording — no further change

#1422 summarises what an invitee can read and keeps the encrypted-room caveat
for `shared` / `world_readable` only (`encrypted && (shared || worldReadable)`,
where the pre-change code was `encrypted && shared`). Left as merged:

- The caveat's "earlier messages" wording matches the summary sentence it
  qualifies. Extending it to `invited` would be defensible — invite-time history
  also travels through the standard pre-share path
  (`with_enable_share_history_on_invite(true)`, `CollectStrategy::AllDevices`),
  which can be skipped for an unverified device — but it would need different
  wording, and #1418 asked for one short sentence per setting.
- The non-retroactive sentence stays out of the dialog: inviting never changes
  room settings, so it does not inform this flow. It remains in
  `docs/help/rooms-and-spaces.md`, where the setting is actually changed.
- `world_readable` shares the `shared` sentence, as #1418 proposed; the removed
  "non-members may read" warning only restated the setting.

## 4. Thread-preview mention marker — no further change

#1423 inserts `@` before a mention label found in the plain-text preview
(`mention_marked_text`: labels collected from the formatted HTML in document
order, first textual match wins, an existing `@` is not doubled). Its known
limits are cosmetic: a label that appears earlier in the prose takes the `@`,
and matching ignores word boundaries (`Bob` inside `Bobby`). Both consumers are
display-only (thread chip, Threads list), and the QA lane asserts
`latest_body_preview` by `contains()`, so no token or contract depends on the
exact string. The function's doc comment already states the document-order
match. Revisit only with a real mis-rendered preview.

## Files changed

- `.github/workflows/ci.yml` — upload browser-headless artifacts on failure; a
  `browser_trace` dispatch input opts a diagnostic run into WebKit tracing.
- `apps/desktop/playwright.config.ts` — WebKit trace only when
  `KOUSHI_PLAYWRIGHT_TRACE` is set.
- `docs/agents/verification.md` — why the required browser tier stays
  trace-free and how to collect a trace on demand.
- `docs/agents/history.md` — retired external-homeserver QA entry points.
- `crates/koushi-qa/src/bin/real_homeserver_qa/tests.rs` — stale script name in
  a comment, now naming the retirement and #1435.
- `docs/worklogs/2026-10-15-post-merge-housekeeping.md` — this record.

## Verification

- `node scripts/check-agents-docs.mjs` and `node scripts/user-help.mjs --check`.
- `actionlint .github/workflows/ci.yml` and `git diff --check`.
- `cargo fmt --check`, `cargo test -p koushi-qa --features qa-bin --bin real-homeserver-qa`.
- `npx tsc --noEmit` in `apps/desktop`.
- The WebKit spec run with tracing off (the required configuration) and with
  `KOUSHI_PLAYWRIGHT_TRACE=1` on the known-failing case, to prove both the
  trace-free default and the retained trace the diagnostic run consumes.
