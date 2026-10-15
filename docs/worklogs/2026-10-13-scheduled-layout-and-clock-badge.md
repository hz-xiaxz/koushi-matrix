# Scheduled-message surfaces: layout and the composer clock badge (2026-10-13)

The user reported two things from screenshots of the running app: the scheduled
message UI was visibly broken (the right panel's card spread its few rows across
the whole pane, and the item's preview or edit form was crushed into a sliver),
and the composer's send-later clock gave no sign that the room already had a
message waiting.

Canon consulted: `REPOSITORY_RULES.md` ("Root-Cause Fix Discipline",
"User-Facing Text And Localization"), `docs/policies/engineering-rules.md`
("Right Panel And Composer Containment"), `docs/architecture/i18n.md` (catalog
IDs, logical CSS properties, remote text), and `docs/agents/verification.md`. No
upstream behavior is imitated: this is this client's own composer and panel.

## Cause

Two independent CSS faults, both in the #1160 scheduled-message surface:

- `.scheduled-messages` is the middle `minmax(0, 1fr)` row of the shared panel
  shell, so it was stretched to the pane height. A grid container with
  `align-content: normal` stretches its `auto` rows, so the heading, the
  local-fallback note, and the item were spread across the whole pane: measured
  511→786px of card for ~90px of content, with 100px+ of dead space between rows.
- `.scheduled-message-item` declared `grid-template-columns: minmax(0, 1fr) auto`.
  The implicit `auto` action column took the buttons' intrinsic width, so in the
  320px panel the preview wrapped a few characters per line, and an opened edit
  form (a full composer plus time field and adjustment row) was squeezed into
  that column while the preview column collapsed. Engineering rules "Right Panel
  And Composer Containment" already prohibits the implicit `auto` track.

The clock had no count because nothing projected the room's reservation queue
into the composer; `state.ui.timeline.scheduled_sends` already carries it for the
selected room (Rust `refresh_timeline_scheduled_sends`).

## Change

- `apps/desktop/src/styles.css`:
  - `.scheduled-messages` gains `align-content: start`; the panel-scoped
    `.thread-pane > .scheduled-messages` hugs its content (`align-self: start`,
    capped by `max-block-size: 100%`) and scrolls inside the card when the queue
    is longer than the pane row.
  - `.scheduled-message-item` becomes one explicit `minmax(0, 1fr)` track, so the
    preview, the action row, and the edit form stack in reading order at every
    width. `--scheduled-*` gaps are unchanged.
  - `.composer-schedule-button` / `.composer-schedule-badge`: the send-later clock
    badge, using the same vocabulary as the room header's thread count and the
    workspace header badge (logical `inset-inline-end`, existing
    `--nav-badge-*` metrics).
- `apps/desktop/src/components/composer.tsx`: one optional `scheduledSendCount`
  prop (default 0). A non-zero count renders the badge and joins the clock's
  accessible name and tooltip; the badge itself is `aria-hidden`. No behavior
  change when the count is zero.
- `apps/desktop/src/components/panes.tsx`: the room composer receives
  `snapshot.state.ui.timeline.scheduled_sends.length`.
- `apps/desktop/src/i18n/messages.ts`: `scheduled.sendLaterWithCount` in `en`
  ("Send later ({count} scheduled)") and `ja`
  ("あとで送信（予約 {count} 件）"); the pseudo catalog derives from `en`.

## Verification

- RED then GREEN, `apps/desktop/e2e/scheduled-messages-layout.spec.ts` (new):
  five cases over the real harness — panel row rhythm, containment at the 320px
  panel floor idle and while editing (including a real Edit click, a real Save
  click dispatching `reschedule_scheduled_send`, and a real close hit-test), a
  12-item queue capped and scrolled inside the panel, the timeline card's edit
  form spanning its item, and the clock badge appearing, updating, and retiring.
  With the CSS reverted, 3 of the 5 fail (row rhythm, containment, inline edit
  width); the other 2 are forward guards for the scroll cap and the badge.
- The fix is CSS layout on the macOS WKWebView and Linux WebKitGTK engine family
  too, so the spec is opted into the WebKit project in
  `apps/desktop/playwright.config.ts`: 5/5 on Chromium and 5/5 on WebKit
  (including the grid item's `max-block-size: 100%` scroll cap).
- `apps/desktop/src/components/composer.test.tsx`: "badges the send-later clock
  with this room's pending reservation count" (count 3 → badge, `data-count`, and
  the counted accessible name; zero → plain label, no badge).
- `apps/desktop/src/i18n/messages.test.ts` and
  `apps/desktop/src/components/scheduledMessagesPanel.test.tsx` cover the new
  catalog ID's completeness and the panel's rendering; both pass.
- Full Playwright suite on a private port: 556 passed. The 4 failures
  (`room-creation-history`, `workspace-rail-indicators`) are another session's
  in-progress specs, unrelated to these files.
- `tsc --noEmit` and ESLint clean for the touched files;
  `scripts/check-ime-text-inputs.mjs`, `scripts/check-frontend-semantic-owners.mjs`,
  `scripts/check-agents-docs.mjs`, and `src/styles.contract.test.ts` pass.
- Pre-existing, reproduced at pristine `HEAD` in a temporary worktree:
  `katexVendor` (installed KaTeX reports 0.18.1 against vendored 0.18.10) and
  `TimelineView.threads` ("falls back to the virtual height model…").

No `docs/help/` change: the guide describes creating and listing reservations,
and neither the steps nor the labels changed. The badge reports the selected
room's queue, not the account-wide panel projection; that is the count the room's
own composer can honestly show.
