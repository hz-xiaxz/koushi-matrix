# #1421 Ask to join (knock) — A1 headless contract

Canon consulted: `REPOSITORY_RULES.md` ("Canon-First Change Protocol",
"Architecture And Ownership", "QA Gates And Cleanup", "Root-Cause Fix
Discipline"), `AGENTS.md`, engineering rules "Build, Dependencies, QA Gates".
Design and staged plan: `docs/plans/2026-10-15-issue1421-ask-to-join.md`.

## Scope

This delivers A1 of #1421: the headless Rust contract and a QA check, with no
user-visible change. B (the create-dialog choice, the server-capabilities
availability read, and the Room Info knock-request review surface) is a
follow-up; the create choice stays dark until it lands, per the issue.

## What landed

- **`knock_restricted` create** (`koushi-sdk/src/room_projection.rs`,
  `room_operations.rs`): `sdk_join_rule_for_access_policy` builds
  `JoinRule::KnockRestricted` from a canonical membership allow list. The create
  request rejects an empty allow list and pins a version that supports it (V10),
  exactly as `restricted` pins V9. Unit tests cover the join-rule content, the
  version pin and the rejection.
- **Create preview consistency** (`koushi-state/src/state/room_management.rs`):
  `preview_create_room_access` rejects public+knock_restricted and an empty allow
  list; the create draft seeds the attachment Space for the knock_restricted
  choice, not only `restricted`. Unit tests cover both.
- **Renderer allow list** (`apps/desktop/src/App.tsx`): the create request sends
  the allow list for any rule that carries one, so a knock_restricted create no
  longer drops its targets.
- **Preset precedence** (`koushi-sdk/src/room_operations.rs`): an explicit access
  policy no longer carries the legacy `private_chat` preset, which won over the
  explicit `m.room.join_rules` on the homeserver and turned a Home knock create
  into invite. Unit-tested.
- **QA** (`crates/koushi-qa/.../scenarios/rooms/knock.rs`): the `room_management`
  stage creates a knock room through `CoreCommand::Room(CreateRoom)` and has a
  second, SDK-only account knock it. The accepted knock request is the
  server-backed proof that the room admits knocking; tokens
  `knock_room_created=ok` and `knock_join_request=ok`.

`RoomAccessPolicy::is_submittable` and `sdk_join_rule_for_update` are
deliberately unchanged, so an existing-room knock_restricted edit stays rejected;
only the create path is admitted.

## Independent review

Pre-implementation review (gpt-6.1-sol, read-only) raised two blockers that
shaped this work — the React submission dropped knock_restricted allow targets,
and deriving availability from a pinned version is circular (it must read the
server's room-version capabilities). The first is fixed here; the second is
carried into B. Its "keep create admission separate from existing-room edits"
point is followed by leaving `is_submittable` alone.

## Verification

- `node scripts/desktop-headless-local-qa.mjs --run --server=tuwunel --core
  --scenario=room_management` — exit 0, knock tokens present.
- Same with `--server=synapse` — exit 0, knock tokens present.
- `cargo test -p koushi-sdk --lib` (277), `-p koushi-state --lib` (88),
  `-p koushi-core --lib` (1614), `cargo test -p koushi-qa --features qa-bin`.
- `cargo fmt --check`, clippy `-D warnings` for the touched crates,
  `npx tsc --noEmit`, `npx vitest run`, `npm --prefix apps/desktop run lint`.

## Not covered

- B: server-capabilities availability, the create-dialog choice, and the Room
  Info knock-request review surface (Knocked membership, accept/deny, reason).
- An encrypted knock room and a knock_restricted Space create are not exercised
  by the QA yet.

## Post-implementation review follow-ups

The post-implementation independent review (gpt-6.1-sol) raised no blocker. Its
actionable findings were applied:

- The QA check now always leaves the disposable room and removes the knocker's
  temporary device (`cleanup_qa_auditor_device`), even when the knock check
  fails, and closes the session stores.
- `knock_room_created=ok` and `knock_join_request=ok` are registered as required
  tokens for the `room_management` scenario, and `docs/agents/qa-lanes.md`
  records the check.
- The normative create contract in `docs/architecture/state-machine.md` records
  the create-only `knock_restricted` policy, its V10 pin, the attachment-Space
  seeding, and the explicit-policy preset rule.
- The `private_chat` preset comment now states the observed local-server
  behaviour instead of asserting a general precedence.
- Left as-is: the reviewer's `Knocked` "local assignment" note (the accepted
  knock request is server-backed), the pre-existing untouched-seeded-Invite
  preview mismatch, and reading the exact server-side rule back (deferred with
  the review surface in B).
