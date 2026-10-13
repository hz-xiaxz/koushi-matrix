# #1394: resident timeline-set export diagnostics

Status: implemented. Parent: [#1394](https://github.com/shinaoka/koushi-matrix/issues/1394)
(measured 0.23.0 resident memory). No behaviour change: the export observes, it
never retires, trims or evicts.

The measurement record closes with one instruction: land the counter for the
resident timeline actors *before* any bound, otherwise a bound's effect cannot be
verified. Its "Measurement gaps" section named two blockers — the detail-ring
`subscription_count_bucket` saturates at "21 or more", and no ring-independent
counter for live timeline actors or retained depth exists. This change lands
those counters as an export-time sample.

## What the counters are

`TimelineResidencyDiagnostics` (koushi-core `timeline::manager`) is computed by
`TimelineManagerActor::residency_diagnostics` from state the manager already
owns. It never awaits an actor:

- `session_resident_rooms` — the session-resident desired Room set.
- `actual_room_subscriptions` — the room subscriptions actually held by the
  SDK Sliding Sync session. It reads `actual_subscribed_rooms()`, not the
  logical `active_room_subscriptions()` set: session expiry can empty the actual
  map while the logical set is unchanged, and the existing coverage check already
  uses the actual map for that reason.
- `resident_actors` and the `room` / `thread` / `focused` split — the resident
  `timelines` map, which is the breadth axis of the record.
- `resident_event_items` and `deepest_actor_event_items` — the sum and maximum
  of each resident actor's position-evidence entry count. Position evidence
  carries one rank per distinct event-ID-backed canonical item, so this is the
  depth proxy for `navigation_items`; transaction-backed local echoes, date
  separators and receipt-only items are not part of it.

Sampling rides the #1344 export-time pattern: per live account tab, CoreRuntime
probe → `AccountMessage::TimelineResidencyDiagnostics` → the timeline manager's
control mailbox, with a `try_send` request and a bounded reply. A full mailbox
or a closed manager settles the row as unavailable instead of holding the
export. `apps/desktop/src-tauri` records one `core.timeline_residency stage=summary`
row per account next to `core.search_retention`.

## What the numbers do not say

- They are not bytes. Retained-byte accounting for an actor's canonical item list
  needs owner-side instrumentation that does not exist; the depth metric is item
  counts, which is what the record's own allocation inference approximated.
- `resident_event_items` is a snapshot of the position index, and a stalled or
  closed sample leaves the account row absent (the aggregate row still reports
  `account_available`).
- The records carry counts and closed tokens only. No Room, thread, event or user
  identifier and no message content.

## Residual

The bound itself is unchanged. `TimelineManagerActor::timelines` is still the
documented deliberately-unfixed unbounded map in
[engineering rules](../policies/engineering-rules.md), and the depth axis still
has no release path. Reopening that work now has the instrument it asked for.

The new sample is enqueued with `AccountActorHandle::try_send`, so a full account
mailbox settles the row as unavailable instead of holding AppActor's loop. The
older `#1344` search-retention handler keeps its timeout-wrapped mailbox wait; it
takes the same path in the same export and converting it is a separate change to
landed behaviour.

## Verification

- `cargo test -p koushi-core --lib residency` — the actor-kind/depth fold and the
  account-routing sample.
- `cargo test -p koushi-desktop --lib timeline_residency_summary` — the exported
  per-account row and its privacy.
- Rust lint gate, workspace tests and the frontend gates as required by
  [verification](../agents/verification.md).
