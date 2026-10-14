# Verified unread counts and read delivery (2026-10-11)

This change follows the unread, thread, Activity, and native attention contracts in
`docs/architecture/state-machine.md` and the user-facing behavior in
`docs/help/rooms-and-spaces.md`.

The numeric badge now comes from SDK-observed main-timeline unread plus tracked
per-root thread unread. Room, Space, Home, account, thread, and OS badges use
that decomposition; mute and low-priority exclude only aggregates. Invitations
remain separate. Activity Unread shows room-level summaries and cannot claim an
event ID from a count. Recent may hydrate older history, but its failure does
not hide verified Unread entries. Native banners carry a room-level count and
no unverified event preview or target. A settled visible thread reply advances
the threaded receipt without requiring a second scroll. A room context-menu
read requires a known main-timeline target and never claims to read threads.

Independent read-only review found two integration risks: Activity's history
loading status appeared in Unread, and old per-root counts could survive a
room leave/rejoin. Both paths were corrected and covered by renderer and
state/core regression tests. Review also confirmed that a room-wide threaded
mark-all lacks safe per-root event targets, so the existing action remains
explicitly scoped to the main timeline.
The final leave review found an in-flight actor could repopulate the shared
thread cache after a membership-left observation; leaving now quiesces that
room's timeline actors before forgetting their counts and signatures. A
membership-left observation also removes room residency, and queued selection
commands cannot recreate a timeline actor until the room rejoins.
The core-testkit Activity integration suite was updated to assert the verified
room-level Unread view, including the absence of inferred event IDs and
synthetic mark-read targets.
The desktop wire-shape golden was regenerated for the explicit main and thread
unread fields and the SDK-based display count.

Verification: core and state Rust suites, SDK and desktop library suites,
desktop typecheck/lint/build, frontend unit tests, relevant Playwright
scenarios, rustfmt, Clippy, SDK submodule check, and user-help check. The full
Playwright run exposed six expectations for the old unread display; the seven
affected specs passed after updating those expectations.
