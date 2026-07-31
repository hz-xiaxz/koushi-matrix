# Space Members implementation ledger

Plan: `docs/superpowers/plans/2026-07-31-space-members-profile-cache.md`

## Task status

- [x] Task 1 — SDK explicit membership facts
- [x] Task 2 — Rust state/core/profile/invite transitions
- [x] Task 3 — Tauri/Desktop transport
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

## Task 2 final closure fixes — 2026-08-01

- Space-level JOIN and INVITE member lookups now preserve their structured SDK
  errors. Core records a lookup failure and reduces `SpaceMembersLoadFailed`
  without constructing an empty projection; state preserves the last valid
  projection. Child-room lookup failures remain partial/incomplete and retain
  the existing last-known merge behavior.
- SDK and core Space diagnostics now use lookup outcome and availability tokens
  (`lookup_failed`, `not_attempted`, and `counts_unavailable`) and emit numeric
  member counts only when the corresponding lookup actually observed them.
- Timeline initial receipt publication and authoritative recovery now use the
  shared local-only room-profile observation helper and ordered,
  generation-fenced reliable batches, with profile actions before receipts.
  Lookup misses/failures still publish receipts for global fallback.
- Verification:
  - New SDK lookup/diagnostic tests — 3 passed; SDK `space_member` focused
    regressions — 9 passed.
  - New core Space failure tests — 2 passed; core `space_member` regressions —
    4 passed.
  - New state preservation test — 1 passed; full `space_members_state` suite —
    13 passed.
  - New timeline initial/recovery guard tests — 2 passed; receipt-diff
    production regressions, including lookup miss/failure and stale generation
    — 5 passed; receipt-observation regressions — 2 passed.
  - `cargo check -p koushi-sdk --lib`, `cargo check -p koushi-state`, and
    `cargo check -p koushi-core --lib` — passed. Core reports only existing
    unused/dead-code warnings.
  - `cargo fmt --all -- --check` and `git diff --check` — passed. Stable
    rustfmt reports the repository's existing nightly-only option warnings.
- Scope remained limited to SDK/core/state diagnostics and timeline behavior;
  no Desktop/Tauri/room People changes or network profile lookup were added.

## Task 3 evidence — 2026-08-01

- Added the `load_space_members(spaceId, generation)` and
  `invite_user_to_space(spaceId, userId, generation)` Tauri handlers, exact
  Core command builders, command registration, and request/event wait paths.
  Both wait predicates require the correlated request ID and generation;
  wrong-generation `SpaceMembersLoaded` and `SpaceMemberInviteSettled` events
  are rejected by the focused Rust regression test.
- Added `space_members` to the Tauri frontend snapshot and changed-slices DTO
  boundary. This fixes the existing IPC omission that otherwise left the
  Rust-owned Space member state invisible to Desktop.
- Added serde-matching TypeScript domain types, Desktop API/client methods and
  exact invoke-shape coverage.
- Added BrowserFakeApi joined, invited, child-room-only, incomplete-child,
  pending-invite, successful-invite, failed-invite, and Space-switch fixtures
  and transitions. Four existing snapshot fixtures were updated only to
  provide the new required `space_members` field; no Task 4 UI was added.
- Verification:
  - `npx vitest run src/backend/client.test.ts src/backend/browserFakeApi.test.ts`
    — 2 files and 84 tests passed.
  - `npm run typecheck` — passed.
  - `cargo test -p koushi-desktop load_space_members --lib -- --nocapture`
    — 1 passed.
  - `cargo test -p koushi-desktop space_member --lib -- --nocapture`
    — 3 passed.
  - `cargo test -p koushi-desktop frontend_snapshot_serializes_to_the_typescript_contract --lib -- --nocapture`
    — 1 passed.
  - `cargo test -p koushi-desktop frontend_app_state_golden_matches_maximally_populated_state --lib -- --nocapture`
    — 1 passed.
  - `cargo check -p koushi-desktop` — passed; only existing koushi-core
    unused/dead-code warnings remain.
  - `cargo fmt --all -- --check` and `git diff --check` — passed. Stable
    rustfmt reports the repository's existing nightly-only option warnings.

## Task 3 review-fix milestone — 2026-08-01

- BrowserFake `loadSpaceMembers` now applies the same admission fences as Rust:
  an active Space requires the exact selected Space and generation, an
  in-flight invite blocks a load, and an unselected Space may admit a new
  Space/generation pair. Rejected calls return the unchanged snapshot and do
  not overwrite `selected_space_id`, `generation`, or the pending operation.
- Added focused positive and negative BrowserFake tests for matching loads,
  wrong Space, stale/future generations, loading after Space clear, and load
  during an invite.
- Added `ProfileState.room_users` as the exact TS representation of Rust's
  `BTreeMap<String, BTreeMap<String, UserProfile>>` and updated all typed
  Desktop test/harness fixtures. The BrowserFake snapshot now includes the
  serialized field, which is already present in the Rust golden contract.
- Verification:
  - TDD RED: the new fence tests reproduced Space/generation overwrites and
    the missing profile field; the in-flight-load test reproduced loss of the
    pending invite.
  - `npx vitest run src/backend/client.test.ts src/backend/browserFakeApi.test.ts`
    — 2 files, 89 passed.
  - `npm run typecheck` — passed.
- `git diff --check` — passed.

## Task 4 first coherent UI slice — 2026-08-01

- Added a standalone `SpaceMembersPanel` that presents the Rust-owned JOIN,
  INVITE, and child-room-only sections in that order. Search covers display
  label, original display label, display name, and user ID; child-room-only
  rows show their contributing room IDs and expose the inline invite callback.
- Invite buttons are disabled from the state-owned per-entry pending flag or
  the caller's permission prop. The panel also announces incomplete child-room
  synchronization and keeps section/classification decisions outside React.
- Added a Space-only Sidebar Members entry before DMs/Rooms. It is hidden on
  account Home and displays the joined count plus the `+N` child-room-only
  warning count. Counts and the open callback are supplied through Sidebar
  props, with the existing snapshot as the compatibility default.
- Added English/Japanese labels and focused component/i18n coverage. Room
  People remains untouched.
- Verification:
  - `npx vitest run src/components/SpaceMembersPanel.test.tsx
    src/components/Shell.test.tsx src/i18n/messages.test.ts` — 3 files, 57
    passed.
  - `npx vitest run src/components/PeoplePanel.test.tsx` — 27 passed.
  - `npm run typecheck` — passed.
  - `npm run lint` — passed, including the IME-safe input check.
  - `git diff --check` — passed.

This bounded slice intentionally leaves App/right-panel wiring, diagnostics,
and global context-menu plumbing for the follow-up UI integration task.

## Task 4 dedicated panel follow-up — 2026-08-01

- Strengthened `SpaceMembersPanel` to expose an accessible localized profile
  row action and call the supplied `onOpenProfile` callback without changing
  the Rust-owned section classification.
- Invite controls now honor the state-owned loading/in-flight operation as
  well as per-entry pending state and the global invite permission; the
  no-results state is announced through an accessible status region.
- TDD RED was observed for the four new profile, pending-operation, and
  empty-state assertions before the component changes; the focused suite then
  passed GREEN.
- Verification:
  - `npx vitest run src/components/SpaceMembersPanel.test.tsx
    src/i18n/messages.test.ts src/components/PeoplePanel.test.tsx` — 3 files,
    54 passed.
  - `npm run typecheck` — passed.
  - `npm run lint` — passed, including the IME-safe input check.
  - `git diff --check` — passed.

The remaining integration work is still App/right-panel wiring, diagnostics,
and global context-menu plumbing; no Room People or network behavior was
changed in this panel slice.

## Task 4 sidebar-entry correction — 2026-08-01

- Tightened the sidebar entry guard to require a real active Space, so an
  inconsistent navigation snapshot with neither Home nor a Space selected
  cannot show a Space-only row.
- Kept joined and child-room-only counts Rust-owned through the existing
  `Sidebar` props/snapshot fallback, omitted the child-only suffix when it is
  zero, and retained the accessible label that announces both counts.
- Added focused coverage for placement immediately before the DMs/Rooms
  controls, zero-warning formatting, click routing, Space visibility, Home
  absence, and the no-real-Space guard.
- App-level Sidebar callback/count wiring remains deferred by scope; the row
  is ready for that integration without changing Room People classification.

## Task 4 right-panel composition — 2026-08-01

- Routed `mode: "people"` with an explicit Space scope to the existing
  `SpaceMembersPanel`, passing the Rust-owned `domain.space_members` state,
  optional invite permission/callback props, and the existing profile callback.
- Kept explicit Room scopes on the existing `PeoplePanel` path with
  `roomManagement`; profile mode and back behavior remain unchanged.
- Added focused `rightPanel.test.tsx` coverage for Space-versus-Room panel
  selection and invite/profile callback forwarding. Row context-menu wiring
  remains deferred because `SpaceMembersPanel` does not currently expose a
  context-menu callback.
- TDD evidence: the Space routing test failed against the pre-change
  composition, then passed after the minimal RightPanel change.
- Verification:
  - `npx vitest run src/components/rightPanel.test.tsx
    src/components/PeoplePanel.test.tsx
    src/components/SpaceMembersPanel.test.tsx
    src/domain/rightPanel.test.ts` — 4 files, 45 passed.
  - `npm run typecheck` — passed.
  - `npm run lint` — passed, including the IME-safe input check.
  - `git diff --check` — passed.

App-level invite permission/command and global context-menu wiring remain
deferred by scope; no App, Sidebar, backend, or Rust files were changed.

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

## Room invite permission fact — 2026-08-01

- Added the exact `can_invite` room permission fact from Matrix SDK power
  levels through core/state and the Desktop IPC/TypeScript contract.
- SDK computes it with `power_levels.user_can_invite(own_user_id)`; it is
  never inferred from `can_edit_roles`.
- State deserializes legacy permission objects with `can_invite=false` while
  serializing the new snake_case key. BrowserFake editable/readonly fixtures
  expose true/false explicitly; no UI/App production code changed.
- TDD evidence: red tests reproduced the missing SDK fact, state key,
  mapping, BrowserFake field, and checked-in golden/IPC contracts before the
  implementation.
- Verification:
  - `cargo test -p koushi-sdk room_permission --lib -- --nocapture` — 1 passed.
  - `cargo test -p koushi-state --test room_management_state -- --nocapture` — 17 passed.
  - `cargo test -p koushi-core --lib room_settings -- --nocapture` — 2 passed.
  - `cargo test -p koushi-state --test invite_workflow_state -- --nocapture` — 4 passed.
  - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden -- --nocapture` — 1 passed.
  - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib core_event_wire_format_matches_checked_in_contract_artifact -- --nocapture` — 1 passed.
  - Focused Vitest suite — 6 files, 140 passed; `npm run typecheck` passed.
  - SDK/state/core/Desktop cargo checks, `cargo fmt --all -- --check`, and `git diff --check` passed.

## Task 4 final App/UI integration — 2026-08-01

- TDD RED: the new App/Space Members integration suite initially failed on
  the missing shared App open/invite path, context-menu forwarding, and
  diagnostics source; the first run reported 4 files failed, 11 tests failed,
  and 17 passed.
- Wired one `openSpaceMembers(trigger)` path to Sidebar and Space Info. It
  captures the active Space and Rust-owned generation, loads exact Space room
  settings and members, and fences settings/member results by request,
  active Space, selected Space, and generation. Sidebar counts come directly
  from `domain.space_members`; React does not classify members.
- Added one fenced invite command for inline and child-only context-menu
  actions. Context targets carry the Space/generation fence, and the exact
  room `permissions.can_invite` fact gates both controls while Rust-owned
  loading/invite-pending state gates pending actions. Room People composition
  remains unchanged.
- Added private-data-free `ui.space_members_panel` diagnostics for open
  trigger, section counts, search/result state, invite trigger/availability,
  and incomplete synchronization; tests assert private IDs, labels, room IDs,
  avatar URIs, and other sample strings are absent.
- TDD GREEN and verification:
  - Focused App/component/domain suite — 4 files, 29 passed, including stale
    context-target dismissal during Space navigation.
  - Broader App/Shell/right-panel/Space Members/context/domain/backend-fake/
    Room People suite — 9 files, 187 passed.
  - `npm run typecheck` — passed.
  - `npm run lint` — passed, including the IME-safe input check.
  - `cargo fmt --all -- --check` — passed; rustfmt emitted only existing
    stable-channel warnings for nightly-only formatting options.
  - `git diff --check` — passed.

No backend or Rust implementation changes were needed; no DMG build was run.
