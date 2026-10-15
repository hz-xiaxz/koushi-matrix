# Reachable older history after leaving the live edge (#1443)

Canon consulted: `REPOSITORY_RULES.md` ("Root-Cause Fix Discipline", "State-
Machine Discipline", "Review And Audit"), engineering rules "Crate Ownership And
Projection Authority", and `docs/architecture/state-machine.md` ("Start of
conversation"). Design and final state:
`docs/plans/2026-10-15-issue1443-timeline-hidden-prefix.md`.

## Root cause

Two faults, both in Core display projection plus one React predicate:

1. `DisplayProjectionContext` bounded a Room at the bottom to the newest 120
   rows and `trim_to_live_edge` hid the older canonical prefix. `ObserveViewport`
   never re-materialized it when the reader left the bottom, and
   `project_sdk_batch` forced every front insertion visible instead of requiring
   boundary adjacency, so a later page jumped the hidden prefix.
2. The React start marker keyed off SDK `EndReached` + authoritative continuity
   only, with no proof that the displayed oldest row reached the canonical start.

## Change

- **Adjacency gate:** `DisplayProjectionContext::include_prepend` is removed;
  `PushFront` and `Insert`-at-0 pass `None` to `DisplayMembershipRope::insert`,
  so its existing adjacency rule (`was_empty || left edge || right edge`) decides
  visibility. The old test asserting the skip is deleted.
- **Re-materialization:** `DisplayProjectionState::rematerialize_window` rebuilds
  a canonical window in place, keeping `pending_items` and
  `suppressed_transaction_ids`, and returns incremental diffs (prefix expansion =
  `Insert`-at-0 prepends, never a reset). `first_visible_canonical_index` exposes
  the oldest displayed canonical index. `TimelineActor::materialize_hidden_history`
  runs on the Room `at_bottom: true → false` transition when that index is above
  zero, emits the diffs as `ItemsUpdated`, and requests the ordinary coalesced
  gap inspection when the room has known gaps, so gap positions are rebuilt from
  displayed boundary identities. `handle_replay_initial_items` is not reused
  because it emits `InitialItems` and would reset the reader's anchor.
- **Marker proof:** the `TimelineStart` synthetic row stays `is_hidden = true`,
  and `materialize` only carries membership-visible cells, so its presence in the
  display items is the Core-owned proof that the displayed oldest boundary
  reaches the SDK start. `TimelineView.tsx` now requires that row in addition to
  `EndReached` + healthy + `authoritative_start`.
- **Canon sync:** the Start-of-conversation rule in
  `docs/architecture/state-machine.md` now records the displayed-boundary
  requirement.

## Independent review

Pre-implementation review (gpt-6.1-sol, read-only) raised: a naive rebuild would
drop pending sends/suppressed echoes (BLOCKER — fixed by expanding the existing
state), a stale `Insert`-at-0 override, gap-position staleness, a missing reverse
transition, and a marker field that would need a reducer transition. The plan was
revised before coding; all claims were verified against source first.

Post-implementation review (same model) raised three IMPORTANT findings, all
fixed and re-verified:

1. A display-relative gap offset cannot be derived from a canonical slot count
   (hidden suppressed replies change the count) → replaced the shift with a
   coalesced gap inspection.
2. The extra `next_serial` allocation could overtake an in-flight inspection and
   make the renderer reject its later `GapPositionsUpdated` (`timelineStore.ts`
   drops older generations) → the serial allocation is deleted; the inspection
   allocates its own serial.
3. A gap whose boundaries were both hidden stayed missing → the same queued
   inspection rebuilds the full projected gap set.

The reviewer also noted the new `emit_display_items_updated` duplicated the
existing `TimelineActor::emit_items_updated`; the duplicate is deleted. No
BLOCKER remained.

## Verification

- `cargo test -p koushi-core --lib` — 1614 passed.
- `cargo fmt --check`; `cargo clippy -p koushi-core --all-targets -- -D warnings`.
- `npx tsc --noEmit`; `npx vitest run` — 2033 passed.
- Projection tests: adjacency negative/positive, `rematerialize_window` prepends
  with a pending send, the bounded-window → leave-live-edge → `TimelineStart`
  sequence. Renderer test: marker absent until the display reaches the start.

Not covered: the actor transition is exercised only at the projection and
renderer tiers (the crate has no lightweight `ObserveViewport` harness), and
scroll-anchor stability is argued from the existing `batchContainsPrepend` /
anchor-capture path rather than a new scroll-offset assertion.
