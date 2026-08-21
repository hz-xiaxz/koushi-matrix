# Issue #551: TimelineView media presentation seam

## Status

- Design: `reviewer-flash` verified the 14-declaration/resource-owner boundary and recorded `Correct-to-implement`; its import-source/baseline notes are incorporated below.
- Implementation: integrated; 14/14 declaration exactness, 4/4 exports, focused tests, typecheck, lint and diff checks are green.
- Full-diff review: `reviewer-flash` inspected the complete media owner/resource graph and recorded `Correct-to-merge`; no finding remains.
- Delivery: final repository gates, PR CI and merge pending.

## Objective

Move the complete Timeline media attachment/viewer presentation and browser save helpers from `TimelineView.tsx` into one direct leaf. Preserve image/file states, progress, metadata, encrypted badges, details popup, fullscreen viewer, menus, focus trap/restoration contract, keyboard/pointer cleanup, object URL cleanup, transport callback shape, DOM/i18n/CSS/accessibility and Rust-owned media DTO semantics.

Parent TimelineView retains media viewer selection/focus-return state, open/close callbacks, transport action construction and Rust/CoreEvent-driven download state. The leaf renders and invokes supplied callbacks only.

## Immutable baseline

- Commit: `f93cd6a1a28798ba3711c5d1bfef786698f1f85a`
- Source: `apps/desktop/src/components/TimelineView.tsx`
- Size: 7,446 newline count / 7,447 editor positions including EOF; 268,945 bytes
- SHA-256: `d78a92af3933e491774f6840e04e8a1a3ef0fde2a6d6ffebd52c6b1ae60b0a00`
- Focused baseline: `TimelineView.media.test.tsx` + `App.test.tsx`, 103/103 green

## Exact ownership (14/14)

Move complete AST statements in immutable order:

1. `TimelineMediaViewerItem`
2. `TimelineMediaViewerActions`
3. `downloadMediaSource`
4. `saveMediaSource`
5. `TimelineMediaAttachment`
6. `TimelineMediaViewer`
7. `formatBytes`
8. `formatDimensions`
9. `TIMELINE_MEDIA_MAX_INLINE_PX`
10. `TIMELINE_MEDIA_MAX_BLOCK_PX`
11. `TIMELINE_MEDIA_FALLBACK_BOX`
12. `timelineMediaDisplayBox`
13. `timelineMediaDisplayBoxForTests`
14. `uploadProgressPercent`

The first four are around immutable lines 1182–1264; component/helper declarations are around 6680–7325. Ranges are hints only; extraction is statement-keyed.

## Target and exports

Create `apps/desktop/src/components/timeline/TimelineMedia.tsx`; no index/barrel/default export.

Leaf exports exactly:

- `TimelineMediaViewerItem` (type; parent-only);
- `TimelineMediaAttachment` (parent-only);
- `TimelineMediaViewer` (parent-only);
- existing public `timelineMediaDisplayBoxForTests` (compatibility).

All other declarations remain private. `TimelineView.tsx` locally imports the first three and flat-re-exports only `timelineMediaDisplayBoxForTests` so existing tests keep the parent path.

Leaf imports the existing dependencies required by moved bodies:

- React `CSSProperties`, `useCallback`, `useEffect`, `useRef`, `useState`;
- icons `Download`, `FileCode2`, `FileText`, `Forward`, `ImageIcon`, `Info`, `MessageCircle`, `MoreHorizontal`, `RefreshCw`, `Trash2`, `XCircle`;
- `t`, `onMenuKeyDown`, `mediaSourceUrl`;
- `MediaTransferProgress` and `TimelineItem` from `../../domain/coreEvents`, and `TimelineMediaDownloadState` from `../../domain/types`;
- `TimelineForwardDestination` directly from `../../domain/projectionTypes`;
- type-only `TimelineTransport` from `../TimelineView` for the existing `saveMediaFile` callback shape.

The sole reverse edge is erased type-only; there is no runtime cycle. Parent removes only imports proven unused after extraction; action/row imports still used elsewhere remain.

## Ownership and lifecycle invariants

- `TimelineMediaAttachment` retains its details-open state and exact document Escape listener setup/cleanup.
- `TimelineMediaViewer` retains action/forward menu state, focus refs, initial focus, Escape/Tab trap and pointerdown listener setup/cleanup.
- browser save keeps fetch → Blob/object URL → hidden anchor click/remove → deferred URL revoke ordering.
- parent retains `mediaViewerItem`, return-focus ref, open/close callbacks and focus-restoration timer.
- parent retains transport command construction, download request/retry state and CoreEvent projection.
- same media source URL conversion, image dimensions/box, metadata order, progress rounding, labels, badges, menus, actions and callback argument order.
- no Matrix media parsing, encrypted metadata interpretation, bytes, state repair, retry, timer, lifecycle or product semantics move into React.
- no DOM/class/test-id/i18n/ARIA/keyboard/pointer/focus behavior changes.

## Mechanical implementation

One Luna/low worker edits only `TimelineView.tsx` and creates `timeline/TimelineMedia.tsx`. Move all 14 statements verbatim apart from approved exports/path rewrites, add minimum parent import/re-export and remove only newly unused parent imports. No tests/CSS/i18n/domain/config/dependency/other component, wrapper, callback adaptation, generalized hook/context/alias, or behavior fix.

## Exactness

Temporary TypeScript verification proves 14/14 bodies match immutable source, parent zero, leaf exports exactly four approved names, parent imports three/re-exports one, only one type-only reverse edge, no outside leaf importer, and no barrel/default/wrapper/hook abstraction/duplicate/TODO/dependency/test change.

## Integrated implementation evidence

- `TimelineView.tsx`: 7,446 → 6,721 lines; media presentation moved to a 749-line direct leaf.
- TypeScript AST exactness: 14/14 declarations moved once; parent retains zero; leaf exports exactly four approved names.
- Parent imports the two components and viewer-item type and flat-re-exports only the existing test helper; the reverse edge is type-only.
- Focused baseline/post 103/103; typecheck, lint and diff check green. No test/CSS/i18n/domain/transport/media semantics/resource behavior changed.

## Verification

```bash
npm --prefix apps/desktop test -- --run \
  src/components/TimelineView.media.test.tsx src/App.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
git diff --check
```

After full-diff `Correct-to-merge`, run all Issue #551 repository boundary/security/wire/docs/SDK/Rust gates before PR/CI/merge.
