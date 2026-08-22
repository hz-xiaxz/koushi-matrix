# Issue #551 Browser Fake Settings Projection

## Status

Design approved by `reviewer-flash`: `Correct-to-implement`.

## Immutable baseline

- Base: `b0cfcd17bf0a8a3700c6bf19c63ea897ca1b393f`
- `apps/desktop/src/backend/browserFakeApi.ts`: 6,482 newline-delimited lines, 215,338 bytes
- SHA-256: `2ca167cc69fd5eeba22d157e33d72ee3898d94fba9184a42e9ac6602a487c1ae`

## Ownership seam

Extract the pure settings/default/display/composer projection family into private leaf:

`apps/desktop/src/backend/browser-fake/settings.ts`

Declarations in immutable source order:

1. `defaultSettingsState`
2. `defaultLocaleDisplayProfile`
3. `defaultTypographyDisplayProfile`
4. `resolveTypographyDisplayProfile`
5. `resolveLocaleDisplayProfile`
6. `parseLocale`
7. `applySettingsPatch`
8. `resolveComposerKeyActionFromSettings`

The seven parent-called declarations are named exports; `parseLocale` remains leaf-private. The root imports exactly those seven directly from `./browser-fake/settings`; no barrel or re-export.

This seam owns deterministic fake settings defaults, locale/typography display resolution, settings patch projection, and the settings-derived composer key decision. It owns no mutable state, request ID, map, timer, listener, cleanup, session or Matrix semantics. `appHarnessMain.tsx` contains a parallel copy for its standalone mock-Tauri harness; that separate #551 owner is recorded for its own residual audit and remains untouched here.

## Move-only boundary

Move every declaration byte-for-byte except module export keywords and mechanically adjusted imports. No body/branch/token/constant/comment changes. Do not rename, wrap, duplicate, or reformat unrelated code.

Leaf type imports from `../../domain/types` are only:

- `ComposerKeyEvent`
- `ComposerResolvedAction`
- `ComposerResolverOptions`
- `DesktopSnapshot`
- `LocaleDisplayProfile`
- `LocaleSettings`
- `SettingsPatch`

Parent retains Composer/Settings types needed by `DesktopApi` and class signatures; parent drops only `LocaleSettings` and `LocaleDisplayProfile`, which have no remaining direct type site after extraction.

## Preserved surface and behavior

- `DesktopApi`, `BrowserFakeApiContract`, class fields/methods and factory exports unchanged
- update-settings ordering and snapshot mutation unchanged
- exact locale parsing, pseudo-locale/RTL behavior, typography assets and platform labels unchanged
- exact composer IME/autocomplete/shortcut precedence unchanged
- settings defaults, Rust-owned DTO shapes and all wire names unchanged
- zero new runtime resource owner or public package import path

## Deterministic exactness

A TypeScript-AST verifier compares immutable parent against parent+leaf and requires:

- declarations8/8, bodies/tokens/order and exact declaration source slices (including comments) preserved
- parent declaration count0 for all eight
- exports exactly7 and `parseLocale` not exported
- one new direct parent import path `./browser-fake/settings` with exactly7 imported names
- parent call/reference counts preserved after import normalization
- top-level/class/interface/API/export/field/map/timer counts unchanged except the eight moved declarations and one import
- no duplicate declaration, barrel, glob, TODO, shim, or leaf side effect
- `git diff --check`

## Focused and full verification

- same pre/post browser fake tests (current107) and client25
- settings, locale, composer and browser-fake focused tests
- typecheck/lint/Vitest/build and Playwright248 with polling
- workspace all-targets, Tauri, Headless Core QA, wasm
- Tauri/domain/IPC/security/release/SDK/docs/rustfmt/deny/machete/diff gates
- independent full-diff review, latest-main comparison, CI7/7, merge, #551 evidence

## Implementation evidence

- Exact declaration source slices8/8, parent0, leaf order8, exports7, private `parseLocale`, one seven-name parent import.
- Parent drops only `LocaleSettings`/`LocaleDisplayProfile`; bodies/comments/calls are unchanged.
- `browserFakeApi.ts` 6,482→6,306 lines; private leaf192 lines; combined6,498 lines.
- Browser fake107 + client25 and typecheck green.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`; full matrix pending.

## Delivery

One move-only ownership-seam PR. Browser-fake umbrella remains open after merge until a residual cohesion/lifecycle audit confirms no further clean seam.
