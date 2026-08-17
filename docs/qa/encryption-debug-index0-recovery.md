# Manual index-0 recovery QA

The `encryption_debug` core-only lane is a temporary diagnostic check for
issue #541. It uses a disposable Tuwunel/Synapse account and performs the
#538 fresh-session/index-0 checks before advancing the same outbound session
and invoking the one-shot resend.

Run through the local runner:

```bash
node scripts/desktop-headless-local-qa.mjs --run \
  --server=tuwunel --scenario=encryption_debug --core \
  --timeout-ms=600000
```

Required private-data-free stdout tokens:

- `encryption_debug_room=ok`
- `encryption_debug_recipient=ok`
- `force_new_outbound_session=ok`
- `share_index0_room_key=ok`
- `index0_not_consumed=ok`
- `encryption_debug_index_advanced=ok`
- `resend_index0_room_key=ok`
- `resend_index_unchanged=ok`
- `encryption_debug=ok`

The resend stage additionally verifies the latest `core.room_key_debug`
record: `outcome=completed`, outbound `index_before > 0`,
`index_before == index_after`, `inbound_first_known_index=0`,
`peer_accepted + peer_missing == peer_eligible`, `claim` is `not_needed` or
`succeeded`, `elapsed_ms > 0`, `room_event_sent=0`, and `index0_consumed=0`.

The A2 SAS verification prerequisite must settle before the resend stage. A
failure before `encryption_debug_recipient=ok` is a prerequisite/harness
failure, not evidence about the resend implementation; do not report the
resend as tested in that case. No room/user/device identifiers or key material
may be copied into QA output or artifacts.
