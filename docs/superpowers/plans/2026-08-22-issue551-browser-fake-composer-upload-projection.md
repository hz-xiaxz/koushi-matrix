# Issue #551 Browser Fake Composer / Upload Projection

## Scope

Move one already-audited pure value/projection family from `apps/desktop/src/backend/browserFakeApi.ts` into private leaf `apps/desktop/src/backend/browser-fake/composerUploads.ts`. This is mechanical ownership decomposition after #641; no lifecycle, state, API, DTO, fixture, test, or behavior change.

Immutable baseline: main `b3d0c2a2b33539a49166215fee45888196277821`; parent 6,323 lines / 210,720 bytes / SHA-256 `945c9f0520b37e1224dd5777c20fa69ab4fc363924ca50fec8ab188234bea397`.

## Exact declaration set

Move these nine complete top-level function source slices in original order, including comments and formatting:

1. `browserComposerTargetIsActive`
2. `browserComposerForTarget`
3. `browserComposerDraftTargetKey`
4. `browserStagedUploadsForTarget`
5. `setBrowserStagedUploadsForTarget`
6. `browserPreparedUploadKey`
7. `browserPreparedUploadItem`
8. `browserSyntheticVariant`
9. `browserImageFormat`

`browserComposerAccountMatches` remains in the parent: it consumes parent-public `ComposerDraftAccountOwner`; moving it would create a private-leaf→composition-root type dependency. Preserve its exact source and position while removing the surrounding moved slices.

## Leaf boundary

Type-only import exactly `ComposerState`, `ComposerTarget`, `DesktopSnapshot`, `PreparedUploadVariant`, `StageUploadBytesRequestItem`, and `StagedUploadItem` from `../../domain/types`.

Export only the seven parent-called declarations. Keep `browserSyntheticVariant` and `browserImageFormat` leaf-private. No barrel, default export, wrapper, callback bag, class, state, cache, timer, fixture, or new abstraction.

The parent adds one direct seven-name import. Remove only parent type imports proven unused after the move: `PreparedUploadVariant` and `StagedUploadItem`. `ComposerState` and `StageUploadBytesRequestItem` remain parent-used.

## Exactness and call graph

- source slices9/9 exact after stripping only `export` on the seven exported functions;
- parent declarations0 for the moved set and exactly one unchanged `browserComposerAccountMatches`;
- exports7, private2, no duplicate symbol;
- parent call occurrences remain: target-active5, composer-for-target3, target-key8, staged-getter3, staged-setter5, prepared-key2, prepared-item1 (27 total);
- leaf-internal calls remain target-active3, synthetic-variant5, image-format1;
- DesktopApi/BrowserFakeApi methods/fields/maps/timers/request IDs, prepared-byte ownership/bounds/teardown, composer lease/draft/submission lifecycle and snapshot mutation order remain unchanged.

## Implementation evidence

- Exact AST source slices9/9, parent0, original order, exports7/private2; `browserComposerAccountMatches` body/position and six calls unchanged.
- Parent call occurrences27 and leaf-internal calls9; only the approved direct import and two type removals.
- Parent 6,323→6,172 lines; private leaf166; combined6,338. API/class/resource delta0.
- Browser fake114 + client25, typecheck/lint/diff and deterministic verifier green.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`; full matrix pending.

## Verification

Use TypeScript AST statement ranges against the immutable baseline, not line-number slicing. Run body/token exactness, declaration order/count, parent0, export/private surface, import/type removals, 27 parent references, internal calls, API/class/resource inventory and `git diff --check`.

Focused baseline/post-move: browser fake114 and client25, especially composer lease/draft, prepared-upload lifecycle, cross-target staging and batch bounds. Then full frontend/Rust/Tauri/Headless/wasm/policy matrix, post-implementation full-diff review, latest-main comparison, CI7/7, merge and #551 evidence.

## Review gate

Pre-implementation review: `reviewer-flash` `Correct-to-implement`.
