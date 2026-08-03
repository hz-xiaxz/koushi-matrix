# Issue #412 Diagnostics and Legacy QA Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add copyable private-safe Sliding Sync diagnostics, remove obsolete Legacy/Conduit QA surfaces, and close Issue #412 only after all supported and negative gates pass.

**Architecture:** This is the third and final sequential PR for Issue #412. Rust publishes a dedicated latest-wins diagnostics snapshot; Tauri and React only serialize and render it in the existing diagnostics dialog. Active QA supports Tuwunel and Synapse, with Synapse also providing the explicit unsupported fixture. Final source and documentation inventories prove that Legacy production behavior is absent.

**Tech Stack:** Rust, serde, Tauri, TypeScript, React, Vitest, Playwright, Node.js, GitHub Actions, local homeserver QA.

---

## Task 1: Define a private-safe Rust Sliding Sync diagnostics snapshot

**Files:**
- Create: `crates/koushi-core/src/sliding_sync_diagnostics.rs`
- Modify: `crates/koushi-core/src/lib.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/sync.rs`
- Create: `crates/koushi-core/tests/sliding_sync_diagnostics.rs`

- [ ] Add tests for a typed latest-wins snapshot containing discovery result, capability-cache state, session phase, sync lifecycle, committed-response sequence, `pos_present`, all-rooms reconciliation/readiness, reconnect count, and coarse last-failure kind.
- [ ] Add privacy tests proving formatting/serialization cannot contain homeserver URLs, room/event/user/device ids, access/refresh tokens, raw `pos`, raw response bodies, or raw SDK errors.
- [ ] Add lifecycle tests proving discovery, blocked/retry, first commit, reconciliation, disconnect, and reconnect replace the latest snapshot without relying on log parsing.
- [ ] Run `cargo test -p koushi-core --test sliding_sync_diagnostics`; confirm RED.
- [ ] Implement the dedicated typed snapshot and watch/latest-value publisher. Keep it separate from product `AppState` and use bounded/coarse enums and counters.
- [ ] Wire AccountActor and SyncActor transition points to the publisher; no room or event identifiers may enter its API.
- [ ] Run `cargo test -p koushi-core --test sliding_sync_diagnostics`; confirm exit code 0.
- [ ] Commit with message `feat: publish private safe sync diagnostics`.

## Task 2: Expose diagnostics through the existing Tauri command

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/diagnostics.rs`

- [ ] Add a failing Tauri test that `get_diagnostic_snapshot` returns the typed Sliding Sync section with stable snake-case tokens and no private fields.
- [ ] Add a reset/no-session test with explicit `not_started`/`unknown` values rather than omitted ambiguous fields.
- [ ] Run `cargo test -p koushi-desktop diagnostics`; confirm RED.
- [ ] Extend the existing command/DTO to append the Rust snapshot. Do not derive fields from the diagnostic event ring.
- [ ] Run `cargo test -p koushi-desktop diagnostics` and `cargo test -p koushi-desktop core_event_wire_format_matches_checked_in_contract_artifact`; confirm exit code 0.
- [ ] Commit with message `feat: expose sliding sync diagnostics`.

## Task 3: Render and copy the fixed diagnostics fields

**Files:**
- Modify: `apps/desktop/src/domain/diagnostics.ts`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/components/dialogs.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/domain/diagnostics.test.ts`
- Modify: `apps/desktop/e2e/basic-operations.spec.ts`

- [ ] Add TypeScript tests for deterministic report ordering and fixed labels for capability, cache, session phase, lifecycle, committed sequence, `pos` presence, all-rooms readiness, reconnect count, and coarse failure.
- [ ] Add redaction tests with hostile private-looking fixture values and assert they do not appear in copied output.
- [ ] Add a browser-headless test that opens the existing diagnostics dialog, sees the Rust-owned values, copies the report, and dispatches no product-state mutation.
- [ ] Run `npm --prefix apps/desktop run test -- src/domain/diagnostics.test.ts` and `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "Sliding Sync diagnostics" --workers=1`; confirm RED.
- [ ] Add the TypeScript DTO mirror and append the fields to `diagnosticReport()`. Render the same values in the existing dialog; do not add a second diagnostics surface.
- [ ] Add localized labels in every catalog with English fallback behavior matching the existing i18n contract.
- [ ] Run `npm --prefix apps/desktop run test -- src/domain/diagnostics.test.ts`, `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "Sliding Sync diagnostics" --workers=1`, `npm --prefix apps/desktop run typecheck`, `node scripts/check-ime-text-inputs.mjs`, and `npm --prefix apps/desktop run lint`; confirm exit code 0.
- [ ] Commit with message `feat: show copyable sync diagnostics`.

## Task 4: Remove forced Legacy QA and unsupported scenarios

**Files:**
- Modify: `scripts/desktop-headless-local-qa.mjs`
- Modify: `scripts/desktop-linux-gui-qa.mjs`
- Modify: `apps/desktop/package.json`
- Modify: `crates/koushi-core/src/bin/headless-core-qa.rs`
- Modify: `scripts/lib/local-homeserver-qa.test.mjs`

- [ ] Add/update runner parser tests asserting `--core-backend`, `KOUSHI_QA_FORCE_SYNC_BACKEND`, and Legacy scenario names are rejected, while supported/default runs select the sole backend.
- [ ] Run `node --test scripts/lib/local-homeserver-qa.test.mjs scripts/build-structure-contract.test.mjs` and `cargo test -p koushi-core --bin headless-core-qa qa_scenario`; confirm RED while obsolete options remain.
- [ ] Remove forced backend environment injection, dual-backend loops, expected Legacy tokens, and Legacy-only scenario registration.
- [ ] Rename remaining scenario assertions from backend-specific terms to capability/readiness terms.
- [ ] Run `cargo test -p koushi-core --bin headless-core-qa`, `node --test scripts/lib/local-homeserver-qa.test.mjs scripts/build-structure-contract.test.mjs`, and the two invitation commands from PR1; confirm exit code 0.
- [ ] Commit with message `test: remove legacy sync qa modes`.

## Task 5: Remove Conduit from active supported-server QA

**Files:**
- Modify: `scripts/lib/local-homeserver-qa.mjs`
- Modify: `scripts/lib/local-homeserver-qa.test.mjs`
- Modify: `scripts/desktop-headless-local-qa.mjs`
- Modify: `scripts/desktop-linux-gui-qa.mjs`
- Modify: `apps/desktop/package.json`
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/qa/headless-basic-operations.md`
- Modify: `AGENTS.md`

- [ ] Add failing tests that active server choices are only Tuwunel and Synapse, `both` means that pair, and Synapse supports an explicit `msc3575_enabled:false` negative lane.
- [ ] Remove Conduit configuration, startup, downloads, package scripts, CI matrix entries, and active QA instructions. Preserve unrelated historical reports only with a historical marker.
- [ ] Make Tuwunel the primary default and Synapse the second required server. Keep the negative Synapse capability lane distinct from positive integration lanes.
- [ ] Run `node --test scripts/lib/local-homeserver-qa.test.mjs scripts/build-structure-contract.test.mjs`; confirm exit code 0.
- [ ] Commit with message `test: retire conduit from supported server qa`.

## Task 6: Prove positive, reconnect, offline, and unsupported behavior end to end

**Files:**
- Modify: `crates/koushi-core/src/bin/headless-core-qa.rs`
- Modify: `docs/qa/headless-basic-operations.md`

- [ ] Add a Tuwunel scenario assertion for stale cache restore followed by committed-response reconciliation and live readiness.
- [ ] Add a reconnect assertion proving the same Sliding Sync engine resumes without fallback and diagnostics increment reconnect count only after a real reconnect.
- [ ] Add a network-disabled positive-cache restore assertion proving offline is not labeled unsupported and credentials/stores remain intact.
- [ ] Add a Synapse `msc3575_enabled:false` assertion proving typed Unsupported, no authenticated sync request, retry availability, and preserved local session/store material.
- [ ] Run each scenario before implementation adjustments and confirm the new assertion is RED for the exact missing observable/transition.
- [ ] Make only the minimum core/QA fixture corrections needed for the assertions; do not introduce another endpoint probe.
- [ ] Run all four scenarios on their designated server fixtures and record each command's own exit code.
- [ ] Commit with message `test: cover required sliding sync recovery paths`.

## Task 7: Final legacy inventory, full verification, and merge

**Files:**
- Modify: `REPOSITORY_RULES.md`
- Modify: `AGENTS.md`
- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/policies/engineering-rules.md`
- Modify: `docs/qa/known-issues.md`
- Modify: `docs/qa/headless-basic-operations.md`
- Modify: `docs/architecture/desktop-foundation.md`

- [ ] Search production Rust, Core events, Tauri DTOs, TypeScript, active scripts, active CI, and active docs for `LegacySync`, `legacy sync`, `KOUSHI_QA_FORCE_SYNC_BACKEND`, forced-backend arguments, invite-only support probes, Conduit positive support, backend mode/source enums, and production `/v3/sync` construction. Remove active remnants; mark genuinely historical records as superseded/historical.
- [ ] Update canon with the final supported-server matrix, diagnostics contract, retry semantics, positive-cache offline rule, and committed-response readiness invariant.
- [ ] Run `node scripts/check-sdk-submodule.mjs`, `node --test scripts/build-structure-contract.test.mjs scripts/lib/local-homeserver-qa.test.mjs`, `cargo test --workspace`, `cargo test -p koushi-desktop`, `npm --prefix apps/desktop run lint`, `npm --prefix apps/desktop run typecheck`, `npm --prefix apps/desktop run test`, and `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "Sliding Sync diagnostics|sliding sync capability" --workers=1`. Capture each command's exit code directly.
- [ ] Run the final Tuwunel positive invitation/login/reconnect/cache lanes, Synapse positive invitation/login lane, and Synapse negative capability lane once as the integrated gate.
- [ ] Review `git diff origin/main...HEAD` and `git status --short`; inspect new files and generated artifacts explicitly. Confirm no user worktree files were touched.
- [ ] Commit with message `docs: finalize sliding sync support contract`.
- [ ] Push and open PR3 as ready for review. Monitor CI/reviews, address failures, enable auto-merge, and wait for merge.
- [ ] After merge, refresh `origin/main`, inspect Issue #412 acceptance criteria one by one against merged code and green runs, post a concise evidence comment, close the issue if GitHub did not auto-close it, and verify the issue is closed.
