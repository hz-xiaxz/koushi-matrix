# Issue #1462: late-decrypted in-room verification requests

## Failure

Nightly focused headless QA, `user_verification` on Synapse, stage
`user_verification B incoming request`:

```text
a_send=finished a_room_list_joined_members=2 b_dm_room_listed_at_send=true
b_dm_joined_members_at_send=2 b_dm_timeline=no_undecryptable_item b_flow=idle
b_target_matched=false b_sync=running
```

This is the same signature that #1323 diagnosed on Tuwunel (see
`2026-10-11-issues-1322-1325-1327-1326-1323.md`): A really sent the request,
B's timeline eventually shows it as decrypted, and B's verification flow never
leaves `idle`. Focused-lane history shows it as an intermittent product gap,
not a regression: five earlier Tuwunel reds since the #1285 observer fence,
green runs in between, and this first Synapse red.

## Root cause

The room key (on the `encryption` Sliding Sync connection) and the request
event (on the `all_rooms` connection) race. When the event wins, the first
decryption is a UTD. Two things then lose the request:

1. The SDK hands an in-room verification event to the verification machine
   only from the successful first-pass decryption in
   `matrix-sdk-base/src/response_processors/e2ee/decrypt.rs`. The event-cache
   redecryptor (R2D2) later decrypts it for display only, so the machine never
   learns about the request.
2. Koushi observed in-room requests only through a room event handler. Event
   handlers do not run for redecrypted events.

The #1323 worklog refused a fork change at the time. With both halves of the
gap identified, the fix is a small, upstreamable SDK bug fix plus a Koushi
observer change, which REPOSITORY_RULES permits for the vendored SDK.

## Fix

- SDK fork, branch `fix/1462-late-decrypted-verification`:
  `retry_decryption_for_events` feeds resolved in-room verification events to
  `receive_verification_event` (same selection as sync's
  `process_if_relevant`, honouring `handle_verification_events`) before it
  publishes `RedecryptorReport::ResolvedUtds`. Test:
  `test_redecryptor_feeds_late_decrypted_verification_request` (RED before the
  change: the request stayed unknown to the machine).
- `koushi-sdk` `observe_incoming_verification_requests` adds a task on the
  event cache's decryption reports. For each resolved event that is a decrypted
  original `m.room.message` verification request, it looks the request up and
  forwards it through the same channel as the other two paths. The task is
  aborted with the observer. Test:
  `crates/koushi-sdk/tests/late_decrypted_verification_request.rs` delivers the
  request before its room key and requires the observer to surface it; it is
  RED without either half.
- `docs/agents/qa-lanes.md` no longer describes the scenario as intermittently
  red by design.

No timeout, retry, or scenario ordering change.

## Verification

- SDK: `cargo test -p matrix-sdk --lib event_cache` 118 passed; the
  `integration` verification tests 5 passed.
- `cargo test -p koushi-sdk --features test-hooks` all green; clippy
  `-D warnings` clean.
- Local headless QA `user_verification --core --cargo-profile=ci`: before the
  fix 1/1 green on Synapse (the race did not fire locally); after the fix 5/5
  green on Synapse and 3/3 green on Tuwunel. Local green runs cannot prove the
  race was hit, so the deterministic proof is the two RED-first tests above.

The fork commit must be pushed to `shinaoka/matrix-rust-sdk-work` before CI can
check out the new gitlink.
