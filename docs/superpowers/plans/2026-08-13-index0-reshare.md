# Bounded Index-0 Duplicate Share Implementation Plan (Issue #510)

> **For agentic workers:** Implement task-by-task with a failing focused test
> before each production change (RED → GREEN). Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** For a newly created outbound Megolm session only, perform at most one
bounded duplicate share of the room key at message index 0 — after the normal
preshare settles and before the first room event consumes index 0 — so a device
that missed only the initial key share can decrypt the first event without
waiting for a later `m.room_key_request`. The duplicate is bounded by an
explicit short deadline, never repeats for the same session, re-evaluates
recipient policy at share time, and never changes the wire protocol or security
policy.

**Architecture:** The bounded duplicate is orchestrated inside the vendored
Matrix Rust SDK's room send path (observation in `matrix-sdk-crypto`, send
orchestration in `matrix-sdk`), so every Koushi send through `room.send()`
gets the guarantee without React or koushi-sdk duplication. The crypto layer
decides and queues the duplicate and reports closed diagnostics through the
existing `RoomKeyDiagnosticHub`; the matrix-sdk layer sends the queued
`m.room.encrypted` to-device requests under a short `tokio::time::timeout`
deadline and reports `sent | failed | deadline`. Koushi projects the new typed
event to fixed-token records and aggregate counters. This builds directly on
the #509 initial-share diagnostics.

**Tech Stack:** Rust, matrix-sdk-crypto (vendored), matrix-sdk (vendored),
koushi-sdk, koushi-diagnostics.

## Global Constraints

- Implement GitHub Issue #510 and no unrelated behavior.
- Only a newly created outbound Megolm session (message index still 0) is
  eligible; at most one duplicate attempt per session per runtime.
- Re-evaluate membership, trust, blacklist, history visibility, and collect
  strategy immediately before the duplicate; never broaden recipients, never
  send to the current device, and never send to left/blocked/untrusted or
  strategy-excluded devices.
- Use only standard Olm-encrypted `m.room_key` (delivered as an
  `m.room.encrypted` to-device request). No plaintext fallback, no unbounded
  retry, no custom acknowledgement protocol.
- A homeserver-accepted duplicate is not a recipient decryption proof.
- The duplicate never downgrades encryption; a failed or timed-out duplicate
  still lets the first event encrypt at index 0.
- Fence by account/runtime, room, and outbound-session identity; cancel stale
  work on rotation/discard, room leave, logout, runtime replacement, shutdown,
  or a stale expected session.
- Keep later standard `m.room_key_request` / `m.forwarded_room_key` recovery
  intact (do not touch the inbound-session answer path).
- Diagnostics: closed tokens only (`initial_share=accepted|failed|withheld|
  no_recipients`, `index0_reshare=sent|deadline|cancelled|policy_blocked|
  failed|not_needed`), eligible own/peer count buckets, elapsed bucket, and
  first-event index; never misreport a later post-send current-index re-share
  as index-0 repair.
- SDK changes are committed as independent commits in the vendored submodule;
  the root gitlink update is a separate commit in the Koushi PR.

---

### Task 1: SDK (crypto) — failing tests for the index-0 duplicate decision

**Files:**
- Add: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/tests/index0_reshare.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/tests/mod.rs`

Tests (each RED against the current SDK):

1. `index0_reshare_queues_one_duplicate_while_message_index_is_zero` — after a
   settled preshare, `reshare_index0_once` queues one duplicate `m.room_key`
   request for every eligible peer device; the session message index is still
   0 afterwards.
2. `index0_reshare_never_repeats_for_the_same_session` — a second call returns
   `not_needed`; after the first event encrypts (index 1), a later call is
   `not_needed` and no new request is queued.
3. `index0_reshare_reevaluates_recipient_policy` — a device blacklisted (or
   whose user left) between preshare and the duplicate is excluded.
4. `index0_reshare_never_rotates_and_blocks_when_rotation_pending` — when the
   recipient re-evaluation says the session must rotate, the duplicate returns
   `policy_blocked` without creating or rotating the session.
5. `index0_reshare_stale_session_is_cancelled` — if the active session changed
   between decision and queue, the attempt is `cancelled` and no stale request
   is queued.
6. `index0_reshare_no_recipients_is_not_needed` — an empty eligible set
   returns `not_needed` without queuing.
7. Privacy: `Debug` of the emitted diagnostics contains no identifiers.

- [x] **Step 1:** Add the failing tests.
- [x] **Step 2:** Run and confirm RED:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto --features testing index0_reshare
```

### Task 2: SDK (crypto) — implement the decision, queue, and diagnostics

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/session_manager/group_sessions/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/room_key_diagnostics.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/lib.rs`

Add:

- `Index0ReshareOutcome` — `Sent | Deadline | Cancelled | PolicyBlocked |
  Failed | NotNeeded` (closed).
- `Index0InitialShareState` — `Accepted | Failed | Withheld | NoRecipients`
  (closed, derived from the #509 per-session tallies).
- `Index0ReshareDiagnostic` — session alias, initial-share token, reshare
  token, eligible own/peer count buckets, elapsed_ms; new
  `RoomKeyDiagnosticEvent::Index0Reshare(...)`.
- Hub state: per-session withheld-device count in the #509 tally and an
  in-memory per-(room, session) `index0_reshare_attempted` one-shot set.
  Hub methods: `index0_reshare_attempted`, `mark_index0_reshare_attempted`,
  `note_index0_reshare` (derives the initial-share token and buckets, emits
  the event once per attempt).
- `GroupSessionManager::reshare_index0_once(room_id, users, settings)`:
  1. load the outbound session; absent → `NotNeeded` (no diagnostic);
  2. `message_index() != 0` or already attempted → `NotNeeded` (no
     diagnostic; the no-repeat case is observable via the first attempt's
     record and the `not_needed` record when the window was open);
  3. re-evaluate recipients with `collect_session_recipients` (membership,
     trust, blacklist, history visibility, strategy);
  4. `should_rotate` → `PolicyBlocked` (never rotate here);
  5. re-check the active session identity → mismatch → `Cancelled`;
  6. filter to eligible devices (not the current device, not already shared,
     no pending share); none → `NotNeeded`;
  7. queue the duplicate via `encrypt_for_devices` (standard Olm-encrypted
     `m.room_key`), persist, mark the one-shot flag;
  8. return the new requests plus the session identity; terminal decisions
     emit their diagnostic immediately.
- `OlmMachine::reshare_index0_once` and
  `OlmMachine::note_index0_reshare(room_id, session_id, outcome)` delegations.

- [x] **Step 3:** Implement.
- [x] **Step 4:** Run the focused tests and confirm GREEN.

### Task 3: SDK (matrix-sdk) — bounded send orchestration before the first event

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-base/src/client.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/futures.rs`
- Add: `vendor/matrix-rust-sdk/crates/matrix-sdk/tests/integration/encryption/index0_reshare.rs` (+ register in `main.rs`)

Add:

- `BaseClient::reshare_index0_once(room_id)` mirroring the #477
  `reshare_unwedged_room_key` shape (uses `room_key_share_context` for
  history-visibility-aware members and settings).
- In the encrypted message send path (`SendMessageLikeEventRaw` /
  `SendMessageLikeEvent` futures), after `ensure_room_encryption_ready`
  (and the secure-backup gate when required) and before the first
  `encrypt_room_event_raw`: call `ensure_index0_duplicate_share(room)`:
  1. `base_client().reshare_index0_once(room_id)`; terminal decisions were
     already reported by the crypto layer;
  2. if requests were queued, send each to-device request and mark it as sent
     inside `tokio::time::timeout(INDEX0_RESHARE_DEADLINE, ...)` with a short
     constant deadline;
  3. report `Sent | Failed | Deadline` through
     `Client::note_index0_reshare` (observation only; never blocks the
     message beyond the deadline, never changes the encrypted content).

Integration tests with the `MatrixMockServer`:

1. `send_first_room_event_queues_exactly_one_index0_duplicate` — one
   `room.send` produces the preshare to-device request and exactly one
   duplicate to-device request, then the room event at message index 0
   (verified through the observer events), then a second `room.send` produces
   no duplicate.
2. `duplicate_send_failure_never_downgrades_the_first_event` — the duplicate
   to-device endpoint fails; the room event still encrypts at index 0 and is
   sent.
3. `duplicate_deadline_is_bounded_with_controlled_time` — the duplicate
   to-device endpoint stalls; with `tokio::time::pause()` +
   `tokio::time::advance()` the send completes with the `deadline` outcome and
   the room event still sends (no wall-clock sleep).

- [x] **Step 5:** Implement the orchestration.
- [x] **Step 6:** Run the integration tests and confirm GREEN:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing,experimental-encrypted-state-events index0_reshare
cargo fmt --all -- --check
```

### Task 4: Koushi — closed-token projection

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`

Add `record_index0_reshare_diagnostic` handling the new event:

- record `core.index0_reshare` / `outcome` with `session_alias`,
  `initial_share`, `reshare`, `eligible_own_bucket`, `eligible_peer_bucket`,
  `elapsed_ms`;
- increment aggregate counters per reshare token and per initial-share token
  (exported independently of the detail ring);
- add the new counter names to the reset list in
  `install_room_key_diagnostic_observer`.

Tests mirroring the #509 projection tests: closed tokens + counters, privacy
(no identifiers/material), counters survive detail-ring eviction.

- [x] **Step 7:** Add failing tests, confirm RED, implement, confirm GREEN:

```bash
cargo test -p koushi-sdk --lib index0_reshare
```

### Task 5: integrated gates, submodule sync, PR

**Files:**
- Modify: `docs/superpowers/plans/2026-08-13-index0-reshare.md` (mark tasks done)
- Modify: `docs/agents/plans.md` (index the new plan)
- Modify: `vendor/matrix-rust-sdk` (gitlink update)

- [x] **Step 8:** Run the integrated gates:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto --features testing
cargo test -p matrix-sdk --features testing
cd ../.. && cargo test -p koushi-sdk --lib
cargo test --workspace --exclude koushi-backend --exclude sidebar-composition --exclude key-management
npm --prefix apps/desktop run typecheck && npm --prefix apps/desktop run lint
npm --prefix apps/desktop run qa:secret-scan
node scripts/check-sdk-submodule.mjs
git diff --cached --check
```

- [ ] **Step 9:** Interop verification — run the local-homeserver send lane
  (`qa:headless-core` `send_queue` scenario against both homeservers) to
  confirm Koushi↔Koushi sends still decrypt; record the Element X / Element
  Web/Desktop compatibility statement (standard `m.room_key` only, no wire
  change) in the PR.
- [ ] **Step 10:** Commit the SDK changes in the vendored submodule
  (independent commits), update the root gitlink, commit the Koushi changes,
  push, and open the PR referencing #510 with the measured-deadline note and
  the no-repeat/no-downgrade evidence.

## Out of scope

- Changing the preshare, rotation, or retry policy.
- The post-send current-index re-share path (unchanged; never reported as
  index-0 repair).
- The `m.room_key_request` / `m.forwarded_room_key` recovery path (#461).
- Encrypted state events (`experimental-encrypted-state-events` is not
  enabled by Koushi; the message path is the hook point).
