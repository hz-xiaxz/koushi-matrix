# Issue #551 Timeline diagnostics projection extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `55fad7b2391c8bc24112a49f5ad36d40b30084de` (message-source dialog PR #611 merged).
- `TimelineView.tsx`: 4,104 newline-delimited lines / 156,370 bytes / SHA-256 `80fb3d755ae3f4b01d949d3da7b57404ddbe6ce5afb76b5d093a7a5ee45c0912`.
- Focused baseline: `npm --prefix apps/desktop test -- --run src/components/TimelineView.media.test.tsx`, 23/23.

## Ownership decision

Move the complete timeline diagnostics schema and avatar diagnostic projections to direct leaf `apps/desktop/src/components/timeline/TimelineDiagnostics.ts`:

1. `TimelineDiagnostics`
2. `timelineAvatarDiagnostics`
3. `timelineRenderedAvatarDiagnostics`

The leaf owns the externally consumed diagnostics shape, the pure item/profile/thumbnail diagnostic projection, and the DOM-local rendered/broken-avatar count. `TimelineView` retains diagnostics emission/deduplication, downloaded-item bookkeeping, pagination labels, container ownership, avatar download lifecycle, event handling, transport and rendering.

`timelineAvatarMxcsForItems` and `avatarThumbnailLogMessage` remain with the avatar request/event controller pending that separate ownership decision; this prevents a line-count-only helper grab from widening the diagnostics leaf.

## Imports, visibility and compatibility

Leaf imports exactly two type-only statements:

- `AvatarThumbnailState`, `TimelineItem` from `../../domain/coreEvents`;
- `UserProfile` from `../../domain/types`.

Export all three moved declarations for direct parent use. Parent imports all three and explicitly type re-exports `TimelineDiagnostics`, preserving existing imports from `components/TimelineView` in `App.tsx` and pane surfaces. There is no reverse edge to `TimelineView`, barrel, glob or public path break.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- All three bodies/comments/types/field order remain exact apart from export modifiers and relative paths.
- Exact diagnostic fields, avatar selection precedence, thumbnail-state categorization, missing counts, DOM selector `.avatar img`, `complete`/`naturalWidth` broken-image test and null-container result remain unchanged.
- Emission signature/dedupe, callback timing, container ref, download/retry sets and event/request lifecycle remain parent-owned.
- No Matrix/DTO/wire, CSS/i18n/a11y, dependency, test/config, resource, wrapper, duplicate logic or TODO change.
- Existing `TimelineDiagnostics` public import path remains exact; no new public root export is introduced.

## Verification

- AST exactness: 3/3 leaf, parent 0, exports 3, imports 2/2; parent one type re-export.
- Existing external type users remain exactly `App.tsx` once and `panes.tsx` twice; parent callback/ref annotations remain local users.
- Same focused command before/after 23/23.
- Typecheck, lint and diff; then full matrix after diff approval.

## Review gate

- Design: `reviewer-flash` traced the complete declarations, dependency/caller graph and focused assertions and recorded `Correct-to-implement`; its two minor reproducibility notes are folded into the immutable hash/bytes and explicit external-user count above.
- Implementation: integrated by `luna-implementer` and parent-audited.
- Exactness: 3/3 statements, parent 0, exports 3/3, type-only imports 2/2, retained controller helpers 2/2 and one parent type re-export; `TimelineView.tsx` 4,104 → 4,048 newline-delimited lines and the leaf is 64.
- Focused post-move 23/23; typecheck, lint and diff checks green; emission/dedupe/container/download/event owners remain parent-owned.
- Full diff: `reviewer-flash` independently traced every moved body/type/caller and retained owner and recorded `Correct-to-merge`; parent `wc -l` confirms 4,048 newline-delimited lines.
- Delivery: final repository gates, PR CI and merge pending.
