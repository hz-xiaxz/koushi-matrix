# Issue #551: TimelineView row presentation seam

## Status

- Design: `reviewer-flash` found and verified the runtime-default dependency/import/order amendments, then recorded `Correct-to-implement`.
- Implementation: integrated; 21/21 declaration exactness, 6/6 exports, focused tests, typecheck, lint and diff checks are green.
- Full-diff review: pending.
- Delivery: pending.

## Objective

Move the complete Timeline row presentation owner—row action contract, thread placeholder, item row, reaction/menu/edit/spoiler/link-preview/avatar/thread/recovery helpers and row-local popup/listener state—into one direct leaf. Preserve all DOM, callbacks, Rust-projected DTO interpretation, i18n, CSS, accessibility, focus/menu behavior and action ordering.

Parent TimelineView retains store/event projection, item mapping, media-upload lookup, transport callback construction, alias dialog state, viewport/scrollback and every product/resource owner outside one mounted row.

## Immutable baseline

- Commit: `4ecda213b08349e4b724a53dd30ac0472e289e09`
- Source: `apps/desktop/src/components/TimelineView.tsx`
- Size: 6,721 lines; 243,604 bytes
- SHA-256: `45bb35e4f926eb025de64460bcc4de569a64437c315efaaf6eb1e91f8d232590`
- Focused baseline: App + rendering/interactions/threads/live-state/media, 200/200 green

## Exact ownership (21/21)

Move complete TypeScript AST statements in immutable order:

1. `TimelineThreadAttention`
2. `TimelineRowActionHandlers`
3. `reactionPickerBoundaryElement`
4. `ignoreSendQueueAction`
5. `LazyEmojiPicker`
6. `TimelineAliasTarget`
7. `ThreadRootProjectionPlaceholder`
8. `TimelineItemRow`
9. `aliasTargetIsActive`
10. `formatReactionTooltip`
11. `syntheticDateDividerTimestampMs`
12. `formatDateDividerLabel`
13. `thumbnailSourceUrl`
14. `replyQuoteBody`
15. `localizedTimelineItemBody`
16. `senderInitials`
17. `formatThreadSummary`
18. `recoveryStageText`
19. `recoveryGuidanceText`
20. `keyRequestStateText`
21. `withheldCodeText`

The declarations are intentionally non-contiguous; adjacent viewport/store/diagnostic/media-upload/message-source declarations remain parent-owned. Extraction is AST-keyed and preserves immutable relative order.

## Target and exports

Create `apps/desktop/src/components/timeline/TimelineItemRow.tsx`; no index/barrel/default export.

Leaf exports exactly:

- existing public `TimelineThreadAttention`;
- existing public `TimelineRowActionHandlers`;
- parent-only `TimelineAliasTarget`;
- parent-only `ThreadRootProjectionPlaceholder`;
- existing public `TimelineItemRow`;
- parent-only `aliasTargetIsActive`.

All other declarations remain private. Parent imports all six and explicitly flat-re-exports the three existing public names. Existing callers (`panes.tsx`, `mediaLists.tsx`, `rightPanel.tsx`, `App.test.tsx`, `TimelineMessageBody.tsx`) retain `./TimelineView` paths.

The sole reverse parent edge is type-only `TimelineTransport`, used for the existing save-file callback. It is erased at runtime. All other types are imported from their direct domain/leaf owners, so no runtime cycle exists.

## Direct dependencies

The leaf imports only existing values/types used by moved statements:

- React row hooks/types (`Suspense`, `lazy`, `FormEvent`, `MouseEvent`, `useCallback`, `useEffect`, `useRef`, `useState`);
- row icons (`Copy`, `Edit3`, `FileCode2`, `Forward`, `KeyRound`, `MessageCircle`, `MoreHorizontal`, `Pin`, `PinOff`, `RefreshCw`, `Reply`, `SmilePlus`, `Trash2`, `XCircle`);
- `getActiveLocale`, `t`, plus `peopleFacingLabel`, `MentionCandidate`, and the existing shared `ignoreComposerKeyAction` from `../../app/uiShared`; the parent retains its existing local no-op for its separate main-composer default;
- `contextMenuItems`/`ContextMenuItem`, `openExternalHttpUrl`/`toExternalHttpUrl`, `mediaSourceUrl`;
- core/domain values/types `timelineItemDomId`, `AvatarThumbnailState`, `MediaTransferProgress`, `ReactionSender`, `TimelineItem`, `ComposerDocument`, `LiveReadReceipt`, `PresenceKind`, `ResolveComposerKeyAction`, `ThreadOpenIntent`, `TimelineMediaDownloadState`, `UserProfile`, `TimelineForwardDestination`, `TimelineDisplayRow`;
- `documentFromText`, `plainBodyFromDocument`, `trimDocument`;
- `Composer`, `ImeSafeForm`, `Tooltip`, `onMenuKeyDown`, `useRecoverableImageSource`;
- direct leaves: message-body renderers/type, `MessageMeta`/timestamp, `ReceiptReaders`/details, `TimelineMediaAttachment`/viewer-item;
- type-only `TimelineTransport` from the parent.

The worker removes parent imports only after retained-use proof. The parent retains its local `ignoreComposerKeyAction` for the main TimelineView default; `ignoreSendQueueAction` moves because only the row consumes it. `mediaUploadProgressForItem`, Timeline store access, `MessageSourceDialog`, diagnostics, alias dialog state, transport callbacks, `ROOT_EVENT_THREAD_ORDER`, viewport and projection remain parent-owned.

Path rewrites are fixed: dynamic `import("./EmojiPicker")` becomes `import("../EmojiPicker")`, the four inline `import("../i18n/messages").MessageId` references become `../../i18n/messages`, sibling component imports use `../...`, and domain/app imports use `../../...`.

## Ownership/lifecycle invariants

- Row-local edit document, reaction picker, action/forward menus, spoiler reveal set, avatar recovery hook, refs and requested-preview set move with `TimelineItemRow`.
- Row-local pending-preview effect, action-menu focus effect and document pointerdown listener/cleanup move intact.
- No row timer, observer, frame, subscription or product task is added.
- Parent continues to decide item/window mapping, current room/thread facts, media progress lookup and action callback construction from Rust-owned state.
- Message/reaction/reply/edit/redact/pin/forward/key-request/media legality remains Rust-projected or parent typed callbacks; the row does not infer or repair product state.
- Alias dialog target is moved as a type/value contract only; parent retains dialog state and submit/cancel behavior.
- Every class, key, data/ARIA attribute, role, menu order, keyboard path, i18n key, visible text, link handling, avatar fallback, thread summary, recovery label and callback argument order remains byte-equivalent.

## Mechanical implementation

One Luna/low worker edits only `TimelineView.tsx` and creates `timeline/TimelineItemRow.tsx`. It moves all 21 AST statements, adds minimum imports/exports/re-exports and removes only proven-unused parent imports. Tests, CSS, i18n, domain, config, dependencies and other leaves are forbidden. No wrapper, prop change, callback adaptation, generalized hook/context/registry/alias, duplicate helper or behavior fix.

## Exactness

Temporary TypeScript verification proves 21/21 bodies match immutable source after export/path normalization, parent zero, leaf exports exactly six approved names, parent imports six/re-exports three, only one erased reverse type edge, no outside direct leaf importer, and no test/CSS/domain/dependency/barrel/default/wrapper/duplicate/TODO change.

## Integrated implementation evidence

- `TimelineView.tsx`: 6,721 → 5,198 lines; row presentation moved to a 1,570-line direct leaf.
- TypeScript AST exactness: 21/21 declarations moved once; parent retains zero; leaf exports exactly six approved names.
- Parent imports six and flat-re-exports the three existing public names; reverse parent edge is only erased `TimelineTransport` type.
- Focused baseline/post 200/200; typecheck, lint and diff check green. No test/CSS/i18n/domain/transport/row semantics/resource behavior changed.

## Verification

```bash
npm --prefix apps/desktop test -- --run \
  src/App.test.tsx \
  src/components/TimelineView.rendering.test.tsx \
  src/components/TimelineView.interactions.test.tsx \
  src/components/TimelineView.threads.test.tsx \
  src/components/TimelineView.live-state.test.tsx \
  src/components/TimelineView.media.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
git diff --check
```

After full-diff `Correct-to-merge`, run all Issue #551 repository boundary/security/wire/docs/SDK/Rust gates before PR/CI/merge.
