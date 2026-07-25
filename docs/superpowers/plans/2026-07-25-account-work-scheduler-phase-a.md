# Account-wide priority scheduler (issue #306) — Phase A

Date: 2026-07-25

## Problem restated

`MessagesBackpressure` is an endpoint gate with two hard-coded classes
(`Timeline`, `Crawler`). It cannot express the account's real work mix:

- timeline gap repair calls `repair_room_timeline_gap` without joining the gate,
  so an offscreen 64-event batch loop competes with everything else;
- activity/history resolution acquires the *timeline* class even though it is
  background work;
- outgoing sends neither signal nor preempt background work.

Fixing each caller separately moves the contention. One policy boundary is
needed.

## Design

New module `crates/koushi-core/src/account_work.rs` replaces
`messages_backpressure.rs`.

### Central policy

```rust
pub(crate) enum AccountWorkKind {
    MessageSend, UserRoomOperation,
    VisibleGapRepair, ExplicitPagination,
    OffscreenGapRepair, SearchCrawl, Maintenance,
}

pub(crate) struct AccountWorkPolicy {
    pub priority: u8,        // lower is more important
    pub preemptible: bool,
    pub max_concurrency: u8,
    pub batch_limit: u16,
}
```

`AccountWorkKind::policy()` is the only place numbers appear. Call sites name
the kind. Priorities follow the issue's bands (0 / 16 / 32 / 40 / 96 / 128 /
192) with gaps left for later insertion.

### Two admission classes

Interactive kinds (`MessageSend`, `UserRoomOperation`) never queue. They take a
non-blocking `InteractiveWorkGuard` scoped to the **SDK enqueue**, not remote
settlement:

- entering cancels every active preemptible permit whose priority is worse,
- while held, preemptible work with worse priority is not admitted, so a
  yielding crawler cannot immediately re-enter and re-contend,
- leaving wakes waiters.

Scheduled kinds take an `AccountWorkPermit` from `acquire(kind)`:

- admitted when in-flight work of that kind is below `max_concurrency`, the
  account-wide history slot is free, no strictly-better-priority waiter is
  queued, and no active interactive guard outranks it;
- FIFO inside one priority, so equal-priority work cannot starve;
- `permit.cancelled()` resolves when better-priority work needs the slot.
  Cancellation is cooperative and is **not** a failure: the caller finishes its
  current bounded batch, keeps its checkpoint, and re-enters scheduling.

Sync and SDK-owned essential traffic never enter the scheduler.

### Batch granularity, not request abort

Gap repair already runs bounded batches (`event_limit: 64`). Each batch takes
one permit, so the yield point is between batches and the existing
`gap_repair` tracker (serial, `batches_processed`, `demand_revision`) is the
checkpoint. The vendored SDK exposes no cancellation argument for
`repair_timeline_gap_with_projection` (unlike live-tail refresh), so this phase
does not abort an in-flight request; it stops the next batch. That limit is
recorded here deliberately rather than patching vendored SDK.

### Diagnostics

`core.account_work` events at `queued`, `started`, `preempted`, `yielded`,
`completed`, `failed` carrying account-local work id, kind token, priority,
preemptible flag, queue wait ms, run ms, batch/item counts, preemption source,
active better/worse counts, and cancellation generation. Private-data-free:
no room ids, event ids, user ids, bodies, or raw SDK errors.

## Migration order

1. Add the module with policy + scheduler + permits + interactive guard and its
   deterministic unit tests. Keep `acquire_timeline`/`acquire_crawler`
   behavior reproducible through the new API.
2. Route existing callers by kind:
   - `TimelineActor::paginate_once_for` → `ExplicitPagination`
   - initial empty-room hydrate → `ExplicitPagination`
   - `search_crawler` page runner → `SearchCrawl`
   - `activity_resolution` → `SearchCrawl` (non-visible history hydration)
3. Join gap repair to the scheduler with the visibility-derived kind
   (`VisibleGapRepair` when the gap intersects the reported viewport, else
   `OffscreenGapRepair`).
4. Wrap the send enqueue path (`spawn_send_enqueue` →
   `enqueue_timeline_send`) in the interactive guard.
5. Delete `messages_backpressure.rs` and migrate the source-text guard tests to
   behavioral scheduler tests.

## Tests

Deterministic `tokio` tests in the new module: policy mapping and numeric
ordering; FIFO within a priority; better-priority waiter admitted first;
active preemptible work cancelled by a better-priority waiter; interactive
guard cancels active background work and defers re-admission; non-preemptible
work is not cancelled; permit release on drop after panic/timeout/cancel;
dropped waiter does not starve others; background progress on an idle account.

Actor-level: pagination still precedes crawling; gap repair takes a permit
before the SDK call; a send's interactive guard is entered before
`enqueue_timeline_send`.

## Out of scope for Phase A

- Aborting an in-flight SDK request mid-batch.
- Per-endpoint concurrency above one for history traffic.
- Reordering sync or SDK-owned traffic.
- Classifying every remaining app-owned operation. `UserRoomOperation` and
  `Maintenance` stay reserved bands: they are covered by the policy tests and
  carry no production caller yet. Room membership/tag/moderation traffic is the
  next audit step (issue migration item 6).

## Phase A status (2026-07-25)

Landed:

- `account_work.rs` with the policy table, three scheduling classes, permits,
  the interactive guard, diagnostics, and nine deterministic scheduler tests.
- `messages_backpressure.rs` deleted; every caller migrated to a named kind:
  pagination and initial hydrate (`ExplicitPagination`), search crawling
  (`SearchCrawl`), stale-unread activity hydration (`SearchCrawl`, previously
  mis-classified as timeline work).
- Gap repair joins the scheduler with a visibility-derived kind, takes one
  permit per bounded batch, reports the yield, and releases before local
  projection settlement. The batch bound now comes from the policy.
- Sends, redactions, and reactions hold the interactive guard across the SDK
  enqueue only; admission and the local echo stay ahead of it.
