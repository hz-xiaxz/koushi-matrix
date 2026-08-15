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
no blocking or nonblocking findings. A later deterministic-test amendment
replaced wiremock delay with test-hooks-only held results at the exact
RoomActor→SDK call boundary; reviewer-gpt first returned findings for incomplete
operation/teardown coverage, then approved the amended all-five-operation matrix
and real AccountActor teardown race as `Correct-to-merge`. Implementation may
proceed in the order below.

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
test-hooks-only held success/failure completions at the exact
`koushi_sdk` membership-operation call boundary for leave, decline, accept,
direct join, and directory join, while driving the real RoomActor handlers;
real AccountActor replacement/teardown barriers around handle clear,
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
   hold the injected SDK-boundary leave success after real admission, begin manager replacement, prove the
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
13. `room_subscription_residency_local_rejoin_is_replacement_fenced`: hold the
    injected SDK-boundary success after real admission for invite decline,
    accept, direct join, and directory join, begin
    AccountActor replacement, and prove acknowledgement plus every existing
    reducer/event settlement completes under the admitted permit before
    replacement; no old operation terminal may arrive afterward.
14. `room_subscription_residency_failed_operations_settle_before_replacement`:
    table-drive held injected SDK-boundary failures after real admission for
    direct leave, invite decline, accept, direct join, and directory join; prove
    every existing failure action/event settles under the old
    permit before replacement and none arrives after replacement completion.
15. `room_subscription_residency_final_permit_drop_cannot_miss_drain`: start
    close/drain with one active permit, assert a new `begin_operation` is rejected
    after close without changing active count, arrange the original final drop
    before the waiter first polls, and assert retained watch state completes drain
    without timeout.
16. `room_subscription_residency_timeline_setup_precedes_room_observation`:
    deterministically assert the manager accepts the generation before the room
    observer can submit visible IDs.
17. `room_subscription_residency_manager_teardown_is_account_isolated`: with one
    held admitted membership operation, drive real AccountActor teardown and
    assert binding clear, post-close admission rejection, old-manager liveness
    through operation acknowledgement plus reducer/event settlement, permit
    drain before manager shutdown, and no late terminal; then prove a new
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
   after the matching production behavior. For membership-operation race tests,
   inject only the awaited `koushi_sdk` result at the existing RoomActor call
   boundary; admission, handler routing, residency acknowledgement, reducer
   actions, CoreEvents, replacement, and teardown must all remain the real paths.
   Keep the harness compiled out of default builds and remove unused controls.
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

## Post-implementation review correction (approve before corrective code)

The complete-diff review found four correctness/evidence gaps. Correct them as
one bounded amendment without changing the session-residency policy:

1. `RoomActor` may forward visible-room residency only when the same complete
   projection passes duplicate-identity authority validation. A duplicate
   vector remains fail-closed for UI projection and adds no residency.
2. Membership-operation admission also rejects a closed manager sender. The
   admitted operation returns the real `room_left`/`room_rejoined`
   acknowledgement. If the SDK succeeds but that acknowledgement is lost,
   emit a correlated `RoomOperationFailed { Sdk }` plus a private-safe closed
   `manager_unavailable` diagnostic and emit no success reducer action,
   refresh, or success `CoreEvent`. The permit still drops only after this
   failure settlement; normal replacement/teardown keeps the manager alive and
   therefore continues to acknowledge.
3. The integration harness spawns a real `RoomActor` bound to the same real
   test manager. Leave/decline use the approved held SDK-boundary result and
   real `RoomCommand`; visible and membership observations enter through
   test-hooks-only `RoomActor` messages that call the same extracted production
   duplicate validator and ordered typed-ingress forwarder used by the live
   observer. Delete direct manager policy shortcuts and add a fresh duplicate
   identity case that is not already resident.
4. Diagnostics use only the approved source tokens
   `opened|visible_range|restore|room_left|room_rejoined|membership|session_restart`.
   Room/thread/focused/rebuild map to `opened`; sync-start reconciliation maps
   to `session_restart`. Reconcile records include previous, desired, added,
   removed, and retained count buckets plus generation fields. The diagnostic
   integration test takes the diagnostics test lock and a start index and
   inspects only records produced by that test. Delete the obsolete restored
   lease-defer helper/test and unused trigger variant.

Corrective RED evidence must prove the fresh duplicate is admitted by the old
path, lost acknowledgement publishes success, direct-manager shortcuts can pass
while RoomActor wiring is absent, and global diagnostics can satisfy the old
assertion. The same focused checks then turn GREEN, followed by the full 25-test
residency lane and core library. Reviewer-gpt reviewed this correction before
corrective production edits and returned `Correct-to-merge` with no findings;
it explicitly rejected inventing an extra account-fatal protocol.

## Local-QA reconnect correction (approve before corrective code)

The mandatory `timeline_reconnect` lane exposed one oldest event missing after
reopening a retained room subscription. This is the expected boundary of the
upstream Element X-compatible room-subscription window: the branch retains and
immediately projects the newest 20/21 events, while `origin/main` projects only
1/21 after destructive remove/re-add. Matrix Rust SDK intentionally drops the
limited-response `prev_batch` when all 20 response events are already known,
and its token-free live-tail refresh reconciles omissions newer than the cached
suffix, not history older than that suffix. Re-arming that refresh therefore
cannot recover event 1 and must not be added as speculative product machinery.

Keep the vendored SDK and live-tail policy unchanged. Remove the attempted Room
Subscribe/live-tail coordinator changes and their proposed state-machine
additions. Beyond the approved residency fix, the only product correction is
the narrow `timeline.rs` room-key-reshare task ownership change in item 2;
the remaining changes update the deterministic QA proof to match normal
Element X behavior. The active reconnect branch must create the
room with encryption enabled and wait until the Rust room-list projection for
the exact room reports `is_encrypted=true` before either account subscribes or
sends; plaintext setup is a failed harness, not evidence.

1. after reconnect and explicit Room Subscribe, require at least the newest 20
   distinct encrypted synthetic bodies in the initial projection before any
   pagination; this fails on `origin/main` and proves retained subscription
   continuity rather than a coincidental re-add;
2. generate each offline synthetic event through the typed send command and
   require both the exact room/body local echo and its correlated
   `SendCompleted`; a correlated `OperationFailed` or `NotSent` is terminal
   failure. Encrypted RED proved a stable-manager deadlock: a delayed room-key
   reshare message can enter `handle_room_key_reshare`, await the lower-priority
   account-work permit (and then SDK network work) directly inside the manager,
   while the next send holds the interactive guard; that guard releases only
   after this same manager polls the global send terminal and ingress. Move the
   permit plus SDK reshare call into a cancelable task owned by the existing
   per-key/per-outbound-session `RoomKeyReshareSchedule`; return only a typed
   completion message to the manager, revalidate exact key, actor generation,
   and outbound-session token before recording or mutating schedule state, and
   abort the owned task with schedule replacement, actor cleanup, or manager
   teardown. The manager handler itself must never await account work or SDK
   network I/O. Never substitute presentation `Sent` for the required command
   terminal. Before implementation, add a compact table-driven RED proof for:
   terminal progress while a reshare waits behind an interactive guard; one
   valid completion; stale key, actor-generation, and outbound-token
   completions; schedule replacement/unsubscribe cancellation; duplicate
   completion; and manager shutdown abort with scheduler waiter/permit release;
3. if one older body remains, issue one real typed backward
   `TimelineCommand::Paginate` with a bounded page size of 64. Require the
   matching key, request ID, and backward direction to emit `Paginating` before
   accepting `Idle|EndReached`; a terminal without prior `Paginating` proves the
   gap-repair skip path and fails. Under one absolute deadline, apply every
   diff batch in order to one authoritative accumulated projection with full
   `PushFront|PushBack|Insert|Set|Remove|Truncate|Clear|Reset` semantics before
   evaluating counts; continue until both the correlated terminal and all 21
   unique bodies exactly once are observed, since relay diffs may follow the
   terminal. Any UTD row, operation failure, duplicate, timeout, or
   `EndReached` with a missing body fails;
4. preserve `live_catchup_checkpoint=ok` and
   `live_catchup_gap_repaired=ok` only after the complete decrypted set is
   proven on both supported servers.

This is not a test-only fallback or weakened assertion: it separately proves
session-resident newest-window continuity and standard SDK historical
pagination, which is the actual user path beyond the upstream window. Do not
change the SDK timeline limit, synthesize gaps, patch duplicate policy, weaken
Megolm, or add a second history owner. A disposable experiment already proved
that the real backward paginate recovers the 21st body and turns the Tuwunel
lane GREEN; the retained test must now encode the correlated, duplicate-safe
form above. Reviewer-gpt approved the pagination/encrypted-harness portion as a
QA-only `Correct-to-merge` before implementation after requiring correlated
`Paginating`, full authoritative diff semantics, and the explicit encrypted-room
projection gate. That verdict does not cover the later product reshare-worker
correction, which has its own required pre-implementation design gate.
The encrypted Tuwunel RED reproduced four times: after 17–20 sequential sends,
the exact local echo reached Rust `Sent` but the correlated `SendCompleted`
never arrived and the command waited to deadline. An initial biased-select
reorder plus deterministic unit test did not change the real lane (ordinal 18
still failed) and is therefore reverted rather than retained speculatively.
The confirmed cycle is the inline `handle_room_key_reshare` permit/SDK await
against the next send's interactive guard and the manager-owned terminal poll.
The correction above retains the canonical terminal requirement and moves only
that auxiliary network work out of the stable session owner.

## Implementation gate record / worklog

- Task 2A compile scaffold: `cargo test -p koushi-core --test room_subscription_residency --no-run` exited 0.
- Slice A RED: `cargo test -p koushi-core --test room_subscription_residency` exited 101 with four assertion-level failures: final unsubscribe and shared Room/Thread/Focused retained `active_rooms=[]` instead of the synthetic room; build failure retained `desired_rooms=[]` instead of the admitted room; 140-room retention observed `active_rooms.len()=0` instead of 140. The binary ran 5 tests (the four behavior tests plus the compile probe); no compilation, zero-match, no-session, or harness failure occurred.
- Slice A reviewer hardening: teardown checks now assert manager desired-room retention, each teardown admits a distinct extra room and verifies a subsequent real reconcile retains prior desired/active rooms, build failure asserts zero actors/leases, and Room/Thread/Focused asserts three actors/leases before the no-op generation check. `cargo test -p koushi-core --test room_subscription_residency` exited 0 with 5 passed; no production policy or harness policy was added.
- Slice B RED (Task 3 source union/expiry): scaffold `cargo test -p koushi-core --test room_subscription_residency --no-run` exited 0. The exact binary then ran 10 tests and exited 101 with 5 assertion-level failures: opened/visible/restored retained only D, identical visible retained no A, invalid/stale visible retained no valid A, proven restore imported no B, and expiry reconciliation retained no A/B. No compile, zero-match, no-session, or harness failure occurred.
- Slice B GREEN (Task 3 source union/expiry): the same `cargo test -p koushi-core --test room_subscription_residency` exited 0 with all 10 tests passed after the matching manager generation/restore/visibility/expiry implementation.
- Slice B RED (Task 3 lifecycle/gate scaffold): `cargo test -p koushi-core --test room_subscription_residency --no-run` exited 0. The exact binary ran 24 tests and exited 101 with 13 assertion-level failures: leave/decline terminal, replacement binding mismatch, pre-sync tombstone, inflight drain, delayed projection, ordered rejoin, stale membership, local rejoin fence, failed-operation settlement, final permit drain, setup ordering, account isolation, and diagnostics. No compile, zero-match, no-session, or harness failure occurred.
- Corrective Slice A RED (Findings 1/3 visible): the fresh duplicated valid room was admitted by the old direct-manager path, so the focused invalid/stale-visible test exited 101. After production authority gating and the shared real-RoomActor visible ingress, the focused test, three visible/union tests, and then-current 24-test lane exited 0.
- Corrective Slice B RED (Finding 2): the real RoomActor acknowledgement-loss test exited 101 with `operation_failed_sdk_count=0` before the fix, and the narrow closed-receiver gate test exited 101 because `begin_operation` admitted after the receiver closed. Both failures were assertion-level, not harness or compile failures.
- Corrective Slice B GREEN: the two focused tests exited 0; the full `room_subscription_residency` binary exited 0 with 25 passed, including all five held SDK-success operation paths; `cargo test -p koushi-core --lib` exited 0 with 1002 passed and 8 ignored; default `cargo check` and `git diff --check` exited 0.
- Corrective Slice C RED (Finding 3 operations/membership): with the RoomActor binding deliberately cleared, the old direct-manager leave helper still tombstoned the room and the new focused guard exited 101. After real RoomCommand plus held SDK-boundary routing and shared ordered membership ingress replaced the shortcuts, the binding guard, leave/decline, pre-sync/delayed projection, ordered/stale membership, acknowledgement-loss, and then-current 25-test lane exited 0.
- Corrective Slice D RED (Findings 4–6): after adding the isolated record-index assertion, `cargo test -p koushi-core --test room_subscription_residency room_subscription_residency_diagnostics_are_private_safe_and_closed` exited 101 on the old `room_selected`/other non-approved reconcile source token; the same pre-fix reconcile records did not contain `previous_bucket` or `desired_bucket` (the assertion was reached after the token failure was corrected). The failure was assertion-level, with the diagnostics lock held and only post-index `core.subscription` records inspected.
- Corrective Slice D GREEN: the diagnostic test exited 0; the full `cargo test -p koushi-core --test room_subscription_residency` lane exited 0 with 25/25 passed; `cargo test -p koushi-core --lib subscription_` exited 0 with 5 passed; `cargo test -p koushi-core --lib` exited 0 with 1001 passed and 8 ignored; default `cargo check` exited 0; touched-file `rustfmt --edition 2024 --check` and `git diff --check` exited 0. Full core library validation was run, so no pending core-lib check remains for this slice.
- Local-QA reconnect correction design approval: reviewer-gpt recorded `Correct-to-merge` for the QA-only change, with product code, vendored SDK, and live-tail behavior explicitly out of scope.
- Local-QA reconnect correction RED: the focused `reconnect_initial_projection_requires_twenty_distinct_bodies` test ran against the passive boolean oracle and exited 101 on its assertion-level underfilled-window failure; no homeserver was run.
- Local-QA reconnect correction GREEN: `cargo test -p koushi-core --features qa-bin --bin headless-core-qa` exited 0 with 127 passed; the final focused `reconnect_` run exited 0 with 8 passed; `rustfmt --edition 2024` was run on the touched QA binary and `git diff --check` exited 0. `timeline_reconnect --core` then exited 0 independently on Tuwunel and Synapse with `timeline_reconnect_recv_after_reconnect=ok`, `live_catchup_checkpoint=ok`, and `live_catchup_gap_repaired=ok`; the normal `timeline --core` and `send_queue --core` lanes also exited 0 with `--server=both`. The aggregate `all --core --server=both` attempt remains blocked before #532-specific assertions by a Synapse `session_status` settlement timeout reproduced unchanged in a detached `origin/main` worktree; this baseline evidence is retained rather than misreported as branch GREEN.
- Final E2EE QA correction RED (reviewer plaintext finding): the focused `active_reconnect_uses_encryption_gate_before_timeline_work` source test exited 101 before the fix because the active reconnect branch created the room with `encrypted=false` and had no dual encryption-projection gates; no homeserver was run.
- Final E2EE QA correction GREEN (local only): the focused source test exited 0, the full `cargo test -p koushi-core --features qa-bin --bin headless-core-qa` exited 0 with 128 passed, and `git diff --check` exited 0. No homeserver run or server-green claim is made for this correction.
- Rejected send-terminal hypothesis: reviewer-gpt approved a narrow biased-select reorder and a ready-read-pressure proof, and the unit/full suites were GREEN, but the next real encrypted Tuwunel run still failed at ordinal 18 with the same missing terminal. The reorder and its test are therefore reverted; they are not retained as speculative product behavior or reported as the final fix.
- Room-key-reshare deadlock RED: four real encrypted reproductions failed on the last sends (ordinals 17, 18, 19, and 20): the exact local echo reached Rust `Sent`, but the correlated `SendCompleted` terminal was missing until the flow deadline. Source inspection then identified the manager-inline reshare permit/SDK await cycle. QA removed the temporary send diagnostic snapshot/stage dump and ordinal-20 10-second timeout; `wait_for_send_flow_completion` still requires correlated `SendCompleted`. No homeserver green claim is made for this correction.

## Approved room-key-reshare deadlock correction

**Design approval:** The task approval for issue #532 authorizes this narrow
`timeline.rs`-only product correction. The stable manager keeps only exact
`TimelineKey`, actor-generation, outbound-session-token, and active-schedule
validation. It starts one executor-owned task in the existing per-key schedule;
the task owns `AccountWorkKind::RoomKeyReshare` admission and the SDK
`force_reshare_room_key` await, maps to a private closed completion enum, and
sends that completion back through `TimelineMessage`. The schedule retains
per-attempt handle slots so replacement, unsubscribe/actor cleanup, normal
shutdown, and abnormal manager drop abort every worker; completion takes its
own worker handle before any schedule mutation, making duplicates/stale inputs
inert and preventing self-abort. A second currentness check guards insertion;
failed insertion aborts the new task. No SDK or protocol changes are allowed.

**Rejected reorder removal:** The speculative biased-select reorder and its
ready-read-pressure test were removed before this product correction. The
manager select is back to the original control, navigation, reads/retries,
terminal, enqueue/diagnostic, observer, mailbox order. The reorder is not a
priority policy and must not return.

**Deterministic RED before product code:**
`cargo test -p koushi-core --lib room_key_reshare_handler_does_not_hold_the_manager_on_sdk_work`
exited `101` after compiling the library; the test reached its assertion and
failed because the old handler still contained the inline SDK/permit await.
This is supplemental to the four encrypted local reproductions above; no
homeserver was run for the focused unit RED.

**Room-key-reshare GREEN:** the eight focused reshare tests exited 0, including
actual correlated `SendCompleted` delivery while the reshare worker waited
behind an interactive guard, exact-once/stale completion fences, schedule
replacement/unsubscribe cancellation, and queued/admitted shutdown release.
`cargo test -p koushi-core --lib` exited 0 with 1005 passed and 8 ignored;
`cargo test -p koushi-core --features qa-bin --bin headless-core-qa` exited 0
with 128 passed; the residency lane exited 0 with 25 passed; default
`cargo check` and `git diff --check` exited 0. The real encrypted
`timeline_reconnect --core` lane then exited 0 independently on Tuwunel and
Synapse with `timeline_reconnect_recv_after_reconnect=ok`,
`live_catchup_checkpoint=ok`, and `live_catchup_gap_repaired=ok`; all 21
recognizable decrypted bodies were proven exactly once with no UTD.

## Final review findings worklog

- F1 RED: after replacing the reconnect assertions with the three required cases, `cargo test -p koushi-core --features qa-bin --bin headless-core-qa reconnect_initial_projection` exited 101 with three assertion-level failures: missing newest body, oldest present before pagination, and the all-21 no-pagination shortcut. No homeserver ran.
- F1 GREEN: the same `reconnect_initial_projection` command exited 0 with 3 tests passed; the final `cargo test -p koushi-core --features qa-bin --bin headless-core-qa reconnect_` exited 0 with 10 tests passed. The helper now requires initial indices 1..=20 exactly once, rejects index 0, always issues one real backward page of 64, and requires the matching key/request/backward `Paginating` then terminal before final exact-once 21-body completion.
- F2 review finding: cancellation coverage was nondeterministic because replacement, unsubscribe, and shutdown did not await whether the reshare worker was queued or admitted. The corrective test now uses cfg(test)-only one-shot acquire-entry/permit-admission signals, a held completion channel, both queued and admitted paths, explicit replacement/unsubscribe/shutdown cancellation, channel closure, and scheduler reuse; the production worker has none of those fields or hooks. `cargo test -p koushi-core --lib room_key_reshare` exited 0 with 8 passed, including the retained terminal-progress test.
- F3 GREEN: `RoomKeyReshareCompletion` is private to `timeline.rs`; `cargo check` and the core library both compile it successfully.
- F4 GREEN: removing `RoomSubscriptionResidencyHarness::new`, `compile_probe`, and the no-op compile test reduced the residency lane to the real 25 tests. `cargo test -p koushi-core --test room_subscription_residency` exited 0 with 25 passed.
- Final requested gates: `cargo test -p koushi-core --features qa-bin --bin headless-core-qa` exited 0 with 129 passed; `cargo test -p koushi-core --lib` exited 0 with 1005 passed and 8 ignored; default `cargo check` exited 0; `git diff --check` exited 0. The final strict encrypted oracle then exited 0 independently on Tuwunel and Synapse, proving exact newest indices 1..=20 before a mandatory correlated backward page of 64 and exact decrypted indices 0..=20 afterward, with all three required success tokens and no UTD. No SDK files changed.
