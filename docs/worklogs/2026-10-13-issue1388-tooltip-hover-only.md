# The shared tooltip is hover-only (2026-10-13, #1388)

The user reported that after changing a sidebar section's sort option ("DMの
オプション" → **Recent**/**Name**) the trigger's explanation bubble
("Options for DMs") stayed on screen over the DM rows.

Canon consulted: `REPOSITORY_RULES.md` ("Root-Cause Fix Discipline",
"Documentation And Work Records", "User-Visible Text"),
`docs/policies/engineering-rules.md` ("Popup Dismissal And Floating Surfaces"),
and `docs/agents/verification.md` (a failing headless check first). Upstream
reference: Element Web's icon-only controls use a native `title`, which is
pointer-hover only and is not exposed on focus.

## Cause

Choosing an item closes the menu and returns focus to the trigger
(`Shell.tsx` `SectionTitle`'s item handler). The shared `Tooltip` opened on any
focus (`Tooltip.tsx` `focus()` → `openNow()`), and the press suppression only
covered the case where the pointer stayed parked on the trigger: moving into the
menu ran `onMouseLeave` → `release()`, which re-armed the bubble. The refocus
after the choice therefore opened the explanation, and it stayed until focus
moved elsewhere.

This is a different surface from #1365/#1368 (the menu's own outside-press
dismissal, fixed by #1371); the bubble in that report's screenshot was already
the tooltip, not the menu.

## Decision

The shared bubble is pointer-owned and hover-only. `Tooltip` no longer handles
`focus`/`blur`, so neither a keyboard visit nor a programmatic refocus opens it;
`aria-describedby` is present only while the bubble is open, as before. The
wrapped control keeps its own accessible name, and #1376's fix (moving a dialog's
open-time focus off the clear action) stays valid but is no longer what protects
that surface.

This intentionally diverges from #1345's acceptance ("renders the styled
explanatory bubble on hover and focus") and from the promise in
`docs/help/rooms-and-spaces.md` that the participation indicator is readable by
reaching it with the keyboard. The engineering rules now state the hover-only
contract and the obligation that a fact described only in a hover bubble also
has a keyboard-reachable statement. For the access pills that statement is
already reachable: the room rows keep their permanent `aria-describedby`
sentence, and the header pill's room/Space route is visible in **Room info**,
which the pill opens at that property.

## Evidence (RED then GREEN)

RED on the pre-fix component, GREEN after changing `Tooltip.tsx` alone:

- `apps/desktop/src/components/Tooltip.test.tsx` — "a keyboard visit does not
  open the bubble" (received `.tooltip-bubble.is-open` with the label).
- `apps/desktop/src/components/Shell.test.tsx` — "leaves no explanation over the
  sidebar after a sort choice" (received the `Options for DMs` bubble after
  choosing **Name**).
- `apps/desktop/e2e/popup-outside-press-dismissal.spec.ts` — "choosing a section
  sort option leaves no explanation bubble" (browser tier).

Converted focus-driven expectations to hover:

- `apps/desktop/src/components/Tooltip.test.tsx` (delay path kept with fake
  timers; placement and press-suppression cases now hover).
- `Shell.toolbar.test.tsx`, `roomAccessSurfaces.test.tsx`,
  `RoomInfoPanel.test.tsx`, `TimelineView.interactions.test.tsx`, through the new
  `apps/desktop/src/test/tooltipHover.ts` helper.
- `apps/desktop/e2e/tooltip-press-dismiss.spec.ts` (rewritten for the hover
  contract), `e2e/room-space-invites.spec.ts` (rail bubble),
  `e2e/room-access-conditions.spec.ts` (two focus-opened bubbles).

Rows and rail items that carry a permanent `aria-describedby` description (the
access sentence) were left alone: they never depended on the bubble.

## Files

- `apps/desktop/src/components/Tooltip.tsx` (contract),
  `apps/desktop/src/test/tooltipHover.ts` (new test helper).
- The unit and e2e specs above.
- `docs/policies/engineering-rules.md` ("Popup Dismissal And Floating Surfaces"),
  `docs/help/rooms-and-spaces.md`.

## Gates

- `npx vitest run` in `apps/desktop`: the tooltip-related files are green. Eight
  failures remain in `katexVendor.test.ts`, `RoomCreationHistory.test.tsx` (an
  untracked work-in-progress spec), and the two `TimelineView` virtual-window
  specs; all are in files outside this change and reproduce from the pre-existing
  working tree, which already carries in-flight edits to `dialogs.tsx` and
  `TimelineView.tsx`.
- Playwright: `tooltip-press-dismiss`, `popup-outside-press-dismissal`,
  `tooltip-panel-stacking`, `room-access-conditions`, `room-space-invites`.

## Pending manual checks (confirmation only)

- macOS GUI: change a section's sort option and confirm no bubble remains; hover
  the `⋯` trigger and confirm the explanation still appears.
