# Progressive Room-List Connectivity Design

Date: 2026-08-04
Status: Approved in conversation

## Evidence

After migrating the legacy to-device token, the affected account receives a
committed Sliding Sync response with a position, projects rooms progressively
from 5 to 103 plus 3 spaces, and can send and receive messages. Ten seconds
after the first response, `SyncActor` reports `sync_failed_internal` solely
because the complete all-rooms range has not acknowledged reconciliation.

## Decision

Separate network liveness from complete-range authority:

- A committed `all_rooms` response with `pos` is connectivity evidence only
  after `RoomActor` has successfully projected the response-correlated live
  snapshot into AppState.
- That first projection may remain provisional. It must not authorize removal
  of cache-only rooms that have not yet appeared in the growing range.
- Complete-range reconciliation independently promotes the live room list to
  authoritative and enables absence/removal decisions.
- A fixed timeout while the range is making progress must never stop
  `SyncService`, encryption sync, timelines, or message send/receive.
- Closed actor channels, stopped observers, malformed correlation, and failed
  AppState delivery remain internal failures.

The room acknowledgement contract therefore distinguishes `Projected` from
`Reconciled`. `SyncActor` enters Running on a matching `Projected` or
`Reconciled` acknowledgement. `RoomActor` continues reconciliation after
`Projected` and emits authoritative projections only when the complete range
is available.

## Alternatives Rejected

- Extending the ten-second timeout remains dependent on account size and
  network speed.
- Remaining in Starting after usable committed data is projected misrepresents
  a working connection.
- Marking the first partial range authoritative can incorrectly remove rooms
  that are merely outside the current growing window.

## Diagnostics And Tests

Diagnostics separately expose first-projection acknowledgement, full-range
authority, reconciliation progress, and failure reason without room IDs or
tokens. Regression tests cover a partial committed response followed by a
range that completes after the former timeout: lifecycle becomes Running,
projection remains provisional while partial, message/timeline owners remain
active, and the later complete range becomes authoritative.
