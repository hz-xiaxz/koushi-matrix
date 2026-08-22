# Issue #551 App UI-latency hook extraction

Status: design review pending. Scope is the final clean App lifecycle seam found by residual review.

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

Add one bounded jsdom test `app/useUiLatencyDiagnostics.test.ts` using `renderHook`:

- stub `requestAnimationFrame` and `cancelAnimationFrame`;
- assert the hook schedules the first frame;
- unmount and assert the owned frame is cancelled;
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
- Formal `reviewer-flash` design verdict pending.
