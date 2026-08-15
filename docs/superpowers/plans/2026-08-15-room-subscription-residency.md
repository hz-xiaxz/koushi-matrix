# Session-Resident Room Subscriptions (Issue #532)

> Implement only after this plan has a `reviewer-gpt` `Correct-to-merge` verdict.
> Implementation is delegated to `luna-implementer` at max thinking. Every
> behavioral edit follows a deterministic RED check.

**Goal:** Stop presentation/actor lifetime from removing valid Sliding Sync room
subscriptions. During one account session, every opened, validated-visible, or
validly restored room remains subscribed until a successful leave or whole
session teardown.

**Root cause:** `TimelineManagerActor::reconcile_subscriptions` currently sends
the exact set of `subscribed_room_leases`. The final
`TimelineCommand::Unsubscribe` drops the lease and reconciles the room away.
Also, the first lease after startup replaces valid restored coverage with the
lease subset. Re-entry therefore looks like genuine coverage loss to the SDK,
which correctly marks members missing and may rotate Megolm.

## Approved policy

- `TimelineManagerActor` remains the only production caller of
  `RoomListService::reconcile_room_subscriptions_with_generation`.
- Add one manager-owned `BTreeSet<OwnedRoomId>` for account-session residency.
  It is the desired SDK subscription set.
- The set grows from:
  1. successfully admitted opened room/thread/focused timeline room IDs;
  2. unique, non-left room IDs from a successfully delivered live room-list
     projection (the Element X visible-range input in Koushi's adapter model);
  3. SDK-restored room subscriptions only while the matching restored Sliding
     Sync position proves continuity.
- Timeline actor leases continue to own actor resources only. Actor
  unsubscribe, rebuild, replay, cache eviction, and build failure never remove
  session residency.
- A successful explicit room leave removes exactly that room and reconciles.
  Logout, account switch, reset, and account deletion drop the whole manager and
  therefore the whole session set. No residency is copied between managers or
  accounts.
- There is no count cap, LRU, duration, timer, eviction task, persistence format,
  UI toggle, or environment override. This deliberately supersedes issue #532's
  initial bounded-recent-room suggestion: the user selected uncapped,
  session-only monotonic residency after comparing Element X behavior.
- UnknownPos/session expiry is not treated as continuity. The SDK may clear its
  actual subscription map and use its standard conservative member reload. The
  manager retains only its in-process session intent and re-reconciles it on the
  replacement/next valid room-list observation; it does not suppress SDK member
  invalidation or Megolm rotation.
- Do not modify vendored matrix-rust-sdk behavior. Standard member reload,
  rotation, generation, checkpoint, key sharing, gossip, and backup remain
  authoritative.

## Message and generation ownership

- Extend private `TimelineMessage` with internal residency intents:
  - `VisibleRoomsObserved { core_generation, room_ids }`;
  - `RoomLeft { room_id, cause: RoomRemovalCause, acknowledged }`, where the
    private closed cause is `DirectLeave|InviteDecline`;
  - `RoomMembershipObserved { core_generation, transitions }`, where
    `transitions` preserves per-`RoomUpdates` receipt order;
  - `RoomRejoined { room_id, acknowledged }` for successful local
    join/accept-invite/directory-join only.
- Expose exactly four narrow methods (`visible_rooms_observed`,
  `membership_observed`, admitted/acknowledged `room_left`, and
  admitted/acknowledged `room_rejoined`) through one cloneable crate-private
  `TimelineSubscriptionResidencyHandle` that wraps the private manager sender.
  Do not make the internal message enum public or give `RoomActor` generic
  timeline-command authority.
- Add a private `watch` control slot to `RoomActorHandle`/`RoomActor`, separate
  from the public `RoomMessage` enum. Its value atomically binds the exact
  `Arc<MatrixClientSession>` and typed manager handle that AccountActor will use
  for the unchanged `SessionEstablished`. Membership admission requires
  `Arc::ptr_eq` between that bound session and `RoomActor`'s current session;
  `None` or mismatch fails closed before any SDK call. Thus the replacement
  install→`SessionEstablished` gap can pair neither old session A with manager B
  nor new session B with manager A. The typed handle owns a linearizable
  membership-operation gate shared with `TimelineManagerHandle`: one mutex owns
  `accepting` plus `active_count`; `begin_operation` synchronously checks and
  increments under that mutex; a retained `watch<usize>` mirrors active count.
  Close flips `accepting=false` under the same lock, then uses `watch::wait_for`
  until the retained value is zero; permit drop decrements under the mutex and
  publishes the new count with `send_replace`. A final drop before waiter polling
  is still observed, so no lost-wakeup window or arbitrary semaphore bound exists.
  `RoomActor` synchronously snapshots `(session, typed handle, admitted permit)`
  before every SDK leave/decline/join/accept await. Successful leave/decline
  sends `RoomLeft`; successful join/accept/directory join sends `RoomRejoined`;
  each waits for manager acknowledgement. The permit remains held through every
  existing success/failure reducer action and `CoreEvent` and drops only after
  that handler's public settlement is complete; failed operations send no
  residency intent but settle their existing failure while still holding the
  permit. Direct leave and invite decline share
  one admitted SDK-leave helper so no sibling path omits the terminal.
- On replacement/teardown, `AccountActor` clears the watch slot to reject new
  membership-operation admission, drains already-admitted permits while the old
  manager processes and acknowledges their terminals, then shuts down that
  manager. It creates the replacement manager, installs its typed handle, and
  only then sends unchanged `RoomMessage::SessionEstablished`. This preserves
  the public API, prevents old-session operations from targeting a new account,
  and prevents successful same-session membership changes from being lost.
- `SyncActor` establishes the timeline service/generation before starting the
  room-list observer. Existing `RoomMessage::SyncStarted` remains unchanged;
  `RoomActor` snapshots the already-installed typed handle plus its existing
  backend generation into the owned observation task.
- Successful leave/decline and local join/accept are manager-instance-fenced and
  valid before sync starts. The manager records session-local leave state so
  later restored or visible evidence cannot resurrect that room. Visibility
  never clears leave state: a successful local join/accept/directory join clears
  it directly, while an external rejoin requires an ordered SDK membership
  observation of `left` followed by `joined|invited` in the current core
  generation. A stale joined response or delayed pre-leave projection is ignored
  until `left` was observed.
- A room-list projection submits visible IDs only after projection delivery
  succeeds and duplicate-identity validation accepts the vector. Partial but
  valid non-left ranges may add rooms; later range exit never removes them.
- `TimelineManagerActor` accepts observer intents only for its current core
  generation. The room observer emits one ordered membership-transition vector
  per drained `RoomUpdates` item in receipt order; it never folds `left` and
  `joined|invited` into unordered aggregate sets. Delayed observations from a
  retired backend cannot mutate replacement-session residency.
- All mutations and SDK reconciliation stay serialized in the timeline-manager
  mailbox. The same desired set remains an SDK no-op.

## Diagnostics

Reuse `core.subscription` and existing private-safe room ordinals. Record only:

- input source: `opened|visible_range|restore|room_left|room_rejoined|membership|session_restart`;
- previous/desired/added/removed/retained count buckets;
- exact-set suppression;
- removal cause `room_left|invite_declined` (session teardown owns no live SDK call);
- current core/service generation and build version through existing diagnostic
  context.

Never record room/user/device/session IDs, sync positions, event/message content,
or raw errors. No per-room source map is retained: provenance no longer affects
the monotonic policy after admission.

## Task 1 — Canon first

**Files:**

- `docs/architecture/overview.md`
- `docs/architecture/state-machine.md`
- `docs/agents/state-ownership.md`
- `docs/policies/engineering-rules.md`
- `docs/agents/plans.md`
- this plan

Document the manager-owned session residency set, the three additive sources,
explicit leave/session teardown removals, uncapped/no-persistence policy,
generation fence, membership-operation admission/drain, ordered leave/rejoin
transitions and failures, unchanged SDK security ownership, and the distinction
between an uncapped session-local room-ID intent set and prohibited unbounded
owned task/stream-handle maps.

Run:

```bash
node scripts/check-agents-docs.mjs
git diff --check
```

Obtain `reviewer-gpt` `Correct-to-merge` on this complete plan/canon diff before
any test or production implementation edit.

**Gate record (2026-08-15):** reviewer-gpt rounds 1–8 were not approvals
(timeouts or findings, all corrected). Round 9 reviewed this complete plan plus
`overview.md`, `state-machine.md`, `state-ownership.md`,
`engineering-rules.md`, and the plan index and returned `Correct-to-merge` with
no blocking or nonblocking findings. Implementation may now begin in the order
below.

## Task 2 — RED: actor-independent session residency

**Files:**

- `crates/koushi-core/tests/room_subscription_residency.rs` (new)
- `crates/koushi-core/src/room_subscription_residency_test_support.rs` (new,
  compiled/exported only with `feature = "test-hooks"`)
- `crates/koushi-core/src/lib.rs`
- `crates/koushi-core/src/account.rs`
- `crates/koushi-core/src/timeline.rs`
- `crates/koushi-core/src/room.rs`
- `crates/koushi-core/src/sync.rs`

Edits to existing source files in Task 2A are limited to compile-only
`test-hooks` probe/barrier plumbing and the gated module export. They must not
change default-build control flow or product results.

### Task 2A — compile-only testability scaffold

Before any behavior change, add the gated module/export, read-only snapshot
probe, controllable mock transport, and pause/resume barriers over the current
real actors. Private binding/gate shells may be added but must not yet alter
command routing, subscription reconciliation, unsubscribe, restore, or operation
results. Prove only scaffold compilation first:

```bash
cargo test -p koushi-core --test room_subscription_residency --no-run
```

The test-support module wraps the real `AccountActor`, `TimelineManagerActor`,
`RoomActor`, typed residency handle, and mock-backed live `RoomListService`; it
must not reimplement the policy. Expose one `RoomSubscriptionResidencyHarness` with deterministic
methods/barriers for: SyncStarted with restored evidence and a chosen core
 generation; real timeline admit/unsubscribe/build-failure; valid/invalid visible
and membership intents through the production handles; SDK subscription expiry;
wiremock-delayed real leave, decline, accept, direct-join, and directory-join
operations; real AccountActor replacement/teardown barriers around handle clear,
permit drain, manager shutdown, replacement binding install, the deliberate
install→`SessionEstablished` gap, and `SessionEstablished` completion;
ordered per-update membership delivery in both directions; and an acknowledged
private-safe snapshot containing only synthetic
test room IDs, desired/active counts/sets, tombstone state, actor/lease counts,
SDK generation, and last closed trigger token. This module is absent from default
builds; no parallel product API or policy model is allowed.

For every behavior below, add only its required probe/barrier, rerun `--no-run`
to prove the scaffold, then add and run that exact named assertion to RED before
its matching production edit. A compile failure, zero matched tests, no-session
short circuit, or failure in the harness itself is not RED.

Add behavior tests before production edits:

1. `room_subscription_residency_final_actor_unsubscribe_retains_room`: open A
   with room and thread actors, unsubscribe both, and assert A remains in the
   SDK active set while both actors and leases are gone.
2. `room_subscription_residency_actor_build_failure_retains_admitted_room`:
   after the manager admits A and actor construction fails, assert A remains
   desired/subscribed.
3. `room_subscription_residency_has_no_count_or_lru_eviction`: admit more rooms
   than the room-list page size and prior actor bounds, unsubscribe every actor,
   and assert every room remains.
4. `room_subscription_residency_room_thread_focused_share_one_room`: admit all
   three keys for A and assert one room-level SDK subscription/no generation
   bump, then unsubscribe all three and assert the new requirement that A remains
   resident. The final assertion is the required deterministic RED.

Run the named integration binary and preserve the failing assertions:

```bash
cargo test -p koushi-core --test room_subscription_residency
```

The RED must fail because the final actor lease currently removes A, not because
of compilation, zero matched tests, or fixture failure.

## Task 3 — RED: lifecycle, source union, expiry, and isolation

**File:** `crates/koushi-core/tests/room_subscription_residency.rs`

Add to the same feature integration binary before production edits:

1. `room_subscription_residency_opened_visible_restored_are_unioned`: restored
   `{A,B}` plus valid visible `{B,C}` plus opened D yields `{A,B,C,D}`.
2. `room_subscription_residency_identical_visible_range_is_noop`: repeat the
   same visible IDs and assert unchanged SDK generation.
3. `room_subscription_residency_invalid_or_stale_visible_is_rejected`:
   duplicate identity, malformed/left entries, and an old core generation add
   nothing.
4. `room_subscription_residency_unproven_restore_is_rejected`: actual SDK rooms
   without matching restored-position continuity, including expired/UnknownPos
   state, import nothing.
5. `room_subscription_residency_unknown_pos_reconciles_complete_intent`: after
   actual SDK subscriptions expire, the next current-generation observation
   restores the complete `{A,B}` intent without bypassing SDK invalidation.
6. `room_subscription_residency_leave_and_decline_share_success_terminal`:
   direct leave and invite decline both remove only A from `{A,B}` on SDK
   success; each failure emits no removal.
7. `room_subscription_residency_pre_sync_leave_targets_replacement_manager`:
   after matching `SessionEstablished`, successful leave before SyncStarted uses
   the replacement manager. In the deterministic handle-install→session-message
   gap, the harness must prove RoomActor holds `Some(session A)`, the private
   binding holds `Some(session B)`, and `!Arc::ptr_eq(A, B)` before issuing the
   command. It must fail before any SDK call with an explicit session-mismatch
   probe token; `SessionRequired` from `None` is rejected as insufficient RED/GREEN evidence.
8. `room_subscription_residency_pre_sync_leave_blocks_restore_resurrection`:
   leave A before SyncStarted, then offer restored `{A,B}`; only B is imported.
9. `room_subscription_residency_inflight_leave_drains_before_replacement`:
   block the real SDK leave after admission, begin manager replacement, prove the
   old manager remains alive, complete leave, observe acknowledged removal plus
   all existing reducer/event settlements, then prove replacement completes and
   no old operation action/event arrives afterward or targets a new account.
10. `room_subscription_residency_delayed_projection_cannot_clear_leave`:
   hold a valid A projection before leave, complete/acknowledge leave, then
   deliver the old projection and assert A remains tombstoned/unsubscribed.
11. `room_subscription_residency_rejoin_requires_ordered_transition`:
    joined/visible before left cannot clear; ordered left→joined clears/re-adds,
    while joined→left ends removed. Drive both orders through the real observer
    drain path so coalescing cannot erase order.
12. `room_subscription_residency_stale_membership_cannot_clear_leave`: stale
    core-generation `left`, `joined`, and `invited` transitions neither advance
    pending leave state nor clear/re-add a left room.
13. `room_subscription_residency_local_rejoin_is_replacement_fenced`: block each
    real accept/direct-join/directory-join after operation admission, begin
    AccountActor replacement, and prove acknowledgement plus every existing
    reducer/event settlement completes under the admitted permit before
    replacement; no old operation terminal may arrive afterward.
14. `room_subscription_residency_failed_operations_settle_before_replacement`:
    delay SDK failures for direct leave, invite decline, and directory join after
    admission; prove every existing failure action/event settles under the old
    permit before replacement and none arrives after replacement completion.
15. `room_subscription_residency_final_permit_drop_cannot_miss_drain`: start
    close/drain with one active permit, assert a new `begin_operation` is rejected
    after close without changing active count, arrange the original final drop
    before the waiter first polls, and assert retained watch state completes drain
    without timeout.
16. `room_subscription_residency_timeline_setup_precedes_room_observation`:
    deterministically assert the manager accepts the generation before the room
    observer can submit visible IDs.
17. `room_subscription_residency_manager_teardown_is_account_isolated`: a new
    manager/account starts empty and imports only its own valid restore evidence.
18. `room_subscription_residency_rapid_intents_serialize`: interleaved visible
    and open intents converge to one deduplicated set.
19. diagnostics Debug/records expose counts/tokens only and distinguish the
    closed `room_left|invite_declined` removal causes.

Run the entire binary again and preserve the RED evidence:

```bash
cargo test -p koushi-core --test room_subscription_residency
```

## Task 4 — Minimal production implementation

**Files:**

- `crates/koushi-core/src/timeline.rs`
- `crates/koushi-core/src/room.rs`
- `crates/koushi-core/src/sync.rs`
- `crates/koushi-core/src/account.rs`

Implement only the approved seams:

1. Add `session_subscribed_rooms`, per-room leave state
   (`pending_left_observation|left_observed`), and current core-generation
   fencing to `TimelineManagerActor` and every explicit test constructor.
2. Make `reconcile_subscriptions` derive desired rooms from that set, not actor
   leases.
3. On timeline admission, insert the parsed room ID before reconciliation.
   Keep it after actor-build rollback. Keep lease refcounting solely for actor
   lifecycle assertions/resources.
4. On `Unsubscribe`, remove actor/lease but do not remove residency or issue a
   shrinking reconcile.
5. On `SyncStarted`, union valid restored SDK coverage except rooms with leave
   state, then reconcile once; replace the startup-empty special case rather than
   layering another defer path.
6. Accept current-generation valid non-left observations, extend only rooms
   without leave state, and reconcile even when the set did not grow so SDK
   expiry can be repaired. Visibility alone never clears leave state.
7. Accept acknowledged successful-leave/decline through an admitted permit,
   mark `pending_left_observation`, remove exactly one room, reconcile when a
   service exists, then acknowledge. Process ordered current-generation
   membership transitions: `left` advances pending state to `left_observed`;
   only a later `joined|invited` clears/re-adds. Accept acknowledged successful
   local accept/direct-join/directory-join through the same admitted manager and
   clear/re-add directly.
8. Make direct leave and invite decline call one admitted SDK-leave helper.
   Snapshot session+typed handle+membership-operation permit before every
   leave/decline/accept/direct-join/directory-join SDK await; never read the
   watch after completion to choose a manager. Hold the permit through the
   handler's final existing reducer action/CoreEvent on success and failure.
9. Add the private session+typed-handle watch binding and admission gate.
   Membership handlers require pointer identity with the actor's current session.
   AccountActor clears admission and drains permits while the old manager is
   alive, then shuts it down, installs the replacement binding before unchanged
   `SessionEstablished`, and clears/drains it on every session teardown. Preserve every public
   `RoomMessage` variant.
10. Establish TimelineManager SyncStarted before RoomActor observation starts;
   wire valid room-list observations plus one ordered transition vector per SDK
   update through the already-installed handle. Preserve receipt order while
   retaining existing coalesced room-list/mention work. Add no detached task,
   generic cross-actor command access, or second `RoomListService`.
11. Complete each test-hooks harness method/barrier only in Task 2A immediately
   before its named RED assertion; Task 4 must not introduce a new test control
   after the matching production behavior. Keep the harness compiled out of
   default builds and remove any scaffold control not used by a retained test.
12. Update existing subscription diagnostics, carry `RoomRemovalCause` through
   the shared helper/message, and delete superseded lease-derived
   policy helpers/tests. Do not leave both ownership models active.

Turn every Task 2/3 RED check GREEN, then run the surrounding suites:

```bash
cargo test -p koushi-core --test room_subscription_residency
cargo test -p koushi-core --lib subscription_
cargo test -p koushi-core --lib live_room_list_observation
cargo test -p koushi-core --lib room_actor_
cargo test -p koushi-core --lib sync_
cargo test -p koushi-core --lib
```

## Task 5 — Full validation and review

Run from the root; read each command's own exit status:

```bash
cargo fmt --all -- --check
cargo test --workspace --exclude koushi-backend --exclude sidebar-composition --exclude key-management
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test -- --run
npm --prefix apps/desktop run build
npm --prefix apps/desktop run qa:secret-scan
node scripts/check-agents-docs.mjs
node scripts/check-sdk-submodule.mjs
git diff --check
git status --short
```

Run supported homeserver evidence on both Tuwunel and Synapse:

```bash
npm --prefix apps/desktop run qa:headless-local -- --server=both
npm --prefix apps/desktop run qa:headless-local -- --server=both --scenario=timeline_reconnect --core
```

The reconnect lane must cover A→B→A/actor close and prove first encrypted
messages remain decryptable after any legitimate SDK rotation. If the existing
lane cannot distinguish retained residency from a coincidental pass, add the
smallest private-safe QA token/assertion before production acceptance.

Use the `preflight-review` checklist on the complete root/submodule diff,
including untracked files. Obtain `reviewer-gpt` `Correct-to-merge` for the
complete final diff and fix every blocking and nonblocking finding.

## Task 6 — PR and merge

- Commit the focused root change; the SDK gitlink must remain unchanged.
- Push and open one PR with `Closes #532`, RED→GREEN evidence, deliberate
  uncapped divergence, diagnostics/privacy evidence, local QA results, and both
  reviewer-gpt verdicts.
- Verify every GitHub check. Fix failures at their cause and rerun review when
  the production diff changes.
- Merge only when all required checks are green. Confirm the merge commit is an
  ancestor of `origin/main` and issue #532 is closed.

## Completion audit

Completion requires fresh evidence for: one SDK mutation owner; additive opened,
visible, and restored union; no actor-driven removal; uncapped session retention;
shared leave/decline removal; fenced local accept/direct/directory rejoin;
ordered external rejoin; teardown/account isolation; stale visible/membership
rejection; proven-restore admission and unproven-restore rejection; UnknownPos
conservative recovery; unchanged vendored SDK/security behavior;
private-safe diagnostics; focused/full/local/CI green; final reviewer approval;
PR merged; issue closed. Nothing may be deferred as follow-up debt.

## Implementation gate record / worklog

- Task 2A compile scaffold: `cargo test -p koushi-core --test room_subscription_residency --no-run` exited 0.
- Slice A RED: `cargo test -p koushi-core --test room_subscription_residency` exited 101 with four assertion-level failures: final unsubscribe and shared Room/Thread/Focused retained `active_rooms=[]` instead of the synthetic room; build failure retained `desired_rooms=[]` instead of the admitted room; 140-room retention observed `active_rooms.len()=0` instead of 140. The binary ran 5 tests (the four behavior tests plus the compile probe); no compilation, zero-match, no-session, or harness failure occurred.
- Slice A reviewer hardening: teardown checks now assert manager desired-room retention, each teardown admits a distinct extra room and verifies a subsequent real reconcile retains prior desired/active rooms, build failure asserts zero actors/leases, and Room/Thread/Focused asserts three actors/leases before the no-op generation check. `cargo test -p koushi-core --test room_subscription_residency` exited 0 with 5 passed; no production policy or harness policy was added.
