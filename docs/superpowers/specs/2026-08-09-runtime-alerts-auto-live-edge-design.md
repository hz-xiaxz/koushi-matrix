# Runtime Alerts and Automatic Live-Edge Return Design

## Goal

Move user-visible runtime warnings into the existing homeserver/session status control, make privacy-safe diagnostics directly copyable from that control, and automatically leave a focused timeline when it has provably reached the room's live edge.

## Current Problems

The Secure Backup runtime warning is rendered as an extra direct child of the two-row `.desktop` grid. When it appears, the banner, title bar, and application grid compete for two declared rows and the banner overlaps normal UI.

The main timeline uses a focused timeline after Activity, search, date, or permalink navigation. It renders “Jump to latest message” whenever `main_timeline_anchor` exists, even when the focused timeline already ends at the room's latest display event. The label therefore claims that newer content exists when the user is already at the latest message.

The diagnostics dialog can copy its report, but a user who opens the status popover to investigate a runtime warning must first open another dialog before copying the same information.

## Runtime Alert Model

`App` derives a small presentation-only array of runtime alerts from existing authoritative state. The initial alert sources are:

- Secure Backup runtime degradation after the normal shell has already been exposed.
- Sliding Sync states that are failed or reconnecting.
- Current-session inspection failure.

Each alert has a stable kind, warning or error severity, a localized title and detail, and only the actions supported by that source. The model remains presentation-only; it does not duplicate lifecycle state or add persistence.

The status trigger beside the homeserver and sync label displays one warning indicator. Its color reflects the highest current severity and its accessible label includes the number of active alerts. A healthy state has no warning indicator.

The existing session-status popover gains a “Runtime warnings” section. It lists every current alert, including its detail. Secure Backup degradation offers its existing retry action. The existing full-screen runtime banner is removed. The existing rule that blocks composing in an encrypted room while Secure Backup is degraded remains unchanged.

## Diagnostic Actions

The status popover exposes both:

- “Copy diagnostics”, which fetches the current runtime diagnostic snapshot and copies the same privacy-safe report generated for the diagnostics dialog.
- “Show diagnostics”, which opens the existing diagnostics dialog.

Report construction is centralized in `App`, so opening and copying cannot drift. Copy has `idle`, `copying`, `copied`, and `failed` UI feedback. A later attempt clears an earlier failure. Clipboard failure does not close the status popover and does not expose raw exception text.

## Automatic Return to the Live Timeline

The focused timeline may leave focused mode only when all of the following are true:

1. The main timeline is currently anchored.
2. The viewport is at the focused timeline's bottom, using the existing tolerant bottom/latest-visible calculation.
3. The focused timeline's latest readable display event ID is known.
4. The active room's authoritative latest display event ID is known.
5. Those display event IDs are equal.

For ordinary messages, the room summary event ID is the display event ID. For edits and annotations, the relation target is the display event ID. The TypeScript DTO exposes the already-serialized optional `relation_type` and `relation_event_id` fields so this mapping is explicit.

`TimelineView` reports the proven condition once through `onReturnToLive`. An in-flight guard prevents repeated `closeFocusedContext()` calls while the state transition is settling. Switching rooms or changing the anchor resets the guard. If either event ID is unavailable, differs, or the viewport is above the bottom, focused mode and the explicit return button remain.

This is deliberately UI-owned. The necessary authoritative room summary already exists, and adding a new backend command or persistent state would duplicate information without improving correctness.

## Accessibility and Layout

The warning indicator is not color-only: it uses an alert icon and an accessible count label. The popover remains keyboard reachable, closes on Escape or outside click, and reports copy completion through a polite live region. Runtime warnings live inside the existing popover and therefore add no row to the root application grid.

## Error Handling

- Secure Backup retry retains the existing in-flight guard and disabled state.
- Diagnostic snapshot or clipboard failure changes only the copy action to a localized failure state; users can retry or open diagnostics.
- Automatic live return failures leave the focused timeline in place. They do not hide the explicit return control.
- Unknown runtime state is not promoted to a warning unless the existing state machine classifies it as failed, reconnecting, or degraded.

## Testing

Component tests cover:

- Healthy status trigger with no warning indicator.
- Multiple alerts with highest-severity indicator and details in the popover.
- Secure Backup retry from the popover and removal of the root runtime banner.
- Successful and failed direct diagnostic copy.
- Focused timeline auto-return only when bottom and authoritative latest display IDs match.
- No auto-return above the bottom, with unknown IDs, or with different IDs.
- Relation events compare using their display target.
- The explicit return button remains for a genuinely historical focused window.

Existing Shell and TimelineView suites remain the regression boundary. Desktop type checking and the focused application tests verify integration.

## Out of Scope

- Changing Secure Backup policy or encrypted-send admission.
- Persisting alert history.
- Adding operating-system notifications for runtime warnings.
- Replacing the existing diagnostics dialog.
- A new backend live-edge API.
