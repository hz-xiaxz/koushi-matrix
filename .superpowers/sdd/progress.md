# Space Members implementation ledger

Plan: `docs/superpowers/plans/2026-07-31-space-members-profile-cache.md`

## Task status

- [x] Task 1 — SDK explicit membership facts
- [x] Task 2 — Rust state/core/profile/invite transitions
- [ ] Task 3 — Tauri/Desktop transport
- [ ] Task 4 — approved Space Members UI
- [ ] Task 5 — integrated review, verification, and DMG

## Task 1 evidence

- Added `MatrixSpaceMembersProjection` with separate local `JOIN` and `INVITE`
  Space sets and a deduplicated child-room `JOIN` union minus both sets.
- Retained contributing child room IDs, local member profiles, and complete /
  incomplete child-room counts; no per-user network lookup is performed.
- Added private-data-free `sdk.space_members_scope` diagnostics and focused
  source/privacy guards.
- Pinned SDK adaptation: the vendored SDK exposes local member reads through
  `Room::members_no_sync(RoomMemberships::...)`; child rooms are read from
  local `m.space.child` state events and completeness from
  `Room::are_members_synced()`.
- Evidence: `cargo test -p koushi-sdk space_member --lib -- --nocapture`
  (5 passed), `cargo check -p koushi-sdk --lib`,
  `cargo fmt --all -- --check`, and `git diff --check` all passed.

## Task 2 evidence

- Added Rust-owned `SpaceMembersState` to `AppState`, including selected Space,
  generation fence, joined/invited/child-room-only sections, completeness
  counts, sanitized operation state, stable entry labels, avatars, roles,
  contributing child rooms, and per-entry invite-pending state.
- Added reducer transitions for load, stale-generation discard, duplicate-safe
  invite requests, authoritative JOIN/INVITE reconciliation, and failure
  rollback to child-room-only.
- Added the profile precedence resolver: local alias, relevant room, Space room,
  payload, account `ProfileState.users` cache, local homeserver input, then
  `Unknown user`.
- Space/child load now observes non-empty local member profiles into
  `ProfileState.users` by reducing `UserProfilesUpdated` before
  `SpaceMembersLoaded`; live read-receipt projections refresh from that cache.
  This is covered by the child-profile load-path test and the existing-receipt
  Seen fallback test.
- Added core load/invite commands and actor handlers using the Task 1 local SDK
  projection and existing invite primitive; no per-person network profile
  fan-out or plaintext profile store was introduced.
- Added `core.space_members_projection` and `core.profile_resolution` with
  count/token/bool fields only. The privacy test serializes diagnostics and
  rejects Matrix IDs, names, MXC URLs, and raw profile content. Profile source
  resolution remains state-owned, so the core boundary marks those detailed
  cache/source counters as deferred rather than duplicating AppState.
- Evidence:
  - `cargo fmt --all` — passed (stable rustfmt emitted warnings for unsupported
    nightly-only formatting options).
  - `cargo test -p koushi-state --test space_members_state -- --nocapture` —
    5 passed.
  - `cargo test -p koushi-state --test profile_state -- --nocapture` — 26
    passed.
  - `cargo test -p koushi-core space_members --lib -- --nocapture` — 2 passed.
  - `cargo check -p koushi-state` — passed.
  - `cargo check -p koushi-core --lib` — passed; only pre-existing
    `media_preparation` / `read_state` warnings remain.
  - `cargo fmt --all -- --check` — passed.
  - `git diff --check` — passed.

## Decisions and invariants

- All implementation workers use Luna (`gpt-5.6-luna`) with reasoning effort `max`.
- Space joined means Matrix JOIN only.
- Space invited means Matrix INVITE only.
- Child-only is the deduplicated JOIN union of all child rooms minus both Space sets.
- React renders the Rust-owned projection and does not classify membership.
- Existing encrypted Matrix SDK state is the durable profile/member source.
- No per-person network profile fan-out and no plaintext profile database.
- Diagnostics contain no raw IDs, labels, URLs, content, secrets, or raw errors.
- Work remains local; no push or PR.

## Handoff notes

- The branch already contains a committed design spec.
- Three previously verified diagnostics files may be staged before implementation; preserve them.
