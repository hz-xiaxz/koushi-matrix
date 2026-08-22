# Issue #551 App UI-latency hook extraction

Status: full diff approved; delivery pending. Scope is the final clean App lifecycle seam found by residual review.

## Baseline

- Base: `4f5faa2534883ef59599fc614521507234577b78` after desktop-attention PR #630.
- `App.tsx`: 6,242 newline-delimited lines / 217,366 bytes / SHA-256 `7dacfb2ee99d076b94a6469ef75cb2583794fc427f761bca5e4339e3dabe185e`.
- Focused baseline: App78; domain UI-latency2.

## Ownership decision

Create private direct hook module `apps/desktop/src/app/useUiLatencyDiagnostics.ts`. Move exactly `useUiLatencyDiagnostics` unchanged and export it. Import it directly at the existing App call site.

Destination imports exactly React `useEffect`/`useState` and `createUiLatencySampler`, `EMPTY_UI_LATENCY_DIAGNOSTICS`, `UiLatencyDiagnostics` from the existing domain owner. Remove only those three domain import members from App; React hooks remain used elsewhere.

The hook owns one local diagnostics state, recursive RAF sampling, once-per-second publication, cancellation fence and `cancelAnimationFrame` cleanup. The hook call remains at the same unconditional App position, so executed hook order is unchanged. It takes no arguments and returns the same diagnostics DTO.

No Rust/product state, listener, timer, report composition, diagnostics log, QA title, public API, dependency or render changes.

## Test

Add one bounded test `app/useUiLatencyDiagnostics.test.ts` with `// @vitest-environment jsdom` using `renderHook`:

- stub `requestAnimationFrame` to return a pinned frame id and stub `cancelAnimationFrame`;
- assert the hook schedules the first frame;
- unmount and assert `cancelAnimationFrame` receives that exact owned frame id;
- restore globals/cleanup.

Domain sampler tests2 remain unchanged and continue covering sampling, long-frame counting, rounding and invalid gaps.

## Deterministic exactness

- hook declaration1/1, App parent0;
- body/type exact modulo export;
- App call position and consumer exact;
- destination imports2, App orphan import members3;
- retained App declarations/hook order/resources/render/public exports exact;
- no duplicate RAF/state owner, reverse edge or dependency delta.

## Verification

Run App78 + latency domain2 + hook1, typecheck/lint, full Vitest/Playwright with polling, build/boundary/security/exactness/diff and full repository gates. After full-diff approval, integrate latest main, PR CI7/7 and merge; then rerun the final App residual audit.

## Review gate

- Formal App residual review identified this self-contained no-argument hook as the sole missing clean seam.
- `reviewer-flash` independently traced hook closure/imports, call order, RAF/state/publication/cancel ownership and bounded test feasibility and recorded `Correct-to-implement`.
- `App78` is evidence shorthand for the focused App suite, not a QA lane name.
- Implementation integrated by `luna-implementer` and parent-audited.
- Exactness: hook1/1, App parent0, imports2, call1 at the same position, orphan import members3; body/type and retained App structure exact.
- `App.tsx` 6,242 → 6,183 newline-delimited lines; hook61; one pinned-RAF cleanup test.
- Focused App78 + domain2 + hook1 =81, typecheck/lint, full frontend/build and diff checks green.
- Full diff recorded `Correct-to-merge` with one future-hardening note; the exact hook path was added to the existing direct-Tauri restricted-import file list so new platform imports cannot bypass the boundary.
- Delta review recorded `Correct-to-merge-after-finding-fix`.
- Final local evidence: App78 + domain2 + hook1, Vitest1,372, Playwright248 with polling, workspace all-targets, desktop149/1 ignored and Headless Core QA130; typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/exactness/diff gates green.
- Delivery: latest-main integration if required, PR CI and merge pending.
