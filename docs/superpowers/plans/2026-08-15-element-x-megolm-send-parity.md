# Element X Megolm Send-Parity Runtime-Disable Plan

> **Implementation owner:** `luna-implementer` (GPT-5.6 Luna, `max`, write-capable) in one dedicated worktree. The parent agent owns canon, RED evidence, integration, and final audit. `reviewer-gpt` must approve this plan before implementation and the complete diff afterward.

**Goal:** Make the normal Koushi runtime use the same upstream Matrix Rust SDK initial Megolm send behavior as Element X by default: keep standard pre-share, `/keys/claim`, signed one-time/fallback-key processing, per-device share state, encrypted `m.room_key`, recipient key requests, gossip, and backup recovery; runtime-disable the Koushi-only #510 index-0 duplicate and #523 bounded targeted repair without deleting either implementation or its tests.

**Non-goals:** No room-subscription changes (#532), no Megolm rotation-policy changes, no SDK upgrade/rebase, no custom key protocol, no plaintext fallback, no user-facing toggle, no deletion of #510/#523, and no claim that disabling either path improves delivery.

## Decision and invariants

- Element X-compatible upstream behavior is the production default.
- `Room::preshare_room_key` remains unconditional for encrypted sends.
- #510's retained helper has no production caller; Koushi removes its stale builder opt-in while a testing-only caller keeps the implementation covered.
- #523 gains an independent default-off builder option. Koushi does not enable it.
- The SDK option is indispensable: #523 runs inside `matrix-sdk`'s private send future after standard pre-share, so neither a Koushi wrapper nor an existing public API can suppress it without bypassing the standard send path. Keep the addition to one boolean, one builder method, and one read at the owning branch; record the rationale and upstream disposition.
- The two configurations stay independent: the testing seam may exercise #510 without enabling #523, and #523 may be explicitly enabled without restoring #510's production caller.
- Runtime-disable means builder configuration, not `cfg`, dead-code deletion, a product setting, or an environment-variable escape hatch.
- Existing #510/#523 implementation and focused tests remain compiled and runnable through explicit test opt-in.
- A disabled path must not start its timer, claim, duplicate to-device request, wake listener, or diagnostic outcome.
- Standard upstream recovery remains unchanged: automatic `m.room_key_request`, verified own-device gossip/forwarding, and configured backup download.
- No identifier, key material, ciphertext, request ID, raw error, or message body may enter diagnostics.

## Task 1 — Update current canon before behavior

**Files:**
- `REPOSITORY_RULES.md`
- `docs/architecture/overview.md`
- `docs/architecture/state-machine.md`
- `docs/upstream/matrix-rust-sdk-feedback.md`
- `docs/agents/plans.md`

1. Replace the mandatory production #523 repair contract with the upstream-default contract: normal pre-share is authoritative; Koushi-specific duplicate/repair paths are retained but disabled by default.
2. Mark the existing #510/#523 state machines as optional experimental paths rather than active product behavior. Keep their historical plans unchanged.
3. Record that #523 remains upstreamable patch material and that production enablement requires new evidence plus an explicit decision.
4. Index this plan.

**Gate:** reviewer-gpt reviews this document and the canon delta. Do not delegate implementation until the verdict is `Correct-to-merge`; fix and re-review every finding.

## Task 2 — Add RED behavior tests before production edits

**Vendored SDK files:**
- `vendor/matrix-rust-sdk/crates/matrix-sdk/src/client/builder.rs` or the actual `ClientBuilder` owner
- `vendor/matrix-rust-sdk/crates/matrix-sdk/src/client/mod.rs`
- `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/futures.rs`
- `vendor/matrix-rust-sdk/crates/matrix-sdk/tests/integration/encryption/issue_523.rs`
- existing #510 focused tests

**Koushi files:**
- `crates/koushi-sdk/src/lib.rs`
- focused `koushi-sdk` tests only

Add the smallest deterministic tests:

1. **#523 default-off RED:** build a default client with the existing issue-523 mock sequence. Assert normal pre-share occurs, but no second targeted `/keys/claim`, repair wake, or 1.5-second fence is created before index 0. This must fail before the gate because #523 currently runs unconditionally.
2. **#510 Koushi-default RED:** #523 deliberately removed #510's production caller, so default message sending is already behaviorally disabled even though Koushi still sets `.with_index0_duplicate_share(true)`. Add one `testing`-feature-only SDK helper that runs standard pre-share and then invokes the retained #510 helper exactly as its former production call did. Exercise that seam through `desktop_client_builder_defaults` and assert only standard pre-share. This must fail because the stale Koushi builder flag still enables the retained helper. The seam must not compile into normal SDK builds and must not expose cryptographic data. Avoid source-text assertions and do not restore a production caller.
3. Assert disabled behavior does not emit #510/#523 terminal diagnostics while normal initial-share diagnostics remain.

Only after recording those RED failures, add the explicit-opt-in continuity assertions with the production gate:

4. **#523 explicit-opt-in continuity:** the same fixture with the new builder option enabled preserves the existing targeted claim → Olm creation → exact `m.room_key` send before index 0.
5. **#510 explicit-opt-in continuity:** retain the existing SDK test proving one bounded duplicate while `message_index == 0` and none on later messages.

Record the focused commands and expected RED failures in the implementation worklog before production edits.

## Task 3 — Luna max implements the two independent runtime gates

**Delegation contract:**
- Agent: `luna-implementer`
- Thinking: `max`
- Context: focused prompt (`inherit_context: false` equivalent)
- Filesystem: write-capable, one dedicated worktree, no concurrent writer
- Allowed scope: the files listed in Tasks 1–3 plus focused tests
- Completion: RED tests turn GREEN; no unrelated formatting or refactor

Implementation shape:

1. Keep the existing #510 SDK option default-off; remove Koushi's `.with_index0_duplicate_share(true)` enablement.
2. Add an independent `ClientBuilder` boolean for #523, default `false`, with one explicit opt-in method named for initial-share repair.
3. Carry the option into the existing client runtime configuration using the same pattern as #510.
4. In `ensure_room_encryption_ready`, always run standard `preshare_room_key`; call `run_initial_share_repair` only when the #523 option is enabled.
5. Check the option before creating the deduplicated handler/future, deadline, or wake receivers so disabled mode has no hidden latency or task.
6. Update issue-523 integration fixtures to opt in explicitly. Do not weaken their assertions.
7. Do not couple #523 to the #510 flag and do not add a Koushi UI/env/config toggle.

## Task 4 — Focused and regression verification

Run in order and inspect every result:

```bash
cd vendor/matrix-rust-sdk
cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events issue_523
cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events index0_reshare
cargo test -p matrix-sdk-crypto --features testing issue_523
cargo fmt --all -- --check

cd ../..
cargo test -p koushi-sdk --lib issue_523
cargo test -p koushi-sdk --lib index0_reshare
cargo test -p koushi-sdk --lib initial_share
cargo test -p koushi-sdk --lib
cargo test -p koushi-core --lib
node scripts/check-sdk-submodule.mjs
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run qa:secret-scan
git diff --check
```

Run the mandatory aggregate local gate and the existing send-queue smoke against both supported homeservers:

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$PATH \
  npm --prefix apps/desktop run qa:headless-local -- \
    --server=both --timeout-ms=240000
PATH=/tmp/koushi-desktop-local-qa-bin:$PATH \
  npm --prefix apps/desktop run qa:headless-local -- \
    --server=both --scenario=send_queue --core --timeout-ms=240000
```

The focused SDK integration test, not a new broad QA harness, proves absence of the Koushi duplicate/repair request. Do not use a real account for development iteration.

## Task 5 — Audit and review gates

1. Inspect the complete root diff, submodule diff, untracked files, and submodule pin.
2. Prove from the diff that standard pre-share remains unconditional and only #510/#523 activation changed.
3. Confirm explicit opt-in tests keep both retained implementations live.
4. Confirm no timer/task/wake/diagnostic is created in disabled mode.
5. Run `reviewer-gpt` on the complete root and submodule diff. Any blocking or nonblocking finding requires fixes and a fresh verdict.
6. Re-run every affected focused gate after review fixes.
7. Open a focused PR recording:
   - default Element X-compatible path;
   - retained but disabled #510/#523 code;
   - RED→GREEN evidence;
   - reviewer-gpt design and final-diff verdicts;
   - all local and GitHub check results.
8. Merge only with all required GitHub checks green.

## Review / implementation record

- Pre-implementation design gate: `reviewer-gpt` — `Correct-to-merge` (2026-08-15), no findings.
- Canon-first update completed before tests in `REPOSITORY_RULES.md`,
  `docs/architecture/overview.md`, `docs/architecture/state-machine.md`, and
  `docs/upstream/matrix-rust-sdk-feedback.md`; this plan was already indexed in
  `docs/agents/plans.md`.
- RED: `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events issue_523` — exit `101`; `9 passed, 1 failed`. The new default-off behavioral test timed out at the 1.5-second repair-fence assertion, proving #523 currently runs unconditionally.
- #510 seam check: `cargo test -p koushi-sdk --lib koushi_default_builder_does_not_enable_index0_duplicate_share` — exit `0`; `1 passed`. This was not valid RED evidence: commit `3a7f5663b` deliberately removed #510's production caller when #523 superseded its blind duplicate, so ordinary sends cannot observe the stale `.with_index0_duplicate_share(true)` configuration.
- Design amendment: permit one `testing`-feature-only SDK helper that reproduces the former pre-share → retained #510 helper boundary. The Koushi builder test must use it to expose the stale enabled flag behaviorally, fail before removing the flag, and pass afterward. The production caller remains absent.
- Test-seam amendment gate: `reviewer-gpt` — `Correct-to-merge` (2026-08-15), no findings.
- No production behavior changes or vendored SDK commit were made before that amendment approval; subsequent GREEN evidence is recorded below.
- RED: `cargo test -p koushi-sdk --lib koushi_default_builder_does_not_enable_index0_duplicate_share` — exit `101`; the behavioral seam observed 2 encrypted to-device shares instead of the expected 1 (`left: 2`, `right: 1`), proving Koushi's stale #510 opt-in still activates the retained helper.
- GREEN: `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events issue_523` — exit `0`; 10 passed.
- GREEN: `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events index0_reshare` — exit `0`; 3 passed.
- GREEN: `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto --features testing issue_523` — exit `0`; 12 passed.
- GREEN: `cd vendor/matrix-rust-sdk && cargo fmt --all -- --check` — exit `0` (toolchain emitted only stable/nightly configuration warnings).
- GREEN: `cargo test -p koushi-sdk --lib koushi_default_builder_does_not_enable_index0_duplicate_share` — exit `0`; 1 passed.
- GREEN focused Koushi: `cargo test -p koushi-sdk --lib issue_523` — 1 passed; `index0_reshare` — 2 passed; `initial_share` — 4 passed; full `cargo test -p koushi-sdk --lib` — 139 passed.
- GREEN regression: `cargo test -p koushi-core --lib` — exit `0`; 1000 passed, 8 ignored. Normal `cargo check -p koushi-sdk`, `node scripts/check-sdk-submodule.mjs`, and `git diff --check` also passed.
- Parent integration reused the repository desktop dependencies: full Vitest, typecheck, lint, build, secret scan, agents-doc check, SDK-submodule guard, and root/submodule diff checks passed. `koushi-state`, `koushi-key`, `koushi-desktop`, `koushi-sdk`, and `koushi-core` tests passed; the policy's historical `koushi-auth` package name is absent from the current workspace. Vendored SDK formatting passed. Root `cargo fmt --all -- --check` remains blocked by pre-existing drift across unrelated core QA/account/room/store files and older `koushi-sdk` sections; the new Koushi test block was aligned to rustfmt output without reformatting unrelated code.
- The first aggregate local QA attempt hit its default 90-second cold-start timeout. Re-running with the documented 240-second budget passed the aggregate lane against Tuwunel and Synapse. The `send_queue --core` lane also passed against both servers with `safety`, `login_sync`, `send_fail`, `resend`, `cancel_send`, `fifo`, `unsent_restart`, and `restore_cleanup` evidence.
- Vendored SDK changes committed as focused submodule commit `5b55073fbbefd8a3c12f05ef2df599d2fe8d2abb` (`feat: disable optional Megolm send repairs by default`); parent review replaced the wall-time default-off assertion with a causal race between room send and a second repair claim, and the focused test passed. Root implementation and canon were committed after integration.
- Final complete-diff gate: `reviewer-gpt` returned `Correct-to-merge` for the root diff after one documentation-record fix, and separate exhaustive production/test passes returned `Correct-to-merge` for every vendored SDK hunk. No blocking or nonblocking findings remain.

## Re-enable criteria

Do not re-enable #510 or #523 merely because their tests pass. A later decision needs:

- measured recipient coverage improvement over standard pre-share;
- measured send-latency cost and timeout frequency;
- packaged Element X/Web/Koushi interoperability evidence;
- confirmation that the change is upstream-compatible or accepted upstream;
- a new explicit canon and product decision.
