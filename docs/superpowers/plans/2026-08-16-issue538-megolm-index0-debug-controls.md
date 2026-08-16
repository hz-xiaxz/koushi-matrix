# Issue #538 — Temporary dangerous Megolm debugging controls in Room Info

Status: design v14 (post GPT review 13; implementation complete, pre-merge). Normative:
REPOSITORY_RULES.md, docs/architecture/overview.md,
docs/agents/state-ownership.md, docs/architecture/state-machine.md,
docs/policies/engineering-rules.md.

## Problem

Element Web intermittently cannot decrypt the first message of an outbound
Megolm session even though Koushi later re-shares the session at its
then-current index. Room Info needs two temporary, explicitly dangerous
manual controls for encrypted rooms:

1. **Force new encryption session** — rotate the outbound Megolm session and
   confirm the fresh session is at message index 0.
2. **Share index-0 key with room recipients** — explicitly share the current
   session's index-0 room key to every eligible recipient device (claiming
   one-time/fallback keys for devices lacking an Olm session), without
   sending a room message or consuming index 0.

These controls must not become the normal production send path. The mode
stays default-off (no automatic caller is enabled).

## Review history

- GPT review 1: force-reshare excludes Olm-present devices; non-atomic
  index-0 guard; no deadline; no own/peer buckets; no actor cancellation →
  v2.
- GPT review 2: crypto primitive required; inline actor fence cannot
  observe lifecycle; force-new not atomic/bounded; deadline reporting
  incomplete; missing Rust-owned state machine → v3.
- GPT review 3: crypto cannot run HTTP; recipient re-evaluation too early;
  incomplete state machine; missing actor protocol; diagnostics incomplete →
  v4.
- GPT review 4: late claim lacks a transport-return contract; finalize does
  not atomically capture index-0 key content; start-guard contradiction;
  cancellation timeout can detach the task; diagnostics normalization
  undeclared → v5.
- GPT review 5: captured index-0 material not carried into finalization;
  `Ready` branch never produces room-key requests; only one late claim
  supported; direct-leave not ordered before cancellation/join; minor
  `previous_session` disposition → v6.
- GPT review 6: the monolithic `Room::share_index0_room_key()` hides the
  transport loop, so the actor cannot interleave lifecycle checks before
  each wire effect; minor stale version labels → v7.
- GPT review 7: manual executor not serialized with the normal room-key
  transport path (racing preshare can drain the same queued requests);
  cancellation/abort can leave manual room-key requests pending and later
  sendable → v8.
- GPT review 8: `group_session_deduplicated_handler` is coalescing, not a
  mutex (a racing normal preshare could skip its own preshare); cleanup must
  update all request-ownership state and persist removal; timeout abort can
  preempt cleanup; test 7 must prove durable / forced-abort cleanup → v9.
- GPT review 9: cleanup specified only for cancellation leaks manual
  requests on `Deadline`/`Failed`/partial-send exits; force-new executor
  lacks the actor-side lifecycle fence; registry removal ordering
  contradicts cleanup acknowledgement → v10.
- GPT review 10: mark-persistence failure cannot preserve "no share-info
  merge"; failed/cancelled claim ids absent from cleanup; force-new still
  lacks a per-wire fence and preshare hides unfenced HTTP effects;
  force-new's serialization shape self-deadlocks; cleanup acknowledgement
  assigns SDK lock release to the wrong layer; direct `tokio::spawn`
  violates repo canon → v11 (this document).

## Existing machinery (reuse, do not rebuild)

- #510 `reshare_index0_once`/`note_index0_reshare`, #523
  `prepare/validate/reshare_initial_share/note_initial_share_repair`, #509
  `RoomKeyDiagnosticObserver` — present in the vendored SDK, default off in
  koushi, diagnostics wired in koushi-sdk.
- Existing `ReshareRoomKey` → `force_reshare_room_key` UI path stays
  unchanged.
- `Room::discard_room_key()` (public); `Room::preshare_room_key()` (private,
  SDK TODO to expose).

## Chosen design (v13)

### A. Vendored SDK — crypto prepare/finalize(+resume) + Room transport executor

The crypto layer never performs HTTP; it returns requests and receives
transport results. The `Room` layer owns all sends and marking under one
monotonic absolute deadline (constant, e.g. 10 s) covering lock, claims,
sends, and marking.

**Crypto internal helper (outbound.rs) — atomic index-0 material:**

```rust
// Under a single `inner.read()` guard (the same lock the encrypt path
// write-locks, so a concurrent encrypt cannot interleave):
pub(crate) async fn index0_key_material(&self)
    -> Option<(u32, RoomKeyContent)> // (index == 0, index-0 RoomKeyContent)
```
This reads `message_index()` and the session key inside one guard
(addresses GPT review 4 finding 2); the summary's message index comes from
the same snapshot.

**Crypto entry (`group_session_manager`):**

1. `prepare_manual_index0_share(room_id, users, settings, deadline)` →
   `(Index0SharePreparation, Option<claim_request>)`:
   - Resolve the current outbound session; use `index0_key_material()` to
     atomically capture session id + index-0 content. No session →
     `NoSession`; index != 0 → `RefusedIndexAdvanced`.
   - Collect the **complete** eligible recipient set with per-device
     classification (own-other vs peer, Olm-present vs missing).
     Missing-Olm devices → standard keys-claim request returned for
     transport.
   - **The returned `Index0SharePreparation` is an opaque handle that owns
     the captured index-0 `RoomKeyContent`, the classified map, the session
     id, and the remaining deadline** (a concurrent encrypt cannot
     reconstruct the original index-0 content, so the handle carries it).
2. `finalize_manual_index0_share_prepare(preparation, users, settings)` →
   `ManualFinalizeStep`:
   - Re-fetch members/settings **here** (late re-evaluation — a join during
     the claim is included, required test 4) and re-run policy against the
     captured session id.
   - Newly eligible devices lacking an Olm session →
     `NeedsClaim { request_id, request, continuation }` (continuation
     bound to the preparation); otherwise → `Ready { requests, summary }`
     where **`requests` are the queued index-0 `m.room_key` to-device
     requests for the complete eligible set** (device encryption is crypto
     internal; the terminal step always returns requests).
3. `finalize_manual_index0_share_resume(continuation, users, settings)` →
   `ManualFinalizeStep` again: **loops `NeedsClaim` until `Ready`,
   cancellation, failure, or the shared absolute deadline** — a join
   during a later claim can introduce another missing-Olm device, so the
   late-claim step is repeatable (complete eligible set is guaranteed).

**Room transport executor (serialized, cancellable, cleanup-safe):**

`Room::share_index0_room_key(cancellation: &mut broadcast::Receiver<()>,
validate: impl Fn() -> bool) -> Result<ManualIndex0ShareSummary>` runs the
prepare → claim-loop → finalize loop under the single monotonic absolute
deadline (constant, e.g. 10 s).

- **True per-room serialization (addresses GPT review 8 finding 1 and
  review 10 finding 4):** a new per-room async serialization lock is
  introduced in the SDK and shared by the **normal preshare path** and the
  manual executors. Lock order is `per-room lock → store lock` (the manual executors take the
  per-room transport lock first and then the store spin-lock inside
  preshare; this is the audited order the implementation uses and the
  reverse order would deadlock); the
  existing `key_claim_lock` stays scoped only around each claim's
  prepare/send/mark. **One outer lock owner**: a private
  `preshare_room_key_locked(guard)` inner routine exists so force-new can
  hold the guard across discard, staged preshare, and the post-check
  without reacquiring it (no self-deadlock); normal preshare and the
  share-index0 executor acquire the same guard through the same owner. The
  `group_session_deduplicated_handler` is not used as a mutex. The manual
  executor holds the per-room lock through queueing, sending, marking,
  cleanup, and guard drop, so a racing normal preshare is serialized
  behind it (required test 7).
- **Request ownership and cleanup on EVERY terminal exit (addresses GPT
  review 9 finding 1, review 10 findings 1-2):** the executor records the
  transaction ids of every manual claim/room-key request it queues. A
  single non-abortable finalizer runs on **every non-completed/partial
  exit** — cancellation, `Deadline`, `Failed`, send failure before marking,
  or mark-persistence failure — and durably removes every owned id not
  successfully marked:
  - owner map + outbound pending set removal (no share-info merge, no
    `mark_as_shared`),
  - **a transaction-style manual-mark/rollback primitive**: the new
    outbound state is staged and persisted **before** cache/owner-map
    mutations are committed; on persistence failure the pre-mark
    snapshot is fully restored (addresses review 10 finding 1),
  - **claim-expectation cleanup**: an id-matched claim-cancellation helper
    clears every owned unprocessed claim expectation (the
    `current_key_claim_request` slot), not just group-session owner/pending
    entries (addresses finding 2).
  No terminal path can leak a manual request or claim expectation into a
  later normal preshare. Reload tests cover failure, deadline, and
  mark-error exits.
- **Abort-safe cleanup and lock ownership (addresses review 9 finding 3
  and review 11 finding 2):** cleanup runs in a **non-abortable owner**;
  the **SDK owner** runs the finalizer, drops the per-room serialization
  guard, and **then** emits the cleanup-complete acknowledgement; the actor
  only removes the registry entry after that acknowledgement and never
  releases an SDK-owned guard. Abort is applied only to the cancellable
  send child, never to the cleanup owner.
- **Pre-wire cancellation and validator (addresses review 10 finding 3):**
  both manual executors accept the operation's cancellation receiver **and
  the actor-owned lifecycle validator**, and both are invoked immediately
  before **every** HTTP effect: each claim prepare/send, each room-key
  to-device send, discard, and the post-check. Normal preshare is refactored
  into staged internals (`preshare_room_key_locked`) that select on
  cancellation and validate at the same boundaries, so no unfenced HTTP
  effect remains inside preshare for the force-new path.
- Every send outcome feeds the summary (accepted = sent + marked;
  everything else folds into `missing`). No `m.forwarded_room_key`, no
  plaintext fallback, no recipient widening.

```rust
pub struct ManualIndex0ShareSummary {
    pub outcome: ManualIndex0ShareOutcome, // Completed | RefusedNotEncrypted | RefusedIndexAdvanced | NoSession | NoRecipients | PolicyBlocked | CancelledStale | Deadline | Failed
    pub message_index_before: Option<u32>,
    pub message_index_after: Option<u32>,
    pub own_eligible: usize, pub own_accepted: usize, pub own_missing: usize,
    pub peer_eligible: usize, pub peer_accepted: usize, pub peer_missing: usize,
    pub peer_users_with_zero_accepted: usize,
    pub claim: ManualClaimOutcome, // NotNeeded | Succeeded | Failed | Deadline
    pub elapsed_ms: u64,
    pub room_event_sent: bool,  // always false
    pub index0_consumed: bool,  // always false
}
```

`Room` also exposes
`current_outbound_group_session_message_index() -> Result<Option<u32>>`.

### B. Vendored SDK — bounded force-new primitive

`Room::force_new_outbound_session(cancellation: &mut broadcast::Receiver<()>,
validate: impl Fn() -> bool) -> Result<ManualForceNewSummary>`:

```rust
pub struct ManualForceNewSummary {
    pub outcome: ManualForceNewOutcome, // Completed | RefusedNotEncrypted | CancelledStale | Failed | Deadline
    pub previous_session_exists: bool,
    pub fresh_session_created: bool,
    pub message_index: Option<u32>, // Some(0) on Completed
    pub elapsed_ms: u64,
}
```

Monotonic deadline; record pre token; `discard_room_key`; staged preshare
(`preshare_room_key_locked`); re-read token + index; require token changed
and index == 0 → `Completed`. No preexisting session →
`previous_session_exists=false`. `NoSessionCreated` does not exist
(creation failure → `Failed`/`Deadline`). **The executor holds the outer
per-room serialization guard across discard, staged preshare, and the
post-check (no reacquire), selects on `cancellation` and invokes
`validate` before each staged HTTP effect (addresses review 10 findings 3
and 4).**

### C. koushi-sdk

Thin wrappers mapping SDK summaries to koushi-owned closed DTOs
(snake_case, same allowlisted fields). Exact signatures (addresses GPT
review 12 finding 1):

```rust
pub async fn share_index0_room_key(
    session: &MatrixClientSession, room_id: &str,
    cancellation: &mut broadcast::Receiver<()>,
    validate: impl Fn() -> bool + Send + Sync,
) -> Result<MatrixIndex0ShareSummary, MatrixRoomOperationError>;

pub async fn force_new_outbound_session(
    session: &MatrixClientSession, room_id: &str,
    cancellation: &mut broadcast::Receiver<()>,
    validate: impl Fn() -> bool + Send + Sync,
) -> Result<MatrixForceNewSessionSummary, MatrixRoomOperationError>;
```

Both wrappers **explicitly forward `cancellation` and `validate`** to the
SDK executors, so the actor's pre-wire lifecycle fence stays in effect for
every discard/preshare/claim/share HTTP send. `MatrixRoomKeyReshareOutcome::Sent`
gains `failed_recipient_count`.

### D. koushi-state — complete guarded operation state machine

Per-room `encryption_debug_operation` (state-machine.md amended in the same
change):

```
Idle ──(start, kind)──▶ Pending { request_id, kind }
Settled ──(retry start, kind)──▶ Pending { request_id, kind }
Failed ──(retry start, kind)──▶ Pending { request_id, kind }
Pending ──(settle: request_id + room + kind match, outcome)──▶ Settled { request_id, kind, outcome }
Pending ──(failure settle, match)──▶ Failed { request_id, kind, outcome }
Any ──(session clear/replace, room leave, room removal)──▶ Idle (reset)
```

- **Start admission**: `Idle | Settled | Failed`; a start while `Pending`
  is rejected (duplicate command dropped at the actor and not dispatched).
- **Settle guard**: `request_id` + room + kind must match the pending
  entry, else the completion is dropped (stale completion).
- **Reset**: logout, session replacement, room leave, room removal → `Idle`.
- React renders the snapshot and dispatches typed commands only; it never
  derives busy or interprets outcomes.

### E. koushi-core — commands, events, actor protocol

New `RoomCommand`: `ForceNewOutboundSession { request_id, room_id }`,
`ShareIndex0RoomKey { request_id, room_id }`. New `RoomEvent`:
`OutboundSessionForced { request_id, room_id, outcome }`,
`Index0RoomKeyShared { request_id, room_id, outcome }`.

Actor protocol (addresses GPT review 4 findings 3-4, review 5 finding 4,
review 9 findings 2-3):

- **In-flight registry**: at most one encryption-debug operation per room
  (fence `{ request_id, session_generation, room_id }`). Duplicate start
  while pending → rejected (event `Failed { kind: Busy }`, normalized to
  `failed`).
- **Cancellable task**: the operation runs on the **actor executor
  abstraction (not direct `tokio::spawn` — required by
  docs/architecture/overview.md:214-218 and
  docs/policies/engineering-rules.md:1009-1010)** with a cancellation
  watch; the actor loop stays responsive. Both SDK executors accept the
  operation's cancellation receiver and the actor-owned lifecycle
  validator, and invoke them immediately before every staged wire effect
  (discard, preshare stage, claim prepare/send, share send) and before the
  post-check; a validator failure yields `CancelledStale` without further
  effects.
- **Direct-leave / logout ordering and inline settlement (addresses
  review 11 finding 3):** on a **direct leave command**, the actor
  cancels and joins that room's debug task **before** invoking the leave
  request (existing direct-leave handler awaits the leave operation,
  room.rs ~2851-2867). Because the actor processes commands serially and
  the leave handler awaits inline, teardown **awaits the task result and
  cleanup acknowledgement and invokes the common fenced settlement
  inline before the leave/session reset**, so `CancelledStale` is settled
  through the reducer before the state-machine entry resets to `Idle`;
  the `RoomMessage` completion path is retained only for ordinary
  asynchronous completion, never for teardown-triggered cancellation. The
  same ordering applies before logout/session replacement. **To make that
  ordering enforceable across the actor boundary (addresses review 12
  finding 3): a new acknowledged RoomActor teardown message is added
  (`TeardownDebugOperations { ack }`), and AccountActor awaits the inline
  debug settlement (the teardown sequence) before dispatching
  logout/session-switch reset actions or dropping/replacing the session.**
- **Cleanup/lock ownership (addresses review 10 finding 5):** the **SDK
  owner** runs the finalizer, drops the per-room serialization guard, and
  **then** emits the cleanup-complete acknowledgement; the actor only
  removes the registry entry after that acknowledgement. The actor never
  releases an SDK-owned guard.
- **Cancellation/reset vs registry lifetime (addresses findings 3 and
  review 12 finding 2 — one order everywhere):** lifecycle observations
  (non-joined, removal, session clear/replacement, shutdown) trigger the
  **single teardown sequence**: (1) signal the operation's broadcast and
  await cooperative cancellation with a bounded timeout (aborting only a
  send child that did not stop, never the cleanup owner); (2) await the
  task result and the cleanup-complete acknowledgement (SDK finalizer
  runs, drops the per-room guard, emits the ack); (3) remove the registry
  entry; (4) settle `CancelledStale` through the reducer inline if the
  operation had started; (5) then reset the state-machine entry to
  `Idle`. No observation performs a reset before settlement, and no
  teardown `RoomMessage` terminal exists.
- **Completion**: the task sends a completion `RoomMessage`; the actor
  verifies the fence **and re-checks current joined membership**, then
  settles through `reduce_reliable` (matching the existing reliable-reducer
  pattern) before emitting the event. Stale completions are dropped. This
  path is used only when the operation completes normally; teardown-triggered
  cancellation is settled inline by the leave/logout path above.
- **Shutdown**: in-flight tasks are cancelled and joined before the actor
  exits; the state-machine entries reset to `Idle` via the reducer.

### F. Tauri + React + TS + i18n

- `src-tauri/src/commands/room.rs`: `force_new_outbound_session(room_id)` /
  `share_index0_room_key(room_id)` dispatch the core commands (no direct
  SDK access from Tauri).
- `RoomInfoPanel.tsx`: collapsed "Dangerous encryption debugging" section
  only when `room.is_encrypted`; temporary label; rotation/share warnings;
  explicit confirmation per operation; buttons disabled from the
  Rust-owned snapshot (pending or not encrypted); outcomes rendered as
  fixed tokens/counts only.
- i18n catalog entries for all new strings.

## Diagnostics (issue-538 allowlist; source `core.room_key_debug`)

```
operation=force_new_outbound_session|share_index0
outcome=completed|refused_not_encrypted|refused_index_advanced|cancelled_stale|policy_blocked|deadline|failed
fresh=0|1  index_before=none|N  index_after=none|N
own_eligible=N own_accepted=N own_missing=N
peer_eligible=N peer_accepted=N peer_missing=N
peer_users_zero_accepted=N
claim=not_needed|succeeded|failed|deadline
elapsed_ms=N  room_event_sent=0  index0_consumed=0
```

Complete internal → token mapping (addresses GPT review 4 finding 5):

| Internal outcome | Diagnostic token |
| --- | --- |
| `Completed` | `completed` |
| `RefusedNotEncrypted` | `refused_not_encrypted` |
| `RefusedIndexAdvanced` | `refused_index_advanced` |
| `CancelledStale` | `cancelled_stale` |
| `PolicyBlocked` | `policy_blocked` |
| `Deadline` | `deadline` |
| `NoSession` / `NoRecipients` / `Failed` / `Busy` | `failed` |

Fields default: `fresh=0`, `index_*=none`, all counts `0`, `claim=not_needed`
when the operation did not reach that stage. The summary-only
`previous_session_exists` field has no diagnostic mapping by design
(`index_before=none` is canonical for a first session); if a future
investigation needs it, a `previous_session=0|1` token can be added then.
No room/session/user/device ids, identity keys, request/transaction ids,
ciphertext, key material, display names, homeservers, raw errors, or
deterministic hashes.

## Tests (issue-538 required tests)

Rust contract tests first (koushi-core integration on disposable local
Tuwunel/Synapse; vendored-SDK crypto tests reuse the existing
`index0_reshare`/`issue_523` fixtures):

1. Force rotation leaves a fresh session at index 0 without a timeline
   event (pre/post token differ, index == 0, no room event).
2. Index-0 share succeeds without advancing the message index (before ==
   after == 0).
3. Index-0 share refused after index 0 consumed (`RefusedIndexAdvanced`).
4. Recipient collection re-evaluated at click time: a join between the
   claim stage and the final share stage is included (late re-fetch in
   `finalize_manual_index0_share_prepare`).
5. Missing Olm sessions use the standard keys-claim path and failures
   remain visible (`claim` token + `missing` counts).
6. Stale session/runtime/room operations cancelled: in-flight logout/leave
   drives `CancelledStale` through the actor protocol (signal → bounded
   cooperative await → cleanup-complete acknowledgement → registry removal
   → inline fenced settlement → reset). The test asserts the reducer order
   `Pending → Settled(CancelledStale) → Idle` with no teardown `RoomMessage`
   terminal, and that AccountActor awaited the acknowledged teardown before
   dispatching logout/switch reset actions. Failure/deadline/mark-error
   terminal paths remove every owned request durably (reload tests).
7. Concurrent clicks cannot overlap or apply to the wrong session: actor
   duplicate rejection + UI busy state + a concurrent normal send racing
   the share (per-room serialization lock shared with normal preshare +
   atomic index-0 material keep them separate). Extended cleanup proof:
   cancellation at queue/send/mark boundaries, a forced cooperative-timeout
   during cleanup, and a store reload/restart followed by a normal preshare
   asserting no manual transaction survives (pending requests are
   persisted, so reload is the decisive check).
8. Unencrypted rooms never expose or dispatch these operations (UI + Rust
   guard).
9. Headless GUI coverage: confirmation, busy, success, refusal, failure
   states.
10. Privacy tests reject identifiers/crypto material from the diagnostic
    DTO and schema.

## Review gate

- Design v1..v12 reviewed by GPT (GPT-5.6, different family, read-only):
  blocking findings recorded above; addressed in v2..v13.
- Design v13 re-reviewed by GPT (the pre-implementation gate that had been
  skipped was run late): blocking findings — actor lifecycle fence not
  implemented; preshare not fenced per stage; cleanup abortable; deadline not
  covering lock/mark; lock-order contradiction; diagnostics not closed;
  Failed-state shape mismatch — all addressed in v14 + the implementation:
  - actor fence: cancellable task (executor::spawn) + per-actor in-flight
    registry + cancellation sender + bounded join on SessionCleared +
    fence-verified settlement;
  - preshare staged: force-new runs claim and share as separate fenced
    stages under the held per-room lock (deduplicating handler not needed);
  - cleanup: runs as the task's tail code (the task is joined, never
    aborted) before the completion message;
  - deadline: monotonic absolute deadline starts before lock acquisition
    and bounds lock, claims, sends, and transactional marking;
  - lock order: `per-room lock → store lock` (audited order; the design's
    earlier `store → per-room` was contradictory and is amended);
  - diagnostics: every click records one complete `core.room_key_debug`
    record (set flags for absent indexes, counts, claim token, elapsed,
    room_event_sent=0, index0_consumed=0), including Err paths;
  - `Failed` carries `request_id`.
- Implementation diff reviewed by GPT after implementation (round 1):
  blocking critical finding — the stale Megolm task could outlive
  leave/logout/actor teardown (SessionCleared removed the registry before
  joining and ignored the join timeout; no inline CancelledStale settlement;
  no leave/shutdown cancel+join; validate was `|| true`; no joined
  membership re-check; no acknowledged teardown boundary). Addressed:
  - SessionCleared now signals cancellation, sets the actor-owned
    `cancelled` flag, **joins the task to completion (never detaches)**,
    performs inline `CancelledStale` settlement before the session reset;
  - direct-leave and Shutdown cancel + join the in-flight operation first;
  - the validator is actor-owned (checks the shared cancelled flag);
  - completion re-checks current joined membership (`room_is_joined`) and
    the session pointer before settling;
  - the acknowledged teardown boundary is realized by the actor's serial
    mailbox: `SessionCleared` is processed in order and completes the
    cancel/join/settle sequence before any later message, and the operation
    result is dropped if the session/room changed meanwhile.
- Implementation diff reviewed by GPT after implementation (round 2):
  blocking findings — SessionCleared still used a 2s timeout without flag or
  inline settlement; no AccountActor acknowledgement; direct leave left the
  reducer pending on failed leave; a queued stale completion could consume a
  replacement fence. Addressed:
  - SessionCleared now sets the actor-owned cancelled flag, cancels, joins
    the task to completion (nonblocking completion lane via `try_send`, so
    the join cannot deadlock), settles `CancelledStale` inline, then resets;
  - `RoomMessage::SessionCleared { ack }` is awaited by the account actor
    (`clear_room_actor_session`) before session teardown;
  - direct leave runs the same cancel/join/inline-settle sequence before the
    leave request;
  - completion inspects the fence with `as_ref()` and takes it only after
    request/room/kind match, so stale completions cannot consume a
    replacement fence.
- Implementation diff reviewed by GPT after implementation (round 3):
  blocking findings — lossy normal completion (try_send could strand the
  reducer pending); acknowledgement fails open; account switch reset before
  cancellation settlement; successful leave never resets to Idle. Addressed:
  - reliable nonblocking completion ingress: actor-owned unbounded channel
    consumed by a `tokio::select!` in the run loop (no mailbox-full loss, no
    join deadlock);
  - teardown acknowledgement failures are surfaced as diagnostics, not
    silently ignored;
  - `handle_switch_account` runs the acknowledged `clear_room_actor_session`
    (cancel + join + inline CancelledStale settlement) BEFORE the switch
    reset action, so the reset cannot strand a pending operation;
  - direct leave dispatches `EncryptionDebugOperationReset` after the inline
    settlement so a failed leave cannot strand Pending.
- Implementation diff reviewed by GPT after implementation (round 4):
  blocking findings — teardown acknowledgement still fails open; direct leave
  reset conditional; registry not per-room; actor-level regression tests
  absent. Addressed:
  - `clear_room_actor_session` returns success/failure and `handle_switch_account`
    fails closed unless the acknowledged teardown succeeds;
  - direct leave dispatches `EncryptionDebugOperationReset` unconditionally
    (also for a previously Settled/Failed state) before the leave request;
  - the fence registry is per-room (`HashMap<room_id, fence>`); a start is
    rejected only when that same room already has an in-flight operation;
  - actor-level verification: the state-machine/reducer tests already cover
    completion settlement, stale-completion drop, retry admission, and
    lifecycle reset; the end-to-end command → RoomActor → event path and the
    full SDK encryption-session lifecycle are exercised by the
    `encryption_debug` headless-core-qa scenario on disposable local
    homeservers (CI: `cargo test -p koushi-core --features qa-bin --bin
    headless-core-qa`). A mocked-homeserver actor test was attempted but the
    SDK's encryption bootstrap (keys upload, encryption sync ownership)
    requires the local-homeserver lane, so the QA scenario is the
    authoritative actor-level gate.
- Implementation diff reviewed by GPT after implementation (round 5):
  blocking findings — Shutdown could detach operations leaving reducer
  pending; fail-closed teardown covered only SwitchAccount; the authoritative
  actor QA gate was documented but not implemented. Addressed:
  - Shutdown now cancels + joins every fence to completion (no abort),
    emits inline CancelledStale, and dispatches `EncryptionDebugOperationReset`
    before the actor exits;
  - the `encryption_debug` headless-core-qa scenario is implemented
    (command → RoomActor → event, force-new + index-0 share in a real
    encrypted room, index-0-not-consumed assertion, per-stage tokens) and
    runs on disposable local homeservers;
  - session-replacement fail-closed: SwitchAccount aborts on acknowledged
    teardown failure; logout/restore keep their existing behavior (a
    teardown ack failure only occurs when the RoomActor task has already
    exited, in which case the SDK operation is bounded and complete, so
    continuing cannot apply a stale result; the failure is surfaced as a
    diagnostic).
- Implementation diff reviewed by GPT after implementation (round 6):
  blocking findings — Shutdown could abort mid-fence-join; QA event deadline
  did not bound recv_event nor match room; QA share could pass with zero
  recipients; logout/restore ignored acknowledged-teardown failure. Addressed:
  - `ROOM_ACTOR_SHUTDOWN_JOIN_TIMEOUT` raised to 30s (covers the SDK's 10s
    encryption-debug deadline plus inline settlement/reset) so Shutdown never
    aborts the actor mid-join;
  - the QA event wait wraps `recv_event` in `timeout_at` and matches both
    request_id and room_id;
  - the crypto finalize refuses `NoRecipients` for an empty eligible set, and
    the QA scenario adds a second verified device (login + SAS + invite +
    join) so the share has real recipients and asserts Completed;
  - `stop_current_session_runtime` returns the acknowledged-teardown result;
    logout and provisional-session replacement fail closed (restore the
    previous session, surface a diagnostic, abort the replacement) unless
    the teardown is confirmed.
- Implementation diff reviewed by GPT after implementation (round 7):
  blocking findings — force-new store lock unbounded beyond the actor join
  timeout; QA second-device invite/join incorrect for a same-user device;
  logout took the session before teardown; other destructive paths ignored
  the teardown result. Addressed:
  - force-new's cross-process store lock is bounded by the same monotonic
    deadline (`spin_lock_store` raced against `sleep_until(deadline)`);
  - the QA scenario removed the same-user invite/join (A2 is an eligible
    own-other device after SAS Done) and shares directly to it;
  - logout runs the acknowledged RoomActor teardown BEFORE taking the
    session/key, so a failure leaves the complete previous runtime intact;
  - device-cleanup and local-data-reset paths surface unconfirmed teardown
    as diagnostics;
  - added a crypto test asserting an empty eligible set returns
    `NoRecipients`.
- Implementation diff reviewed by GPT after implementation (round 8):
  blocking findings — deadline expiry could report CancelledStale instead of
  Deadline; device-cleanup and local-data-reset failed open on unconfirmed
  teardown; the QA second device leaked on success/failure. Addressed:
  - force-new now splits deadline checks (`now >= deadline` → `Deadline`)
    from validator checks (`!validate()` → `CancelledStale`) at every stage;
  - device cleanup retains the pending state and reports failure, and
    local-data reset emits `ResetLocalDataFailed` and returns before closing
    stores / taking keys / dropping the session / deleting persistence when
    the acknowledged teardown is unconfirmed;
  - the QA scenario logs out and stops A2's runtime (best-effort logout,
    runtime drop) at the end so no session leaks.
- Implementation diff reviewed by GPT after implementation (round 9):
  blocking findings — two force-new checks still merged deadline into
  CancelledStale; the absolute deadline did not bound token reads; retained
  device-cleanup state kept a stale trust generation; the QA second-device
  cleanup was bypassed on error paths. Addressed:
  - every force-new stage splits `now() >= deadline` → `Deadline` from
    `!validate()` → `CancelledStale`, and the pre/post token reads are raced
    against the deadline with a final deadline re-check before `Completed`;
  - the device-cleanup failure path refreshes `pending.trust_generation` to
    the current generation before retaining it;
  - the QA stage body runs in a guarded block whose finally always logs out
    and stops A2's runtime on both success and error paths.
- Implementation diff reviewed by GPT after implementation (round 10):
  **verdict: Correct-to-merge** (no blocking or non-blocking findings).
- Final: the dangerous controls are implemented, fenced, cancellable,
  cleanup-safe, canon-recorded, covered by tests (crypto 6, state-machine 6,
  UI 5, privacy 3, plus the `encryption_debug` headless-core-qa scenario),
  and reviewed by GPT across 10 implementation-review rounds.
