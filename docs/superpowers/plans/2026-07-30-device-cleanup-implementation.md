# Failed Verification Device Cleanup Implementation Plan

> **For Codex:** Execute this plan task by task with the
> `superpowers:executing-plans` and `superpowers:test-driven-development`
> workflows. Preserve RED output and exact exit statuses. Do not combine #369
> live-session/device-naming work into this branch.

**Goal:** Replace the verification gate's local-only reset shortcut with an
explicit, Rust-owned, remote-first provisional-device cleanup flow.

**Architecture:** A `DeviceCleanupState` AppState slice alongside the Rust verification gate
owns offer, remote removal, UIAA, retry, local reset, and local-only escape
semantics. `AccountActor` keeps raw Device IDs, UIAA sessions, and SDK handles;
`koushi-sdk` maps legacy device deletion and OAuth token revocation into coarse
outcomes. Tauri and React only submit typed commands and render snapshots.

**Tech stack:** Rust, Matrix Rust SDK, Tokio actors, Serde DTOs, Tauri, React,
TypeScript, Vitest, Playwright, local Conduit/Tuwunel QA.

**Design:** `docs/superpowers/specs/2026-07-30-device-cleanup-design.md`

---

## Task 1: Amend the canonical state-machine contract

**Files:**

- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/policies/engineering-rules.md`
- Modify: `REPOSITORY_RULES.md` only if the work reveals a new durable rule

**Steps:**

1. Add the provisional-device cleanup ownership and remote-first invariant to
   the architecture overview.
2. Add a Mermaid state diagram with all `DeviceCleanupState` variants, guards,
   request/flow correlation, stale-input behavior, retry behavior, and reset
   behavior.
3. Add the private-safe `device_cleanup` diagnostic field allowlist and the
   legacy/OAuth split to engineering rules.
4. State explicitly that OAuth device naming and account-management link
   discovery remain #369.
5. Run:

   ```bash
   git diff --check
   ```

6. Commit:

   ```bash
   git add docs/architecture/overview.md docs/architecture/state-machine.md \
     docs/policies/engineering-rules.md REPOSITORY_RULES.md
   git commit -m "docs: define remote-first device cleanup state machine"
   ```

## Task 2: Add reducer contract tests first

**Files:**

- Modify: `crates/koushi-state/tests/session_state.rs`
- Modify: `crates/koushi-state/src/state/session.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/reducer/session.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`

**RED steps:**

1. Add tests for:
   - verification failure offers cleanup without changing the provisional
     session;
   - start from `Offered` and retry from `RemoteFailed`;
   - matching legacy UIAA challenge and submission;
   - OAuth never enters UIAA;
   - remote success and already-absent both enter local reset;
   - remote failure retains a retryable state;
   - local reset failure retries only the local stage;
   - local-only escape is admitted only after remote failure;
   - stale/duplicate request IDs and UIAA flow IDs are ignored;
   - promotion/logout/switch/rejection reset cleanup state.
2. Run the exact RED gate:

   ```bash
   cargo test -p koushi-state --test session_state device_cleanup
   ```

3. Record the non-zero exit status and the missing contract errors.

**GREEN steps:**

4. Add a private-safe `AppState.device_cleanup` slice and enums:
   - `DeviceCleanupState`
   - `DeviceCleanupAuthMode`
   - `DeviceCleanupOfferReason`
   - `DeviceCleanupRemoteOutcome`
   - `DeviceCleanupFailureKind`
5. Give the cleanup slice serde defaults needed for old fixtures and method
   discovery failures that occur before a gate exists.
6. Add request/settlement actions and reducer handlers.
7. Keep confirmation visibility out of Rust state.
8. Run:

   ```bash
   cargo test -p koushi-state --test session_state device_cleanup
   cargo test -p koushi-state --test session_state
   ```

9. Commit:

   ```bash
   git add crates/koushi-state
   git commit -m "feat(state): model provisional device cleanup"
   ```

## Task 3: Add SDK remote-cleanup classification tests first

**Files:**

- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-sdk/tests/password_login.rs` if the integration fixture
  is a better fit than the inline SDK test module

**RED steps:**

1. Add Matrix mock tests proving:
   - active session auth mode is `Legacy` for Matrix login and `OAuth` for an
     OAuth full session;
   - legacy cleanup sends the authoritative current Device ID;
   - a 401 UIAA response becomes `UiaaRequired` without exposing the UIAA
     session in `Debug`;
   - password continuation is used only for legacy cleanup;
   - `M_UNKNOWN_TOKEN`/generic not-found remains retryable because it does not
     prove the target Device ID is absent;
   - network/server failures become coarse private-safe failure kinds;
   - OAuth cleanup calls SDK OAuth logout/revocation and cannot return UIAA.
2. Run:

   ```bash
   cargo test -p koushi-sdk --lib device_cleanup
   ```

3. Record RED.

**GREEN steps:**

4. Add SDK-only types `SessionAuthMode`, `DeviceCleanupRemoteOutcome`, and
   redacted failure/continuation wrappers.
5. Implement `MatrixClientSession::auth_mode()` and a remote cleanup wrapper.
6. Reuse the existing `delete_devices` and OAuth logout primitives; do not call
   password UIAA from OAuth.
7. Classify only authoritative absent/revoked-session responses as
   `AlreadyAbsent`; keep ambiguous token/not-found responses retryable without
   retaining raw errors.
8. Run:

   ```bash
   cargo test -p koushi-sdk --lib device_cleanup
   cargo test -p koushi-sdk --lib delete_devices
   ```

9. Commit:

   ```bash
   git add crates/koushi-sdk
   git commit -m "feat(sdk): classify provisional device cleanup"
   ```

## Task 4: Add AccountActor ordering and failure tests first

**Files:**

- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/runtime.rs`
- Modify: `crates/koushi-core/src/failure.rs` if a new coarse failure mapping is
  required
- Modify: `crates/koushi-core/src/store.rs`
- Modify: `crates/koushi-core/src/event.rs` only if typed cleanup events are
  needed in addition to StateDelta

**RED steps:**

1. Add actor/reducer integration tests proving:
   - command admission is provisional-gate-only;
   - remote cleanup is observed before runtime stop or any persistence delete;
   - legacy UIAA stores the opaque continuation only in `AccountActor`;
   - matching UIAA continuation resumes cleanup;
   - OAuth failure does not produce an AwaitingUia state;
   - remote failure preserves `session`, `session_key_id`, and persistence;
   - already absent follows the same local-reset ordering as success;
   - reducer pending state always receives a matching failure settlement;
   - retry after `RemoteFailed` performs remote work again;
   - retry after `LocalResetFailed` performs local work only;
   - local-only escape skips remote work and reports that remote may remain;
   - stale completion cannot tear down a newer provisional session.
2. Extend store fault injection so local clearing can report an aggregate
   coarse result after attempting every removal.
3. Run:

   ```bash
   cargo test -p koushi-core --lib device_cleanup
   ```

4. Record RED.

**GREEN steps:**

5. Add cleanup `AccountCommand` variants with redacted `Debug` and correct
   `requires_ready_session = false` admission.
6. Project pending actions before routing to `AccountActor`.
7. Add actor-private pending UIAA/local-retry context and generation fencing.
8. Extract result-bearing account persistence clearing without weakening
   ordinary reset/logout cleanup.
9. Emit `device_cleanup` diagnostics through
   `koushi_diagnostics::record_and_stderr` at each specified stage.
10. Keep Device IDs, UIAA sessions, auth secrets, and raw SDK errors out of
    actions/events/diagnostics.
11. Run:

    ```bash
    cargo test -p koushi-core --lib device_cleanup
    cargo test -p koushi-core --lib reset_local_data
    cargo test -p koushi-core --lib server_logout
    ```

12. Commit:

    ```bash
    git add crates/koushi-core
    git commit -m "feat(core): run remote-first provisional cleanup"
    ```

## Task 5: Mirror the wire contract and add thin Tauri commands

**Files:**

- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src-tauri/src/commands/session.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/golden/frontend_app_state.json`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/domain/coreEvents.generated.json` if StateDelta
  serialization shape changes
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/types.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: Rust-shaped snapshots in `apps/desktop/src/test/` and IPC mocks

**RED steps:**

1. Add Tauri builder/serialization tests for all three cleanup commands and the
   maximally populated cleanup DTO.
2. Add TypeScript type/mocked API tests that require the new state and methods.
3. Run:

   ```bash
   cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib device_cleanup
   npm --prefix apps/desktop run typecheck
   ```

4. Record RED.

**GREEN steps:**

5. Mirror the Rust state exactly through Frontend DTOs and TypeScript.
6. Add thin commands:
   - `start_device_cleanup`
   - `submit_device_cleanup_uia`
   - `erase_local_data_anyway`
7. Update browser fake behavior only through Rust-shaped snapshots; do not add
   frontend-only outcome semantics.
8. Regenerate the frontend AppState golden with:

   ```bash
   UPDATE_GOLDEN=1 cargo test \
     --manifest-path apps/desktop/src-tauri/Cargo.toml \
     --lib frontend_app_state_golden
   ```

9. Update `coreEvents.generated.json` separately if the contract test requires
   it.
10. Run:

    ```bash
    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib device_cleanup
    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml \
      core_event_wire_format_matches_checked_in_contract_artifact
    npm --prefix apps/desktop run typecheck
    ```

11. Commit:

    ```bash
    git add apps/desktop/src-tauri apps/desktop/src/domain \
      apps/desktop/src/backend apps/desktop/src/test
    git commit -m "feat(desktop): expose device cleanup transport"
    ```

## Task 6: Add verification-gate component tests first

**Files:**

- Modify: `apps/desktop/src/SessionVerificationGate.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/i18n/messages.test.ts`
- Modify: `apps/desktop/src/styles.css` only for the minimum layout needed

**RED steps:**

1. Add production-default component tests proving:
   - SAS remains absent and undispatchable;
   - verification failure does not invoke cleanup automatically;
   - explicit cleanup opens a dialog with every required consequence;
   - canceling the dialog dispatches nothing;
   - confirming dispatches `start_device_cleanup`;
   - Rust `RemovingRemote`/`ResettingLocal` states disable duplicates;
   - legacy `AwaitingUia` uses `SecureImeTextField`, clears the password after
     submission, and sends no password to React state/logs;
   - OAuth snapshots never show the password form;
   - `RemoteFailed` shows retry and a separately confirmed local-only escape;
   - `AlreadyAbsent` proceeds through Rust state without frontend inference.
2. Run:

   ```bash
   npm --prefix apps/desktop test -- src/SessionVerificationGate.test.tsx
   ```

3. Record RED.

**GREEN steps:**

4. Replace the direct `resetLocalData` gate operation with cleanup API methods.
5. Keep only dialog visibility and transient password DOM ownership in React.
6. Add all strings to the i18n catalog and catalog tests.
7. Run the IME gates because a password field is added:

   ```bash
   node --test scripts/check-ime-text-inputs.test.mjs
   node scripts/check-ime-text-inputs.mjs
   npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx
   ```

8. Run:

   ```bash
   npm --prefix apps/desktop test -- src/SessionVerificationGate.test.tsx
   npm --prefix apps/desktop run test -- --run src/i18n/messages.test.ts
   npm --prefix apps/desktop run typecheck
   npm --prefix apps/desktop run lint
   ```

9. Commit:

   ```bash
   git add apps/desktop/src
   git commit -m "feat(ui): confirm remote-first device cleanup"
   ```

## Task 7: Add browser and local-homeserver acceptance proof

**Files:**

- Modify: `apps/desktop/e2e/session-verification-gate.spec.ts`
- Modify: `apps/desktop/src/test/appHarnessMain.tsx`
- Modify: `crates/koushi-core/src/bin/headless-core-qa.rs`
- Modify: `docs/qa/` scenario contract if a new token/scenario is introduced

**RED steps:**

1. Add browser-headless flows for confirmation, remote success, remote failure
   and retry, legacy UIAA, OAuth-without-UIAA, already absent, and separately
   confirmed local-only erasure.
2. Add or extend a local core QA scenario that logs in a throwaway device,
   reaches the provisional gate, removes that current device, observes the
   correlated cleanup state/events, and verifies a later login gets a new
   server-issued Device ID without printing either ID.
3. Run focused RED:

   ```bash
   npm --prefix apps/desktop exec -- playwright test \
     e2e/session-verification-gate.spec.ts -g "device cleanup" --workers=1
   cargo test -p koushi-core --lib device_cleanup
   ```

**GREEN steps:**

4. Complete only the harness/QA plumbing required by the Rust contract.
5. Run:

   ```bash
   npm --prefix apps/desktop exec -- playwright test \
     e2e/session-verification-gate.spec.ts --workers=1
   PATH=/tmp/koushi-desktop-local-qa-bin:$PATH \
     npm --prefix apps/desktop run qa:headless-local -- \
     --server=conduit --scenario=device_cleanup --core \
     --core-backend=probed --timeout-ms=240000
   ```

6. Commit:

   ```bash
   git add apps/desktop/e2e apps/desktop/src/test \
     crates/koushi-core/src/bin docs/qa
   git commit -m "test: prove provisional device cleanup end to end"
   ```

## Task 8: Update operational notes and verify the whole branch

**Files:**

- Modify: `AGENTS.md`

**Steps:**

1. Replace the “still open” #370 note with the landed state-machine,
   diagnostics, focused gates, and #369 boundary.
2. Run SDK guard:

   ```bash
   node scripts/check-sdk-submodule.mjs
   ```

3. Run focused and broad Rust gates without pipelines:

   ```bash
   cargo test -p koushi-state --test session_state
   cargo test -p koushi-sdk --lib
   cargo test -p koushi-core --lib
   cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
   cargo test --workspace
   ```

4. Run frontend gates:

   ```bash
   npm --prefix apps/desktop run typecheck
   npm --prefix apps/desktop run lint
   npm --prefix apps/desktop test
   npm --prefix apps/desktop exec -- playwright test \
     e2e/session-verification-gate.spec.ts --workers=1
   ```

5. Inspect exact exit statuses and retain logs under `/tmp`, outside the repo.
6. Commit:

   ```bash
   git add AGENTS.md
   git commit -m "docs: record provisional cleanup operations"
   ```

## Task 9: Self-review, publish, and merge

**Steps:**

1. Inspect every changed and untracked file:

   ```bash
   git diff origin/main...HEAD
   git status --short
   git submodule status vendor/matrix-rust-sdk
   ```

2. Judge the diff against `REPOSITORY_RULES.md`, architecture/state-machine
   canon, engineering rules, privacy, Rust/Tauri best practices, and this plan.
3. Fix findings with new RED tests where behavior changes.
4. Re-run every affected focused gate plus final broad gates.
5. Read the `superpowers:verification-before-completion`,
   `superpowers:requesting-code-review`, `github:yeet`, and
   `superpowers:finishing-a-development-branch` workflows before their
   respective actions.
6. Push `codex/issue-370-device-cleanup`.
7. Open a PR with `Closes #370`, design summary, upstream reference, exact
   verification evidence, privacy notes, #369 boundary, and no squash.
8. Watch every GitHub Actions check. If a check fails, use
   `github:gh-fix-ci`, reproduce locally, fix root cause, and rerun.
9. Merge with a merge commit only after all required checks are green.
10. Confirm #370 is closed and `origin/main` contains the merge commit.
