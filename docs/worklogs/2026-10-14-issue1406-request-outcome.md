# Issue #1406: existing-DM request outcome

## Finding

`StartDirectMessage` reuses an existing joined DM without changing the room
projection. The Core request-outcome waiter previously required a `StateDelta`
generation newer than the command baseline even after it received the matching
`DirectMessageStarted` event. A synthetic Core connection with the existing DM
already projected timed out with no unrelated state change.

The diagnostics excerpt in #1406 is consistent with this mechanism, but the raw
export was not available for this change. It cannot establish when the waiter
first observed the event or whether snapshot-priority polling also contributed
to the reported 11–29-second delays.

## Contract and verification

The waiter now admits baseline-generation progress for `DirectMessageStarted`
only after the matching request event. Settlement still requires the same
account and the event room in the current snapshot. A new DM waits for its room
projection; an event alone or an existing room without its event cannot settle.

- RED: `existing_dm_start_settles_without_an_unrelated_state_delta` returned
  `Err(TimedOut)` before the condition changed.
- GREEN: the same test passes after the change. Focused tests also cover a new
  room projection and no event, wrong request, wrong room, wrong account, and an
  older snapshot.
- `cargo test -p koushi-core --lib --locked`: 1,544 passed, 10 ignored.
- SDK existing-DM reuse test, Core formatting, `git diff --check`, and agent
  documentation check passed.

Read-only design and integrated-diff reviews found no blocking correlation or
concurrency issue in the Core settlement change. The reviews also noted that
the UI's silent retry and missing build version in diagnostics remain separate
acceptance items. A durable pending indicator for the profile action needs
Rust-owned operation state; a React-only busy flag would violate the state
ownership contract. Issue #1406 should remain open while those items remain.
