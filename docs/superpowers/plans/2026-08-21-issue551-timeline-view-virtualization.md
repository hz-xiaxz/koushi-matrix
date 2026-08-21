# Issue #551 TimelineView virtualization-model extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `47e1a5254b36211494a2607d4a414cd27388d89c` (merged PR #599).
- `apps/desktop/src/components/TimelineView.tsx`: 5,100 newline-delimited lines.
- Focused immutable baseline 72/72:
  `npm --prefix apps/desktop test -- --run src/components/TimelineView.viewport.test.tsx src/components/TimelineView.scrollback.test.tsx src/components/TimelineView.rendering.test.tsx`.

## Ownership decision

Move the complete pure virtualization model and cancellable browser-frame scheduler to direct private leaf `apps/desktop/src/components/timeline/TimelineViewportVirtualization.ts`.

The leaf owns viewport/range/height data models, immutable empty sentinels, height normalization, binary-search/range calculations and the existing RAF-plus-timeout race. `TimelineView` keeps every React state/ref/effect, scheduled-handle owner, key-change/unmount cancellation call, ResizeObserver, scroll intent, anchor/session/projection/backfill policy and rendered DOM.

This is a model boundary, not a controller wrapper: no hook/class/context/object bundle is introduced. The parent imports explicit types/functions and continues to own resource lifetime.

## Exact inventory

Move exactly 25 top-level AST statements, preserving declaration order within the leaf:

1. `TIMELINE_VIRTUALIZATION_THRESHOLD`
2. `TIMELINE_VIRTUAL_OVERSCAN_ITEMS`
3. `TIMELINE_ESTIMATED_ITEM_HEIGHT_PX`
4. `TIMELINE_MIN_ITEM_HEIGHT_PX`
5. `TIMELINE_MAX_ITEM_HEIGHT_PX`
6. `TimelineViewportMetrics`
7. `TimelineVirtualRangeState`
8. `TimelineItemIndexRange`
9. `TimelineVirtualWindow`
10. `EMPTY_TIMELINE_RANGE`
11. `EMPTY_TIMELINE_ITEM_INDEX_RANGE`
12. `TimelineHeightModel`
13. `estimatedItemHeight`
14. `measuredItemHeight`
15. `TIMELINE_FRAME_FALLBACK_MS`
16. `TimelineScheduledFrame`
17. `scheduleTimelineFrame`
18. `buildTimelineHeightModel`
19. `timelineIndexAtOffset`
20. `virtualRangeEquals`
21. `timelineItemIndexRangeEquals`
22. `timelineItemIndexInRange`
23. `calculateTimelineItemIndexRange`
24. `calculateTimelineVirtualRange`
25. `timelineItemHeightAtIndex`

The leaf imports only `TimelineDisplayRow` from `domain/timelineDisplayProjection`.

Explicit leaf exports are exactly 19 names needed by the parent:

- constants: `TIMELINE_VIRTUALIZATION_THRESHOLD`, `TIMELINE_ESTIMATED_ITEM_HEIGHT_PX`;
- types: `TimelineViewportMetrics`, `TimelineVirtualRangeState`, `TimelineItemIndexRange`, `TimelineVirtualWindow`, `TimelineHeightModel`, `TimelineScheduledFrame`;
- sentinels: `EMPTY_TIMELINE_RANGE`, `EMPTY_TIMELINE_ITEM_INDEX_RANGE`;
- functions: `measuredItemHeight`, `scheduleTimelineFrame`, `buildTimelineHeightModel`, `virtualRangeEquals`, `timelineItemIndexRangeEquals`, `timelineItemIndexInRange`, `calculateTimelineItemIndexRange`, `calculateTimelineVirtualRange`, `timelineItemHeightAtIndex`.

The six remaining declarations are private implementation details: virtual overscan, min/max height, frame fallback, `estimatedItemHeight`, and `timelineIndexAtOffset`.

Only `TimelineView.tsx`, the new leaf and this plan/index may change. No compatibility re-export: no existing external caller imports these private declarations.

## Invariants

- Every moved body/comment/token is exact apart from `export` modifiers and the relative type-import path.
- Threshold 200, overscan 20, estimated height 80, clamp 24..1600, fallback 16ms, 600px empty-client fallback, rounding and binary-search bounds remain exact.
- Scheduler still starts RAF and timeout together; the first callback cancels the other, invokes once, and `cancel()` remains idempotent.
- Existing component refs/effects remain the sole owners that cancel returned handles on supersession, timeline-key change and unmount.
- Empty sentinel object identity is preserved; no fresh-object replacement or extra state commit.
- No Matrix/product semantics, transport/DTO/wire, DOM/a11y/CSS/i18n, viewport intent, anchor/session, projection/backfill policy, test or dependency change.
- No barrel, wrapper-only abstraction, duplicated type/logic, TODO or public façade growth.

## Verification

- AST exactness: 25/25 in leaf, parent 0, exports 19/19; private 6 remain unexported.
- Dependency/resource checks: leaf has one domain type import; no React/TimelineView/store/transport import; parent retains all scheduler handle refs and cancellation sites.
- Focused baseline/post 72/72, typecheck, lint, diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design: pending `reviewer-flash` read-only verdict.
- Implementation prohibited until `Correct-to-implement`.
- Full diff and delivery pending.
