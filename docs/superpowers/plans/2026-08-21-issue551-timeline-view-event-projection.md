# Issue #551 TimelineView event-projection classification extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `7b05d25280133718024f0f3a74664bc449febcd1` (merged PR #600).
- `apps/desktop/src/components/TimelineView.tsx`: 4,869 newline-delimited lines.
- Focused immutable baseline 173/173 across all seven `TimelineView.*.test.tsx` suites.

## Ownership decision

Move the existing DOM-free timeline event/diff classification and privacy-safe diagnostic formatting to direct private leaf `apps/desktop/src/components/timeline/TimelineEventProjection.ts`.

The leaf classifies Rust-projected diffs, reset/outgoing/prepend facts, event completion, key kind and diagnostic labels. It owns no subscription, listener, React hook/state/ref, transport call, retry/backoff, timer, DOM access, store mutation or Matrix semantics. `TimelineView` retains the event-listener effect, local/App-store application, request/generation fences, backfill epochs, projection acknowledgements, anchors, all resource cleanup and every action.

This prerequisite shrinks the listener's pure dependency surface without inventing the 30-field callback object/custom hook that current viewport coupling would require. The subscription/controller seam remains incomplete after this PR.

## Exact inventory

Move exactly 16 top-level AST statements, preserving this order in the leaf:

1. `timelineDiffsContainOwnOutgoingItem`
2. `timelineDiffIsReset`
3. `timelineDiffsContainReset`
4. `timelineDiffItems`
5. `timelineItemIsOwnOutgoing`
6. `timelineRowsArePurePrepend`
7. `timelineRowsArePurePrependForTests`
8. `latestEventBackedItemId`
9. `emitTimelineEventDiagnosticLog`
10. `timelineDiffLinkPreviewSummary`
11. `timelineBackfillCompletionReason`
12. `paginationStateBackfillCompletionReason`
13. `timelineKindDiagnosticLabel`
14. `paginationStateLogLabel`
15. `anchorRestoreStatusLogLabel`
16. `paginationStateDiagnosticLabel`

The leaf has exactly two type-only import declarations:

- `TimelineAnchorRestoreStatus`, `TimelineDiff`, `TimelineEvent`, `TimelineItem`, `TimelineKey`, `PaginationState` from `../../domain/coreEvents`;
- `getPaginationState` from `../../domain/timelineStore`, used only by the existing `ReturnType<typeof getPaginationState>` annotation.

Explicit leaf exports are exactly nine parent-required names:

- `timelineDiffsContainOwnOutgoingItem`
- `timelineDiffsContainReset`
- `timelineRowsArePurePrepend`
- `timelineRowsArePurePrependForTests`
- `latestEventBackedItemId`
- `emitTimelineEventDiagnosticLog`
- `timelineBackfillCompletionReason`
- `timelineKindDiagnosticLabel`
- `paginationStateDiagnosticLabel`

Seven declarations remain private implementation details.

`TimelineView.tsx` imports the eight runtime names it uses and explicitly re-exports only existing public test path `timelineRowsArePurePrependForTests`. No other compatibility re-export or caller change.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- Every moved declaration body/comment/type/token remains exact apart from `export` modifiers and relative import paths. `TimelineView.tsx` additionally removes the four imports made orphaned by the move: `TimelineDiff`, `TimelineEvent`, `PaginationState`, and `TimelineAnchorRestoreStatus`.
- Diff variant coverage/order, reset classification, outgoing sender/send-state check, pure-prepend predicate, newest Event scan, diagnostic source/message fields, link-preview counters, pagination completion strings and privacy-safe key labels remain exact.
- Existing `TimelineView` test import path remains compatible; no new public production API is exposed from the parent.
- Listener registration/unsubscribe and delayed ensure-subscribe timer remain together in `TimelineView`; no lifecycle owner changes.
- CoreEvent/DTO/wire and Rust-owned state remain untouched; the leaf only reads already projected types.
- No React/DOM/store write/transport/runtime dependency, barrel, wrapper, callback registry, duplicated logic, TODO, test/config/CSS/i18n or behavior change.

## Verification

- AST exactness: 16/16 leaf, parent 0, exports 9/9, private 7/7; parent coreEvents type imports remove exactly the four approved orphaned names.
- Dependency checks: two type-only imports and no runtime import; listener/effects/resource cleanup remain parent-owned.
- Same seven-suite focused command before/after: 173/173; typecheck, lint, diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design round 1: `reviewer-flash` verified the complete seam and recorded `Changes-required` because the exactness wording omitted four mandatory unused-import removals.
- Amendment: explicitly permit only those four parent type-import removals and add them to exactness evidence.
- Design round 2: `reviewer-flash` verified the exact orphan-import set and revalidated all inventory, dependency, ownership and coverage claims, then recorded `Correct-to-implement`.
- Implementation: integrated by `luna-implementer` and parent-audited; the worker reached its bounded turn limit after focused/inventory checks, before reporting typecheck/lint, which the parent then ran green.
- Exactness: 16/16 statements, parent 0, exports 9/9, private 7/7, two type-only imports, four approved parent imports removed; `TimelineView.tsx` 4,869 → 4,664 newline-delimited lines and the leaf is 221.
- Focused post-move 173/173; typecheck, lint and diff checks green; listener/unsubscribe/fallback timer and all state/resource owners remain in the parent.
- Full diff and delivery pending.
