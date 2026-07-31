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
  (6 passed), `cargo check -p koushi-sdk --lib`,
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
- Space/child load and invite reconciliation carry non-empty local member
  profiles through the request-correlated `SpaceMembersProjectionReconciled`
  action; live read-receipt projections refresh from the state-owned cache.
  This is covered by the child-profile load-path test, the authoritative invite
  reconciliation test, and the existing-receipt Seen fallback test.
- Added core load/invite commands and actor handlers using the Task 1 local SDK
  projection and existing invite primitive; no per-person network profile
  fan-out or plaintext profile store was introduced.
- Added `core.space_members_projection` and `core.profile_resolution` with
  count/token/bool fields only. The privacy test serializes diagnostics and
  rejects Matrix IDs, names, MXC URLs, and raw profile content. The completion
  milestone records actual `ProfileResolutionSource` outcomes for aliases,
  relevant rooms, Spaces, payloads, global cache, local homeserver profiles,
  and unresolved labels; cache freshness is explicitly `not_tracked` because
  this state has no freshness metadata.
- Evidence:
  - `cargo fmt --all` — passed (stable rustfmt emitted warnings for unsupported
    nightly-only formatting options).
  - `cargo test -p koushi-state --test space_members_state -- --nocapture` —
    12 passed.
  - `cargo test -p koushi-state --test profile_state -- --nocapture` —
    28 passed.
  - `cargo test -p koushi-core space_members --lib -- --nocapture` — 2 passed.
  - `cargo check -p koushi-state` — passed.
  - `cargo check -p koushi-core --lib` — passed; only pre-existing
    `media_preparation` / `read_state` warnings remain.
  - `cargo fmt --all -- --check` — passed.
  - `git diff --check` — passed.

## Task 2 review-fix milestone — 2026-08-01

Completed in this milestone:

- C1: `AppActor` now admits Space-member load/invite commands before routing to
  `RoomActor`; wrong-space, stale-generation, and duplicate invites are
  rejected before any SDK side effect. The production-path test
  `rejected_space_invites_are_fenced_before_room_actor_route` verifies the
  rejected commands do not produce an invite settlement.
- C2: load, failure, profile observation, projection reconciliation, and invite
  settlement actions are request-correlated. Same-generation out-of-order
  results cannot overwrite a newer request or clobber an active invite.
- I1: every active-space mutation handled in this scope synchronizes the
  Space-members generation/clear transition and emits the corresponding state
  effect, including restored navigation, room-list, directory, selection, and
  session-clear paths.
- I2/I4: incomplete child projections and lookup/load failures retain the last
  valid projection, including an optimistic invite whose target is not yet
  observed.
- I3: invite reconciliation applies the authoritative projection and profile
  observations before the correlated settlement outcome is reduced.
- Minor fixes: cached room avatars refresh the Space-member row, and legacy
  AppState/profile fixtures deserialize with the new defaulted fields.

Verification for this milestone:

- `cargo test -p koushi-state --test navigation_state -- --nocapture` — 46
  passed.
- `cargo test -p koushi-state --test space_members_state -- --nocapture` — 12
  passed.
- `cargo test -p koushi-state --test profile_state -- --nocapture` — 28
  passed.
- `cargo test -p koushi-sdk space_member --lib -- --nocapture` — 6 passed.
- `cargo test -p koushi-core --lib rejected_space_invites_are_fenced_before_room_actor_route -- --nocapture` — 1 passed.
- `cargo test -p koushi-core space_members --lib -- --nocapture` — 2 passed.
- `cargo check -p koushi-state`, `cargo check -p koushi-core --lib`, and
  `cargo check -p koushi-sdk --lib` — passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed. Stable rustfmt
  still reports the repository's existing warnings for nightly-only options.

Deferred to the next review milestone:

- I8: the C1 production routing test is complete; production-path coverage for
  the remaining load/failure/profile-observation routes remains.

## Diagnostics/privacy completion milestone — 2026-08-01

- I5 completed: `profile_resolution_diagnostic_event` now counts actual
  resolver source outcomes, while Space projection diagnostics report raw SDK
  input, projected output, child-union deduplication, and `not_tracked`
  freshness when unavailable. Stale and duplicate command rejections carry
  explicit sanitized outcomes.
- I7 completed: Debug output for local member snapshots, raw Space projection
  and entries, profiles, live receipts, and profile-resolution input/results
  reports only redacted presence/count/source facts.
- Timeline receipt profile action construction remains a pure, tested helper;
  its production TimelineActor receipt-diff delivery is completed in the Task
  2 receipt-profile milestone below.

## Task 2 receipt-profile production completion — 2026-08-01

- I6 completed: TimelineActor receipt diffs now collect receipt updates and
  user IDs, perform one local-only `room_member_profiles_no_sync` lookup, and
  deliver one ordered batch of room observations, account-cache observations,
  and receipts.
- Local lookup misses and failures omit only profile actions, preserve receipt
  delivery through the global cache fallback, and record sanitized count,
  outcome, and `network_lookup_attempted=false` diagnostics. Raw SDK errors are
  not retained or logged.
- Receipt action delivery waits for reducer capacity and rechecks the existing
  actor-generation fence, so replacement actors cannot apply stale results.
- Production-path-focused tests cover the normal local profile observation,
  local lookup miss/failure, stale-generation discard, relevant-room precedence
  over the global cache, and refresh of an existing `Unknown user` receipt.
- Verification:
  - `cargo test -p koushi-core --lib production_receipt_diff -- --nocapture` — 5 passed.
  - `cargo test -p koushi-core --lib live_receipt_observation -- --nocapture` — 1 passed.
  - `cargo test -p koushi-core --lib koushi_timeline_builder_projects_sdk_read_receipts -- --nocapture` — 1 passed.
  - `cargo check -p koushi-state`, `cargo check -p koushi-sdk --lib`, `cargo check -p koushi-core --lib`, `cargo fmt --all -- --check`, and `git diff --check` passed.

Final verification for this milestone:

- `cargo test -p koushi-state --test space_members_state -- --nocapture` — 12
  passed.
- `cargo test -p koushi-state --test profile_state -- --nocapture` — 29 passed.
- `cargo test -p koushi-sdk --lib -- --nocapture` — 114 passed.
- Focused core diagnostics/privacy/fencing/helper tests — 8 passed total:
  profile-source accounting, receipt diagnostics, Space diagnostics privacy,
  command fencing, pure receipt action construction, and Space projection
  diagnostics.
- `cargo check -p koushi-state`, `cargo check -p koushi-sdk`, and
  `cargo check -p koushi-core --lib` — passed. Core retains only existing
  `media_preparation` / `read_state` warnings.
- `cargo fmt --all -- --check` and `git diff --check` — passed. Stable rustfmt
  reports the repository's existing nightly-only option warnings.
- A full `cargo test -p koushi-core --lib -- --nocapture` sweep was attempted;
  it encountered the unrelated existing account assertion failure in
  `account::tests::own_user_sas_proof_success_enters_shared_authoritative_promotion_path`
  and then a connection-reset/over-60-second hang in
  `account::tests::soft_logout_reauth_joins_old_observers_before_subscribing_replacements`.
  The sweep was stopped after the bounded wait; all milestone-focused core
  tests passed independently.

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
