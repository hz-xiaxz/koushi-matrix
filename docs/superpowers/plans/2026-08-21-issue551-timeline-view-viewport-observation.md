# Issue #551 TimelineView viewport-observation extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `c281974f04468212b810b5f907a23df1f3653a10` (merged PR #603).
- `apps/desktop/src/components/TimelineView.tsx`: 4,353 newline-delimited lines.
- Focused immutable baseline: all seven `TimelineView.*.test.tsx` suites, 173/173, using the literal command recorded below.

## Ownership decision

Move viewport DOM fact collection, bottom-edge geometry, user keyboard release classification and automatic-backfill threshold math to direct private leaf `apps/desktop/src/components/timeline/TimelineViewportObservation.ts`.

The leaf only observes caller-supplied DOM or performs pure numeric/input classification. It owns no listener, React state/ref/effect, transport call, timer/frame/observer, backfill epoch, scroll write scheduling or cleanup. `TimelineView` continues to decide when observations occur and what commands/state transitions follow.

## Exact inventory

Move exactly 13 top-level AST statements in this order:

1. `CANONICAL_UNSIGNED_DECIMAL`
2. `MAX_U32`
3. `isCanonicalUnsignedDecimal`
4. `parseCanonicalU32`
5. `visibleTimelineViewportFacts`
6. `isScrolledToBottom`
7. `scrollContainerToBottom`
8. `timelineKeyShouldReleaseViewportIntent`
9. `AUTO_BACKFILL_THRESHOLD_PX`
10. `AUTO_BACKFILL_VIEWPORTS`
11. `SCROLL_EDGE_TOLERANCE_PX`
12. `timelineBackfillThreshold`
13. `timelineBackfillThresholdForTests`

Leaf imports exactly three statements:

- type-only `KeyboardEvent` from `react`;
- type-only `TimelineGapId` from `../../domain/coreEvents`;
- runtime `eventIdForTimelineIdentity` from `./TimelineViewportAnchors`.

Explicit leaf exports are exactly seven names:

- `visibleTimelineViewportFacts`
- `isScrolledToBottom`
- `scrollContainerToBottom`
- `timelineKeyShouldReleaseViewportIntent`
- `SCROLL_EDGE_TOLERANCE_PX`
- `timelineBackfillThreshold`
- `timelineBackfillThresholdForTests`

Six declarations remain private.

`TimelineView.tsx` imports the six directly used names, explicitly re-exports only existing public test API `timelineBackfillThresholdForTests`, and removes exactly orphaned `TimelineGapId` and `eventIdForTimelineIdentity` imports. Parent `KeyboardEvent` remains because the callback signature still uses it.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- All 13 bodies/comments/types/tokens/order remain exact apart from export modifiers and relative imports.
- Canonical decimal grammar, overflow-safe u32 parsing, gap deduplication/order, first/last visible activity event selection and invalid dataset rejection remain exact.
- Bottom tolerance remains 2px; direct bottom write remains `scrollHeight - clientHeight`.
- Keyboard modifier and key whitelist remain exact.
- Backfill threshold remains disabled=0 and enabled=`max(80, max(0, clientHeight)*2)`.
- Existing parent test import path remains compatible; no new parent public API.
- Parent retains every observation trigger, scroll intent, write reason, backfill state/fence, listener/effect and resource cleanup.
- Leaf has no hook/state/listener/timer/frame/observer/transport/store mutation; one acyclic sibling runtime import only.
- No Matrix semantics, DTO/wire, dependency, test/config, CSS/i18n/a11y, barrel, wrapper, callback registry, duplicate logic or TODO change.

## Verification

- AST exactness: 13/13 leaf, parent 0, exports 7/7, private 6/6, imports 3/3, orphan imports 2/2.
- Same focused command before/after, 173/173:
  `npm --prefix apps/desktop test -- --run src/components/TimelineView.interactions.test.tsx src/components/TimelineView.live-state.test.tsx src/components/TimelineView.media.test.tsx src/components/TimelineView.rendering.test.tsx src/components/TimelineView.scrollback.test.tsx src/components/TimelineView.threads.test.tsx src/components/TimelineView.viewport.test.tsx`.
- Typecheck, lint and diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design pending `reviewer-flash` read-only verdict.
- Implementation prohibited until `Correct-to-implement`.
- Full diff and delivery pending.
