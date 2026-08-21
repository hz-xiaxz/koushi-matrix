# Issue #551 App QA diagnostics projection extraction

Status: design review pending. Scope is one behavior-preserving stateless diagnostics seam.

## Baseline

- Base: `7f8ffb3903df385f3c5c879275a8cf841fab2f74` after App transport PR #626.
- `App.tsx`: 7,065 newline-delimited lines / 251,718 bytes / SHA-256 `667273f6fcb379fc33b17e2e92df89838a1c007cc127c75c6ae5e3b92c0f88e0`.
- Focused baseline: `App.diagnostics.test.tsx` 14/14; desktop typecheck green.

## Ownership decision and immutable order

Create private direct module `apps/desktop/src/app/qaDiagnostics.ts`. Move exactly these six declarations in original relative order:

1. `INITIAL_TIMELINE_DIAGNOSTICS`
2. `qaRenderedDomDiagnostics`
3. `qaSecurityDiagnostics`
4. `imageSrcScheme`
5. `timelineDiagnosticsEqual`
6. `timelineDiagnosticsLogMessage`

Export exactly five declarations; keep `imageSrcScheme` private. Import the five exports directly in `App.tsx`; no barrel or App re-export.

The leaf owns stateless QA timeline defaults/comparison/log projection and synchronous DOM/security observation. The DOM readers own no listener, observer, animation frame, timer or mutable lifecycle resource.

## Behavior and privacy contract

Preserve all bodies and order exactly:

- DOM screen precedence: boot error → auth → recovery → timeline → unknown → empty;
- root child count and body text length fallback;
- secure-context, protocol and origin observation;
- avatar selector set, URL scheme counting and broken-image predicate;
- invalid URL fallback token;
- all ten timeline diagnostic equality fields;
- exact private-data-free timeline log tokens/order.

Do not consolidate `QaTimelineDiagnostics` with the structurally similar timeline-component type. No new product state, raw private identifiers, logging gate, environment variable or generalized diagnostics hook.

## Imports and App residual

Destination has exactly two type import paths:

- `SecurityDiagnostics` from `../domain/diagnostics`;
- `QaDomDiagnostics`, `QaTimelineDiagnostics` from `../domain/qaTitle`.

Remove only `SecurityDiagnostics` from App imports. Retain `QaDomDiagnostics` and `QaTimelineDiagnostics` because App state/title/report composition still uses them.

Keep in App:

- QA error/unhandled-rejection listeners;
- `useUiLatencyDiagnostics` and RAF cleanup;
- timeline diagnostic state/ref/update/reset ordering;
- request-generation fencing and prior-success retention;
- report composition, clipboard write and dialog state;
- QA title/token composition.

React remains the lifecycle owner. Rust-owned state/DTO/command/event semantics remain unchanged.

## Tests and source contract

Add one focused jsdom test `apps/desktop/src/app/qaDiagnostics.test.ts` that checks in one bounded case:

- exact initial object;
- DOM precedence/count output;
- avatar scheme/broken-image output;
- all-field equality sensitivity;
- exact log string.

Update only the `diagnostics runtime source contract` in `App.diagnostics.test.tsx`:

- add `./app/qaDiagnostics.ts` to the scanned owner list;
- replace source-string concatenation with per-owner assertions so one owner cannot hide another owner's forbidden gate;
- preserve both forbidden-token assertions for every owner.

No lifecycle test is moved or weakened.

## Deterministic exactness

A temporary TypeScript AST verifier compares immutable base with parent + leaf:

- declarations6/6 in relative order, parent0;
- bodies/types/comments exact modulo export modifiers;
- exports5/private1, destination import paths2, App direct import5;
- App orphan import1 and no other import deletion;
- retained App top-level declarations/hooks/listeners/timers/render/public exports exact;
- source-contract forbidden tokens and owner set exact; no concatenated source;
- public API/dependencies/reverse edges/product-state deltas0;
- duplicate/missing/excess declarations0.

## Verification

Run diagnostics tests14 + new leaf test1, App tests80, typecheck/lint, full Vitest/Playwright with polling, build, source/boundary/security checks and diff/format checks. After full-diff approval, integrate latest `origin/main` if required, run the complete repository matrix and PR CI7/7.

The App umbrella remains open for verification/destructive UI, composer/attention re-evaluation and final residual audit.

## Review gate

- Read-only diagnostics reconnaissance separated stateless collectors/projections from React-owned RAF/listener/state/async fencing.
- Formal `reviewer-flash` verdict pending; implementation prohibited until `Correct-to-implement`.
