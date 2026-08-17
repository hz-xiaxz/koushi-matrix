# Issue #541 review follow-ups

## Scope

Fix the two blocking findings from the final PR review without changing the
one-shot index-0 resend contract:

1. cancellation during SDK manual request persistence must restore the
   in-memory outbound queue and retry bookkeeping;
2. authoritative room removal must cancel in-flight encryption-debug work and
   reset its Rust-owned UI state before the removed room can produce later
   effects.

The live A2 SAS prerequisite remains a separate infrastructure blocker and is
not bypassed by this change.

## SDK transaction boundary

`GroupSessionManager` will use a small RAII rollback guard around the three
manual request mutation paths:

- `finalize_manual_index0_resend`;
- `mark_manual_request_as_sent`;
- `cleanup_manual_pending_requests`.

The guard snapshots the outbound session before mutation. It restores the
snapshot when the future is dropped or returns an error. The finalize path also
removes newly registered `sessions_being_shared` entries on rollback; mark and
cleanup retain those entries until their save succeeds. Successful
`save_changes` returns only after the guard is disarmed, and synchronous
bookkeeping then commits the corresponding owner-map removal where required.

This makes a caller-side `timeout_at` safe: cancellation cannot strand a
partially mutated queue or manual owner. The existing store `Changes` write
remains the durable commit boundary; no new persistence abstraction is added.

Focused crypto tests will hold a save operation across cancellation for
finalize, mark, and cleanup, then assert the pending/manual request, durable
pickle, and `sessions_being_shared` owner map are unchanged and an explicit
retry remains possible. The delayed hook is placed before serialization/commit;
the SQLite store's `with_transaction` commit is atomic, so there is no
commit-then-drop window after the guard is disarmed. Existing injected
save-error tests remain.

The rollback guard is created only after all validation awaits and immediately
before the first mutation (`add_request_with_kind`/`mark_as_being_shared`, or
request removal). It is disarmed synchronously immediately after
`save_changes` returns `Ok`; the caller's owned-ID cleanup does not need to
cover a dropped-finalize ID because store atomicity plus the guard restore the
pre-mutation state.

## Authoritative room removal

The live room-list projection will compute the old-known minus new-authoritative
room IDs before publishing the authoritative snapshot. The known-room set is an
authoritative book only: `replace_known_room_ids` runs only for authoritative
snapshots, never for provisional snapshots that happen to contain payload. This
keeps provisional gaps from failing the operation validator and makes the next
authoritative old-minus-new diff complete.

The exact ordering is: update the authoritative known-room set, send the
removal message to the actor, then deliver the authoritative snapshot action.
The existing source-text ordering test will be updated to cover this deliberate
lifecycle ordering. It makes start admission and every per-effect validator
fail closed as soon as authoritative removal is observed, before reducer/UI
side effects are delivered.

It sends the removed IDs to the actor in a new internal `RoomMessage` before the
snapshot action. The actor uses one private cancellation helper shared by
shutdown/session-clear and room removal:

1. remove the matching encryption-debug fence;
2. set its cancellation flag and signal;
3. bounded-join, then use the existing abort fallback;
4. emit exactly one `CancelledStale` outcome and reset the reducer operation.

The operation validator also reads the actor's shared authoritative known-room
set, so a removed room fails closed before any subsequent wire effect. The
known-room set is updated before the snapshot action is delivered, while
provisional snapshots never generate removal messages.

The authoritative room-list reducer removes interaction entries for rooms no
longer present, gated on `authoritative` rather than crawler admission. When
entries are removed it emits `UiEvent::RoomInteractionsChanged`, so the
RoomInfoPanel cannot retain a stale pending operation. This prevents a pending
encryption-debug state from remaining visible after external leave/removal.
Stale completions find no matching fence and are ignored; a replacement
operation cannot be settled by the old request.

The per-removal helper uses the existing bounded join/abort fallback. Removed
rooms are processed in the actor loop with the existing shutdown join timeout;
the SDK cancellation receiver is checked at every wire-effect boundary, so the
normal path returns promptly and the total work remains bounded even if several
rooms disappear together. Cache-source authoritative snapshots are allowed to
send the same lifecycle message (there are normally no active fences during
bootstrap), which is harmless and keeps the projection path single-source.

Focused tests cover authoritative removal cancellation/reset, late completion
suppression, a new start rejected after authoritative removal, interaction
changed effect emission, and provisional removal not cancelling an operation.

## Verification

Run the focused crypto/core/state tests, then the existing full Rust,
frontend, privacy, documentation, and CI gates. Do not merge while the live A2
SAS prerequisite remains red or unproven.

## Review gate

Pre-implementation design reviewer: `reviewer-flash`, read-only and
cross-model. Implementation starts only after its verdict is Correct-to-merge
or all design findings are fixed and re-reviewed. Post-implementation review
must inspect the complete root and vendored-SDK diffs again.
