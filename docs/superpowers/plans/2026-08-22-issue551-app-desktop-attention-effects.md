# Issue #551 App desktop-attention effects extraction

Status: design review pending. Scope is the final clean App lifecycle seam before residual audit.

## Baseline

- Base: `4a4f50ca488bf88b333633cf08e4fb656a11ef8f` after verification-gate PR #629.
- `App.tsx`: 6,367 newline-delimited lines / 221,792 bytes / SHA-256 `e8e09fc419d41358482972a58bf0dad342afbd915ac16ca545e5a1925673ee96`.
- Focused baseline: App78; desktop-attention domain26.

## Ownership decision and immutable order

Create private direct hook module `apps/desktop/src/app/useDesktopAttentionEffects.ts`. Move these module-lifetime resources in order:

1. `tauriNotificationTransport`
2. `tauriAttentionTransientTransport`
3. `tauriNativeBadgeTransport`
4. `desktopBadgeSoundDispatcher`

Move the contiguous App hook sequence in exact order into exported `useDesktopAttentionEffects`:

1. `attentionCapabilities` memo;
2. title/badge/overlay/tray plus badge-delta sound effect;
3. candidate activation plus desktop notification effect;
4. zero-badge notification-clear effect.

Call the hook unconditionally at the exact former memo position with one bounded object:

- `snapshot`;
- `attentionWindowTitle`;
- `safeAttentionSummary`;
- `appendDiagnosticLog`.

The hook returns nothing. Do not create callback registries, context, state mirrors or a general desktop-effects abstraction.

## Ownership and order contract

Preserve the underlying hook sequence (`useMemo`, then three `useEffect`s) and every dependency array exactly. The custom hook call occupies the same unconditional App position, so React's executed hook order is unchanged.

Preserve:

- document title assignment before Tauri-only work;
- Rust-owned badge count/capabilities/candidate as the sole attention decisions;
- native window title/badge/overlay/tray application and diagnostic tokens;
- ready-session sound reset/observe behavior and settings sound input;
- candidate activation before notification send, with transient sound explicitly false;
- zero-badge notification clearing;
- one module-lifetime badge-sound dispatcher and transport set;
- private-data-free `native.attention` diagnostic entries.

No Rust state, DTO, command, Tauri registration, title policy, notification policy or resource cleanup changes.

## Imports and App residual

Destination imports are explicit:

- React `useEffect`, `useMemo`;
- Tauri `invoke`, `getCurrentWindow`;
- `isTauriRuntime`;
- desktop-attention functions/type;
- desktop-notification functions;
- `DesktopSnapshot`;
- `TimelineDiagnosticLogEntry` as a type-only edge.

App imports only `useDesktopAttentionEffects`. Remove only moved desktop-attention/notification import members; retain `desktopAttentionSummary` and `desktopAttentionWindowTitle` for App title/QA composition. App continues to own `attentionSummary`, `safeAttentionSummary`, `attentionWindowTitle` and the diagnostic log callback.

Keep all QA-send listeners, timeline transport augmentation, avatar effects, composer lifecycle, search/space/room operations and render composition in App.

## Source contract

Update only `feeds Rust-owned native attention into window title and notification adapters` in `App.test.tsx`:

- read `App.tsx` and `app/useDesktopAttentionEffects.ts` separately;
- keep summary/QA-title/right-panel assertions against App;
- keep candidate/activation/notification/clear/window/sound assertions against the hook owner;
- never concatenate source strings;
- preserve every assertion and forbidden legacy token check.

The 26 domain attention tests remain unchanged.

## Deterministic exactness

A temporary TypeScript AST verifier compares immutable base with parent + leaf:

- resources4/4 and hook statements4/4 in original order, parent0;
- resource initializers, effect bodies and dependency arrays exact modulo the hook parameter prefix;
- leaf export1/private resources4 and approved import closure;
- App call1 at the former hook position and moved import orphans only;
- retained App declarations/hooks/render/public exports exact;
- source-contract assertion set exact with owner paths split/no concatenation;
- module-lifetime resource count, public API, dependencies, product-state and reverse-edge deltas0;
- duplicate/missing/excess logic0.

## Verification

Run App78, desktop-attention26, typecheck/lint, full Vitest/Playwright with polling, build/source/boundary/security and exactness/diff checks. After full-diff approval, integrate latest `origin/main` if required, run the full repository matrix and PR CI7/7.

After merge, perform the final App residual audit; no App checkbox closes before its reviewer verdict.

## Review gate

- Read-only residual audit found this one lifecycle seam and rejected composer, QA, timeline/avatar/search/room and shell extraction as ownership-damaging.
- Formal `reviewer-flash` verdict pending; implementation prohibited until `Correct-to-implement`.
