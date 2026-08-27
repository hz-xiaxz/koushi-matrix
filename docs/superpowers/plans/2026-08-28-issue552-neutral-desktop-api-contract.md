# Issue #552 Phase 2A — Neutral DesktopApi Contract

Status: design approved by `reviewer-flash`; implementation complete on the reviewed branch pending final gates, exact-diff review and merge.

Base: `origin/main` `f61a9eef2c102b767997365587459c9ea2941230` after merged #708.

## Objective

Correct the frontend transport dependency direction without changing runtime behavior: the production Tauri adapter and browser fake both implement a neutral `DesktopApi` contract, and the renderer composition root alone selects the implementation.

This is structural isolation, not a semantic migration and not evidence that #552 is complete.

## Current boundary

- `backend/browserFakeApi.ts` declares `DesktopApi` and also implements it. Production `backend/client.ts` imports its contract from the fake.
- `backend/client.ts::createDesktopApi` selects Tauri versus browser and owns a duplicate local `isTauriRuntime` check.
- `backend/appRuntime.ts` calls that factory even though it is the existing application composition root.
- `backend/tauriTimelineTransport.ts` separately guards its timeline adapter. That seam is intentionally unchanged until Phase 2B.
- App-focused tests mock `backend/client::createDesktopApi`, so test composition currently follows the inverted production factory. Their replacement `appRuntime` mock must export both the injected `api` and `startSessionVerificationWindowDrag` used by `SessionVerificationGate`.

## Target boundary

1. Add `backend/desktopApi.ts` containing only the `DesktopApi` interface and the type-only imports required by that contract.
2. `browserFakeApi.ts` imports `DesktopApi`, continues to export `BrowserFakeApiContract` and `createBrowserFakeApi`, and retains all fake behavior unchanged.
3. `client.ts` imports `DesktopApi`, exports `TauriDesktopApi`, and contains only the Tauri implementation. Delete `createDesktopApi` and its local `isTauriRuntime`.
4. `appRuntime.ts` imports `TauriDesktopApi`, `createBrowserFakeApi`, and the existing `tauriTimelineTransport::isTauriRuntime`; it constructs exactly one `api: DesktopApi` at module composition time.
5. Leave `tauriTimelineTransport.ts` and its runtime guard unchanged. Do not move platform ports, rename IPC commands, alter DTOs, split the large interface, or change any command semantics.
6. Update all type imports from `browserFakeApi` to `desktopApi`. App tests inject the API at the composition-root module rather than mocking a production adapter factory. Client tests instantiate `TauriDesktopApi` directly.

## Verify-first proof

Add `backend/appRuntime.test.ts` before production edits. Use `@vitest-environment jsdom`, mock `@tauri-apps/api/window`, and reset modules between cases. With isolated module mocks it proves both branches:

- Tauri runtime constructs only `TauriDesktopApi`;
- browser runtime calls only `createBrowserFakeApi`;
- selection is owned by `appRuntime`, not adapter constructors.

The test is RED on the base because `appRuntime` imports `client::createDesktopApi` and does not consume the mocked implementations/runtime predicate directly. No sleeps, log assertions, or GUI evidence.

Existing client/fake/App tests remain behavior equivalence checks. TypeScript compilation proves every contract import and implementation still matches the same interface.

## Implementation sequence

1. Add and run the RED composition-root test; record the exact failure.
2. Move the interface and type-only imports to `desktopApi.ts`.
3. Export `TauriDesktopApi`; move selection to `appRuntime`; remove the duplicate client runtime branch.
4. Update type imports and App/client tests without changing method bodies or IPC names.
5. Update the frontend ownership inventory and this phase record.
6. Run focused tests, full Vitest, typecheck, lint, build, Playwright, boundary guards, secret scan, manually verify the acyclic dependency graph, and run `git diff --check`.
7. Obtain `reviewer-flash` exact-diff approval before PR/merge.

## Expected files

- `apps/desktop/src/backend/desktopApi.ts` (new)
- `apps/desktop/src/backend/{browserFakeApi,client,appRuntime}.ts`
- `apps/desktop/src/backend/{client,appRuntime}.test.ts`
- App tests importing/mocking the old fake-owned type/factory
- `apps/desktop/src/app/{viewportSyncReporter,viewportSyncReporter.test}.ts`
- `apps/desktop/src/test/tauriIpcMock.ts`
- `apps/desktop/eslint.config.js` (correct the stale adapter guidance text only)
- `docs/architecture/frontend-ownership-inventory.md`
- `docs/architecture/tauri-react-shell.md`
- `docs/superpowers/plans/2026-08-27-issue552-remaining-ownership-phases.md`
- `docs/agents/plans.md`

## Implementation record

The composition test failed on the base because `appRuntime` requested the missing mocked `client::createDesktopApi` export, proving selection still lived in the adapter. It is GREEN after moving the neutral interface and selector: Tauri runtime constructs only `TauriDesktopApi`; browser runtime constructs only `createBrowserFakeApi`. All contract type importers and App injection mocks now target the neutral contract/composition root; client tests instantiate the Tauri adapter directly. No implementation method, IPC name, DTO or timeline transport changed.

## Design review record

- Design, `reviewer-flash`: **Correct-to-merge**. No Critical or Important finding. The four Minor documentation gaps were fixed: the nonexistent cycle-tool claim was replaced by an explicit graph audit, all type importers/stale ESLint guidance are in scope, App mocks retain the drag export, and the RED test environment/mocks are specified.
- Exact implementation diff, `reviewer-flash`: **Correct-to-merge**. The stale pre-Phase-2A inventory bullet was corrected before focused re-review; that review then found one historical shell-doc `createDesktopApi()` reference, also corrected before the final confirmation.

## Acceptance

- One neutral contract module; fake and Tauri adapters depend on it.
- One implementation selector at `appRuntime`; `client.ts` has no browser fallback or runtime predicate.
- Browser Fake remains a contract mirror, never product authority.
- Existing API signatures, IPC names, snapshots, behavior and `tauriTimelineTransport` are unchanged.
- No new dependency, generic port framework, compatibility shim, TODO, or speculative interface split.
- Deterministic RED/GREEN composition tests and full frontend gates pass.
- Inventory records Phase 2A as structural only; #552 remains open.
