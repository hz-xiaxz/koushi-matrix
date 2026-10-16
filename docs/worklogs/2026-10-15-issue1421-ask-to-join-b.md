# #1421 Ask to join (knock) — B: create choice and review surface

Canon consulted: `REPOSITORY_RULES.md` ("Canon-First Change Protocol",
"Product Logic Stays In Rust", "State-Machine Discipline"), `AGENTS.md`, and the
#1177 access/history contract in `docs/architecture/state-machine.md`. Design:
`docs/plans/2026-10-15-issue1421-ask-to-join.md`.

## What landed

- **Rust-owned availability** (`koushi-sdk`, `koushi-state`): a `knock` create
  pins room version V10 (knocking needs v7+, knock_restricted v10+), so the
  availability is a decision about the version Core creates, not a server probe.
  `CreateRoomAccessPreview` reports `knock_available` and
  `knock_restricted_available` (the latter requires the attachment Space that
  carries the allow list); `room_version_pinned` is true for a knock create.
- **Create dialog** (`apps/desktop`): renders "Ask to join" at Home and the
  Space-member knock route inside a Space, only when the Rust preview reports it
  available. en/ja catalog entries added.
- **Review surface** (`koushi-state`, `koushi-sdk`, `apps/desktop`):
  `RoomMemberMembership::Knocked`; the SDK maps `MembershipState::Knock` and the
  member query includes KNOCK; the room people panel lists pending requests in a
  "Join requests" section with Accept (`inviteUser`) and Decline (kick), gated on
  the room's invite/kick permissions.
- `state-machine.md` records the Knocked membership and the two existing commands
  that resolve it.

## Verification

- `cargo test -p koushi-sdk --lib` (277), `-p koushi-state --lib` (88),
  `-p koushi-core --lib` (1615), `-p koushi-qa --features qa-bin` (124).
- `cargo fmt --check`; clippy `-D warnings` for the touched crates.
- `npx tsc --noEmit`; `npx vitest run` (2035); `npm --prefix apps/desktop run
  lint`; `check-agents-docs`; `check-headless-qa-lanes`.
- New tests: SDK knock create pins V10; create preview reports both availability
  facts; the dialog renders the knock choice only when available; the people
  panel resolves a knocked request with both actions.

## Independent review

Pre- and post-implementation reviews on `gpt-6.1-sol`. The pre-review's blocker
about availability came from a pinned version not proving the server supports
it; B answers it by pinning V10 and reporting a typed failure if the server
cannot create that room, and by making the availability a Rust fact the renderer
only displays. The post-review findings are recorded in the PR.

## Not covered

- An encrypted knock room and a real `knock_restricted` Space create are not in
  the QA yet.
- The knock request's free-text reason is not shown; the requester label only.
