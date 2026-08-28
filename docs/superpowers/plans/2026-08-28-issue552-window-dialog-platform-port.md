# Issue #552 Phase 2B3 — Window and dialog platform port

Status: design pending independent review. Implementation is unauthorized until `reviewer-flash` returns `Correct-to-merge`.

## Scope and invariant

Phase 2B3 isolates the existing App/appRuntime window and general dialog operations behind one neutral platform port. It changes dependency direction only. React dialog/gesture state, Rust commands/state, IPC/DTOs, translations, shortcut semantics, and failure behavior remain unchanged.

After this phase:

- `App.tsx` imports only the still-deferred Tauri event listener directly; its window and general dialog imports are removed;
- `backend/appRuntime.ts` imports no Tauri package;
- `backend/tauri/windowDialogPort.ts` is the sole owner of general current-window/fullscreen/drag and account/key-file dialog operations;
- the media-save dialog stays in its already-reviewed `backend/tauri/linkMediaPort.ts` family;
- no product-state owner, browser UX, compatibility shim, or speculative abstraction is introduced.

Out of scope: Core/state/menu event subscriptions (Phase 2B4), media-save behavior, desktop-attention window operations, React confirmation overlays, Rust/IPC changes, and later semantic-owner phases.

## Current behavior to preserve exactly

1. Fullscreen shortcut obtains the current Tauri window, reads `isFullscreen()`, then sets the inverse; rejection remains an unhandled fire-and-forget task exactly as today.
2. Top-bar drag and session-verification drag run only when `isTauriRuntime()` and swallow `startDragging()` rejection at their callers.
3. Sign-out confirmation calls Tauri `confirm(message, { title, kind: "warning" })` without a runtime guard; all four sign-out entry points still route through `requestLogout`, and the existing in-flight guard/finally settlement remains React-owned.
4. Room-key export and secure-backup destination return `null` before any dialog call outside Tauri. Their titles, default paths, filters, and `selected || null` normalization remain unchanged.
5. Room-key import returns `null` before any dialog call outside Tauri. Its `multiple: false`, filters, `fileAccessMode: "scoped"`, and string-only normalization remain unchanged.
6. No window or dialog object is created or queried at module evaluation; operations acquire the current window only when called.
7. Existing App source guards, accessibility, and browser/Tauri harness behavior remain intact.

## Design

### Neutral contract

Add `backend/windowDialogPort.ts` with only the five existing operation shapes:

- `toggleFullscreen(): Promise<void>`;
- `startDragging(): Promise<void>`;
- `confirm(message, options): Promise<boolean>`;
- `saveFile(options): Promise<string | null>`;
- `openFile(options): Promise<string | string[] | null>`.

Define local neutral option/filter types containing only fields and value domains currently used: `title: string`; `kind: "warning"`; `defaultPath: string`; `filters: { name: string; extensions: string[] }[]`; `multiple: boolean`; and `fileAccessMode: "scoped"`. Do not import Tauri types or expose a Tauri window handle.

### Composition and exact browser parity

Add `backend/windowDialogRuntime.ts` exporting one factory-created `windowDialogPort`. It intentionally constructs the Tauri implementation unconditionally because current `App.tsx` imports the Tauri dialog/window modules in every build and sign-out confirmation calls the Tauri plugin without an `isTauriRuntime()` guard. Keep the existing per-operation guards in App/appRuntime. Do not invent a browser `window.confirm`, Fullscreen API, file picker, no-op, or automatic sign-out path in this structural PR.

The Tauri factory must be lazy with respect to platform operations: constructing the adapter performs no window lookup, dialog call, or IPC.

### Tauri adapter

Add `backend/tauri/windowDialogPort.ts` with `createTauriWindowDialogPort()`:

- `toggleFullscreen` calls `getCurrentWindow()` at invocation, awaits `isFullscreen`, and awaits `setFullscreen(!fullscreen)`;
- `startDragging` calls `getCurrentWindow().startDragging()` and lets callers retain their existing catch policy;
- dialog methods delegate once to Tauri `confirm`, `save`, and `open`, forwarding the neutral options unchanged;
- no translation, normalization, runtime check, retry, or React lifecycle moves into the adapter.

### Caller migration

`backend/windowDialogPort.ts` is contract-only. Both App and appRuntime import the concrete `windowDialogPort` object from `backend/windowDialogRuntime.ts`; their tests mock that runtime module.

- In `App.tsx`, replace `getCurrentWindow`, `confirmDialog`, `saveDialog`, and `openDialog` calls with the corresponding `windowDialogPort` operations. Keep every existing `isTauriRuntime()` guard, async wrapper, normalization, options object, and request/logout guard exactly in place. Remove only the window/dialog Tauri imports and their two now-obsolete disable directives; retain the event-listener import/disable. Narrow the adjacent header comment to that single deferred event import and remove its stale #87/plural wording.
- In `backend/appRuntime.ts`, replace only `getCurrentWindow().startDragging()` with `windowDialogPort.startDragging()`; preserve the runtime guard and swallowed rejection.
- Update `App.test.tsx` sign-out source assertion from `confirmDialog` to `windowDialogPort.confirm`. Count actual Tauri import statements with a line-anchored import/from regex rather than raw `@tauri-apps` substrings, assert the sole statement is the event import, and separately assert no window/dialog import statement. Also assert `backend/appRuntime.ts` is Tauri-import-free.
- Update `backend/appRuntime.test.ts` to mock `windowDialogRuntime` rather than `@tauri-apps/api/window`; assert browser drag remains guarded and Tauri drag delegates with rejection swallowed.
- Existing specialized App tests may retain official Tauri package mocks where they exercise the adapter transitively; do not churn unrelated test setup.

### Static boundary

Update the App ESLint comment from three to one grandfathered direct import. Extend the existing restricted-import file list to `src/backend/appRuntime.ts`, so its Tauri-free acceptance criterion is statically enforced; approved concrete adapters remain under `backend/tauri/`. Add the statement-level App and appRuntime source assertions described above. General dialog/window imports must occur only in approved `backend/tauri/` adapters.

## Verify-first tests

Before production edits, add jsdom `backend/windowDialogRuntime.test.ts` using `vi.resetModules()` and a `vi.doMock("./tauri/windowDialogPort")` factory spy. It proves one adapter is created at composition and no operation is called eagerly. This is RED because the runtime module does not exist.

Add jsdom `backend/tauri/windowDialogPort.test.ts` with mocked Tauri window/dialog packages and prove:

- fullscreen reads then inverts state with ordered awaited calls;
- drag acquires the current window and propagates rejection;
- confirm/save/open forward exact arguments and return values;
- factory construction performs no Tauri operation.

Focused App/appRuntime source/behavior tests must prove all guards, option payloads, normalizations, and catch behavior remain.

## Expected files

- `apps/desktop/src/backend/windowDialogPort.ts` (new)
- `apps/desktop/src/backend/windowDialogRuntime.ts` (new)
- `apps/desktop/src/backend/windowDialogRuntime.test.ts` (new)
- `apps/desktop/src/backend/tauri/windowDialogPort.ts` (new)
- `apps/desktop/src/backend/tauri/windowDialogPort.test.ts` (new)
- `apps/desktop/src/backend/appRuntime.ts`
- `apps/desktop/src/backend/appRuntime.test.ts`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/App.test.tsx`
- `apps/desktop/eslint.config.js`
- ownership inventory and Phase 2 plan/index docs

No domain, component, Rust, IPC, DTO, generated-artifact, CSS, dependency, BrowserFakeApi, or app-harness change is expected.

## Verification matrix

- focused runtime/Tauri adapter/appRuntime/App tests;
- full Vitest and Playwright;
- typecheck, lint, build, IME/docs checks;
- Tauri/domain boundary guards, SDK submodule, secret scan, and `git diff --check`;
- exact-final-diff `reviewer-flash` verdict and current-head CI before merge.

## Design review record

- Round 1 timed out before reading the design and returned unverified `Not correct-to-merge`; it established no actionable design finding.
- Round 2, `reviewer-flash`: `Correct-to-merge` with four Minor/Nit precision findings. The appRuntime lint/source gate, import-statement regex, literal option value domains, and narrowed App comment were incorporated.
- Round 3, `reviewer-flash`: `Correct-to-merge`; the final contract-module/runtime-module import-target clarification was accepted before implementation.

## Implementation evidence

- RED: focused composition test failed before production edits because `backend/windowDialogRuntime` did not exist.
- Focused runtime/Tauri adapter/appRuntime/App tests: 4 files / 88 tests passed.
- Full Vitest: 96 files / 1475 tests passed.
- Playwright: 263 tests passed.
- Typecheck, lint/IME/docs, production build, Tauri/domain guards, SDK-submodule check, secret scan, and `git diff --check` passed.

## Acceptance

- App has one direct Tauri event import and no direct window/dialog import;
- appRuntime and all non-adapter production modules are window/dialog-Tauri-free except the previously approved media/attention adapters;
- all operation guards, args, returns, normalization, and failure behavior are unchanged;
- full tests/gates pass, inventory is exact, and #552 remains open.
