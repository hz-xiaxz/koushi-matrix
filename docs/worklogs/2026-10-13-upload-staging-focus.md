# Upload staging opens on the caption, not the clear action (2026-10-13)

The user reported that staging an attachment opened the upload dialog with focus
on the clear (X) action, whose focus bubble ("アップロード準備をクリア") therefore
appeared on screen every time a file was attached.

Canon consulted: `REPOSITORY_RULES.md` ("Root-Cause Fix Discipline", "User-Visible
Text", "Documentation And Work Records"), `docs/policies/engineering-rules.md`
("Popup Dismissal And Floating Surfaces", "Desktop Text Input And IME Safety"),
and `docs/agents/verification.md` (a failing headless check first). Element Web's
`UploadConfirmDialog`/`BaseDialog` were read as the upstream reference; upstream
lets `react-focus-lock` land on the dialog's close control, so this is an
intentional divergence for a dialog whose purpose is captioning.

## Cause

`UploadStagingDialog` explicitly focused its clear button on mount (a leftover
from the pre-`NativeModal` focus trap). The shared `Tooltip` opens immediately on
focus — a correct explanation for a keyboard visit — so the programmatic
open-time focus surfaced the bubble. The modal's own focus steps would have
picked the first focusable control (the same X) anyway.

## Change

- `apps/desktop/src/components/dialogs.tsx`: the staging dialog focuses the first
  caption editor in a layout effect (after `NativeModal` opens the dialog, before
  paint) instead of the clear button, and passes `onCancel={onClear}` to that
  caption. The caption's `Composer` consumes Escape through the Rust resolver, so
  without an owner for `cancel` the key could no longer dismiss the dialog at all.
- `apps/desktop/src/components/composer.tsx`: one optional `editorHandleRef` prop,
  so a hosting surface can own when its editor takes focus. Composer otherwise
  keeps its internal handle.
- `apps/desktop/e2e/upload-staging-scroll.spec.ts`: real-browser regression, now
  opted into the WebKit project in `apps/desktop/playwright.config.ts`. The
  defect lives in the native `<dialog>` focus steps and the focus bubble, so the
  macOS WKWebView engine family needs the coverage: Chromium 21/21 and WebKit
  21/21 for the spec file.

No `docs/help/` change: the guide describes the staging flow's steps, not where
focus lands on open, and nothing user-visible in the steps changed.

## Verification

- RED then GREEN, `apps/desktop/src/components/dialogs.test.tsx`
  ("opens on the first caption editor, never the clear action"): with the old
  code `document.activeElement` was the clear button and its tooltip was open;
  now the first caption is focused and no tooltip is mounted.
- RED then GREEN, `apps/desktop/e2e/upload-staging-scroll.spec.ts`
  ("staging a file opens on its caption editor without explaining the clear
  action"): Chromium confirmed the real modal focus steps, and the same spec file
  caught the Escape regression (the macOS staging-space case, which previously
  dismissed the dialog from the X). Full file: 21 passing on Chromium and 21
  passing on WebKit.
- `apps/desktop/src/App.test.tsx` + `composer.test.tsx` + `dialogs.test.tsx`:
  198 passing. Full vitest suite: no new failures (the 7 remaining failures in
  `katexVendor`, `RoomCreationHistory`, and `TimelineView.threads` reproduce on a
  pristine `HEAD`). `tsc --noEmit` and ESLint clean for the touched files, plus
  `scripts/check-ime-text-inputs.mjs` and `scripts/check-frontend-semantic-owners.mjs`.
