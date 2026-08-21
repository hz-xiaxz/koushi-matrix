# Issue #551 TimelineView anchor/session ownership extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `1933c779a477d648a1c9eb291898a37f80341c63` (merged PR #601).
- `apps/desktop/src/components/TimelineView.tsx`: 4,664 newline-delimited lines.
- Focused immutable baseline: all seven `TimelineView.*.test.tsx` suites, 173/173.

## Ownership decision

Move TimelineView's complete DOM anchor primitives and JavaScript-session room-anchor memory to direct private leaf `apps/desktop/src/components/timeline/TimelineViewportAnchors.ts`.

The leaf becomes the single owner of the process-local `timelineViewportSessionMemory` map plus the exact capture/restore/query/signature operations that read or write its anchor values. It also owns generic item-anchor DOM geometry used by prepend/projection correction. `TimelineView` remains the coordinator: every React ref/state/effect, key-change/unmount path, persistence transport call, projection transaction, timer/frame/observer and cleanup site remains there.

No accessor wrapper is introduced around the map: the existing binding and operations move intact, and the parent imports only what it already uses.

## Exact inventory

Move exactly 26 top-level AST statements in this order:

1. `ScrollAnchor`
2. `ScrollAnchorCaptureOptions`
3. `TimelineEventIdentity`
4. `captureAnchor`
5. `captureFreeScrollAnchor`
6. `restoreAnchorWithDelta`
7. `restoreAnchor`
8. `PendingHeightModelCommit`
9. `CapturedTimelineScrollAnchor`
10. `TimelineViewportSessionMemory`
11. `TimelineSessionAnchorAgeBucket`
12. `timelineSessionAnchorAgeBucket`
13. `timelineViewportSessionMemory`
14. `clearTimelineViewportSessionMemoryForTests`
15. `setTimelineViewportSessionAnchorForTests`
16. `captureRoomScrollAnchor`
17. `restoreRoomScrollAnchor`
18. `currentRoomScrollAnchorOffset`
19. `findRoomScrollAnchorNode`
20. `roomScrollAnchorSignature`
21. `roomScrollAnchorStableSignature`
22. `canonicalTimelineContainsActivityEventId`
23. `timelineEventIdentityAttribute`
24. `eventIdForTimelineIdentity`
25. `findTimelineEventNode`
26. `cssEscape`

Leaf imports:

- type-only `TimelineItem`, `TimelineKey` from `../../domain/coreEvents`;
- type-only `TimelineScrollAnchor` from `../../domain/types`;
- runtime `timelineStoreKeyId` from `../../domain/timelineStore`, used only by the existing test seeding operation.

Explicit leaf exports are exactly 18 names needed by the parent or existing parent façade:

- types: `ScrollAnchor`, `PendingHeightModelCommit`, `TimelineSessionAnchorAgeBucket`;
- state/value: `timelineViewportSessionMemory`;
- generic anchors: `captureAnchor`, `captureFreeScrollAnchor`, `restoreAnchorWithDelta`, `restoreAnchor`;
- session operations: `timelineSessionAnchorAgeBucket`, `clearTimelineViewportSessionMemoryForTests`, `setTimelineViewportSessionAnchorForTests`, `captureRoomScrollAnchor`, `restoreRoomScrollAnchor`, `roomScrollAnchorSignature`, `roomScrollAnchorStableSignature`, `canonicalTimelineContainsActivityEventId`, `eventIdForTimelineIdentity`, `findTimelineEventNode`.

Eight declarations remain private implementation details.

`TimelineView.tsx` imports the 16 names it directly uses and explicitly re-exports only the two existing test APIs. No other caller/import path changes.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- All 26 bodies/comments/types/tokens and declaration-relative behavior remain exact apart from export modifiers, relative imports and pruning newly orphaned parent imports.
- Session memory remains module-process-local, keyed by canonical `timelineStoreKeyId`, reset only by its existing test helper or process restart.
- First room entry/live-edge, in-session anchor restoration, age buckets 30s/5m, top/bottom offsets, last-visible capture order, rounded offsets and stable/full signatures remain exact.
- Generic free-scroll anchoring still rejects relocated thread-root projections and uses escaped item/event selectors with unchanged geometry math.
- Existing parent test paths for `clearTimelineViewportSessionMemoryForTests` and `setTimelineViewportSessionAnchorForTests` remain compatible; no other parent API is added.
- Parent retains all React/resource/persistence/projection ownership; leaf has no React, hook, listener, timer, frame, observer, transport call or store mutation except its own session `Map`.
- No Matrix semantics, DTO/wire, CSS/i18n/a11y, test/config/dependency, barrel, wrapper, callback registry, duplicate logic or TODO change.

## Verification

- AST exactness: 26/26 leaf, parent 0, exports 18/18, private 8/8.
- Imports and parent-use audit; no React/transport/resource API in leaf; map binding occurs once.
- Same focused seven-suite command before/after: 173/173; typecheck, lint, diff check.
- After full-diff approval: complete frontend/Rust/policy matrix and CI.

## Review gate

- Design: `reviewer-flash` verified all 26 declarations, 18/8 visibility split, map/test paths, DOM geometry, dependencies and parent lifecycle owners and recorded `Correct-to-implement`.
- Implementation: integrated by `luna-implementer` and parent-audited; the worker reached its bounded turn limit after typecheck/focused/lint, before final exactness reporting.
- Exactness: 26/26 statements, parent 0, exports 18/18, private 8/8, three imports, one map binding; after the banner delta `TimelineView.tsx` is 4,441 newline-delimited lines and the leaf is 248.
- Focused post-move 173/173; typecheck, lint and diff checks green; parent retains every React/resource/persistence/projection owner.
- Full-diff round 1: `reviewer-flash` verified the complete move and recorded `Correct-to-merge`; its only minor finding was the now-orphaned three-line `Scroll anchor` banner in the parent.
- Banner delta: move that exact banner to the anchor owner leaf.
- Final delta review: `reviewer-flash` revalidated the final tree and recorded `Correct-to-merge`; no blocker remains.
- Final local evidence: focused 173/173; Vitest 1,367; Playwright 248 with polling; workspace all-targets, desktop 149/1 ignored and Headless Core QA 129; typecheck/lint/build/wasm, Tauri/domain boundaries, tracked-secret/release/IPC wire, SDK/docs, deny/machete/rustfmt/exactness/diff checks all green.
- Delivery: PR CI and merge pending.
