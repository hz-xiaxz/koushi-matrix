# Issue #569 Unread Navigation and Thread Notifications

## Scope

Hide the unstable “Jump to first unread” control while preserving Rust-owned navigation state and in-timeline markers. Render each thread root's Rust-owned `notificationCount` in its existing summary-chip area. No new unread target, row scan, count derivation, Rust DTO/reducer change, or thread sorting change.

## Contract

- `TimelineView` never renders the first-unread jump button, even when `first_unread_event_id`, count, and position would previously enable it.
- Keep `navigationSnapshot` and `first_unread_event_id` consumption for the in-timeline unread marker; keep the normal “Read up to here” marker.
- Keep Jump to latest/bottom behavior unchanged. Remove only first-unread-only callback/code and stale local variables.
- For a thread root matching `threadAttention.rootEventId`, render a summary badge only when `threadAttention.notificationCount > 0`.
- Do not use `liveEventMarkerCount`, room-wide unread counts, visible-row scanning, latest-reply order, or React-local state as the displayed count.
- A zero notification count renders no badge even when `liveEventMarkerCount` or `highlightCount` is nonzero.
- Thread attention remains associated by root event ID, so reordering roots cannot move a count to another thread.
- Add localized English/Japanese copy describing notifications rather than “new replies”; existing unrelated copy remains compatible.

## Verify first

1. RED: a navigation snapshot that previously enabled first-unread renders no jump button while the unread/read markers remain.
2. Jump-to-bottom remains rendered and functional when independently eligible.
3. RED: notification count 3 with live marker 0 renders badge 3 for the matching root.
4. Notification count 0 with live marker/highlight nonzero renders no badge.
5. Two reordered roots retain notification badges by root ID.
6. Existing thread summary text, open-thread action, and zero-count behavior remain unchanged.
7. Focused browser-headless coverage pins the hidden control and Rust-projected badge source.

## Implementation

Delete the first-unread-only render predicate/button and now-unused `jumpToEvent` callback. Keep marker IDs. In `TimelineItemRow`, replace the badge count source with `notificationCount` and use one new catalog key for its label. No helper, new component, state, effect, DTO, or CSS.

## Gates

- `reviewer-flash-opencode-go` design verdict before implementation.
- `luna-implementer` at max thinking for verify-first implementation.
- Focused Vitest/browser-headless, typecheck, lint, and catalog checks.
- `reviewer-flash-opencode-go` exact full-diff verdict after implementation.
- Integrated full local matrix, CI, merge, issue evidence, and build-artifact cleanup in the shared PR.
