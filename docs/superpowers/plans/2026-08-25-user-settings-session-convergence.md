# User Settings Session Convergence

## Problem

User Settings mixed three Rust-owned projections without explaining their
different scopes. `current_session_status` described the active Koushi session,
`device_sessions` described homeserver sessions, and `e2ee_trust.devices` was
rendered as though it were a complete device inventory. An empty trust
projection therefore appeared as the false statement `0 devices`. Account and
session controls were also separated across the panel, and a fail-open login
discovery left the previously delivered Manage account action invisible for the
rest of a restored session.

## Delivery

1. Add component tests which reproduce the contradictory status and empty
   trust-device projection.
2. Use `current_session_status` for active-session verification,
   cross-signing, identity, and key-backup labels. Keep `e2ee_trust` as the
   command and operation-state owner, but do not present its device array as a
   complete server device count.
3. Group Session, saved Accounts, homeserver Sessions, Account management, and
   Encryption immediately after Profile. Point the Settings navigation entry
   at that group.
4. Keep Manage account visible but disabled when discovery has no safe URL, and
   retry authoritative login/account metadata discovery once after an
   authenticated or restored session becomes ready.
5. Run focused UI tests, the desktop typecheck/lint/build, and install the
   resulting macOS app for manual verification.

## Boundaries

- React does not infer SDK trust state or construct an account URL.
- Device/session IDs remain Rust-owned; destructive session actions continue
  to use the existing ordinal-based command surface.
- The authenticated rediscovery is bounded to one attempt per active account
  identity and preserves fail-open startup behavior.

