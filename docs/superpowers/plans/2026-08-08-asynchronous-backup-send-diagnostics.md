# Asynchronous Backup Send Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow ordinary encrypted sends to complete without waiting for Secure Backup while continuously monitoring backup health and recording privacy-safe send failure classes.

**Architecture:** Koushi stops enabling the vendored SDK's opt-in per-session durability fence and stops rejecting encrypted content solely because the backup gate is degraded. `AccountActor` retains event-driven backup observation and owns one 60-second inspection timer. A focused core module converts SDK send errors into closed diagnostic tokens carried by the existing manager-global send terminal path.

**Tech Stack:** Rust, Matrix Rust SDK, Koushi actor runtime, structured Koushi diagnostics, Tauri release packaging.

## Global Constraints

- Normal Matrix recipient-device key sharing remains mandatory.
- Secure Backup upload follows asynchronously and is inspected every 60 seconds while the verified session is active.
- No raw SDK error, Matrix identifier, message content, endpoint, response body, or key material enters diagnostics.
- Retry/cancel/UI behavior and public DTOs remain unchanged.
- No vendored SDK behavior change is required.

---

### Task 1: Remove the opt-in send durability fence

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Test: existing focused unit tests in those two crates

**Interfaces:**
- Consumes: `Client::send_queue().require_secure_backup_for_encrypted_sends(bool)` and normal `Room::send`.
- Produces: sessions configured with the opt-in fence disabled and encrypted-content admission based on authoritative room encryption metadata rather than backup upload settlement.

- [ ] Write tests asserting encrypted content remains admitted when backup readiness is false and source contracts no longer opt into `require_backed_up_session`.
- [ ] Run the focused tests and confirm they fail under the current strict policy.
- [ ] Configure all new/restored clients with `require_secure_backup_for_encrypted_sends(false)`, admit authoritatively encrypted rooms independently of backup health, and remove the scheduled-send per-session fence opt-in.
- [ ] Re-run the focused tests and confirm they pass.

### Task 2: Add single-owner periodic backup inspection

**Files:**
- Modify: `crates/koushi-core/src/account.rs`
- Test: focused AccountActor source/behavior tests in `crates/koushi-core/src/account.rs`

**Interfaces:**
- Consumes: existing `RetrySecureBackupInspection`, `start_secure_backup_inspection`, and executor timer primitives.
- Produces: one owned periodic task scheduled at `Duration::from_secs(60)`, replaced on every settled inspection and cancelled during teardown.

- [ ] Write a failing test for one-owner scheduling, 60-second cadence, and teardown cancellation.
- [ ] Run it and confirm the existing actor lacks the periodic owner.
- [ ] Add the periodic task owner and route both degraded retry and healthy periodic inspection through one scheduler.
- [ ] Re-run the focused test and confirm it passes.

### Task 3: Preserve privacy-safe SDK send failure diagnostics

**Files:**
- Create: `crates/koushi-core/src/send_diagnostics.rs`
- Modify: `crates/koushi-core/src/lib.rs`
- Modify: `crates/koushi-core/src/timeline.rs`
- Test: `crates/koushi-core/src/send_diagnostics.rs`

**Interfaces:**
- Produces: `classify_send_failure(&matrix_sdk::Error, bool) -> SendFailureDiagnostic` with closed `reason` and `recoverable` fields.
- Consumes: `RoomSendQueueUpdate::SendError { error, is_recoverable, .. }`.

- [ ] Write failing classifier tests for Secure Backup, HTTP, concurrent-request, crypto/store, timeout, and fallback classes.
- [ ] Run the focused tests and confirm the classifier is absent.
- [ ] Implement the closed classifier, carry it through `SendCompletionObservation` and `ObservedSendTerminal`, and add `reason` plus `recoverable` to `core.send stage=sdk_terminal_observed`.
- [ ] Run focused Koushi Core tests, `cargo fmt --all --check`, and `node scripts/check-sdk-submodule.mjs`.
- [ ] Build the release DMG and verify the artifact exists and is a readable UDIF image.
- [ ] Commit, push, open a ready PR, monitor checks, merge, sync local `main`, and rebuild the final DMG from the merge commit.
