# Non-modal popup dismissal owner (#1365, #1368) (2026-10-12)

The user reported that the sidebar section's own options menu ("DMのオプション")
stayed on screen until the list was navigated away, and an audit of every
floating surface found the same defect on four more popups.

Canon consulted: `docs/policies/engineering-rules.md` ("UI Presentation",
"Concurrent Work Details", "Desktop Text Input And IME Safety"),
`REPOSITORY_RULES.md` ("GitHub Issue Closure", "Documentation And Work
Records"), and `docs/agents/verification.md` (verify first, no human eyes).
Element Web's `matrix-react-sdk` `ContextMenu.tsx` (screen-sized click guard,
Escape, focus restore) was read as the upstream reference.

## Cause

No shared owner existed for press-driven, non-modal dismissal. Each surface held
its own flag (or derived state) and implemented only Escape:
`.section-menu` (`Shell.tsx`), `.trust-help-popover` (`TrustHelp.tsx`),
`.message-media-details-popover` (`TimelineMedia.tsx`),
`.message-source-dialog` (`MessageSourceDialog.tsx`), and
`.composer-autocomplete` (`composer.tsx`). `FloatingLayer` owns geometry only,
`useModalFocusTracking`/`NativeModal` own modals, and `useHoverFocusPopup` owns
hover/focus popups, so nothing between those popups and `document` could catch
an outside press. Three of the five kept the flag per row or per button, so two
instances could be open at once.

## Change

- `apps/desktop/src/components/popupDismissal.ts` (new): the shared
  `useOutsidePressDismissal` owner. A `pointerdown` outside the popup and its
  trigger dismisses it without swallowing the press, one popup per `kind` stays
  open, and Escape/focus return stay with each surface.
- Applied to all five surfaces (`Shell.tsx`, `TrustHelp.tsx`,
  `MessageSourceDialog.tsx`, `TimelineMedia.tsx`, `composer.tsx`).
- `Shell.tsx`: the section menu focuses its first item on open, roves with
  `onMenuKeyDown`, and its label is `role="presentation"`.
- `ContextMenuSurface.tsx`: `moveRovingMenuFocus` matches `[role^="menuitem"]`,
  so a `menuitemradio` menu roves too.
- `composer.tsx`: refocusing the editor clears a dismissed mention query, so the
  same query offers its candidates again in a new editing session.
- `docs/policies/engineering-rules.md`: new "Popup Dismissal And Floating
  Surfaces" subsection under "UI Presentation".
- `docs/help/rooms-and-spaces.md`: the section menu's dismissal behavior.

## Verification

RED-then-GREEN, both tiers, before and after the fix:

- jsdom (`npx vitest run`): new `popupDismissal.test.tsx`, `TrustHelp.test.tsx`,
  `timeline/MessageSourceDialog.test.tsx`, `timeline/TimelineMediaDetails.test.tsx`,
  plus the new "section options menu dismissal" and "mention autocomplete
  dismissal" cases. Pre-fix: 7 failing (the five surfaces plus two
  single-instance cases); post-fix: green.
- Headless Chromium (`npx playwright test
  e2e/popup-outside-press-dismissal.spec.ts`): pre-fix on `origin/main` the
  outside-press case left `.section-menu` connected (expected 0, received 1) and
  two section menus were open at once (expected 1, received 2); post-fix green,
  with the emoji picker (which owned its own dismissal) as the green control.
- `e2e/composer-send-queue-upload.spec.ts`: the main/thread mention-target
  independence case now also asserts the main suggestions are dismissed when the
  thread panel opens, and the re-open path is covered in jsdom.
- Gates: `npx tsc --noEmit`, `npx eslint src`, `check-ime-text-inputs.mjs`,
  `check-agents-docs.mjs`, `check-frontend-semantic-owners.mjs`,
  `user-help.mjs --check`, full Vitest (1916 passed) and the full Playwright DOM
  suite. `src/i18n/katexVendor.test.ts` fails identically on unmodified
  `origin/main` (local dependency state), and
  `TimelineView.threads.test.tsx > falls back to the virtual height model` is a
  pre-existing flake reproduced on `origin/main`.

## Follow-up

- #1369: the section menu is positioned inside `.sidebar-scroll` (clipped) and
  an open menu can cover the next section's options trigger. Recorded, with the
  headless evidence, instead of changing placement in this change.
