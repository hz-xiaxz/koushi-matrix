# Issue #552 Phase 2B2 — Desktop attention platform port

Status: design pending independent review. Implementation is unauthorized until `reviewer-flash` returns `Correct-to-merge`.

## Scope and invariant

Phase 2B2 isolates the existing native notification and desktop-attention platform operations. It changes dependency direction only: Rust-owned `native_attention` state, capability policy, privacy-safe notification projection, diagnostic tokens, IPC command names, and React effect timing remain unchanged.

After this phase:

- `domain/desktopAttention.ts` and `domain/desktopNotification.ts` contain only platform-neutral policy, projection, contracts, and failure-token handling;
- `app/useDesktopAttentionEffects.ts` consumes one selected neutral platform port and imports no Tauri package;
- only `backend/tauri/desktopAttentionPort.ts` imports the notification plugin, Tauri window API, and attention command `invoke` calls;
- non-Tauri/browser behavior remains exactly the current `null` platform seam: document title still updates, while native notification/window/badge/sound operations do not run;
- no new product-state owner or compatibility shim is introduced.

Out of scope: general App window/dialog operations, menu/Core/state event subscriptions, Rust native-attention semantics, notification copy/localization, permission prompting, IPC/wire changes, and later #552 semantic-owner phases.

## Current behavior to preserve exactly

1. `document.title` updates in every runtime.
2. Native attention runs only when `"__TAURI_INTERNALS__" in window`.
3. The current Tauri window is obtained at effect execution, then supplies title, WebView badge/overlay/tray operations, and `requestUserAttention`.
4. Native badge invokes `set_native_attention_badge` with `{ count }` and returns `applied | unsupported | mismatch`.
5. Sound invokes `play_native_attention_sound` and returns `played | unsupported | failed | skipped`; the domain dispatcher retains cooldown/in-flight policy and diagnostics.
6. Passive notification delivery calls `isPermissionGranted` once per adapter lifetime, never calls `requestPermission`, and sends only when already granted.
7. Notification clear settles both `cancelAll()` and `removeAllActive()`; either rejection becomes the fixed adapter error consumed as `attention_notification_clear_failed`.
8. Notification payload construction remains privacy-safe and domain-owned: title/body only, from the Rust candidate's allowed room display name and counts.
9. All async effect calls and fixed diagnostic tokens retain their current fire-and-observe behavior.

## Design

### Neutral contract and selection

Add `backend/desktopAttentionPort.ts` with the smallest existing-operation aggregate:

- `currentWindow(): DesktopAttentionWindowPort`, where the neutral window contract extends the existing `DesktopWindowLike` operations with `requestUserAttention(requestType: typeof DESKTOP_ATTENTION_REQUEST_TYPE)`; use a type-only reference to the domain constant and never import Tauri's `UserAttentionType`;
- `notifications: DesktopNotificationTransport`;
- `sound: DesktopAttentionTransientLike`;
- `nativeBadge: DesktopNativeBadgeLike`.

Add `backend/desktopAttentionRuntime.ts` exporting one `desktopAttentionPort` selected by the existing leaf `runtimeEnvironment.isTauriRuntime()`. The Tauri module exports `createTauriDesktopAttentionPort()`; the runtime calls that factory exactly once and only in the Tauri branch, making the browser non-construction contract observable. It returns the selected instance or `null`; do not add a speculative browser no-op object.

### Tauri adapter

Add `backend/tauri/desktopAttentionPort.ts` with the factory `createTauriDesktopAttentionPort()`:

- `currentWindow()` calls `getCurrentWindow()` each time, preserving effect-time acquisition;
- `nativeBadge.setBadgeCount` and `sound.playAttentionSound` keep the exact invoke names, args, and typed outcomes;
- the notification transport owns the adapter-lifetime permission promise and exact send/clear behavior above;
- no Matrix/product state, retry policy, copy construction, diagnostics, or React lifecycle moves into the adapter.

### Domain and hook cleanup

- Delete `createTauriDesktopNotificationTransport` and its plugin imports from `domain/desktopNotification.ts`; retain the neutral interface and all pure content/send/clear functions.
- Delete `createTauriDesktopAttentionTransientTransport` from `domain/desktopAttention.ts`; the one-line platform wrapper belongs in the Tauri adapter.
- Update `useDesktopAttentionEffects` to read `desktopAttentionPort`. Keep its three effects, dependency arrays, document-title update, dispatcher singleton, diagnostic sinks, capability checks, and candidate derivation intact. It obtains the current window at each existing use site and passes the port's neutral sub-ports to the existing domain functions.
- Update the existing `App.test.tsx` attention source contract in place without widening its slices: replace `tauriNotificationTransport` with the port's `notifications` sub-port and `getCurrentWindow()` with `desktopAttentionPort.currentWindow()`. Add the static assertions there that neither attention domain module nor the hook imports `@tauri-apps`.

### Static boundary

- Remove `domain/desktopNotification.ts` from the Phase 2B ESLint exception.
- Apply the existing `@tauri-apps/**` restriction to all `src/app/**/*.{ts,tsx}` files, including app tests, as well as components/App. There is no app-test exception; future app tests must mock the neutral port rather than Tauri packages.
- Tauri imports remain admitted only under `backend/tauri/` plus separately deferred adapter families already enumerated in the inventory.

## Verify-first tests

Before production edits, add jsdom `backend/desktopAttentionRuntime.test.ts` using `vi.resetModules()` plus `vi.doMock("./runtimeEnvironment")` and a factory spy for `./tauri/desktopAttentionPort`:

- mocked Tauri runtime selects exactly the Tauri port and constructs it once;
- browser runtime selects `null` and never calls the factory.

This is RED because the runtime module does not exist.

Move implementation-specific tests from the domain tests to jsdom `backend/tauri/desktopAttentionPort.test.ts`; mock `@tauri-apps/api/window`, `@tauri-apps/api/core`, and `@tauri-apps/plugin-notification` following the existing link/media adapter pattern, and prove:

- exact window acquisition and native badge/sound command names/args/outcomes;
- permission-granted send, denied passive skip, no permission prompt, and permission check cached across two sends on the same factory-created port;
- clear calls both plugin operations and rejects when either fails.

Keep domain payload/privacy/failure-token tests unchanged apart from removed Tauri imports. Add a static source assertion that neither attention domain module nor the hook imports `@tauri-apps`.

## Expected files

- `apps/desktop/src/backend/desktopAttentionPort.ts` (new)
- `apps/desktop/src/backend/desktopAttentionRuntime.ts` (new)
- `apps/desktop/src/backend/desktopAttentionRuntime.test.ts` (new)
- `apps/desktop/src/backend/tauri/desktopAttentionPort.ts` (new)
- `apps/desktop/src/backend/tauri/desktopAttentionPort.test.ts` (new)
- `apps/desktop/src/app/useDesktopAttentionEffects.ts`
- `apps/desktop/src/domain/desktopAttention.ts`
- `apps/desktop/src/domain/desktopAttention.test.ts`
- `apps/desktop/src/domain/desktopNotification.ts`
- `apps/desktop/src/domain/desktopNotification.test.ts`
- `apps/desktop/src/App.test.tsx`
- `apps/desktop/eslint.config.js`
- ownership inventory and Phase 2 plan/index docs

No `App.tsx`, Rust, IPC, DTO, generated artifact, browser fake, CSS, or dependency change is expected.

## Verification matrix

- focused runtime/Tauri adapter/domain attention/notification/App tests;
- full Vitest and Playwright;
- typecheck, lint, build, IME/docs checks;
- Tauri/domain boundary guards, SDK submodule, secret scan, and `git diff --check`;
- exact-final-diff `reviewer-flash` verdict and current-head CI before merge.

## Design review record

- Round 1, `reviewer-flash`: `Correct-to-merge` with five Minor precision findings. The factory/non-construction shape, literal request type, deterministic mock recipe, all-app ESLint scope, and exact App test assertion placement are incorporated above; a focused Round 2 confirms the amended document before implementation.

## Acceptance

- notification and attention platform imports exist only in the approved Tauri adapter;
- hook/domain source is Tauri-free and behaviorally unchanged;
- the non-Tauri branch remains null/no native effects;
- all behavior-preservation tests and full gates pass;
- the inventory records the reduced exact direct-import set and #552 remains open.
