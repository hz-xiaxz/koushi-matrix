# Issue #551 Browser Fake Snapshot Defaults

## Scope

Move the remaining cohesive pure domain-default constructor family into private leaf `apps/desktop/src/backend/browser-fake/snapshotDefaults.ts`. Mechanical decomposition only; no snapshot composition, lifecycle, API, DTO, class, state, fixture, test, or behavior change.

Immutable baseline: main `910bb0c4652a083ae0b584e67882921effae707d`; parent 5,869 lines / 196,591 bytes / SHA-256 `f170a5ef48027a5715a4685af25cd9cea1711ba84cd5fec7f45b089d9d2d7374`.

## Exact declaration set

Move these eight contiguous complete function source slices in original order:

1. `defaultDirectoryState`
2. `defaultE2eeTrustState`
3. `defaultDelegatedAuthLinks`
4. `defaultE2eeKeyManagementState`
5. `defaultLiveSignalsState`
6. `defaultNativeAttentionState`
7. `defaultCjkTextPolicyState`
8. `defaultProfileState`

Do not move snapshot factories, settings/display defaults, room-management defaults, member defaults, invite defaults, sidebar composition, or live-signal mutation/projection helpers. Those have separate owners. Parallel private defaults in `src/test/appHarnessMain.tsx` remain untouched; harness ownership/consolidation is its separate #551 candidate.

## Leaf boundary

Type-only import exactly `DesktopSnapshot` from `../../domain/types`.

Export the seven parent-called constructors. Keep `defaultE2eeKeyManagementState` leaf-private because only `defaultE2eeTrustState` calls it. Parent adds one direct seven-name import. Remove no parent type import; `DesktopSnapshot` remains the composition-root contract.

No barrel, wrapper, aggregate default object, class, callback registry, state/cache/timer/fixture, or default export.

## Exactness and references

- AST source slices8/8 exact/order, parent0, exports7/private1.
- Parent occurrence counts after import: directory4, E2EE trust4, delegated auth2, key-management0, live signals4, native attention4, CJK policy3, profile4.
- Leaf key-management total2 (declaration+one call); every other leaf declaration total1.
- `createReadySnapshot`, `createSignedOutSnapshot`, `clearSessionViews`, auth discovery and their call ordering remain exact; all DTO shapes and Rust-owned state semantics unchanged.

## Implementation evidence

- Exact AST slices8/8/order, parent0, exports7/private1; parent/leaf counts exact.
- One leaf type, one direct parent import, no parent type removal; snapshot factories/call order and API/class/resource surfaces unchanged.
- Parent 5,869→5,777 lines; private leaf102; combined5,879.
- Browser fake114 + client25, typecheck/lint/diff and deterministic verifier green.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`.
- Final local matrix: exactness green; browser fake114, client25, Vitest1,400, Playwright248, workspace all-targets, Tauri149/1 ignored plus keyring5, Headless Core QA130, wasm state/search, typecheck/lint/build, Tauri/domain/IPC boundaries, secret/release/version, SDK/docs, rustfmt, `cargo deny`, `cargo machete`, and diff checks green without reruns.

## Verification

Use TypeScript AST statement ranges against immutable `910bb0c4`; verify exact bodies/order, parent0, export/private/import/counts, snapshot-factory and API/class/resource inventories, and clean holes. Baseline/post browser fake114 + client25, including #641 reset/locked/replacement tests. Then full frontend/Rust/Tauri/Headless/wasm/policy matrix, full-diff review, latest-main check, CI7/7, merge and #551 evidence.

## Review gate

Pre-implementation review: `reviewer-flash` `Correct-to-implement`.
