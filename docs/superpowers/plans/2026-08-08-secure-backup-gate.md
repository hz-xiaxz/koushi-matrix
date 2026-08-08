# Mandatory Secure Backup Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Require an authoritative, recoverable Matrix Secure Backup before Koushi exposes encrypted sending, and render the mandatory setup/recovery states in the existing verification gate.

**Architecture:** Add one Rust-owned `secure_backup_gate` slice alongside the existing session state rather than duplicating the session state machine. Device verification may start the read-only sync runtime, but React exposes the normal shell and Core admits encrypted sends only when the additional gate is `Ready`. `koushi-sdk` maps Matrix SDK recovery/backup facts into one closed, privacy-safe inspection; `AccountActor` performs setup/recovery and projects state; Tauri transports the closed state; React extends `SessionVerificationGate` and uses existing native artifact and IME-safe secret-input primitives. The implementation deliberately reuses Matrix SDK upload settlement and does not build a parallel backup scheduler.

**Tech Stack:** Rust, matrix-rust-sdk, Tokio actors, serde DTOs, Tauri, React/TypeScript, Vitest/Testing Library.

## Global Constraints

- Encrypted sending is permitted only when device verification and Secure Backup readiness are authoritative.
- Existing server backups are never automatically deleted, reset, or replaced.
- Re-enabling an explicitly disabled backup is an explicit account-wide action whose effect on other Matrix clients is stated in the UI.
- Recovery Keys never enter Rust `Debug`, snapshots, Tauri output, React state, logs, diagnostics, screenshots, or clipboard; input uses `SecureImeTextField` and an uncontrolled DOM ref.
- All visible copy uses `MessageId` and both English and Japanese catalogs.
- Rust owns readiness, retries, and account-generation fencing; React renders state and dispatches commands only.
- Unencrypted-room sending remains unchanged.
- Avoid a new generic workflow framework, new backup scheduler, or duplicate secret-delivery path.

---

### Task 1: Canon and closed state contract

**Files:**
- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/policies/engineering-rules.md`
- Modify: `crates/koushi-state/src/state/session.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/effect.rs`
- Modify: `crates/koushi-state/src/reducer/session.rs`
- Test: `crates/koushi-state/tests/session_state.rs`

**Interfaces:**
- Produces: `SecureBackupGateState`, `SecureBackupGateFailureKind`, and `AppState::secure_backup_gate`.
- Produces: `AppEffect::InspectSecureBackup`. Secret-bearing recovery/setup and retry intent travels later as redacted Core commands, not reducer effects.

- [ ] **Step 1: Add reducer tests that currently fail**

  Cover verified-device transition to session-ready plus `SecureBackupGateState::Checking`, every non-ready backup state keeping the combined messaging gate closed, backup `Ready` opening it exactly once, runtime degradation closing only encrypted sends, and drafts remaining untouched. Stale Core completion fencing is covered in Task 3 where the generation exists.

- [ ] **Step 2: Run the focused tests and confirm the new symbols or transitions fail**

  Run: `cargo test -p koushi-state --test session_state secure_backup -- --nocapture`
  Expected: compile failure before the state contract exists, then assertion failures while reducer transitions are incomplete.

- [ ] **Step 3: Implement the smallest closed state contract**

  Use one tagged enum carrying only coarse facts in `AppState`; do not add another `SessionState` variant:

  ```rust
  pub enum SecureBackupGateState {
      Checking,
      ExistingBackupNeedsRecovery { failure: Option<SecureBackupGateFailureKind> },
      SecureStorageIncomplete,
      SetupRequired,
      ExplicitlyDisabledRequiresSetup,
      CreatingBackup,
      RecoveryKeyDeliveryRequired,
      UploadingExistingKeys { pending: PendingKeyCountBucket },
      DegradedRetrying { failure: SecureBackupGateFailureKind },
      BlockedFailed { failure: SecureBackupGateFailureKind },
      Ready,
  }
  ```

  Distinct SDK facts remain in the inspection type; the session enum contains only product states needed by Core and UI.

- [ ] **Step 4: Update canon with the exact state transition and side effect policy**

  Document `Verified -> AwaitingSecureBackup -> Ready`, runtime `Ready -> AwaitingSecureBackup`, read-only sync while gated, encrypted-only send closure, preservation of drafts, and explicit account-wide re-enable confirmation.

- [ ] **Step 5: Run state tests**

  Run: `cargo test -p koushi-state --test session_state secure_backup -- --nocapture`
  Expected: PASS.

### Task 2: Matrix SDK backup inspection adapter

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/error.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/futures.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/mod.rs`
- Test: `vendor/matrix-rust-sdk/crates/matrix-sdk/tests/integration/encryption/backups.rs`
- Modify: `crates/koushi-sdk/src/lib.rs`
- Test: focused tests in `crates/koushi-sdk/src/lib.rs`

**Interfaces:**
- Produces: `MatrixSecureBackupInspection` with closed server/local/recovery/upload/trust facts and `recommended_gate_state()`.
- Produces: `inspect_secure_backup()`, `recover_secure_backup()`, `setup_secure_backup()`, and `wait_for_secure_backup_steady_state()` on `MatrixClientSession`.
- Produces: an opt-in `SendMessageLikeEvent::require_backed_up_session()` SDK send fence used only by Koushi encrypted user-content sends.

- [ ] **Step 1: Add classification tests before network code**

  Test the Cartesian cases required by #462: enabled recovery plus enabled backup; existing backup without local key; no server backup; explicitly disabled recovery; unknown/probe failure; mismatch/incomplete; and upload failure. Assert no version, key, identifier, path, or raw error is serializable or printable.

- [ ] **Step 2: Run and verify failure**

  Run: `cargo test -p koushi-sdk secure_backup_inspection --lib -- --nocapture`
  Expected: compile failure until the inspection contract exists.

- [ ] **Step 3: Implement typed inspection from existing Matrix SDK APIs**

  Read `Recovery::state()`, `Backups::state()`, `fetch_exists_on_server()`, and upload settlement. Treat `Unknown` and probe errors as closed. Treat `RecoveryState::Incomplete` as recovery-required, and never call `recover_and_fix_backup`.

- [ ] **Step 4: Implement non-destructive recovery and setup wrappers**

  Recovery uses the SDK typed recovery path against the existing backup. Setup calls the existing `bootstrap_secure_backup` native-artifact workflow only when the inspection proves no server backup, or after the explicit-disabled action. Both wait for backup steady state before returning ready.

- [ ] **Step 5: Run adapter tests**

  Run: `cargo test -p koushi-sdk secure_backup --lib -- --nocapture`
  Expected: PASS.

- [ ] **Step 6: Add the minimal SDK per-session durability fence**

  Extend the existing room-send future with one opt-in boolean, not a new queue. After `ensure_room_encryption_ready()` has created/shared and persisted the outbound/inbound session pair, wait for `Backups::wait_for_steady_state()`, then verify that the same current inbound counterpart is marked backed up before encryption and HTTP send. If the session changed or is not backed up, return a closed SDK error before user content reaches the homeserver. Existing SDK callers retain current behaviour unless they opt in.

- [ ] **Step 7: Prove the fence order in the vendored SDK**

  Extend the existing backup integration fixture so the room-message endpoint is asserted uncalled while backup upload is held, then called once after upload confirmation. Add a rotation case proving a second session fences again and an existing session does not add a second upload.

  Run: `cargo test -p matrix-sdk --test integration encryption::backups -- --nocapture`
  Expected: PASS for the focused backup integration tests.

### Task 3: AccountActor lifecycle and encrypted-send admission

**Files:**
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/timeline.rs`
- Modify: `crates/koushi-core/src/runtime.rs`
- Test: `crates/koushi-core/tests/secure_backup_gate.rs`
- Test: focused actor tests in `crates/koushi-core/src/account.rs` and `crates/koushi-core/src/timeline.rs`

**Interfaces:**
- Consumes: Task 1 state/effects and Task 2 SDK adapter.
- Produces: account commands for inspect/recover/setup/retry; an AccountActor-owned `secure_backup_ready` admission fact fenced by session generation.

- [ ] **Step 1: Add headless actor tests**

  Prove verification does not start normal encrypted messaging; inspection ready promotes; disabled/incomplete/unknown remain gated; stale completion cannot promote another account; runtime loss closes encrypted sending; encrypted sends fail with a typed `SecureBackupRequired` failure; unencrypted sends pass; and draft content remains in Rust state.

- [ ] **Step 2: Run and confirm failure**

  Run: `cargo test -p koushi-core --test secure_backup_gate -- --nocapture`
  Expected: compile/assertion failure before AccountActor wiring.

- [ ] **Step 3: Wire post-verification inspection and generation fencing**

  Start sync in gated read-only mode, invoke inspection after verification, project only closed actions, and promote only a matching-generation `Ready`. Subscribe to backup/recovery state changes or re-inspect on their typed updates; any loss of readiness immediately clears the admission fact and projects degradation.

- [ ] **Step 4: Add the ordered encrypted-send barrier**

  Revalidate admission in `AccountActor` immediately before routing composer-affecting commands. Query room encryption without exposing room identifiers to diagnostics. Reject only encrypted rooms when the gate is closed and preserve the composer permit/draft; do not change unencrypted sends. Koushi's encrypted text/reply/edit send adapters opt into `require_backed_up_session()` so the first message under each new/rotated session cannot reach Matrix before the SDK marks its inbound counterpart backed up.

- [ ] **Step 5: Run Core tests**

  Run: `cargo test -p koushi-core --test secure_backup_gate -- --nocapture`
  Expected: PASS.

### Task 4: Tauri transport and mandatory gate UI

**Files:**
- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/e2ee.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.css`
- Test: `apps/desktop/src/SessionVerificationGate.test.tsx`
- Test: `apps/desktop/src/backend/client.test.ts`
- Test: `apps/desktop/src/backend/browserFakeApi.test.ts`

**Interfaces:**
- Consumes: Rust `SessionState::AwaitingSecureBackup` and account commands.
- Produces: gate screens that invoke `recoverSecureBackup`, `setupSecureBackup`, `confirmSecureBackupReenable`, and `retrySecureBackup`.

- [ ] **Step 1: Add failing UI and transport tests**

  Seed every Rust gate state and assert: no normal shell flash; no encrypted composer; checking/upload progress; recovery input; setup destination flow; explicit re-enable warning; typed failures; retry/diagnostics/logout; ready transition; stale account snapshot rejection; and runtime degradation preserving draft display.

- [ ] **Step 2: Run and verify failure**

  Run: `npm --prefix apps/desktop test -- --run src/SessionVerificationGate.test.tsx src/backend/client.test.ts src/backend/browserFakeApi.test.ts`
  Expected: FAIL until DTOs and rendering are added.

- [ ] **Step 3: Add closed DTOs and commands**

  Serialize only enum tags, pending-count buckets, retry-count buckets, and typed failures. Secret parameters remain command inputs with redacted `Debug`; they never return in snapshots.

- [ ] **Step 4: Extend `SessionVerificationGate`**

  Reuse the existing component and native Recovery Key artifact destination. Recovery Key uses `<SecureImeTextField ref={recoveryRef}>`; submit reads, normalizes, sends, and clears the DOM value without React state. No skip/close/Escape/deep-link path is added.

- [ ] **Step 5: Run UI tests**

  Run: `npm --prefix apps/desktop test -- --run src/SessionVerificationGate.test.tsx src/backend/client.test.ts src/backend/browserFakeApi.test.ts`
  Expected: PASS.

### Task 5: Catalogs, diagnostics, and privacy checks

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/i18n/messages.test.ts`
- Modify: existing diagnostics modules under `crates/koushi-core/src/` and `apps/desktop/src/domain/diagnostics.ts`
- Test: existing command-redaction and diagnostics tests

**Interfaces:**
- Produces: English/Japanese `secureBackupGate.*` messages and privacy-safe diagnostic tokens.

- [ ] **Step 1: Add failing catalog and privacy tests**

  Assert every new `MessageId` exists in both catalogs and that synthetic Recovery Keys, backup versions, user/device/room/session identifiers, paths, message bodies, and raw SDK errors are absent from Debug, DTO, diagnostics, and QA output.

- [ ] **Step 2: Add copy and coarse diagnostics**

  Add only server/local/recovery/upload/gate/fence status tokens and count buckets. Include the explicit cross-client re-enable warning in both locales.

- [ ] **Step 3: Run checks**

  Run: `npm --prefix apps/desktop test -- --run src/i18n/messages.test.ts`
  Run: `cargo test -p koushi-core --test command_redaction -- --nocapture`
  Expected: PASS.

### Task 6: Proportional verification and delivery

**Files:**
- Modify only files needed to fix failures from the commands below.

**Interfaces:**
- Consumes: Tasks 1-5.
- Produces: reviewable branch and PR for #462 and #463.

- [ ] **Step 1: Format and focused Rust verification**

  Run: `cargo fmt --all -- --check`
  Run: `cargo test -p koushi-state --test session_state -- --nocapture`
  Run: `cargo test -p koushi-sdk secure_backup --lib -- --nocapture`
  Run: `cargo test -p koushi-core --test secure_backup_gate -- --nocapture`

- [ ] **Step 2: Frontend safety and focused verification**

  Run: `npm --prefix apps/desktop run lint`
  Run: `npm --prefix apps/desktop test -- --run src/SessionVerificationGate.test.tsx src/i18n/messages.test.ts src/backend/client.test.ts`

- [ ] **Step 3: Repository guards**

  Run: `node scripts/check-sdk-submodule.mjs`
  Run: `npm --prefix apps/desktop audit --audit-level=high`
  Run: `npm --prefix apps/desktop audit --omit=dev --audit-level=high`

- [ ] **Step 4: Self-review the final diff**

  Verify issue coverage, no raw secret/input/state leakage, no automatic destructive backup operation, no unencrypted regression, and no unnecessary new abstraction.

- [ ] **Step 5: Commit, push, and open a ready-for-review PR**

  Use a focused commit message referencing `#462` and `#463`; include focused test evidence and clearly state that explicit re-enable changes the account-wide backup setting.
