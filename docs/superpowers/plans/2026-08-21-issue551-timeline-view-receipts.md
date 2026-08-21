# Issue #551: TimelineView receipt surface seam

## Status

- Design: `reviewer-flash` verified the closed 13-declaration dependency set and recorded `Correct-to-implement`; two non-blocking baseline/import wording notes are incorporated below.
- Implementation: integrated; 13/13 declaration exactness, 3/3 exports, focused tests, typecheck, lint and diff checks are green.
- Full-diff review: `reviewer-flash` independently compared all 13 statements and recorded `Correct-to-merge`; no code finding remains.
- Delivery: final repository gates, PR CI and merge pending.

## Objective

Move the read-receipt avatar stack, floating reader popup, sizing policy and receipt label/avatar/timestamp helpers from `TimelineView.tsx` into one direct presentation leaf. Preserve all DOM, hover/focus behavior, placement boundary, labels, timestamps, overflow, avatars, CSS, i18n and Rust-owned receipt ordering exactly.

This is the third independently mergeable TimelineView Wave 4 slice. React continues to render the Rust-projected `LiveReadReceipt[]` order/cap/overflow and owns only popup visibility/placement for the mounted row.

## Immutable baseline

- Commit: `0017c1ad8f925c86ba6a89a0d2ff42129be86fb0`
- Source: `apps/desktop/src/components/TimelineView.tsx`
- Size: 7,590 newline count / 7,591 editor positions including EOF; 273,503 bytes
- SHA-256: `680acb2081abb5570c17bbc77079cc4ff4aaae722331b194f0b498a909954675`
- Focused baseline: `TimelineView.live-state.test.tsx` + `App.test.tsx`, 103/103 green

## Exact ownership (13/13)

Move complete TypeScript AST statements in immutable order:

1. `RECEIPT_POPUP_INLINE_SIZE_PX`
2. `RECEIPT_POPUP_ROW_BLOCK_SIZE_PX`
3. `RECEIPT_POPUP_ROW_GAP_PX`
4. `RECEIPT_POPUP_PADDING_BLOCK_PX`
5. `RECEIPT_POPUP_BORDER_BLOCK_PX`
6. `receiptPopupBlockSize`
7. `ReceiptReaders`
8. `receiptPopupBoundaryElement`
9. `formatReceiptDetails`
10. `receiptDisplayName`
11. `receiptInitials`
12. `receiptAvatarSource`
13. `formatReceiptTimestamp`

Line ranges are navigation hints only: popup declarations are around 6300–6401 and formatting declarations around 6657–6687 plus 6739–6747. Extraction is statement-keyed, never range-sliced.

## Target and exports

Create `apps/desktop/src/components/timeline/ReceiptReaders.tsx`; no index/barrel/default export.

Leaf exports exactly:

- `ReceiptReaders` — parent-only component import;
- `formatReceiptDetails` — parent-only helper import;
- existing public `receiptDisplayName` — compatibility surface.

The other ten declarations remain private.

`TimelineView.tsx` locally imports all three exports and flat-re-exports only `receiptDisplayName`, preserving its existing path. No external caller currently imports it, but move-only work does not narrow visibility.

Leaf imports only existing dependencies:

- React `useRef`;
- `FloatingLayer`, `floatingPlacementStyle`, `useFloatingPlacement`, `useHoverFocusPopup` from `../floatingLayer`;
- `t` and `getActiveLocale` from `../../i18n/messages`;
- `peopleFacingLabel` from `../../app/uiShared`;
- `mediaSourceUrl` from `../../domain/mediaUrl`;
- `LiveReadReceipt` from `../../domain/types`.

There is no leaf→parent import or cycle. Parent removes the complete four-name floating-layer import block (`FloatingLayer`, `floatingPlacementStyle`, `useFloatingPlacement`, `useHoverFocusPopup`), which has no retained use. `peopleFacingLabel`, `mediaSourceUrl`, `getActiveLocale`, `t`, `useRef` and `LiveReadReceipt` remain because retained parent code still uses them.

## Behavior and ownership invariants

Preserve declaration bodies/comments/literals/JSX byte-equivalently apart from approved exports and path rewrites:

- popup width/row/gap/padding/border arithmetic and one-row minimum;
- hover and focus share the same popup owner;
- floating placement remains above/end and bounded by thread pane then main pane;
- avatar key, ready-thumbnail URL, initials fallback, `dir="auto"`, tooltip role and detail keys/order;
- Rust-projected receipt order, overflow count and display/original label precedence are unchanged;
- timestamp remains active-locale medium date + short time;
- same catalog keys, classes, tab index, title, ARIA label/hidden state and image alt;
- no profile join, sorting, deduplication, cap, Matrix semantics, transport, retry or product state moves into React;
- the same mounted `ReceiptReaders` component owns popup hook state and cleanup; no timer/listener/resource owner is added.

## Mechanical implementation

One Luna/low worker may edit only `TimelineView.tsx` and create `timeline/ReceiptReaders.tsx`. It moves the 13 statements, adds minimum direct imports/re-export, and removes only newly unused parent imports. No test/CSS/i18n/domain/config/dependency/other-component edit, wrapper, callback adaptation, generalized hook, context, alias or behavior fix.

## Exactness

Temporary TypeScript verification proves 13/13 declaration bodies match immutable source after export/path normalization, parent retains zero, leaf exports exactly three approved names, parent imports three/re-exports one, no other file imports the leaf, and no barrel/default/hook abstraction/duplicate/TODO/dependency/test change exists.

## Integrated implementation evidence

- `TimelineView.tsx`: 7,590 → 7,446 lines; receipt surface moved to a 156-line direct leaf.
- TypeScript AST exactness: 13/13 declarations moved once; parent retains zero; leaf exports exactly the approved three names.
- Parent imports all three and flat-re-exports only existing public `receiptDisplayName`; no reverse import/cycle.
- Focused baseline/post 103/103; typecheck, lint and diff check green. No test/CSS/i18n/domain/transport/receipt semantics/resource behavior changed.

## Verification

```bash
npm --prefix apps/desktop test -- --run \
  src/components/TimelineView.live-state.test.tsx src/App.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
git diff --check
```

After full-diff `Correct-to-merge`, run all Issue #551 repository boundary/security/wire/docs/SDK/Rust gates before PR/CI/merge.
