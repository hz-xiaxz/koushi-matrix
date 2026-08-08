# Secure Backup Send Stall Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every admitted encrypted send settle as sent or retryable failure while preserving the mandatory Secure Backup durability fence.

**Architecture:** Keep account-level admission waiting at the queue boundary, but treat per-session durability failures while admission remains open as normal recoverable send errors. Bound the backup steady-state wait to five seconds so the queue cannot remain inside an unobservable future forever.

**Tech Stack:** Rust, Tokio, Matrix Rust SDK send queue, Wiremock integration tests, Koushi Core diagnostics/contracts.

## Global Constraints

- The durability wait ceiling is exactly five seconds; successful waits return immediately.
- Never send encrypted content unless the exact outbound Megolm session is confirmed outside the active backup backlog.
- Admission closure pauses without emitting a false send failure.
- A durability failure while admission is open emits one recoverable `SendError` and leaves the local echo retryable.
- Never immediately retry the same unchanged durability failure in a hot loop.
- Plaintext sending and Secure Backup setup/recovery behavior remain unchanged.
- Do not log identifiers, key material, message content, or raw SDK errors in Koushi diagnostics.

---

### Task 1: Settle Secure Backup fence failures

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/futures.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/send_queue/mod.rs`
- Test: `vendor/matrix-rust-sdk/crates/matrix-sdk/tests/integration/encryption/backups.rs`
- Modify if required: `crates/koushi-core/src/timeline.rs`
- Test if required: `crates/koushi-core/tests/send_queue_fast.rs`
- Modify: `vendor/matrix-rust-sdk` gitlink

**Interfaces:**
- Consumes: `Backups::wait_for_steady_state()`, `SendQueue::secure_backup_send_is_admitted()`, `RoomSendQueueUpdate::SendError`, and Koushi's existing retry path that re-enables the room queue.
- Produces: a bounded `ensure_room_secure_backup_ready()` and queue handling that distinguishes a closed admission latch from an admitted durability failure.

- [ ] **Step 1: Write failing SDK regression tests**

Extend the existing Secure Backup integration tests with two focused cases:

```rust
#[async_test]
async fn admitted_durability_failure_emits_recoverable_send_error() -> TestResult {
    // Arrange an encrypted room with the durability policy and admission open.
    // Make the backup upload fail or remain unable to confirm the exact session.
    // Queue one text message and receive room send-queue updates.
    // Assert no /send/ request reaches the homeserver.
    // Assert exactly one SendError for the transaction and is_recoverable == true.
    Ok(())
}

#[async_test]
async fn closed_admission_keeps_request_pending_without_send_error() -> TestResult {
    // Arrange the same room with admission closed before queueing.
    // Assert no /send/ request and no SendError during a short observation window.
    // Reopen admission and prove the request can proceed once backup settles.
    Ok(())
}
```

Use synthetic room/session data already provided by the integration harness. Do not assert implementation-only counters.

- [ ] **Step 2: Run the regression tests and verify RED**

Run the exact new tests with:

```bash
cargo test -p matrix-sdk --test integration encryption::backups::admitted_durability_failure_emits_recoverable_send_error --features testing
cargo test -p matrix-sdk --test integration encryption::backups::closed_admission_keeps_request_pending_without_send_error --features testing
```

Expected: the first test times out or observes no `SendError` because `SecureBackupRequired` is silently requeued; the second establishes the unchanged admission-wait behavior.

- [ ] **Step 3: Add the five-second durability ceiling**

In `ensure_room_secure_backup_ready`, wrap only the steady-state wait:

```rust
const SECURE_BACKUP_SEND_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

tokio::time::timeout(
    SECURE_BACKUP_SEND_WAIT_TIMEOUT,
    room.client.encryption().backups().wait_for_steady_state(),
)
.await
.map_err(|_| Error::SecureBackupRequired)?
.map_err(|_| Error::SecureBackupRequired)?;
```

Keep all existing exact-session checks before the homeserver send request.

- [ ] **Step 4: Separate admission waiting from admitted failure**

In the `SecureBackupRequired` branch after dequeue:

```rust
if matches!(&err, crate::Error::SecureBackupRequired)
    && !room.client().send_queue().secure_backup_send_is_admitted()
{
    queue.mark_as_not_being_sent(&txn_id).await;
    notifier.notified().await;
    continue;
}
```

When admission is open, do not `continue`. Classify this specific error as recoverable, mark the request as not being sent, disable the room queue, emit the existing global room error, and emit `RoomSendQueueUpdate::SendError`. Reuse the normal recoverable-error path rather than introducing another state machine.

- [ ] **Step 5: Run focused SDK tests and verify GREEN**

Run:

```bash
cargo test -p matrix-sdk --test integration encryption::backups --features testing
cargo test -p matrix-sdk send_queue --features testing
cargo fmt --all --check
```

Expected: all selected tests pass, with no formatter diff.

- [ ] **Step 6: Commit the SDK change**

Inside `vendor/matrix-rust-sdk`:

```bash
git status --short
git add crates/matrix-sdk/src/room/futures.rs \
  crates/matrix-sdk/src/send_queue/mod.rs \
  crates/matrix-sdk/tests/integration/encryption/backups.rs
git commit -m "fix: settle secure backup send failures"
```

- [ ] **Step 7: Verify the Koushi terminal contract**

Confirm the existing `SecureBackupRequired` mapping and `SendError` terminal handling. Add a focused Koushi test only if the current tests do not prove that a recoverable SDK send error settles `message_send` and leaves the local echo retryable.

Run:

```bash
cargo test -p koushi-core --test send_queue_fast
cargo test -p koushi-core --lib room_key_reshare
node scripts/check-sdk-submodule.mjs
cargo fmt --all --check
```

Expected: all commands exit zero and the submodule guard reports that the SDK path and gitlink are synchronized.

- [ ] **Step 8: Commit the root gitlink and documentation**

At the repository root:

```bash
git add vendor/matrix-rust-sdk \
  docs/superpowers/specs/2026-08-08-secure-backup-send-stall-design.md \
  docs/superpowers/plans/2026-08-08-secure-backup-send-stall.md
git commit -m "fix: prevent secure backup send stalls"
```
