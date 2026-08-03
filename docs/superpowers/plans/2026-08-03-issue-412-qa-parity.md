# Issue #412 QA and Element X Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish reproducible Tuwunel and Synapse invitation coverage and prove Koushi's Simplified Sliding Sync request shape matches Element X before removing Legacy Sync.

**Architecture:** This is the first of three sequential PRs for Issue #412. It intentionally keeps the current Legacy fallback while adding server fixtures, an invitation acceptance lane, and an SDK request-contract test. The PR must merge and both positive server lanes must be green before the runtime-removal plan begins.

**PR1 integration correction:** Tuwunel 1.7.1 returns the real SyncService
`all_rooms` invitation data but omits the transitional `koushi_invites` probe
list, so the known obsolete invite-only probe selects LegacySync before the
scenario can exercise SyncService. The two positive invitation lanes therefore
use a debug/test-only explicit `sync_service` override solely to bypass that
probe and prove the actual unfiltered `all_rooms` path. Release selection is
unchanged. PR2 deletes the invite-only probe, this override, and the QA backend
selector together; changing the probe in PR1 would invalidate the required
migration ordering.

**Tech Stack:** Rust, matrix-rust-sdk, wiremock, Node.js test runner, local homeserver QA, GitHub Actions, Markdown.

---

## Task 1: Align the active canon with the migration sequence

**Files:**
- Modify: `docs/policies/engineering-rules.md`
- Modify: `docs/superpowers/specs/2026-07-22-sync-capability-probe-isolation-design.md`
- Modify: `docs/superpowers/specs/2026-08-02-room-list-readiness-design.md`
- Modify: `docs/superpowers/specs/2026-07-06-session-sync-lifecycle-redesign-design.md`
- Modify: `scripts/build-structure-contract.test.mjs`

- [ ] Extend `scripts/build-structure-contract.test.mjs` so active SDK dependency instructions require the vendored `vendor/matrix-rust-sdk` path and reject a remote matrix-sdk `git`/`rev` pin in active engineering rules.
- [ ] Run `node --test scripts/build-structure-contract.test.mjs`. Confirm the new assertion fails on the stale remote-revision rule.
- [ ] Replace the stale rule with the submodule-path contract already enforced by `AGENTS.md`, `REPOSITORY_RULES.md`, and `scripts/check-sdk-submodule.mjs`.
- [ ] Mark the three older sync designs as superseded by `docs/superpowers/specs/2026-08-03-single-sliding-sync-diagnostics-design.md` for the Issue #412 behavior they conflict with. Preserve their historical context and explicitly state that Legacy fallback and invite-only probing are transitional until PR2.
- [ ] Run `node --test scripts/build-structure-contract.test.mjs` and confirm exit code 0.
- [ ] Commit with message `docs: align sync migration canon`.

## Task 2: Make Tuwunel and Synapse fixtures express Sliding Sync capability

**Files:**
- Modify: `scripts/lib/local-homeserver-qa.mjs`
- Create: `scripts/lib/local-homeserver-qa.test.mjs`
- Modify: `scripts/desktop-headless-local-qa.mjs`
- Test: `scripts/lib/local-homeserver-qa.test.mjs`

- [ ] Add failing Node tests for a Synapse fixture pinned to `matrixdotorg/synapse:v1.157.0`, a positive configuration containing `experimental_features.msc3575_enabled: true`, and a negative configuration containing `msc3575_enabled: false`.
- [ ] Add failing tests that Tuwunel remains a positive Simplified Sliding Sync fixture and that server selection can run exactly `tuwunel`, exactly `synapse`, or the positive pair without including Conduit.
- [ ] Run `node --test scripts/lib/local-homeserver-qa.test.mjs`; confirm failures identify the old Synapse pin and missing capability controls.
- [ ] Export only the pure fixture/configuration helpers needed by the tests. Do not expose credentials or Docker state.
- [ ] Update the Synapse pin and generated YAML. Add an explicit positive/negative Sliding Sync capability parameter rather than mutating generated text in callers.
- [ ] Extend the headless runner's server selection so `--server=both` means Tuwunel plus Synapse for this migration lane. Keep the old explicit Conduit mode temporarily for other pre-PR3 scenarios.
- [ ] Run `node --test scripts/lib/local-homeserver-qa.test.mjs`; confirm exit code 0.
- [ ] Commit with message `test: add sliding sync homeserver fixtures`.

## Task 3: Lock the Element X all-rooms request contract

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`
- Modify: `docs/upstream/matrix-rust-sdk-feedback.md`
- Test: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`

- [ ] Add a failing SDK test that captures the serialized first request made by `RoomListService::new`/`all_rooms` and asserts connection id `room-list`, list name `all_rooms`, `is_invite` omitted, timeline limit `1`, required state parity, and enabled account-data/receipt/typing/thread extensions.
- [ ] Keep the expected request fields traceable to Element X 26.07.28's SDK pin `ccd225e58eb900e321411397d1c13c2d9b312bb6`; compare behavior, not source formatting.
- [ ] Run `(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-ui all_rooms_request_matches_element_x_26_07_28)`; confirm it fails before the request-contract test/observable exists.
- [ ] If the existing request already passes, retain the test as the proof and make no speculative SDK rewrite. If one field differs, make the minimum request-builder correction required by the asserted contract.
- [ ] Record the comparison and decision in `docs/upstream/matrix-rust-sdk-feedback.md`, including that no wholesale SDK rebase is justified by this guard.
- [ ] Run `node scripts/check-sdk-submodule.mjs` and `(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-ui all_rooms_request_matches_element_x_26_07_28)`; confirm both exit 0.
- [ ] Commit with message `test: lock element x sliding sync request parity`.

## Task 4: Add an invitation acceptance scenario that reports the selected backend

**Files:**
- Modify: `crates/koushi-core/src/bin/headless-core-qa.rs`
- Modify: `scripts/desktop-headless-local-qa.mjs`
- Modify: `apps/desktop/package.json`
- Test: `crates/koushi-core/src/bin/headless-core-qa.rs`

- [ ] Extend the existing invitation scenario assertion so the invited room is observed from the live all-rooms projection and the run reports the selected backend as `sync_service`; do not infer success from logs or from `Client::invited_rooms()`.
- [ ] Run `cargo test -p koushi-core --bin headless-core-qa invites_dm_requires_expected_sync_backend`; confirm the assertion fails because the runner cannot yet require/report the positive backend.
- [ ] Add a typed expected-backend input to the QA harness and propagate it through the Node runner. Apply a fixed expectation only to explicit `sync-service` and `legacy` legs; ordinary `probed` legs remain behavior-selected because the transitional invite-only probe may legitimately choose LegacySync until PR2. Keep the existing forced-Legacy leg only as a temporary PR1 regression comparison.
- [ ] Add package scripts for the positive Tuwunel and positive Synapse invitation lanes with `--core --scenario=invites_dm --core-backend=sync-service` and a 240000 ms timeout. The selector sets the temporary debug/test-only `KOUSHI_QA_FORCE_SYNC_BACKEND=sync_service` override and requires `SyncService` from the typed Core event.
- [ ] Run `cargo test -p koushi-core --bin headless-core-qa invites_dm`; confirm exit code 0.
- [ ] Run `node scripts/desktop-headless-local-qa.mjs --run --server=tuwunel --core --scenario=invites_dm --core-backend=sync-service --timeout-ms=240000` and record its exit code in the PR body.
- [ ] Run `node scripts/desktop-headless-local-qa.mjs --run --server=synapse --core --scenario=invites_dm --core-backend=sync-service --timeout-ms=240000` and record its exit code in the PR body.
- [ ] Commit with message `test: prove sliding sync invitations on supported servers`.

## Task 5: Put the positive invitation matrix in CI

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/qa/headless-basic-operations.md`
- Test: `.github/workflows/ci.yml`

- [ ] Replace the single Conduit-oriented invitation/login job with a two-entry Tuwunel/Synapse matrix for the new positive invitation script. Do not delete unrelated Conduit jobs until PR3.
- [ ] Ensure each matrix entry uploads its server/core artifacts under a server-specific name and preserves the command's exit status.
- [ ] Update the active QA guide with the two local reproduction commands and the requirement that both are green before PR2.
- [ ] Run `node --test scripts/build-structure-contract.test.mjs` and `npm --prefix apps/desktop run lint`; confirm exit code 0.
- [ ] Self-review `git diff origin/main...HEAD` and `git status --short`, including the submodule diff and every new file.
- [ ] Re-run `node scripts/check-sdk-submodule.mjs`, the Node fixture tests, the SDK parity test, and both homeserver invitation lanes. Read each command's own exit status.
- [ ] Commit with message `ci: gate sliding sync invitations on tuwunel and synapse`.
- [ ] Push, open PR1 as ready for review, monitor checks, fix failures, enable auto-merge, and wait for merge before starting `2026-08-03-issue-412-single-sync-runtime.md`.
