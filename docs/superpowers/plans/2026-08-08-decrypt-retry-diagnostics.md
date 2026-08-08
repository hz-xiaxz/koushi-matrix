# Undecryptable Message Retry Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make one exported diagnostic report distinguish where an undecryptable-message Retry stopped, without recording Matrix identifiers, message content, cryptographic material, or raw SDK errors.

**Architecture:** Rust owns the retry lifecycle inside the timeline actor. A bounded, actor-local pending record correlates backup lookup, device-key request, and later timeline decryption by an opaque process-local operation token; only fixed tokens enter `koushi-diagnostics`. React dispatches the command and renders state but does not infer cryptographic outcomes.

**Tech Stack:** Rust, Matrix Rust SDK adapter, `koushi-core`, `koushi-diagnostics`, React/TypeScript, Vitest.

## Global Constraints

- Implement GitHub Issue #466 and no unrelated behavior.
- Follow TDD strictly: add each failing behavioral/privacy test and observe RED before production code.
- Product semantics and the retry lifecycle stay Rust-owned; React must not classify backup or Matrix outcomes.
- Diagnostic records use fixed tokens only. Never record room/event/user/device IDs, Megolm session IDs, message bodies, homeserver URLs, paths, secrets, recovery material, backup versions, or raw SDK errors.
- Use one opaque process-local correlation token to connect stages. It must not be a Matrix identifier and must not enter product state.
- Keep the implementation bounded and minimal: no persistent incident database, no new retry scheduler, and no SDK fork unless a required typed outcome cannot be obtained through the existing adapter.
- Main and thread timelines use the same actor implementation.
- Repeated Retry for the same still-pending event coalesces or supersedes deterministically; stale actor generations cannot settle a current operation.

---

### Task 1: Rust-owned retry diagnostic lifecycle and thin UI wiring

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs` only if a typed, private-data-free adapter result is required
- Modify: `crates/koushi-core/src/timeline.rs`
- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Test: `crates/koushi-core/src/timeline.rs`
- Test: `apps/desktop/src/components/TimelineView.test.tsx`
- Modify only if the public wire contract changes: `crates/koushi-core/src/event.rs`, `apps/desktop/src/domain/coreEvents.ts`

**Interfaces:**
- Consumes: existing `TimelineActorMessage::RequestRoomKey`, `download_room_key_from_backup`, `request_room_key_for_event`, SDK timeline diffs, and `koushi_diagnostics::record`.
- Produces: fixed-token diagnostic records under source `core.decrypt_retry`, with stages `request`, `backup_lookup`, `device_request`, and `settled`.
- Internal pending record: actor-local and bounded, keyed internally by event identity but logging only an opaque operation token, start time bucket inputs, and bounded attempt count.
- Settled results: `decrypted`, `still_missing`, `withheld`, `malformed`, `timeout`, or `superseded`.

- [ ] **Step 1: Add failing tests for fixed diagnostic construction and privacy**

Add Rust tests that drive the diagnostic helper/lifecycle and assert exact fixed fields for:

```text
operation=decrypt_retry stage=request reason=missing_room_key
operation=decrypt_retry stage=backup_lookup result=found|not_found|network|forbidden|invalid_backup|timeout|sdk
operation=decrypt_retry stage=device_request result=sent|failed failure=network|forbidden|timeout|sdk
operation=decrypt_retry stage=settled result=decrypted|still_missing|withheld|malformed|timeout|superseded
```

The tests must serialize captured diagnostics and assert that synthetic room ID, event ID, user/device ID, session ID, body, URL, path, token, recovery key, backup version, and raw SDK error strings are absent.

- [ ] **Step 2: Run focused Rust tests and verify RED**

Run:

```bash
cargo test -p koushi-core --lib decrypt_retry_diagnostic
```

Expected: FAIL because the Rust-owned lifecycle and fixed stage records do not yet exist.

- [ ] **Step 3: Implement the minimal Rust-owned lifecycle**

Add a small private enum/helper set in `timeline.rs` for fixed reason/result/failure tokens. On `RequestRoomKey`:

1. Allocate or deterministically replace/coalesce one pending operation for the event.
2. Record `stage=request` with the projected undecryptable reason, opaque operation token, bounded attempt, elapsed bucket, and coarse Secure Backup readiness available to this actor. If the actor cannot access the gate without widening ownership, record only the safe backup-observation state available from the adapter and document that limitation in the PR.
3. Preserve `Ok(true)`, `Ok(false)`, and classified `Err` from backup lookup instead of merging miss and error.
4. Record device-request `sent` or typed failure.
5. Observe subsequent canonical SDK timeline diffs before publication. If the target changes from undecryptable to decryptable, settle exactly once as `decrypted`.
6. Use one bounded timeout message owned by the actor to settle a still-pending operation. Generation and operation token fence stale timeout completions.
7. Settle replacement as `superseded`; do not create unbounded tasks, maps, or records.

Do not add these facts to `AppState`, public timeline DTOs, or webview-visible product state unless a test proves a public contract is necessary.

- [ ] **Step 4: Run focused Rust tests and verify GREEN**

Run:

```bash
cargo test -p koushi-core --lib decrypt_retry_diagnostic
```

Expected: PASS with all fixed-token and privacy cases green.

- [ ] **Step 5: Add failing frontend test for removal of misleading classification**

Update `TimelineView.test.tsx` so Retry still calls `requestRoomKey`, but React no longer emits the misleading catch-all `operation=request_keys stage=failed kind=transport`. The test must fail against the old component behavior.

- [ ] **Step 6: Run focused frontend test and verify RED**

Run:

```bash
npm --prefix apps/desktop test -- src/components/TimelineView.test.tsx
```

Expected: FAIL because React still synthesizes the transport diagnosis.

- [ ] **Step 7: Make React a thin dispatcher and verify GREEN**

Remove only the duplicate cryptographic classification from `TimelineView.tsx`. Keep command rejection safely handled so there is no unhandled promise rejection; authoritative stage/outcome diagnostics come from Rust.

Run:

```bash
npm --prefix apps/desktop test -- src/components/TimelineView.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Run integrated focused gates**

Run:

```bash
cargo test -p koushi-core --lib
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test -- src/components/TimelineView.test.tsx src/App.diagnostics.test.tsx
node scripts/check-sdk-submodule.mjs
```

Expected: all commands exit 0 with no privacy leaks or contract drift.

- [ ] **Step 9: Self-review and commit**

Review `git diff origin/main...HEAD` and `git status --short` against Issue #466 and the Global Constraints. In particular, verify bounded cleanup, stale-generation fencing, exact one-time settlement, thread/main reuse, and absence of private values in all diagnostics and tests.

Commit with:

```bash
git add docs/superpowers/plans/2026-08-08-decrypt-retry-diagnostics.md crates/koushi-sdk/src/lib.rs crates/koushi-core/src/timeline.rs apps/desktop/src/components/TimelineView.tsx apps/desktop/src/components/TimelineView.test.tsx
git commit -m "feat: diagnose undecryptable message retries"
```

Stage only files actually changed; include wire-contract mirror files only if the implementation required them.
