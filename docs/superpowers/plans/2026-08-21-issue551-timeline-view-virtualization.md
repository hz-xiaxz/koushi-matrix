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

The leaf imports only `TimelineDisplayRow` through exact type-only path `../../domain/timelineDisplayProjection`.

Explicit leaf exports are exactly 18 names needed by the parent:

- constants: `TIMELINE_VIRTUALIZATION_THRESHOLD`, `TIMELINE_ESTIMATED_ITEM_HEIGHT_PX`;
- types: `TimelineViewportMetrics`, `TimelineVirtualRangeState`, `TimelineItemIndexRange`, `TimelineVirtualWindow`, `TimelineScheduledFrame`;
- sentinels: `EMPTY_TIMELINE_RANGE`, `EMPTY_TIMELINE_ITEM_INDEX_RANGE`;
- functions: `measuredItemHeight`, `scheduleTimelineFrame`, `buildTimelineHeightModel`, `virtualRangeEquals`, `timelineItemIndexRangeEquals`, `timelineItemIndexInRange`, `calculateTimelineItemIndexRange`, `calculateTimelineVirtualRange`, `timelineItemHeightAtIndex`.

The seven remaining declarations are private implementation details: virtual overscan, min/max fallback height, frame fallback, `TimelineHeightModel`, `estimatedItemHeight`, and `timelineIndexAtOffset`.

Only `TimelineView.tsx`, the new leaf and this plan/index may change. No compatibility re-export: no existing external caller imports these private declarations.

## Invariants

- Every moved body/comment/token is exact apart from `export` modifiers and the relative type-import path.
- Base `47e1a525` values remain exact: virtualization threshold 600, overscan 60, estimated fallback height 72, fallback-height clamp 36..480, measured-height floor 1, frame fallback 16ms and empty-client viewport fallback 600px; rounding and binary-search bounds also remain exact.
- Scheduler still starts RAF and timeout together; the first callback cancels the other, invokes once, and `cancel()` remains idempotent.
- Existing component refs/effects remain the sole owners that cancel returned handles on supersession, timeline-key change and unmount.
- Empty sentinel object identity is preserved; no fresh-object replacement or extra state commit.
- No Matrix/product semantics, transport/DTO/wire, DOM/a11y/CSS/i18n, viewport intent, anchor/session, projection/backfill policy, test or dependency change.
- No barrel, wrapper-only abstraction, duplicated type/logic, TODO or public façade growth.

## Verification

- AST exactness: 25/25 in leaf, parent 0, exports 18/18; private 7 remain unexported.
- Dependency/resource checks: leaf has one domain type import; no React/TimelineView/store/transport import; parent retains all scheduler handle refs and cancellation sites.
- Focused baseline/post 72/72, typecheck, lint, diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design round 1: `reviewer-flash` recorded `Changes-required` because the first draft documented stale constant values despite the source inventory being correct.
- Amendment: pin base `47e1a525` values 600/60/72/36..480 and exact leaf import path; retain the deliberate `wc -l` newline-delimited baseline count.
- Design round 2: `reviewer-flash` revalidated all 25 declarations, exports, constants, dependencies and cancellation owners and recorded `Correct-to-implement`.
- Implementation: integrated by `luna-implementer` and parent-audited.
- Exactness: 25/25 statements, parent 0, exports 18/18, private 7/7; `TimelineView.tsx` 5,100 → 4,869 newline-delimited lines and the leaf is 241.
- Focused post-move 72/72; typecheck, lint and diff checks green; all scheduler handle refs and cancellation sites remain in the parent.
- Full-diff round 1: `reviewer-flash` recorded `Correct-to-merge` and noted that `TimelineHeightModel` was inferred internally rather than imported by the parent.
- Minimal-visibility delta: make that type private, narrowing exports 19 → 18 and correcting fallback/measured-height wording; delta re-review pending.
- Delivery pending.
