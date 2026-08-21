# Issue #551 TimelineView projection commit-boundary extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `5a2834399d805b79374955bb628a091eed3c40f0` (merged PR #602).
- `apps/desktop/src/components/TimelineView.tsx`: 4,440 newline-delimited lines.
- Focused immutable baseline: all seven `TimelineView.*.test.tsx` suites, 173/173.

## Ownership decision

Move the complete projection snapshot/commit boundary to direct private leaf `apps/desktop/src/components/timeline/TimelineProjectionBoundary.ts`.

This leaf owns the pre-DOM-mutation `getSnapshotBeforeUpdate` class boundary, immutable snapshot/transaction types and pure structure/stable-row/signature comparisons. `TimelineView` retains every projection ref, revision, scheduled frame, acknowledgement/retry timer, callback, effect, DOM write and cleanup site.

The class is not a wrapper abstraction added for decomposition: it already exists as the required React commit-phase bridge and moves together with all of its pure model operations.

## Exact inventory

Move exactly seven top-level AST statements in this order:

1. `TimelineProjectionSnapshot`
2. `PendingProjectionLayoutTransaction`
3. `ProjectionSnapshotBoundaryProps`
4. `ProjectionSnapshotBoundary`
5. `timelineProjectionSignature`
6. `projectionStructureChanged`
7. `stableProjectionAnchorRowIds`

Leaf imports exactly three statements:

- `import { Component, type ReactNode } from "react";`;
- type-only `TimelineDisplayRow` from `../../domain/timelineDisplayProjection`;
- type-only `ScrollAnchor` from `./TimelineViewportAnchors`.

Explicit leaf exports are exactly six names used by the parent: both model types, the class boundary and the three pure functions. `ProjectionSnapshotBoundaryProps` remains private.

`TimelineView.tsx` imports those six names and removes exactly orphaned React imports `Component` and `ReactNode`. No parent re-export or external caller changes.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- All seven bodies/comments/types/tokens/order remain exact apart from export modifiers and relative import paths.
- `getSnapshotBeforeUpdate` remains the sole pre-mutation call point, calls `onBeforeProjectionChange(previous, next)` once, returns `null`, renders no DOM and keeps empty `componentDidUpdate`.
- Signature fields/order, structural comparison fields and stable-row filtering/order remain exact.
- `ScrollAnchor` remains the transaction anchor type from the direct anchor owner; no duplicate type.
- Parent retains projection snapshots/transactions, intent revision, frame scheduling/cancellation, ack correlation/retry and all React effects/resource cleanup.
- No transport/store/Matrix semantics, DTO/wire, DOM selector/write, timer, listener, CSS/i18n/a11y, dependency, test/config, barrel, wrapper, callback registry, duplicate logic or TODO change.

## Verification

- AST exactness: 7/7 leaf, parent 0, exports 6/6, private 1/1.
- Imports3 and orphan parent React imports2; no parent/external reverse edge.
- Same focused command before/after, 173/173:
  `npm --prefix apps/desktop test -- --run src/components/TimelineView.interactions.test.tsx src/components/TimelineView.live-state.test.tsx src/components/TimelineView.media.test.tsx src/components/TimelineView.rendering.test.tsx src/components/TimelineView.scrollback.test.tsx src/components/TimelineView.threads.test.tsx src/components/TimelineView.viewport.test.tsx`.
- Typecheck, lint and diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design round 1: `reviewer-flash` validated the seam and recorded conditional `Correct-to-implement` with two minor reproducibility clarifications.
- Amendment: pin the combined React import statement and literal focused command.
- Design round 2: `reviewer-flash` revalidated the complete seam and recorded unconditional `Correct-to-implement`.
- Implementation: integrated by `luna-implementer` and parent-audited.
- Exactness: 7/7 statements, parent 0, exports 6/6, private 1/1 and imports 3/3; `TimelineView.tsx` 4,440 → 4,353 newline-delimited lines and the leaf is 98.
- Focused post-move 173/173; typecheck, lint and diff checks green; every parent projection ref/frame/ack timer/effect/cleanup remains in place.
- Full diff: `reviewer-flash` independently verified the final class/model/import/resource graph and recorded `Correct-to-merge`; parent `wc -l` is confirmed at 4,353 while editor display count includes the unterminated final line.
- Delivery: final repository gates, PR CI and merge pending.
