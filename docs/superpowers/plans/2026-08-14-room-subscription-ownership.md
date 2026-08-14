# Room-Subscription Ownership Reconciliation Implementation Plan (Issue #518)

> **For agentic workers:** Implement task-by-task with a failing focused test
> before each production change (RED → GREEN). Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Centralize Sliding Sync room-subscription ownership by **room ID** so
that presentation-only TimelineKey changes (Room / Thread / Focused navigation,
actor rebuilds, sync-started rebuilds) never replace a continuously covered
room subscription, eliminating false `room_subscription` member invalidations
and outbound Megolm rotations — while preserving every security-required
rotation path (genuine coverage loss, membership transitions, limited sync,
key-share failure, restart).

**Architecture:**

- **SDK (vendored):** a new atomic, differential
  `RoomListService::reconcile_room_subscriptions_with_generation` that computes
  `current -> desired` under one state lock: exact-equal set is a true no-op
  (no generation bump, no checkpoint clear); retained intersection keeps
  subscription entries and member completeness; additions subscribe and mark
  only newly added rooms missing; removals unsubscribe; the generation and
  checkpoint view are published coherently for the resulting set. Add an
  `active_room_subscription_set()` reader. Existing `SlidingSync` primitive
  ops (`subscribe_to_rooms` additive / `unsubscribe_to_rooms`) are reused.
- **Koushi (koushi-core `TimelineManagerActor`):** a room-ID lease owner.
  Room/Thread/Focused subscriptions contribute refcounted leases per room;
  `handle_subscribe` and every rebuild path reconcile the full desired set once
  (never per-key `subscribe_to_rooms_with_generation`); the existing-key replay
  path first verifies the room is in the active subscription set and restores it
  if absent; `Unsubscribe` releases the lease and reconciles only when the last
  lease for the room is removed; `sync_started` reconciles the deduplicated
  desired set exactly once before actor rebuilds.
- **Diagnostics:** closed-token reconcile records (trigger, added/removed/
  retained buckets, noop flag, generation before/after, checkpoint retention,
  coverage check) plus counters that survive detail-ring eviction; rotation
  correlation distinguishes avoided churn from security-required re-add.

**Tech Stack:** Rust, matrix-sdk-ui (vendored), matrix-sdk (vendored),
koushi-core, koushi-diagnostics.

## Global Constraints

- Implement GitHub Issue #518 and no unrelated behavior.
- A room continuously retained across reconciliation is never marked missing;
  a room removed and later re-added follows the full-reload/rotation precaution.
- Never skip rotation merely because pre/post member-ID sets compare equal
  (MSC4268 protects against missed join/leave pairs).
- Rebuilding a Timeline actor must not mutate the room-subscription set.
- Bounded ownership: desired rooms derive from retained Timeline actors, which
  are already bounded; eviction is explicit and re-entry is security-required.
- Diagnostics never export room IDs, names, event IDs, session IDs, position
  tokens, message content, or raw errors; runtime ordinal room aliases only.
- SDK changes committed independently in the vendored submodule; the root
  gitlink updated separately.

---

### Task 1: SDK — failing tests for atomic differential reconciliation

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/tests/integration/room_list_service.rs`
- Add: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs` (unit tests in-module)

Add failing tests:

1. `reconcile_identical_set_is_a_true_noop` — `{A} -> {A}` returns `noop=true`,
   the generation is unchanged, retained checkpoints are unchanged, and no
   `members-missing` invalidation occurs for A.
2. `reconcile_retains_intersection_and_adds_only_new_rooms` — `{A} -> {A,B}`:
   A is not invalidated, B is added and marked missing, generation bumps once.
3. `reconcile_removes_only_removed_rooms` — `{A,B} -> {B}`: B retained
   unchanged (checkpoint kept), A removed, generation bumps once.
4. `reconcile_re_add_marks_the_room_missing` — `{A} -> {} -> {A}`: the second
   addition marks A missing (security-required reload path).
5. `reconcile_publishes_coherent_generation_and_active_set` — after any change
   the published active set equals the desired set and the generation is
   monotonic; a concurrent reader never observes the intermediate empty set.
6. Privacy: `Debug` of the reconcile result and new types contains no
   identifiers.

- [x] **Step - [ ] **Step 1:** Add the failing tests.
- [x] **Step - [ ] **Step 2:** Run and confirm RED:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-ui --features testing room_list_service
```

### Task 2: SDK — implement the reconcile API

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`

Add:

- `RoomSubscriptionReconcile` result type (generation, `noop`, added/removed/
  retained counts, `checkpoints_retained`).
- `reconcile_room_subscriptions_with_generation(&self, room_ids) -> RoomSubscriptionReconcile`:
  1. dedupe the desired set;
  2. under the `room_subscription_state` lock, diff against `active_rooms`;
  3. exact equal → return `noop` with the current generation (no channel
     churn, no checkpoint mutation);
  4. otherwise `SlidingSync::subscribe_to_rooms(added)` (additive; marks only
     new rooms missing) and `SlidingSync::unsubscribe_to_rooms(removed)` under
     the same lock, bump the generation once, set `active_rooms`, and publish
     the checkpoint map with retained rooms preserved and removed rooms
     dropped;
  5. return the closed result.
- `active_room_subscription_set(&self) -> BTreeSet<OwnedRoomId>` reader.
- Reimplement `subscribe_to_rooms_with_generation` as a thin wrapper over the
  reconcile (full replace semantics preserved for existing callers).

- [x] **Step - [ ] **Step 3:** Implement.
- [x] **Step - [ ] **Step 4:** Run the focused tests and confirm GREEN; run the existing
  room_list_service suite for regressions.

### Task 3: Koushi — failing tests for lease ownership and coverage

**Files:**
- Modify: `crates/koushi-core/src/timeline.rs` (manager tests)

Add failing tests (drive the TimelineManagerActor against a test
RoomListService double or a mocked SDK):

1. `subscribe_room_then_thread_then_focused_keeps_one_room_subscription` — no
   same-room invalidation or rotation between Room A, Thread A, Focused A.
2. `existing_key_replay_restores_missing_room_coverage` — a retained actor
   whose room fell out of the active set is re-subscribed before replay.
3. `unsubscribe_releases_the_room_lease_only_at_zero` — two leases for A;
   removing one keeps A subscribed; removing the last removes A.
4. `sync_started_reconciles_the_full_desired_set_once` — with multiple
   existing actors, exactly one reconcile of the deduplicated set occurs and
   actor rebuilds do not collapse it.
5. `actor_rebuild_does_not_mutate_room_subscriptions`.
6. Diagnostics: reconcile tokens/counters are closed and identifier-free.

- [x] **Step - [ ] **Step 5:** Add the failing tests.
- [x] **Step - [ ] **Step 6:** Run and confirm RED:

```bash
cargo test -p koushi-core --lib subscription_ownership
```

### Task 4: Koushi — implement the lease owner and reconcile wiring

**Files:**
- Modify: `crates/koushi-core/src/timeline.rs`

Add:

- `subscribed_room_leases: BTreeMap<OwnedRoomId, usize>` on the manager.
- `reconcile_subscriptions(trigger)` — builds the desired set from live
  leases, calls `room_list_service.reconcile_room_subscriptions_with_generation`,
  records the closed-token diagnostic and increments counters.
- `handle_subscribe`: add a lease before building; the existing-key replay
  path first checks `active_room_subscription_set()` and reconciles when the
  room is missing; new-build path reconciles before the actor build and rolls
  the lease back on failure.
- `build_timeline_actor_handle`: remove the per-key
  `subscribe_to_rooms_with_generation` call (rebuilds no longer mutate the
  subscription set).
- `TimelineCommand::Unsubscribe`: release the lease; reconcile when it drops
  to zero (or unconditionally — the reconcile is a no-op when the set is
  unchanged).
- `handle_sync_started`: `subscribe_existing_timeline_rooms` builds the
  deduplicated desired set from leases and reconciles once with
  `trigger=SyncStarted`; `rebuild_existing_room_timelines_after_sync_started`
  no longer re-subscribes.
- Rotation correlation: when a `room_subscription`-reason rotation is recorded,
  correlate whether the room had continuous lease coverage
  (`continuous_coverage=true|false|unknown`) and count avoided churn vs
  security-required re-add.

- [x] **Step - [ ] **Step 7:** Implement.
- [x] **Step - [ ] **Step 8:** Run the focused tests and confirm GREEN; run the full
  koushi-core suite for regressions.

### Task 5: integrated gates, submodule sync, PR

**Files:**
- Modify: `docs/superpowers/plans/2026-08-14-room-subscription-ownership.md` (mark done)
- Modify: `docs/agents/plans.md` (index)
- Modify: `vendor/matrix-rust-sdk` (gitlink update)

- [x] **Step - [ ] **Step 9:** Run the integrated gates:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-ui --features testing
cd ../.. && cargo test -p koushi-core --lib
cargo test --workspace --exclude koushi-backend --exclude sidebar-composition --exclude key-management
npm --prefix apps/desktop run typecheck && npm --prefix apps/desktop run lint
npm --prefix apps/desktop run qa:secret-scan
node scripts/check-sdk-submodule.mjs
git diff --cached --check
```

- [x] **Step 10:** Interop verification — run the local-homeserver
  `login_sync` + `timeline_reconnect` lanes (tuwunel/synapse where available)
  confirming live events continue across room switches and reconnects.
- [x] **Step 11:** Commit SDK changes in the submodule (independent commits),
  update the gitlink, commit the Koushi changes, push, and open the PR
  referencing #518 with the avoided-churn vs security-rotation evidence.

## Out of scope

- Changing member-consuming UI paths (Room Info, mention refresh) — the false
  invalidation is fixed at the source; those consumers remain legitimate.
- Unbounded account-wide detailed subscriptions (desired set derives from
  bounded retained actors).
- The `m.room_key_request` / `m.forwarded_room_key` path (#461, suspended).
