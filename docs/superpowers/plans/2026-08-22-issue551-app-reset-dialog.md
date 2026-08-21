# Issue #551 App destructive confirmation dialog extraction

Status: design approved. Scope is one move-only presentation seam and prerequisite for verification-gate re-evaluation.

## Baseline

- Base: `37edc648ed725150df2556092cfa4b32a338e79a` after App diagnostics PR #627.
- `App.tsx`: 6,975 newline-delimited lines / 248,613 bytes / SHA-256 `fec7a06e1f019940f927b87ae811745494eb31a2fddd4f1df43d1756dd218df8`.
- Focused baseline: App80, dialogs13.

## Ownership decision

Move exactly `ResetLocalDataConfirmationDialog` from `App.tsx` to existing owner module `components/dialogs.tsx`. Export it directly from that module and add it to App's existing direct dialogs import. Do not create a wrapper module, barrel, generic dialog abstraction or new prop type.

Preserve App's established public import compatibility with one explicit flat re-export:

```ts
export { ResetLocalDataConfirmationDialog } from "./components/dialogs";
```

The component declaration, inline prop type, defaults, markup, classes, ARIA attributes, Escape handling, disabled behavior, callbacks and i18n calls remain exact.

## Call sites and ownership

Keep all four App call sites unchanged:

1. SessionVerificationGate device cleanup confirmation;
2. SessionVerificationGate erase-local-anyway confirmation;
3. Settings local-data reset;
4. room-leave confirmation.

App retains every boolean/state/handler, Rust snapshot interpretation, busy/error behavior, API call and resolved caller-specific title/copy/confirm label. The component is generic presentation despite its compatibility name.

No CSS/i18n/domain/backend changes. Shared global dialog classes and default message IDs remain unchanged.

## Test ownership

Move the focused destructive-confirmation rendering test from `App.test.tsx` to `components/dialogs.test.tsx`:

- add `ResetLocalDataConfirmationDialog` to the direct dialogs import;
- remove the obsolete `vi.stubGlobal("window", …)` setup so no global stub can leak in the dialogs suite;
- render it without importing the App composition root;
- preserve assertions for role, modal, default localized title/copy, cancel label, danger class and confirm label.

Keep the App source test `reset local data uses an in-app confirmation before deleting local state` unchanged; it owns state/handler/API wiring. SessionVerificationGate tests remain unchanged.

Focused test totals after movement: App79, dialogs14, combined93.

## Deterministic exactness

A temporary TypeScript AST verifier compares immutable base with parent + destination:

- component declaration1/1, App parent0;
- body/props/defaults/JSX exact modulo export placement;
- App direct import1 and explicit compatibility re-export1;
- four call sites exact;
- focused test assertions exact with only owner/import setup changes;
- all other App/dialog declarations, public exports, hooks/state/listeners/timers/render and dependencies exact;
- duplicate/missing/excess declarations0.

## Verification

Run App79 + dialogs14, SessionVerificationGate tests, typecheck/lint, full Vitest/Playwright with polling, build/source/boundary/security and diff checks. After full-diff approval, integrate latest `origin/main` if required, run the full repository matrix and PR CI7/7.

The App umbrella remains open for verification-gate extraction/re-evaluation, composer/attention ownership and final residual audit.

## Review gate

- Read-only reconnaissance confirmed one component, four presentation call sites and one focused test with no state/resource ownership.
- `reviewer-flash` independently traced the component closure, four call sites, import/re-export compatibility, assertion set, focused counts and state/resource exclusions and recorded `Correct-to-implement`.
- Branch was documentation-only before implementation.
