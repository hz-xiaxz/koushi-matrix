# Room-key rotation correlation diagnostics

## Goal

Determine whether a full member reload is causing an outbound Megolm session
to be discarded and replaced, without recording Matrix identifiers, device
identifiers, key material, message content, or raw errors.

## Scope

- Preserve process-local provenance from the first member invalidation until
  the next successful full `/members` reload.
- Emit one typed member-reload diagnostic containing closed reasons, bounded
  count buckets, request/processing timings, and the discard outcome.
- Reuse the existing anonymous room alias so a later rotation can be correlated
  with the reload.
- Record elapsed time from an authoritative discard to replacement-session
  creation.
- Add Koushi detail records and aggregate counters for user-exported
  diagnostics.

This is observation-only. It does not change member loading, Megolm rotation,
key sharing, retry, or trust policy.

## Privacy boundary

The diagnostic contract permits only anonymous ordinal aliases, closed enums,
booleans, bounded count buckets, and elapsed milliseconds. It must not expose
room/user/device IDs, session IDs, keys, event content, URLs, or raw errors.

## Verification

- Unit-test process-local invalidation provenance.
- Unit-test anonymous correlation between member reload and later rotation.
- Unit-test Koushi serialization and privacy-safe fields.
- Run focused SDK and Koushi tests, formatting, submodule-pin validation, and
  `git diff --check`.
