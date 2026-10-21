# Thread posting and reply timestamps — #1556 follow-up

The user selected the “separate posting and reply” mockup. An old root moved
to its latest reply position must make the two dates explicit, without changing
Rust-owned event identity, ordering, or thread actions.

## Contracts and upstream inspection

Consulted REPOSITORY_RULES.md (ownership, localization, verification, review),
docs/architecture/i18n.md, docs/agents/verification.md and engineering rules
(product text, design simplicity and required gates). The approved display
contract is recorded in the i18n canon before the renderer change.

- Element Web: `packages/shared-components/src/message-body/MessageTimestampView/MessageTimestampView.tsx`
  consumes formatted short/full timestamps from its view model and presents a
  keyboard-accessible full-date tooltip; optional sent/received labels distinguish
  different meanings. No new Matrix command is needed for timestamp presentation.
- Element X Android: `TimelineEventTimestampView.kt` renders `event.sentTime`
  with edit/send-state indicators. Actions concern shields and failed sends.
- Element X iOS: `EventBasedTimelineItemProtocol.swift` builds `localizedSendInfo`
  from `timestamp.formattedTime()` and an optional edited prefix.

Intentional divergence: Koushi already supports relocating thread roots using
Rust's separate content/activity identities. A relocated root in a room shows
an absolute local date, weekday and original time in a `Posted` line above the
body. Its reply summary shows `Latest reply` below the existing reply button.
Thread/focused surfaces and ordinary message headings retain their current
formatting. The shared timestamp renderer gives both lines the same full-date
tooltip, midnight refresh and locale subscription as ordinary message times.
All shipped catalogs include the new labels; no setting or DTO is added.

## Verification

- RED: `ThreadTimestamps.test.tsx` failed three assertions before implementation
  (separate posting line, same-day/year display, Japanese labels).
- GREEN: focused timestamp, metadata, thread-view and catalog tests: 89 passed.
- Final local gates and CI results are recorded in the PR.

The help page describes the two timestamps. No settings location changes.
