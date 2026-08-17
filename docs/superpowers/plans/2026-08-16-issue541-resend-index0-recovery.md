# Issue #541 — Temporary manual "resend index-0 key of current session" recovery control

Status: design v5 (pre-implementation; GPT review 4 found an underspecified legacy-pickle request-kind migration; corrected below). Normative:
REPOSITORY_RULES.md, docs/architecture/overview.md,
docs/agents/state-ownership.md, docs/architecture/state-machine.md,
docs/policies/engineering-rules.md.

## Problem

Koushi can create and initially share a fresh Megolm session, and the
homeserver accepts the peer's index-0 `m.room_key`, yet an Element Web peer
that was offline fails to decrypt the first message once it starts. The #538
diagnostic controls cannot test the recovery hypothesis once the outbound
session has advanced:

> The session has advanced past index 0. Force a new encryption session first.

We need a **temporary, one-shot manual control** that resends the original
index-0 key for the current Megolm session (even when `message_index > 0`),
so we can determine whether Element Web recovers without restart or client
changes.

Scope: implement only the manual diagnostic operation. **No** periodic or
activity-triggered automatic replay.

## Review history

- GPT review 1: **Critical** — `SharingView::iter_shares()` includes pending
  `to_share_with_set` entries and the committed `shared_with_set` includes
  later re-shares, so it is neither committed-only nor an immutable original
  index-0 ledger. The design incorrectly treated it as the original ledger.
  Corrected in v2 by adding a persisted, immutable `initial_share_ledger`
  captured only when the session's first share transaction completes; legacy
  sessions without that proof fail closed. GPT review 2 then found that the
  untyped request queue still permits later/manual requests to be mistaken for
  initial requests while `shared == false`. Corrected in v3 by tagging every
  outbound request with an explicit persisted `ShareRequestKind` and tracking
  an explicit initial-share batch; capture is allowed only for requests tagged
  `Initial` by the newly-created-session normal share path. GPT review 3 then
  found that forwarded-key Olm sessions were not persisted and that manual
  mark/rollback/cleanup did not update the new kind side-map. Corrected in v4
  by making forwarding persistence atomic with Manual request queueing and
  making every request lifecycle mutation update both maps transactionally.
  GPT review 4 then required an explicit legacy-pickle migration rule for
  pending requests whose kind map did not exist; v5 adds that rule.

## Existing machinery (reuse, do not rebuild)

- Issue #538 (`share_index0_room_key`, `force_new_outbound_session`,
  per-room transport lock, monotonic `MANUAL_ENCRYPTION_DEBUG_DEADLINE`,
  transactional mark + `cleanup_manual_pending_requests`, `encryption_debug`
  headless-core-qa scenario, `EncryptionDebugOperation*` state machine,
  `core.room_key_debug` diagnostics) — all in the vendored SDK, koushi-sdk,
  koushi-state, koushi-core, Tauri, React following the pattern below.
- Vendored SDK capabilities relevant to #541:
  - `OutboundGroupSession.shared_with_set` is persisted, but it is **not** the
    original ledger: it accumulates later re-shares, and
    `SharingView::iter_shares()` also chains pending `to_share_with_set`
    entries. It remains useful for normal sharing, but resend never enumerates
    it directly.
  - Add `initial_share_tracking_enabled: bool`,
    `initial_share_ledger: Option<ShareInfoSet>`, and an
    `initial_share_candidates: ShareInfoSet` to `OutboundGroupSession` and
    matching optional/defaulted pickle fields. New sessions set tracking
    enabled before any request is queued; old pickles restore it as disabled
    and are never eligible for ledger reconstruction. Add a persisted
    `ShareRequestKind` side-map for pending request ids: `Initial`, `Normal`,
    or `Manual`. This avoids changing the serialized shape of the existing
    request tuple while making request ownership explicit.
  - `share_room_key` passes `Initial` only when it created/rotated the
    outbound session, and passes `Normal` for later shares. The #538 manual
    share and #541 resend pass `Manual`. `mark_request_as_sent` removes the
    request's kind; for an `Initial` request it merges only `Shared` entries
    into `initial_share_candidates`. When no `Initial` request remains and
    `initial_share_ledger` is still `None`, it snapshots the candidates once
    and persists them with the normal outbound-session save. It never derives
    the snapshot from the untyped queue, from `shared_with_set`, or from a
    later/manual request. A request-kind mismatch or missing kind fails closed
    for resend. The immutable ledger is never mutated thereafter.
  - `from_pickle` handles legacy pending requests explicitly: if the old
    pickle has no request-kind map, it restores every existing pending request
    with `ShareRequestKind::Normal`, disables initial tracking, and never
    treats those requests as initial proof. If a kind map is present but its
    keys differ from the restored request map, loading rejects the pickle (or
    normalizes the entire session to fail-closed before exposing it); no
    request without exactly one kind is ever exposed to a sender/cleanup path.
    A session loaded from an old pickle with no initial ledger or request-kind
    proof has tracking disabled, has no resend targets, and fails closed
    (`OriginalLedgerMissing`); later requests cannot turn tracking on. Each
    ledger entry retains `ShareInfo::Shared(SharedWith {
    Curve25519PublicKey, message_index, olm_wedging_index })`; resend accepts
    only committed `Shared` entries and revalidates the current device's
    Curve25519 identity against the recorded `sender_key`. A changed key is a
    refusal, never a target widening.
  - The inbound Megolm counterpart is saved to our own crypto store when the
    outbound session pair is created
    (`changes.inbound_group_sessions.push(inbound)` in
    `create_outbound_group_session`). `store.get_inbound_group_session(room_id,
    session_id)` + `InboundGroupSession::first_known_index()`.
  - `Device::encrypt_room_key_for_forwarding(session, Some(0))` produces an
    **`m.forwarded_room_key`** to-device request (uses
    `export_at_index(0)`). This is the standard recovery form Element/Element
    X accept; no custom event type.
- The #538 manual share executor (`Room::share_index0_room_key`) is the
  structural template: per-room transport lock → monotonic deadline →
  prepare → claim loop → finalize → send+mark → cleanup on every terminal
  exit.

## Chosen design (v1)

### A. Vendored SDK — crypto prepare/finalize + Room transport executor

New manual **resend-index-0** operation. It differs from the #538 share in
three ways: it operates on the current outbound session **regardless of
message index**, it derives the target set from the **immutable persisted
initial-share ledger** (never `shared_with_set` or pending requests), and it
sends **`m.forwarded_room_key`** derived from the inbound counterpart's
index-0 export instead of `m.room_key`.

**Immutable original-ledger capture (`OutboundGroupSession`):**

- `mark_request_as_sent` continues to merge the request into
  `shared_with_set`, but uses the persisted request-kind side-map to update
  `initial_share_candidates` only for `Initial` requests while tracking is
  enabled. When the last `Initial` request is committed, it snapshots the
  candidates once into `initial_share_ledger`; this is independent of whether
  `Normal` or `Manual` requests are also pending. The snapshot is persisted
  with the outbound session save already used by the caller.
- The normal `share_room_key` path explicitly labels requests for a newly
  created/rotated session as `Initial`; all other paths explicitly label them
  `Normal` or `Manual`. Thus an isolated manual request, a later preshare, or
  a request interleaved while the initial batch is pending cannot enter the
  original ledger. If the initial batch is never committed, resend returns
  `OriginalLedgerMissing`.
- The normal mark path, `mark_manual_request_as_sent`, transactional
  `remove_request_captured`/`restore_request`, and cleanup all remove or
  restore the request tuple **and its request-kind entry together**. Any
  persistence failure restores both in-memory maps; successful removal
  persists both. No path may leave an orphan kind entry or a request without
  a kind.
- Add a crate-internal accessor that returns only the cloned immutable ledger.
  It must not expose `SharingView::iter_shares()`, `shared_with_set`, or
  pending request entries to the resend operation.

**Crypto entry (`OlmMachine`, machine/mod.rs):**

`prepare_manual_index0_resend(room_id, users, settings, deadline) ->
(ManualIndex0ResendPreparation, Option<(OwnedTransactionId,
KeysClaimRequest)>)`:

1. Resolve the current outbound session (`current_outbound_group_session`).
   None → `NoSession`. **No `message_index == 0` requirement.**
2. Load the **inbound counterpart** from our store:
   `store.get_inbound_group_session(room_id, outbound.session_id())`. Missing
   → `InboundSessionMissing`. `first_known_index() != 0` →
   `InboundIndexAdvanced` (refuse: no index-0 material to export).
3. Load the cloned **immutable initial-share ledger**. Missing/empty ledger →
   `OriginalLedgerMissing` (fail closed). Select only committed
   `ShareInfo::Shared(...)` entries, **excluding the current device**, and
   require each current device's Curve25519 key to equal the recorded
   `sender_key`. A changed/missing identity → `StaleIdentityRefused`; never
   fall back to the current member/device set. Pending requests and later
   re-shares are structurally unavailable to this operation.
4. Re-evaluate current membership/policy for those ledger entries, classify
   own-other vs peer (as #538), and return a keys-claim request for eligible
   ledger devices lacking an Olm session. The
   `ManualIndex0ResendPreparation` owns the inbound session id, immutable
   ledger-derived classification, and remaining deadline.

`finalize_manual_index0_resend(preparation, users, settings) ->
ManualFinalizeResendStep`:

- Re-run policy at this point (membership, history visibility, blacklist,
  dehydration, trust) over the **immutable ledger-derived** target set (a
  ledger device that is no longer a current member / is blacklisted /
  dehydrated / fails trust is dropped → `policy_blocked` count). A join during
  a claim cannot widen the target set; a device-list refresh can only remove
  or block a ledger target. A newly eligible current member that was not in
  the immutable ledger is never introduced. Missing-Olm ledger devices still
  use the repeatable `NeedsClaim` loop as in #538.
- Once no claim is needed, produce one `m.forwarded_room_key` per target
  device via `device.encrypt_room_key_for_forwarding(inbound, Some(0))` where
  `inbound` is re-loaded by `(room_id, session_id)` (matches #538 late
  re-evaluation). Each call returns a ratcheted Olm `Session`; collect all
  returned sessions.
- Queue the resulting requests as `Manual` requests on the outbound session,
  and persist `Changes { sessions: returned_olm_sessions,
  outbound_group_sessions: [outbound_with_manual_requests] }` **atomically in
  the same store save before returning `Ready`**. If this save fails, remove
  every queued request and restore the pre-operation request-kind map; do not
  return requests. This prevents Olm ratchet rollback after reload and makes
  the later send/mark/cleanup lifecycle durable.
- `shared_with_set` and `initial_share_ledger` are **not** modified (a resend,
  not a new share); only the pending Manual request queue and Olm sessions are
  changed.

Outcomes (`ManualIndex0ResendOutcome`): `Completed`, `RefusedNotEncrypted`,
`NoSession`, `InboundSessionMissing`, `InboundIndexAdvanced`,
`NoRecipients`, `OriginalLedgerMissing`, `PolicyBlocked`,
`StaleIdentityRefused`, `CancelledStale`, `Deadline`, `Failed`.

**Room transport executor** (`Room::resend_index0_room_key(cancellation,
validate) -> CryptoManualIndex0ResendSummary`): copy of the #538 executor
body — per-room transport lock, one monotonic absolute deadline starting
before lock acquisition, prepare → claim loop → finalize, send each
returned to-device request and `mark_manual_request_as_sent` transactionally,
`cleanup_manual_pending_requests` on every non-completed/partial exit. Every
send outcome folds into the summary buckets. `room_event_sent = false`,
`index0_consumed = false`.

```rust
pub struct ManualIndex0ResendSummary {
    pub outcome: ManualIndex0ResendOutcome,
    pub message_index_before: Option<u32>, // current outbound index
    pub message_index_after: Option<u32>,
    pub peer_eligible: usize, pub peer_accepted: usize, pub peer_missing: usize,
    pub peer_ledger: usize,       // ledger devices before policy re-eval
    pub peer_sender_key_changed: usize, // refused by identity mismatch
    pub policy_blocked: usize,
    pub inbound_first_known_index: Option<u32>,
    pub claim: ManualClaimOutcome,
    pub elapsed_ms: u64,
    pub room_event_sent: bool,    // always false
    pub index0_consumed: bool,    // always false
}
```

`Room` also exposes `current_outbound_group_session_id()` (already exists) and
`get_inbound_first_known_index(room_id, session_id) -> Option<u32>` for the
"missing/inbound-index-advanced" pre-checks surfaced to the UI.

### B. koushi-sdk

Thin wrapper (same shape as `share_index0_room_key`): `resend_index0_room_key(
session, room_id, cancellation, validate) -> Result<MatrixIndex0ResendSummary,
MatrixRoomOperationError>` mapping the SDK outcome enum to a closed
snake_case DTO with the same allowlisted fields. Privacy: counts + indices +
buckets only; no room/user/device/session ids, no curve keys, no key material.

### C. koushi-state — extend the guarded operation machine

`EncryptionDebugOperationKind` gains `ResendIndex0Key`. `EncryptionDebugOperationOutcome`
gains `InboundSessionMissing`, `InboundIndexAdvanced`, `StaleIdentityRefused`.
Transitions/guards unchanged (state-machine.md narrative + diagram updated in
the same change): start admission `Idle | Settled | Failed`, settle guard on
`request_id + room + kind`, lifecycle reset to `Idle`.

### D. koushi-core — command, event, actor

New `RoomCommand::ResendIndex0RoomKey { request_id, room_id }`; new
`RoomEvent::Index0RoomKeyResent { request_id, room_id, outcome }`. Actor:
`handle_encryption_debug_operation` gains the `ResendIndex0Key` branch calling
`koushi_sdk::resend_index0_room_key` (cancellable fenced task, actor-owned
validator, reliable nonblocking completion lane, inline `CancelledStale`
settlement on teardown — all identical to #538). `emit_encryption_debug_outcome`
gains the event/action mapping. Diagnostics record
`operation=resend_index0` with the allowlisted tokens.

### E. Tauri + React + TS + i18n

- `src-tauri/src/commands/room.rs`: add `resend_index0_room_key(room_id)`
  dispatching the core command (no direct SDK access); register it in
  `generate_handler!` (the #540 exhaustive registration test will enforce
  this).
- `RoomInfoPanel.tsx` dangerous section: add the manual resend button with a
  confirmation dialog stating that (1) it permanently grants index-0
  decryption capability for the session to the originally-shared recipient
  devices, (2) shared keys cannot be revoked, (3) it is temporary and for
  diagnosis only. No "all current members" control, no arbitrary device
  selection. Buttons disabled from the Rust-owned snapshot; outcomes rendered
  as fixed tokens only.
- i18n catalog entries (en + ja) for all new strings, including the new
  outcome tokens.

## Diagnostics (issue-541 allowlist; source `core.room_key_debug`)

```
operation=resend_index0
outcome=completed|refused_not_encrypted|no_session|inbound_session_missing|inbound_index_advanced|no_recipients|original_ledger_missing|policy_blocked|stale_identity_refused|cancelled_stale|deadline|failed
index_before=none|N  index_after=none|N
peer_ledger=N peer_sender_key_changed=N peer_eligible=N peer_accepted=N peer_missing=N policy_blocked=N
inbound_first_known_index=none|N
claim=not_needed|succeeded|failed|deadline
elapsed_ms=N  room_event_sent=0  index0_consumed=0
```

No room/user/device/session ids, curve keys, request/transaction ids,
ciphertext, key material, display names, homeservers, raw errors, or
deterministic hashes.

## Tests (issue-541 required tests)

Rust contract tests first (vendored-SDK crypto tests reuse the
`manual_index0_share`/issue-538 fixtures; koushi-core integration on local
homeservers via the `encryption_debug` QA scenario which is extended):

1. Resend succeeds with `message_index > 0` (index-before > 0, index-after
   unchanged, index-0 material exported, no room event, no rotation, no
   shared_with_set change).
2. Inbound counterpart absent → `InboundSessionMissing` refusal.
3. Inbound counterpart `first_known_index > 0` → `InboundIndexAdvanced`.
4. Session id mismatch between outbound and stored inbound → refusal
   (`NoSession` / stale).
5. Immutable-ledger targets only: a device added to the room after the
   initial share is **not** targeted (not in `initial_share_ledger`); a later
   normal re-share or pending request cannot widen the set; interleaved
   `Initial`/`Normal`/`Manual` requests and a store reload snapshot only the
   explicitly tagged Initial batch; a legacy session without the persisted
   proof returns `OriginalLedgerMissing`; a ledger device whose stored
   Curve25519 changed → `StaleIdentityRefused`, not sent.
6. Policy re-evaluation before send: a ledger device that left / is
   blacklisted / dehydrated is dropped → `policy_blocked` count; a join
   during the claim stage is handled (late re-fetch).
7. Missing-Olm ledger devices use the standard keys-claim path; failures
   visible in `claim` + `missing`.
8. Forwarded encryption persists every returned Olm `Session` together with
   the queued Manual requests; a forced store-save failure rolls back both
   request tuple and request-kind side-map. Success, mark, rollback,
   cancellation, deadline, partial-send, cleanup, and store reload tests
   prove no orphan kind entry and no manual `m.forwarded_room_key` request
   survives to a later normal preshare. Per-room serialization lock shared
   with normal preshare; UI busy + actor duplicate rejection.
9. Unencrypted rooms never expose/dispatch; refusal paths (all terminal
   outcomes) render.
10. Privacy tests reject identifiers/crypto material from the diagnostic DTO
    and schema.
11. headless-core-qa `encryption_debug` scenario extended: after the #538
    steps, advance the outbound index and run one resend; assert Completed,
    index unchanged, only ledger devices accepted, and (where buildable)
    refusal when the inbound counterpart / identity is missing.

## Review gate

- Design v1–v4 reviewed by GPT (read-only) with blocking target-ledger,
  request-attribution, forwarded-session persistence, cleanup, and legacy
  migration findings. Design v5 must receive a fresh GPT `Correct-to-merge`
  verdict before implementation.
- Implementation diff reviewed by GPT after implementation (round 1): five
  Important findings — final identity revalidation could partially send,
  finalization could widen targets, tuple/kind rollback and pickle snapshot
  were incomplete, claim/elapsed diagnostics were inaccurate, and focused
  security/rollback tests were missing. Addressed in SDK commit
  `38a784266`: every prepared ledger identity is validated fail-closed before
  any encryption, final policy is intersected with prepared targets, request
  tuple+kind share one lock, mark/cleanup restore on persistence failure,
  pickle captures one request-state snapshot, claim state/outcome and elapsed
  are explicit, and the happy-path SDK/QA invariant coverage is extended.
- A fresh GPT re-review is required after these fixes; `Correct-to-merge` is
  required before opening the PR.
