# macOS modal-time window drag (#1445)

Canon consulted: `REPOSITORY_RULES.md` ("macOS Native Window Controls And
Overlay Layout"), engineering rules "macOS Native Window Controls And Overlay
Layout", and `docs/superpowers/specs/2026-07-14-verification-retry-window-drag-design.md`.

## Root cause

The macOS shell uses an overlay title bar, so the only WebView drag surface is
the shell `.titlebar`. Every modal opens through `NativeModal`, which calls
`dialog.showModal()`; the top layer inerts the rest of the document, including
`.titlebar`, and the `::backdrop` cannot take event handlers. Pressing the
title-bar strip over a modal therefore hit an inert region and the window did
not move. Window resizing was unaffected (AppKit owns the edges).

## Fix

`NativeModal` now renders its own drag strip as the first child of the dialog:
a `position: fixed` element spanning the top of the viewport, with
`block-size: var(--native-titlebar-safe-block, 0px)`, `aria-hidden="true"`,
`data-tauri-drag-region=""` and a primary-button-only `onMouseDown` that calls
`startSessionVerificationWindowDrag` — the same guarded path the shell titlebar
and `SessionVerificationGate` already use. The height collapses to `0px` off
macOS, so Windows/Linux are untouched and the element cannot intercept content.

`ModalDialog` forwards an optional `onStartWindowDrag` for testability; the
default is the shared helper, so no modal caller had to change. A `position:
fixed` child escapes the `overflow` of full-window overlays (its containing
block is the viewport), so the strip works for `.app-modal` framed dialogs and
`dialog-overlay` / media-viewer full-window surfaces alike. It sits inside the
already-reserved safe block, above dialog content, so the native buttons stay
clear.

## Verification

- `apps/desktop/src/components/ModalDialog.test.tsx` (new): the strip exists
  with `data-tauri-drag-region`, a secondary-button press does not start a
  drag, and a primary-button press calls `onStartWindowDrag` once.
- `apps/desktop/e2e/macos-modal-safe-area.spec.ts` (extended, Chromium
  headless): on macOS the strip starts at `y = 0` with height ≥ 44 px and the
  dialog header stays at `y ≥ 44`; off macOS the strip's height is `0px`.
- `npx vitest run` (2034 passed), `npx tsc --noEmit`, `npm --prefix
  apps/desktop run lint`, and the full `macos-modal-safe-area.spec.ts`
  Chromium project (17 passed).
- Manual macOS check still owed: with Settings/Diagnostics open, dragging the
  title-bar strip moves the window, resizing still works, and the traffic-light
  buttons stay clickable.

The three test files that mock `../backend/appRuntime` now also stub
`startSessionVerificationWindowDrag`, because `ModalDialog` imports the same
shared helper those modules export.
