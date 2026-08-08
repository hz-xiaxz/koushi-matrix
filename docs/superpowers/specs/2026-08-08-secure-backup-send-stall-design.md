# Secure Backup Send Stall Fix Design

## Problem

The Secure Backup durability fence introduced in Matrix SDK commit `98251f6d3`
can leave an encrypted message permanently pending after its local echo appears.
The send queue treats every `SecureBackupRequired` result as if account-level
admission had closed: it puts the request back into the queue and continues
without emitting `SentEvent` or `SendError`. When admission is still open, no
state transition is guaranteed to wake or terminate that request. The pending
request survives restart and encounters the same path again.

## Required behavior

- Never send encrypted content before the exact outbound Megolm session is
  confirmed absent from the active backup backlog.
- Account-level admission closure pauses queued encrypted messages without
  reporting a false failure.
- A per-session durability check may wait for at most five seconds.
- The wait ends immediately when backup reaches steady state; five seconds is a
  ceiling, not an added delay.
- A timeout or another per-session durability failure while admission remains
  open produces a recoverable send-queue error and a retryable local echo.
- Retrying re-enables the room queue and repeats the durability check.
- No path may immediately retry the same unchanged durability failure in a hot
  loop, and every admitted send must eventually produce either `SentEvent` or
  `SendError`.
- Existing plaintext send behavior is unchanged.

## Design

Keep the account-level admission latch and the per-session durability check as
separate decisions.

1. Before dequeue, a closed admission latch continues to wait on the existing
   notifier.
2. At the final encrypted-send boundary, wrap the backup steady-state wait in a
   five-second timeout.
3. If `SecureBackupRequired` is returned after dequeue, read the admission latch
   again:
   - when closed, return the request to pending and wait for the admission
     notifier;
   - when open, route the result through the normal recoverable `SendError`
     path, disabling that room queue until explicit retry.
4. Preserve the queued request and local echo in both cases. Never release the
   homeserver message request without successful durability confirmation.

The fix stays in the vendored Matrix Rust SDK. Koushi Core should need no new
state machine; its existing retry operation already re-enables the room queue
and unwedge/retries the pending request.

## Diagnostics

The existing Koushi `core.send` lifecycle must reach its failure terminal when
the SDK emits `SendError`. Do not add identifiers or raw SDK errors. Add only a
closed failure mapping if the existing mapping does not already classify the
new timeout as `secure_backup_required`.

## Tests

Test first in the vendored SDK:

1. Admission closed before dequeue keeps the encrypted request pending and does
   not emit `SendError`.
2. Admission open plus a durability failure emits one recoverable `SendError`
   instead of silently requeueing forever.
3. A delayed backup that settles within five seconds sends successfully.
4. A backup wait that exceeds five seconds never reaches the room send endpoint
   and produces a recoverable terminal update.

Add or adjust a focused Koushi Core contract test only if necessary to prove
that the SDK `SendError` settles `message_send` and leaves the local echo
retryable. Run the SDK submodule guard and focused Rust tests before PR.

## Non-goals

- Do not weaken the mandatory Secure Backup policy.
- Do not send after timeout.
- Do not redesign Secure Backup setup/recovery UI.
- Do not add automatic background retry policy or a new send state machine.
- Do not change Megolm rotation or key-sharing policy.
