# Space Members implementation ledger

Plan: `docs/superpowers/plans/2026-07-31-space-members-profile-cache.md`

## Task status

- [x] Task 1 — SDK explicit membership facts
- [ ] Task 2 — Rust state/core/profile/invite transitions
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
