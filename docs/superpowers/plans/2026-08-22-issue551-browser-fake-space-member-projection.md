# Issue #551 Browser Fake Space Member Projection

## Scope

Move the cohesive pure space-member value/default/ordering family into private leaf `apps/desktop/src/backend/browser-fake/spaceMembers.ts`. Mechanical ownership decomposition only; no lifecycle/API/DTO/class/fixture/test/behavior change.

Immutable baseline: main `4086052934001e3ac5f971ee4225129e20837bc4`; parent 5,942 lines / 198,357 bytes / SHA-256 `dd3d03390ecf5011f1e811d5415f9966ea8b9bad0092cb395f7357e198d7ec1d`.

## Exact declaration set

Move these four complete top-level function source slices in original order:

1. `compareSpaceMemberEntries`
2. `browserFakeSpaceMemberEntry`
3. `emptyBrowserFakeSpaceMembersState`
4. `createBrowserFakeSpaceMembersState`

Do **not** move adjacent `isCompleteSpaceOrder`: it validates navigation rail reordering over `SpaceSummary`, not member projection. It stays exact in the parent with its one class call. Snapshot constructors and member invite/cancel mutation/request owners also remain parent-owned.

## Leaf boundary

Type-only import exactly `SpaceMemberEntry` and `SpaceMembersState` from `../../domain/types`.

Export only the three parent-used functions: comparator, empty state, and fixture state constructor. Keep `browserFakeSpaceMemberEntry` leaf-private. Parent adds one direct three-name import. Remove no parent type imports: class mutation methods still use both member types. No barrel, wrapper, class, state, fixture collection, request ID, timer, cache, or default export.

## Exactness and references

- AST source slices4/4 exact/order, parent0, exports3/private1.
- Parent occurrence counts after import: comparator4 (import+3), empty state4 (import+3), fixture constructor2 (import+1); private entry0.
- Leaf private entry total4 (declaration+3 calls).
- `isCompleteSpaceOrder` body exact, one declaration+one call.
- BrowserFakeApi member mutation ordering, pending/completed operations, generation fences, options, snapshot constructors, spaces/rooms/profile fixtures and all public surfaces remain unchanged.

## Implementation evidence

- Exact AST slices4/4/order, parent0, exports3/private1; `isCompleteSpaceOrder` exact with one call.
- Parent/leaf occurrences exact; two leaf types, one direct import, no parent type removal; API/class/state/generation/request/resource delta0.
- Parent 5,942→5,869 lines; private leaf79; combined5,948.
- Browser fake114 + client25, typecheck/lint/diff and deterministic verifier green; post-implementation review/full matrix pending.

## Verification

Use TypeScript AST statement ranges against immutable `40860529`; verify exact bodies/order, parent0, surfaces/counts, retained ordering helper, API/class/resource inventory and clean holes. Baseline/post browser fake114 + client25, especially Space member audit and invite cancellation/generation cases. Then full frontend/Rust/Tauri/Headless/wasm/policy matrix, full-diff review, latest-main check, CI7/7, merge and #551 evidence.

## Review gate

Pre-implementation review: `reviewer-flash` `Correct-to-implement`.
