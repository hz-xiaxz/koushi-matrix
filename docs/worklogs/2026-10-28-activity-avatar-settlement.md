# Activity sender-avatar resolution and thumbnail settlement (#1641)

Consulted `REPOSITORY_RULES.md`, `AGENTS.md`, the Activity and Profiles/Avatars
contracts in `docs/architecture/state-machine.md`, the Activity ownership section
in `docs/agents/state-ownership.md`, and the verification guidance.

## Reproduction

Two headless checks reproduced the report at `main`:

- A Core Activity projection test showed self-authored rows kept
  `sender_avatar: None` even when `AppState.profile.own.avatar` had the same
  sender's image. Recent canonical rows and the room-latest fallback both failed.
- A Core runtime test dispatched `AvatarThumbnailUpdated(Ready)` after caching an
  Activity row with a `NotRequested` sender avatar. The projection kept the row
  copy `NotRequested`, because the AppState reducer cannot reach avatars copied
  into the AppActor's cache.
- A renderer test rendered an Activity event row with a `NotRequested` avatar
  and a demand callback; no request was submitted. The App also did not pass the
  existing typed demand bridge to `ActivityPane`.

## Fix

Activity sender avatars now resolve from Rust-owned profile state first: the own
profile for self-authored rows, then relevant-room profiles, then the global
profile cache, and finally the row's copied avatar. The projection records
exact-MXC thumbnail updates for avatars its cached rows reference and settles a
row copy from that record, including the `NotRequested` downgrade that retires
evicted bytes; the record is pruned to references that still exist. `ActivityPane` now forwards visible `notRequested` avatars to
the same typed thumbnail-demand callback used by sidebar and member surfaces.

The state-machine and state-ownership canon were amended for the Activity-row
copy settlement; no DTO, command, or wire shape changed.

## Verification

RED then GREEN, focused:

- `cargo test -p koushi-core --lib runtime::activity`
- `cargo test -p koushi-core --lib avatar_thumbnail_updated_settles_activity_sender_avatar_copies`
- `npx vitest run src/components/panes.test.tsx src/domain/avatarThumbnails.test.ts`
- `npx vitest run src/App.test.tsx`
- `npx playwright test e2e/activity-avatars.spec.ts e2e/activity-resolution-states.spec.ts`
- `pnpm typecheck`, `cargo fmt --check`, and
  `cargo clippy -p koushi-core --lib -- -D warnings`
