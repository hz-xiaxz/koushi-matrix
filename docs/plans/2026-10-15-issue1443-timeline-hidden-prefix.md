# #1443 — reachable older history when the viewport leaves the live edge

Status: implemented. Refs [#1443](https://github.com/shinaoka/koushi-matrix/issues/1443).

## Problem

After sign-in or app launch, a room with >120 messages opens at the live edge.
The display is bounded to the newest 120 rows, while background gap repair has
already inserted the older events into the SDK timeline. When the reader scrolls
up, `Start of conversation` appears directly above the oldest visible row and
everything older stays unreachable for the rest of the session. Room re-select
repairs it because that path rebuilds the window.

There are two distinct faults:

1. **Stranded membership.** Leaving the live edge only updates the viewport
   observation; it never re-materializes the canonical prefix that the bounded
   window trimmed, so the older rows exist in the SDK timeline but not in the
   display.
2. **Missing marker-boundary proof.** The React marker is drawn from SDK
   `EndReached` plus authoritative continuity alone, without proving that the
   displayed oldest row actually reaches the canonical start.

## Confirmed root cause (source)

- `display_projection.rs` `DisplayProjectionContext::for_timeline` gives a Room
  observed at the bottom `max_live_edge_items = ROOM_REPLAY_INITIAL_ITEMS_MAX`
  (120); `trim_to_live_edge` hides the oldest visible cells as `Gap` cells and
  `materialize` keeps only visible cells, so the hidden prefix's items live only
  in the actor's `navigation_items`.
- `actor.rs` `ObserveViewport` stored the observation and evaluated gap repair;
  it never re-materialized that prefix, and `reproject_display_items` re-renders
  only the already-selected slots.
- `project_sdk_batch` forced every `PushFront` (and `Insert` at index 0) visible
  whenever a prepend was allowed, instead of requiring boundary adjacency, so a
  front insertion jumped the still-hidden prefix. The old test
  `display_projection_backward_push_front_prepends_historical_page` asserted that
  skip.

## RED evidence

`display_projection_front_insertion_requires_adjacency_to_the_displayed_window`
(tests.rs, commit `25d2d24b`) failed before the fix: the display grew to 121 rows
with the non-adjacent front item.

## Required contract (from #1443)

- The hidden historical prefix is reachable, in order, as the reader scrolls
  back, with stable viewport anchoring.
- Displayed history stays ordered and continuous across pagination and repair.
- The start marker requires authoritative SDK start proof **and** a Core-owned
  proof that the displayed oldest boundary reaches that start.
- An SDK `EndReached` must not make an internal projection gap terminal.

## Implemented change

### A. Adjacency-gated front insertion (Core projection)

`DisplayProjectionContext::include_prepend` is gone. `project_sdk_batch` now
passes `None` to `DisplayMembershipRope::insert` for `PushFront` and
`Insert`-at-0, so the rope's existing adjacency rule decides visibility
(`was_empty || left edge visible || right edge visible`). The forced-visibility
overrides that bypassed it are removed.

### B. Re-materialize the hidden prefix when the viewport leaves the live edge

- `DisplayProjectionState::rematerialize_window(canonical, window, context)`
  rebuilds the membership and display from a canonical window in place, keeping
  `pending_items` and `suppressed_transaction_ids`, and returns incremental diffs
  (a prefix expansion is `Insert`-at-0 prepends, never a reset).
- `DisplayProjectionState::first_visible_canonical_index()` exposes the display's
  oldest canonical index.
- `TimelineActor::materialize_hidden_history` runs on the Room
  `at_bottom: true → false` transition when that index is above zero, emits the
  diffs as `ItemsUpdated`, and — when the room has known gaps — requests the
  ordinary coalesced gap inspection so the projected gap set is rebuilt from the
  displayed boundary identities rather than shifting stale display-relative
  offsets.
- `handle_replay_initial_items` is deliberately not reused: it emits
  `InitialItems`, which would reset the reader's anchor.

Returning to the live edge does not re-trim in the same turn. The bounded window
is re-applied by the next SDK batch (or room re-select), so a quiet room keeps
the expanded history until then; that matches the existing behavior where a
re-select at the bottom rebuilds the bounded window.

### C. Start marker requires the displayed boundary (React)

`item_projection` already keeps the `TimelineStart` synthetic row at
`is_hidden = true`, and `materialize` only carries membership-visible cells, so
that row is present in the display items **only** when a front insertion is
boundary-adjacent. `TimelineView.tsx` now requires that row to be displayed in
addition to `EndReached` + healthy + `authoritative_start`, giving the
Core-owned boundary proof without a new DTO field or reducer transition.

### D. Tests

- Projection: adjacency negative/positive; `rematerialize_window` prepends and
  keeps a pending send; the end-to-end sequence (bounded window → leave the live
  edge → SDK `TimelineStart`) makes the start row visible first.
- Renderer: the marker stays absent while the display does not reach the SDK
  start and appears once Core projects the timeline-start row.
- Removed the test that asserted the skip.

## Risks / out of scope

- **Gap positions**: expansion requests a coalesced gap inspection for rooms
  with known gaps, which recomputes positions from the displayed boundary
  identities; rooms without known gaps pay no extra inspection. Ordinary SDK
  prepends can still leave positions stale, which is pre-existing.
- **Renderer anchoring** is not asserted by a new scroll-offset test; the
  expansion is emitted as `Insert`-at-0 prepends, which
  `batchContainsPrepend` (timelineStore.ts) and the anchor-capture path in
  `TimelineView.tsx` already handle.
- **Actor-level integration test**: the crate has no lightweight
  `ObserveViewport` harness; coverage is projection-level plus renderer, so the
  actor transition itself (one `at_bottom` edge check) is not exercised
  end-to-end headlessly.
- Memory: the expanded window holds the downloaded canonical run while off the
  bottom, which is what `handle_replay_initial_items` already does on re-select.
- The marker half depends on Core projecting the timeline-start row; a platform
  or path that never inserts it would not show the marker.
