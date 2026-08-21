# Issue #551 App session-verification gate extraction

Status: design approved. Scope is one behavior-preserving verification/secure-backup/cleanup presentation-controller seam.

## Baseline

- Base: `e76c2222ead6238b9fbadb52bc8a01c673054670` after destructive-dialog PR #628.
- `App.tsx`: 6,933 newline-delimited lines / 247,587 bytes / SHA-256 `afe1f36d4c5e42467f4bcb5695385aa78410692e2db4b8f0f2a89ab67b852e84`.
- Focused baseline: App79; SessionVerificationGate28.

## Dependency owner prerequisite within the same atomic PR

Create `apps/desktop/src/backend/appRuntime.ts` to preserve one shared API instance and the existing native drag default:

1. move `const api = createDesktopApi()` from App and export it;
2. move `startSessionVerificationWindowDrag` unchanged and export it.

The backend leaf imports `createDesktopApi`, `getCurrentWindow`, and `isTauriRuntime`. App imports the same `api`; the gate imports both exports. This preserves one browser-fake/Tauri API owner, avoids a second stateful fake, keeps Tauri packages out of components, and preserves the optional `onStartWindowDrag` default behavior.

## Gate ownership and immutable order

Create `apps/desktop/src/components/SessionVerificationGate.tsx`. Move these 12 declarations in original relative order:

1. `provisionalPhaseKind`
2. `provisionalPhaseFailure`
3. `SecureBackupOperationKind`
4. `SessionVerificationGateOperations`
5. `defaultSessionVerificationGateOperations`
6. `secureBackupFailureLabel`
7. `secureBackupPendingLabel`
8. `secureBackupGateHeading`
9. `secureBackupGateFailure`
10. `SessionVerificationGate`
11. `gateFailureLabel`
12. `gateRejectLabel`

Export exactly the operations interface, component, `secureBackupFailureLabel`, and `secureBackupGateFailure`; keep the other eight declarations private.

Move bodies, JSX, refs/state, single-flight guards, secret clearing, default operations, command calls and presentation ordering unchanged. Type-only `import("./domain/types")` references change only to the correct `../domain/types` path.

## App compatibility and residual

The gate module has exactly six approved import statements: React `useRef`/`useState`; the three IME controls; `ResetLocalDataConfirmationDialog`; `t`; `api` plus `startSessionVerificationWindowDrag`; and one type import containing `DesktopSnapshot`, `PendingKeyCountBucket`, `SecureBackupGateFailureKind`, and `SecureBackupGateState`. Inline `import()` type paths in moved signatures change only from `./domain/types` to `../domain/types`.

App directly imports the component and two shared secure-backup helpers. Preserve the established public App paths with:

```ts
export {
  SessionVerificationGate,
  type SessionVerificationGateOperations
} from "./components/SessionVerificationGate";
```

Remove only these orphan App imports/members:

- `createDesktopApi`;
- `ImeSafeForm`, `ImeTextField`, `SecureImeTextField`;
- `PendingKeyCountBucket`, `SecureBackupGateFailureKind`, `SecureBackupGateState`.

Retain `currentSessionStatusFailureLabel`, `getCurrentWindow`, `isTauriRuntime`, React refs/state, `DesktopSnapshot`, `ResetLocalDataConfirmationDialog`, `api` call sites and all other imports used by the composition root.

Keep in App:

- verification-gate admission decision and secure-backup startup-vs-runtime exposure refs;
- `snapshot`/`setSnapshot`, logout/sign-out, diagnostics and file chooser ownership;
- runtime alert construction and `currentSessionStatusFailureLabel`;
- Auth/Recovery/capability render branches;
- all non-gate API calls and render composition.

Rust remains authoritative for session, verification, secure-backup and cleanup state. The component retains only existing secret DOM refs, ephemeral dialogs and single-flight presentation state.

## Exact gate invariants

Preserve:

- provisional phase normalization/failure mapping;
- operations default/override merge and one shared `api` object;
- gate and secure-backup single-flight refs;
- secret/passphrase/password clearing before command dispatch;
- secure-backup destination selection and artifact delivery flow;
- SAS match/mismatch/cancel, bootstrap start/save and recovery flows;
- cleanup offer/UIA/erase-anyway/retry flows;
- heading/failure/rejection labels and private-data-free UI;
- drag-region button guard and native drag default;
- all ARIA/classes/i18n/message IDs and four destructive-dialog interactions.

No command/DTO/Rust reducer/CSS/i18n catalog/backend behavior change. No giant prop bag, wrapper, context, barrel or second API instance.

## Tests

Retarget `SessionVerificationGate.test.tsx` import from `./App` to `./components/SessionVerificationGate`; keep all existing tests unchanged.

Move the component-owned App test `renders verification admission phases and an actionable preparation failure` into the gate suite:

- direct component import;
- remove the obsolete App dynamic import and window stub; jsdom provides a real window, `isTauriRuntime()` remains false, and BrowserFakeApi state is instance-owned;
- add the existing `renderToStaticMarkup` dependency directly to the gate suite if not already present;
- preserve all five assertions and browser-fake snapshot setup.

Keep App's source/admission test `renders verification states before and mutually exclusive with the desktop shell` unchanged.

Focused totals after movement: App78, gate29, combined107.

## Deterministic exactness

A temporary TypeScript AST verifier compares immutable base with parent + leaves:

- backend declarations2/2 and gate declarations12/12, App parent0;
- bodies/types/JSX exact modulo approved exports and relative type-import paths;
- gate exports4/private8/import statements6, App direct gate imports3, backend exports2/import closure3;
- App orphan import members7 and no other import deletion;
- one shared API initializer across production;
- gate call site/admission ordering and residual secure-backup helper calls exact;
- public App re-export2, focused assertion set exact;
- all other App hooks/listeners/timers/render/public exports/dependencies exact;
- duplicate/missing/excess declarations0.

## Verification

Run App78 + gate29, dialogs14, e2e verification-gate12, typecheck/lint, full Vitest/Playwright with polling, build/source/boundary/security and exactness/diff checks. After full-diff approval, integrate latest `origin/main` if required, run the full repository matrix and PR CI7/7.

The App umbrella remains open for composer/attention re-evaluation and final residual audit.

## Review gate

- Read-only reconnaissance found the gate closure and rejected adding Tauri or a second API instance to the component.
- `reviewer-flash` independently traced the API singleton/eager mock order, all declarations/imports, App compatibility, gate invariants, tests/counts and dependency graph and recorded `Correct-to-implement`.
