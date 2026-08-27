# Issue #708 — Rust-Owned Thread-Root Projection Lifecycle

Status: design awaiting independent `reviewer-flash` verdict. Implementation is not authorized before `Correct-to-merge`.

Delivery base: `origin/main` `ea7b802f48f9490f326b0e0839503c30bfa9a6ef` after PR #709 merged. Design reconnaissance began on `aea695f63a588c63cd7f9c0d9a5717752cef1d69`.

## Delivery shape

One PR contains the #552 phase insertion, canon amendments, RED/GREEN tests, Rust/TypeScript cutover, QA evidence, and final review. It changes no unrelated #552 adapter, acknowledgement-retry, request-ref, or mutation-queue phase.

## Observed failure and diagnostic chronology

The #708 reporter reproduced a thread representation disappearing and later returning without logout, redaction, or deletion:

1. `18:19:19.927Z`: the event cache supplied 300 events with many `m.thread` relations, but the initial Thread projection emitted `count=0`.
2. `18:19:20.535Z`: backward pagination processed 73 SDK diffs and the projection grew from 0 to 14 items.
3. `18:21:04.410Z`: another open began with 2 projected items.
4. `18:21:05.175Z`: pagination expanded it to 72 items.
5. The Rust thread aggregate stayed at reply count 65 with repeated `decision=no_op`; no app or JavaScript error explained the visual loss.
6. Room reselection/replay reconstructed the missing presentation.

This proves the Matrix events and accepted aggregate survive while bounded display observations can independently destroy and later recreate frontend projection state. Dropped diagnostic-ring records prevent claiming a direct log event for every disappearance; the design does not overstate that evidence.

## Root cause: three semantic owners

### Core owner

`ThreadRootProjectionService` in `crates/koushi-core/src/threads_list.rs` owns hydration, authoritative aggregates, activity/summary revisions, redaction reconciliation, and explicit `Updated`/`Cleared` completion. However, `reconcile_room_with_affected` also prunes terminal records from a temporary bounded Room window. `ReplayKnownThreadRootProjectionRegistry` separately derives another lifetime from replay display contents.

### Rust State owner

`ThreadRootProjectionState::reconcile_room` in `crates/koushi-state/src/state/thread.rs` stores the current bounded activity set, deletes Ready/Failed records absent from that set, and rejects later terminals through `is_active_or_unreported`. It therefore independently decides projection death after Core already admitted the lifecycle.

### TypeScript owner

`retainActiveThreadRootProjections` in `apps/desktop/src/domain/timelineStore.ts` removes ordinary Pending/Ready/Failed projections when the current display items omit a matching reply. `timelineDisplayProjection.ts` also chooses root-versus-reply placement, replay fallback, row identity, and ordering from transient frontend inputs.

A bounded Reset, initial empty window, replay replacement, repair batch, or event ordering can therefore be treated as deletion by any of three owners. The lazy root-cause fix is one Rust owner, not another retention exception.

## Desired invariant

`ThreadRootProjectionService` and the current Room `TimelineActor` together form the sole semantic owner:

- the service owns per-root lifecycle, renderable root snapshot, aggregate, activity and terminal clear;
- the actor-owned `DisplayProjectionState` owns bounded visibility, root/reply placement and ordered display diffs;
- Rust State mirrors explicit service transitions only;
- TypeScript caches and renders Rust-projected items/diffs only;
- temporary bounded-window absence is dormant visibility, never semantic deletion.

## Rust lifecycle contract

### Retention and explicit clearing

A known projection remains until one of these Rust-owned terminals:

1. an accepted aggregate resolves `reply_count == 0` while neither a canonical root nor active reply remains;
2. authoritative root/reply redaction reconciliation produces that same empty result;
3. the Room timeline unsubscribes and `ThreadRootProjectionService::clear_room` runs;
4. account/session teardown drops the manager and reducer state.

A network/SDK failure while checking disappearance retains the last accepted record and records a coarse failure; failure is not evidence of deletion.

Remove bounded-window pruning from `ThreadRootProjectionService::reconcile_room_with_affected`. Keep `active_root_event_ids` and `activity_active` only as Core-owned visibility inputs for aggregate scheduling; they never authorize deletion. Delete the test-only pruning wrapper and remove `is_active_or_unreported` plus the inactive-removal branches from `mark_ready` / `mark_failed`, so a late terminal remains retained until explicit validation/clear. The method may update current visibility/activity observations and schedule aggregate validation, but it may not remove a record. Keep the existing `THREAD_SUMMARY_PROJECTION_MAX_ROOTS = 120` hard admission bound per active Room owner. Dormant records count toward the cap; at capacity, reject additional projection admission rather than evicting an accepted root from incomplete evidence. Room unsubscribe/session teardown is the explicit storage release policy.

### Canonical and hydrated root snapshots

Store one renderable root `TimelineItem` in the service for both canonical roots and fetched off-window roots. Change canonical seeding to pass the complete item, not only `ThreadSummaryDto`. The same item/aggregate record then replaces the separate replay-known lifecycle. Delete `ReplayKnownThreadRootProjectionRegistry`, source epochs, suppressed/emitted terminal bookkeeping, and the `retain_without_reply` exception after equivalent service tests are GREEN.

### Rust State as mirror

Delete `active_root_event_ids`, `is_active_or_unreported`, bounded-window pruning in `ThreadRootProjectionState::reconcile_room`, and `AppAction::ThreadRootProjectionsReconciled`. State changes only through explicit `Observed`, `Ready`, `Failed`, per-root `Cleared`, and room/session clear actions emitted by Core. State must not choose activity rollback or record lifetime.

## Rust-owned display projection

Extend the existing actor-owned `DisplayProjectionState`; do not add another projection service.

### Wire item metadata

Add an optional Rust-owned display metadata object to emitted `TimelineItem` clones:

```rust
pub struct TimelineDisplayMetadata {
    pub row_id: String,
    pub kind: TimelineDisplayKind,
    pub content_event_id: Option<String>,
    pub activity_event_id: Option<String>,
    pub display_timestamp_ms: Option<u64>,
}

pub enum TimelineDisplayKind {
    Event,
    ThreadRoot,
    ThreadRootPending,
    ThreadRootFailed { failure_kind: OperationFailureKind },
}
```

Canonical `navigation_items` remain SDK-order items without display metadata. `DisplayProjectionState.display_items` contains bounded Rust-projected clones with stable row identity and activity placement metadata. Pending/failed placeholders use a bounded `TimelineItemId::Synthetic` identity derived from the root projection slot; they never impersonate a server Event ID and retain default non-actionable message affordances. Custom `Debug` exposes only kinds/count/presence, never identifiers.

### Placement rules

For Room timelines only:

- `RootEvent`: suppress standalone thread replies and retain each root at its canonical origin.
- `LatestReply`: suppress standalone replies; place one complete root block at the accepted service activity slot; use a retained service snapshot when the root is outside the bounded window; use Rust-owned activity timestamp ordering only for a summary-only placement inside the represented window.
- pending/failed hydration emits a Rust-synthesized private-data-safe placeholder item with the same stable root row ID;
- Thread and Focused timelines preserve their ordinary item order;
- canonical SDK vectors, search inputs, read-state/navigation calculations and event-cache ownership remain unchanged.

Initial `TimelineEvent::InitialItems` must use `DisplayProjectionState.display_items()`, matching the already display-relative `ItemsUpdated` contract. SDK batches, service terminals, replay, setting changes, repair and redaction all rebuild through one validated display-diff builder. An actor-generation lease fences every emitted InitialItems/diff.

Extend the existing bounded latest-wins `ThreadSummaryProjectionIngress` to carry either `Updated { activity_revision, summary_revision }` or `Cleared` for the exact root. Every pending/ready/failed service transition and authoritative clear publishes that wake to the current Room actor. `Cleared` is accepted only when the exact generation is current and the service no longer owns the root; the actor then rebuilds `DisplayProjectionState` and emits one validated Remove/Reset diff. A newer Updated for the same root supersedes an older Clear, and actor replacement/unsubscribe makes delayed wakes inert.

### Setting propagation

`TimelineThreadRootOrder` remains Rust-owned settings state. Extend the existing SettingsChanged control path to update current Timeline actors and synchronously reproject through `DisplayProjectionState`. React no longer passes `threadRootOrder` into `TimelineView` for semantic placement.

## TypeScript cutover

- Remove `threadRootProjections` from `TimelineStoreState` and delete `retainActiveThreadRootProjections`, source normalization/epoch matching, and `ThreadRootProjection` event handling.
- Remove the public `TimelineEvent::ThreadRootProjection` wire variant and checked-in TypeScript/generated mirrors after Rust display diffs cover pending/ready/failed/cleared transitions.
- Reduce `timelineDisplayProjection.ts` to a one-to-one renderer adapter over Rust order/metadata plus renderer-local date-divider presentation. Delete root selection, reply suppression, hydrated/replay fallback, insertion ordering, and placeholder synthesis.
- `TimelineView` retains virtualization, DOM measurement, anchors and layout settlement. It uses Rust `row_id`, content ID, activity ID and display timestamp without recomputing Matrix/thread semantics.
- Browser Fake and harness install Rust-shaped display items/diffs; they do not implement a second lifecycle.

Audit every production reader of `TimelineKeyState.items` before cutover. Current source has one production selector consumer, `TimelineView`, including its avatar/media/diagnostic side-effect windows; all receive Rust display items intentionally. Rust `navigation_items` remains the unchanged canonical source for read state, search, receipts, Activity and SDK/event-cache reconciliation. Add wire/behavior tests proving InitialItems and ItemsUpdated are the same display-index domain while canonical Rust navigation/search/read-state inputs retain replies and SDK order.

## Initial existing-thread opening

Existing/pinned threads must not publish a confirmed-empty first projection before required history loading:

1. Add `intent: ThreadOpenIntent` to `AppEffect::OpenThreadTimeline`; the reducer copies the exact accepted intent into both state and effect rather than rereading mutable state later.
2. Runtime maps it to a Core-internal `InitialBackfillPolicy` on `TimelineCommand::Subscribe`: `RequiredForExistingThread` for `ExistingThread` / `PinnedReply`, `Disabled` for `NewThreadDraft` and every Room/Focused subscription. Manager routing and `build_timeline_actor_handle` carry that policy to the pre-spawn gate.
3. Before actor construction emits InitialItems, inspect the SDK Thread timeline. If empty and the policy is required, run one bounded explicit backward page through the account-work scheduler, then subscribe again.
4. On success with items, construct the actor from that settled source. `Ok(true)` end-reached plus zero items is authoritative empty. `Ok(false)` plus zero items is incomplete—not empty—and fails the matching subscription through the existing `TimelineFailureKind::Sdk` / `ThreadSubscriptionFailed` path with a closed diagnostic token. An SDK pagination error uses the same typed failure path. No InitialItems or `ThreadSubscribed` is published in either failure case.
5. The existing Room empty-hydration behavior remains intentionally non-fatal and is not changed by this Thread policy.
6. New-thread drafts remain immediately composer-capable and never perform this history page.

No sleep, polling loop, unbounded pagination, or new retry service.

## Canon amendments before production code

In this PR, amend before implementation:

- `docs/architecture/overview.md`: Core service/actor ownership, explicit clears, display metadata/diffs, initial existing-thread page.
- `docs/architecture/state-machine.md`: retained/dormant lifecycle, explicit clear guards, and `Opening -> Open` only after settled existing-thread initial projection.
- `docs/agents/state-ownership.md`: frontend cache-only rule and full snapshot/wire mirror checklist.
- `docs/architecture/frontend-ownership-inventory.md`: mark #708 as the next high-value #552 leaf and remove placement/lifecycle from frontend ownership after cutover.
- `docs/superpowers/plans/2026-08-27-issue552-remaining-ownership-phases.md`: insert #708 immediately after Phase 0 and shift later phases.

## RED-first verification

Capture assertion failures against current production behavior before wiring:

1. **State retention:** a Ready and Failed projection survive `ThreadRootProjectionsReconciled { activities: [] }`; current State deletes both.
2. **Core retention:** a terminal service record survives empty bounded reconciliation and a failed disappearance aggregate; current service removes it.
3. **Frontend cache:** an ordinary Ready projection survives an empty `InitialItems` until explicit Core clear; current `retainActiveThreadRootProjections` removes it.
4. **Rust placement:** given canonical root/replies and `LatestReply`, Core display items contain one root at the accepted activity slot with no standalone replies; current Core display projection preserves canonical placement/replies.
5. **Ordering convergence:** empty/reset/replay/repair event permutations converge to the same Rust display rows without remove/reinsert oscillation.
6. **Authoritative clear:** aggregate zero/redaction and unsubscribe each emit one removal and clear State/display storage.
7. **Initial thread open:** an empty first Thread subscribe paginates before InitialItems for existing intent; current actor emits empty first. End-reached empty opens authoritatively; non-end empty/error fails without InitialItems. A draft performs zero pagination.
8. **Teardown/bounds:** 120 roots remain bounded; over-cap admission is rejected; unsubscribe clears records and no worker survives manager shutdown.
9. **TypeScript render-only:** shuffled canonical/root-projection fixtures cannot change Rust-provided row order/identity; current selector reorders them.
10. **Metadata-only diff:** changing only Rust display activity/timestamp metadata emits one validated Set at the stable row identity, never remove/reinsert or duplicate.
11. **Promoted draft policy:** `NewThreadDraft -> ExistingThread` state promotion cannot retroactively derive an initial-backfill policy; only the original effect-carried intent controls the subscription.

RED #1/#2 deliberately name the soon-to-be-deleted reconciliation action to capture current failure. They are capture-only. GREEN retention coverage must use only surviving Observed/Ready/Failed/Cleared/RoomCleared actions and service APIs; no source/test reference to the deleted action remains.

Use deterministic channels/barriers/fake clocks; no fixed sleeps and no source-text assertion as behavioral proof.

## Implementation sequence

1. Amend canon and add the focused RED tests.
2. Make Core service retention/clear authoritative; remove State reconciliation ownership.
3. Store canonical/hydrated root snapshots in the service and fold replay-known ownership into it.
4. Extend `DisplayProjectionState`, Timeline display metadata, Updated/Cleared wake propagation, setting propagation, and existing-thread initial backfill policy transport.
5. Emit Rust-projected InitialItems/diffs; remove the `ThreadRootProjection` public event/cache path after auditing every `TimelineKeyState.items` production consumer.
6. Delete TypeScript lifecycle/placement semantics and update Browser Fake/harness/wire artifacts.
7. Add replay/repair/redaction/unsubscribe/order-permutation and initial-open GREEN coverage.
8. Extend an existing thread QA scenario with private-data-free fixed tokens rather than creating another server/session setup.
9. Run full gates, exact diff review, PR, CI and merge.

## Parallel-machine boundary

PR #709 (`fix/issue-696-media-save-attachment-keys`) owned `apps/desktop/src/components/panes.tsx` and `TimelineView.media.test.tsx` plus media/dialog files and is now merged. This branch rebases onto that merge before production edits. #708 does not modify media/dialog/right-panel behavior; it removes only the `threadRootOrder` wiring and adds assertions in thread-specific tests. Do not overwrite, revert, or reformat #709 changes.

Stale historical #551/#552 worktrees and already merged branches are read-only evidence, not integration targets. This branch integrates only from current `origin/main`.

## Expected files

Primary production/test surfaces:

- `crates/koushi-core/src/threads_list.rs`
- `crates/koushi-core/src/timeline/{actor,display_projection,manager,relay,thread_projection}.rs`
- `crates/koushi-core/src/event/timeline.rs`
- `crates/koushi-state/src/{action,effect,reducer,state/thread}.rs`, `crates/koushi-backend/src/lib.rs`, runtime/effect source-contract tests, and focused state tests
- `apps/desktop/src/domain/{timelineStore,timelineDisplayProjection,coreEvents}.ts` and generated wire artifact
- `apps/desktop/src/components/TimelineView.tsx` and thread-specific tests
- Tauri DTO/wire tests, Browser Fake, harness, and required goldens
- canon, #552/#708 plans, plan index, and focused QA contract

No vendor change, dependency, persistence schema, generic projection framework, compatibility shim, custom Matrix event, retry timer, plaintext cache, TODO, or unrelated UX change.

## Validation and merge gate

Focused RED/GREEN commands are selected from the affected test binaries, then run:

```bash
cargo fmt --all -- --check
cargo test -p koushi-state --test threads_list_state
cargo test -p koushi-core --lib thread_root_projection
cargo test -p koushi-core --lib display_projection
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
npm --prefix apps/desktop test -- --run src/domain/timelineStore.test.ts src/domain/timelineDisplayProjection.test.ts
npm --prefix apps/desktop test -- --run src/components/TimelineView.threads.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run build
cargo test --workspace
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
(cd apps/desktop && npx playwright test)
node scripts/check-sdk-submodule.mjs
node scripts/check-agents-docs.mjs
git diff --check
```

Also run wasm, cargo-deny, cargo-machete, secret/boundary/wire/golden checks and the chosen Tuwunel/Synapse thread QA lane. Generate one exact final diff, obtain `reviewer-flash` `Correct-to-merge`, fix and re-review every finding, create one PR closing #708 but not #552, require current-head CI green, merge, verify `origin/main`, and clean the worktree/artifacts.

## Acceptance map

| #708 requirement | Required evidence |
| --- | --- |
| temporary bounded absence never deletes a thread projection | State/Core RED-GREEN plus permutation tests |
| replay/pagination/repair converge without oscillation | Rust display-diff tests and browser-headless thread test |
| one lifecycle owner | deleted State reconcile owner, replay registry and TS pruning |
| no TS deletion inference | no `retainActiveThreadRootProjections`; explicit Rust clear test |
| Rust-owned root/latest placement | `DisplayProjectionState` order/identity tests and deleted TS selectors |
| no transient confirmed-empty existing thread | initial-backfill actor test and QA token |
| genuine empty/deleted thread clears | aggregate-zero/redaction tests |
| unsubscribe/session teardown settles | manager clear/shutdown tests |
| bounded storage | 120-root cap and room-clear test |
| required event permutations | deterministic table-driven Core/TS tests |
| Browser Fake mirrors Core | wire artifact/fake/harness contract tests |

## Design review record

- Round 1, `reviewer-flash`: `Not correct-to-merge`. Blocking findings required explicit intent transport, a terminal for `Ok(false)` plus zero initial Thread items, an authoritative Clear wake into the Room display actor, and an audit of every non-display consumer affected by making InitialItems display-relative. Minor findings required explicit dormant-cap rejection and exact removal/retention of Core activity helpers.
- The design now carries accepted `ThreadOpenIntent` through AppEffect/Core policy, fails non-end empty without publishing InitialItems, extends the existing latest-wins wake with generation-fenced Updated/Cleared transitions, records the sole `TimelineKeyState.items` production consumer and canonical Rust owners, preserves the 120 dormant-inclusive admission cap, and removes inactive terminal gates while retaining Core visibility inputs only.
- Round 2, `reviewer-flash`: **Correct-to-merge**. All six Round 1 findings were resolved; no Critical or Important finding remained. The five minor mechanics are applied: capture-only deleted-action RED tests become surviving-action GREEN tests, backend/runtime effect mirrors are in scope, draft promotion cannot rederive policy, metadata-only changes require a stable Set diff, and pending/failed placeholders use Synthetic identities.

#552 remains open after #708; later adapter/ACK/request-ref/mutation phases continue from the updated plan.
