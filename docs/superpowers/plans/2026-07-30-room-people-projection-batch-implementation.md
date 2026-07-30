# Room People Projection Batch (#380, #355, #374) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan.

**Goal:** Make mention autocomplete room-scoped and joined-member-only, preserve Rust-owned friendly labels across people-facing surfaces, and remove only the Home rail visual tooltip.

**Architecture:** Add a bounded `mention_candidates` domain slice owned by `koushi-state`, populate it through a typed `RoomCommand` and `RoomActor` using the SDK's no-sync joined-member snapshot followed by a deduplicated refresh when necessary, and transport the projection through the existing state-delta/Tauri boundary. Keep eligibility, Unicode matching, ranking, room-notification permission, label precedence, and stale-result fencing in Rust. React renders only the selected room/surface projection and localized missing-label state.

**Tech Stack:** Rust (`koushi-state`, `koushi-sdk`, `koushi-core`, Tauri), TypeScript/React, Vitest, Playwright, local Conduit/Tuwunel headless QA.

---

## Fixed contract

Use these names consistently unless the compiler requires a mechanical naming
adjustment:

```rust
pub enum MentionSurface {
    Main,
    Thread,
}

pub enum MentionCandidatesCompleteness {
    Loading,
    Partial,
    Complete,
    Failed,
}

pub enum RoomMentionPermission {
    Allowed,
    Denied,
    Unknown,
}

pub struct MentionCandidate {
    pub user_id: String,
    pub display_label: Option<String>,
    pub original_display_label: Option<String>,
    pub avatar: Option<AvatarThumbnail>,
    pub membership: MentionCandidateMembership, // Joined only
}

pub struct MentionCandidatesTarget {
    pub room_id: String,
    pub generation: u64,
    pub request_id: u64,
    pub query: String,
    pub surface: MentionSurface,
    pub completeness: MentionCandidatesCompleteness,
    pub candidates: Vec<MentionCandidate>,
    pub room_mention_allowed: RoomMentionPermission,
    pub failure_kind: Option<MentionCandidatesFailureKind>,
}

pub struct MentionCandidatesState {
    pub targets: Vec<MentionCandidatesTarget>,
}
```

`MentionCandidatesState` is bounded to the current main/thread composer targets
plus four recent targets. Equality of a target key is `(room_id, surface)`.
`request_id` on the state DTO is the command's sequence number, while the core
keeps the full `RequestId` for correlation. A missing friendly label remains
`None`; no reducer or core projection replaces it with an MXID.

The command shape is:

```rust
RoomCommand::QueryMentionCandidates {
    request_id: RequestId,
    account_key: AccountKey,
    room_id: String,
    surface: MentionSurface,
    query: String,
}
```

The account key is validated against the active session before demand reaches
the room actor. The query string is private and must not appear in `Debug` or
diagnostics.

## Task 1: Amend the normative contract before code

**Files:**

- Modify: `docs/policies/engineering-rules.md`
- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/state-machine.md`
- Reference: `docs/superpowers/specs/2026-07-30-room-people-projection-design.md`

**Step 1: Update engineering ownership rules**

State explicitly that `ProfileState.users` is an account profile cache, not
room-membership evidence. Record that mention eligibility, matching, ordering,
and `@room` permission are Rust-owned and that normal primary people labels
must not promote an MXID when the friendly label is absent.

**Step 2: Update the architecture overview**

Add `mention_candidates` to the Rust-owned domain slices and document the
RoomActor → reducer → state-delta → projection-store path. Record that React
may manage popup focus/visibility but cannot build, filter, or sort candidates.

**Step 3: Add state-machine transitions**

Document:

```text
query demand
  -> loading/partial from members_no_sync(JOIN)
  -> complete when already synced
  -> one members(JOIN) refresh when incomplete
  -> complete or coarse failed

newer target generation / room switch / account switch / logout
  -> stale completion ignored

base-room membership update
  -> invalidate directory
  -> recompute every demanded target for that room
```

Include independent main/thread targets and fail-closed unknown `@room`
permission.

**Step 4: Review only the documentation diff**

Run:

```bash
git diff --check
git diff -- docs/policies/engineering-rules.md docs/architecture/overview.md docs/architecture/state-machine.md
```

Expected: no whitespace errors; all three canon files agree with the approved
design.

**Step 5: Commit**

```bash
git add docs/policies/engineering-rules.md docs/architecture/overview.md docs/architecture/state-machine.md
git commit -m "docs: define room people projection contract"
```

## Task 2: Add RED state tests and the mention projection slice

**Files:**

- Create: `crates/koushi-state/src/state/mention.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Create: `crates/koushi-state/src/reducer/mention.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`
- Create: `crates/koushi-state/tests/mention_candidates_state.rs`
- Modify: session-clear paths in `crates/koushi-state/src/reducer/session.rs`

**Step 1: Write the failing reducer tests**

Cover these behaviors with value assertions, not source-text guards:

- a query replaces only the matching `(room_id, surface)` target;
- main and thread targets for one room coexist;
- a partial result contains only explicit joined candidates;
- a result with an older request or generation is ignored;
- a room-A completion cannot replace the room-B active target;
- aliases alter labels but do not create membership;
- missing friendly labels remain `None`;
- logout, lock, account switch, and local reset clear the slice;
- the collection evicts the oldest recent target after the fixed bound.

Run:

```bash
cargo test -p koushi-state --test mention_candidates_state
```

Expected RED: the new state types/actions do not exist.

**Step 2: Implement the state types and reducer**

Add actions for demand, projection settlement, coarse failure, and room
invalidation. Fence settlement on room, surface, request sequence, and
generation. Do not normalize or rank in the reducer; it stores the already
projected result.

**Step 3: Clear the slice at every session boundary**

Use the same lifecycle points that clear room/timeline projections. Do not add
an independent React cleanup.

**Step 4: Run the focused tests**

```bash
cargo test -p koushi-state --test mention_candidates_state
cargo test -p koushi-state --test session_state
```

Expected GREEN: all new state transitions and existing session cleanup pass.

**Step 5: Commit**

```bash
git add crates/koushi-state
git commit -m "feat(state): add room mention candidate projection"
```

## Task 3: Add the SDK joined-member snapshot adapter

**Files:**

- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: the existing SDK mock/test support adjacent to room member helpers

**Step 1: Write RED adapter tests**

Define one adapter result carrying:

```rust
pub struct MatrixJoinedMemberSnapshot {
    pub members: Vec<MatrixRoomMemberSummary>,
    pub complete: bool,
}
```

Tests must prove:

- the no-sync method returns cached `JOIN` members without requesting network;
- invite/knock/leave/ban members are absent;
- `complete` mirrors `Room::are_members_synced()`;
- the refresh method delegates once to `Room::members(JOIN)` and returns the
  refreshed joined snapshot.

Run:

```bash
cargo test -p koushi-sdk --lib joined_member_snapshot
```

Expected RED: the adapter methods are absent.

**Step 2: Implement the narrow adapter**

Expose `joined_member_snapshot_no_sync(room_id)` and
`refresh_joined_member_snapshot(room_id)`. Reuse `MatrixRoomMemberSummary`;
preserve room-specific display name and avatar. Return coarse SDK errors to
core without logging identifiers or raw error text.

**Step 3: Run focused SDK tests and guard**

```bash
cargo test -p koushi-sdk --lib joined_member_snapshot
node scripts/check-sdk-submodule.mjs
```

Expected GREEN. No vendored SDK change is expected. If a missing SDK primitive
is discovered, first add a deterministic RED test under
`vendor/matrix-rust-sdk`, make the minimum upstreamable patch, update
`docs/upstream/matrix-rust-sdk-feedback.md`, commit the SDK change on the
approved fork branch, and then update only the submodule gitlink.

**Step 4: Commit**

```bash
git add crates/koushi-sdk
git commit -m "feat(sdk): expose joined room member snapshots"
```

## Task 4: Implement Rust-owned matching, ranking, and room refresh

**Files:**

- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/room.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-core/src/runtime.rs` only if routing requires it
- Create: `crates/koushi-core/src/mention_candidates.rs`
- Modify: `crates/koushi-core/src/lib.rs`

**Step 1: Write RED unit tests for matching**

In `mention_candidates.rs`, test:

- exact alias beats prefix and substring;
- token/prefix beats substring;
- full MXID and localpart match;
- given name and family name substrings match independently;
- Japanese/CJK normalized matching follows the existing locale profile;
- collation then user ID is deterministic;
- non-joined input is rejected before ranking;
- local alias wins the visible label while
  `original_display_label` retains the room/profile label;
- allowed/denied/unknown `@room` permission produces the correct synthetic
  result.

Run:

```bash
cargo test -p koushi-core --lib mention_candidates
```

Expected RED: matcher/projector is absent.

**Step 2: Add typed command and redacted Debug coverage**

Add `QueryMentionCandidates` to `RoomCommand`, `CoreCommand::request_id`, and
the account-routing match. Add a test that formatted command diagnostics contain
surface/request metadata but not account key, room ID, query, user ID, label,
alias, or avatar URI.

**Step 3: Write RED RoomActor lifecycle tests**

Using the existing actor/mock seam, prove:

- complete cached membership publishes once and skips refresh;
- incomplete membership publishes fail-closed partial state and starts one
  deduplicated refresh;
- refresh success produces complete state;
- refresh failure produces only a coarse failure kind;
- a newer request/query/generation ignores an older completion;
- room-A settlement cannot affect room B;
- a base-room membership update invalidates the cache and recomputes all
  demanded main/thread targets for that room;
- departure removes a candidate and join adds one without actor restart.

**Step 4: Implement the actor path**

Store demanded targets and the joined-member directory inside `RoomActor`.
On demand:

1. validate account and room;
2. increment target generation;
3. reduce the demand;
4. project the no-sync snapshot;
5. settle complete immediately when membership is synced;
6. otherwise publish partial/loading and share one in-flight refresh per room;
7. fence the completion against account, room, surface, request, query, and
   generation before reducing it.

Use the existing base-room update observation to invalidate/recompute. Never
restart the room actor for a join/leave.

**Step 5: Add private-data-free diagnostics**

Use source `mention.candidates` and only:

```text
stage, surface, completeness, candidate_count, outcome
```

Test that identifiers, query text, labels, and raw SDK errors are absent.

**Step 6: Run focused core tests**

```bash
cargo test -p koushi-core --lib mention_candidates
cargo test -p koushi-core --lib room_mention
```

Expected GREEN with exact exit status 0 for each command.

**Step 7: Commit**

```bash
git add crates/koushi-core
git commit -m "feat(core): project room-scoped mention candidates"
```

## Task 5: Preserve friendly people labels across Rust projections

**Files:**

- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-core/src/timeline.rs`
- Modify: `crates/koushi-core/src/search.rs`
- Modify: relevant state DTOs under `crates/koushi-state/src/state/`
- Modify: focused tests adjacent to timeline, threads, files, media, pinned
  events, typing, reactions, and receipts

**Step 1: Write RED projection tests**

Add behavioral tests showing:

- an SDK room-specific timeline sender label survives global profile
  reprojection;
- a local alias overrides the room label;
- a missing friendly label stays `None`;
- reaction previews carry `{ user_id, display_label }`, not sender-ID strings;
- typing entries carry structured identity plus optional label;
- thread root/latest, Files, media gallery, pinned events, and receipt DTOs use
  a friendly label or missing-label state;
- updating a local alias/profile refreshes already loaded projections.

Run the individual new test filters and confirm each is RED before production
changes.

**Step 2: Add a shared optional friendly-label resolver**

Implement this precedence:

```text
local alias
  -> room/upstream display label
  -> account profile display name
  -> own profile display name
  -> None
```

Keep the current identity resolver for explicit identity/detail contexts.

**Step 3: Change purpose-built transport shapes**

- preserve `TimelineItem.sender_label` from the SDK unless an alias overrides;
- replace `ReactionGroup.sender_preview: Vec<String>` with structured entries;
- replace typing ID-only presentation with structured entries;
- add optional `sender_label` only to list/result DTOs that lack one;
- keep raw IDs as identity fields but never synthesize them into primary label
  fields.

**Step 4: Run focused Rust tests**

```bash
cargo test -p koushi-core --lib timeline_sender_label
cargo test -p koushi-core --lib reaction_sender_preview
cargo test -p koushi-core --lib typing_user_projection
cargo test -p koushi-core --lib people_facing_label
cargo test -p koushi-state --test timeline_thread_state
cargo test -p koushi-state --test profile_state
```

Expected GREEN.

**Step 5: Commit**

```bash
git add crates/koushi-core crates/koushi-state
git commit -m "fix(core): preserve people-facing room labels"
```

## Task 6: Mirror the state-delta and Tauri wire contracts

**Files:**

- Modify: `crates/koushi-core/src/state_delta.rs`
- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src-tauri/tests/golden/frontend_app_state.json`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/domain/coreEvents.ts`
- Modify: `apps/desktop/src/domain/coreEvents.generated.json`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/test/tauriIpcMock.ts`
- Modify: `apps/desktop/src/test/appHarnessMain.tsx`

**Step 1: Add RED serialization/delta tests**

Extend the Rust state-delta tests so `mention_candidates` is emitted only when
that slice changes. Extend the maximally populated Tauri golden with:

- one partial main target;
- one complete thread target;
- a labelled/avatar candidate;
- an unlabelled candidate;
- explicit room-mention permission;
- structured reaction/typing/list sender labels.

Run:

```bash
cargo test -p koushi-core --lib state_delta
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden
```

Expected RED due to missing DTO fields/artifact changes.

**Step 2: Mirror all hand-maintained boundaries**

Add the slice and label shapes to Rust Tauri DTOs, TypeScript types, browser
fake snapshots, harness snapshots, and IPC mocks. Preserve references for
unchanged slices in `applyStateDelta`.

**Step 3: Regenerate both contract artifacts correctly**

```bash
UPDATE_GOLDEN=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact
```

The second artifact has no update switch: edit
`apps/desktop/src/domain/coreEvents.generated.json` to match the serializer,
then rerun the contract test.

**Step 4: Run focused contract gates**

```bash
cargo test -p koushi-core --lib state_delta
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact
npm --prefix apps/desktop run typecheck
```

Expected GREEN with exit status 0.

**Step 5: Commit**

```bash
git add crates/koushi-core apps/desktop/src-tauri apps/desktop/src
git commit -m "feat(desktop): mirror room people projections"
```

## Task 7: Wire mention demand and consume only the Rust projection

**Files:**

- Modify: Tauri command routing under `apps/desktop/src-tauri/src/commands/`
- Modify: `apps/desktop/src/backend/api.ts`
- Modify: `apps/desktop/src/backend/tauriApi.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/domain/appStore.ts`
- Modify: `apps/desktop/src/components/composer.tsx`
- Modify: `apps/desktop/src/components/panes.tsx`
- Modify: focused composer/store tests

**Step 1: Write RED selector tests**

Replace the global selector contract with:

```ts
selectMentionCandidates(state, roomId, surface)
```

Prove it selects only the exact target, preserves the Rust order, returns no
global-profile fallback, preserves reference identity across unrelated deltas,
and keeps main/thread independent.

**Step 2: Write RED composer dispatch tests**

Prove that changing the active `@` token, room, or surface dispatches one typed
query demand carrying the exact target. Clearing the token hides results and
does not locally synthesize candidates. Partial/loading state renders only the
known Rust candidates plus loading state.

Run:

```bash
npm --prefix apps/desktop test -- src/domain/appStore.test.ts src/components/composer.test.tsx
```

Expected RED.

**Step 3: Implement Tauri/backend command routing**

Route a typed query command through the existing core runtime request-id
allocator. Do not return a React-mutated snapshot; reducer events and
state-deltas settle the projection.

**Step 4: Remove frontend matching/ranking**

Delete the `ProfileState.users` scan, JavaScript `localeCompare`, local token
matching, and locally appended `@room`. Render the Rust ordering exactly.

**Step 5: Run focused frontend gates**

```bash
npm --prefix apps/desktop test -- src/domain/appStore.test.ts src/components/composer.test.tsx
npm --prefix apps/desktop run typecheck
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx
```

Expected GREEN.

**Step 6: Commit**

```bash
git add apps/desktop/src-tauri apps/desktop/src
git commit -m "feat(desktop): use room-scoped mention suggestions"
```

## Task 8: Remove raw IDs from normal people-facing UI

**Files:**

- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Modify: relevant thread/files/media/pinned/receipt components
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/i18n/messages.test.ts`
- Modify: `apps/desktop/e2e/basic-operations.spec.ts`

**Step 1: Add RED browser assertions**

Seed Rust-shaped DTOs and prove:

- known reaction senders show the friendly label;
- missing reaction senders show localized `Unknown user`, never the MXID;
- timeline/reply/thread/list and one file/media/pinned surface do not promote
  `sender` to primary text;
- typing and receipts render Rust-projected labels;
- explicit profile/account secondary identity still shows the MXID.

Run the new named cases:

```bash
npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "room people labels|room mention candidates" --workers=1
```

Expected RED.

**Step 2: Implement presentation-only fallback**

Add the localized `Unknown user` key to every checked catalog. Replace primary
text expressions such as `sender_label ?? sender` and profile-map joins with
`sender_label ?? t("people.unknownUser")`. Keep explicit secondary identity
lines unchanged.

**Step 3: Run focused UI tests**

```bash
npm --prefix apps/desktop run test -- --run src/i18n/messages.test.ts
npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "room people labels|room mention candidates" --workers=1
npm --prefix apps/desktop run typecheck
```

Expected GREEN.

**Step 4: Commit**

```bash
git add apps/desktop/src apps/desktop/e2e/basic-operations.spec.ts
git commit -m "fix(desktop): render friendly people labels"
```

## Task 9: Remove only the Home visual tooltip

**Files:**

- Modify: `apps/desktop/src/components/WorkspaceRail.tsx`
- Modify: its focused component test, or add coverage to
  `apps/desktop/e2e/basic-operations.spec.ts`

**Step 1: Add RED behavior tests**

Prove:

- hover, pointer focus, and keyboard focus on Home produce no tooltip;
- Home retains its dynamic accessible name, focusability, selection, active
  state, and attention badge;
- Space buttons still produce their tooltips;
- a timeline action tooltip still works.

Run:

```bash
npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "Home rail tooltip" --workers=1
```

Expected RED because Home still opens the shared tooltip.

**Step 2: Make the minimal component change**

Remove only the Home `Tooltip` wrapper and tooltip trigger props. Keep the
native button and `accountHomeLabel(...)` `aria-label`. Do not add `title` and
do not change the shared `Tooltip`.

**Step 3: Run focused regression tests**

```bash
npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "Home rail tooltip|Space rail tooltip|timeline action tooltip" --workers=1
npm --prefix apps/desktop run typecheck
```

Expected GREEN.

**Step 4: Commit**

```bash
git add apps/desktop/src/components/WorkspaceRail.tsx apps/desktop/e2e/basic-operations.spec.ts
git commit -m "fix(desktop): remove Home rail tooltip"
```

## Task 10: Add local homeserver acceptance evidence

**Files:**

- Modify: the existing local QA scenario registry under `apps/desktop/qa/`
- Add or modify: the nearest Rust/headless scenario for room membership and
  message sending

**Step 1: Add RED end-to-end scenario assertions**

Using local Conduit/Tuwunel and core events/state, prove:

- ordinary room suggestions include joined users and exclude a matching
  non-member;
- two-person and group DMs contain exactly the joined candidates, including
  the current account;
- main and thread targets remain independent;
- join and leave update an open target without restart;
- the sent message contains structured `m.mentions.user_ids`;
- no account-global profile contaminates a room target;
- known and missing people-facing labels preserve optional-label semantics.

No fixed sleeps and no log-text acceptance assertions.

**Step 2: Run the focused scenario once after the coherent flow is complete**

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$PATH npm --prefix apps/desktop run qa:headless-local -- --server=conduit --scenario=room_people_projection --core --core-backend=both --timeout-ms=240000
```

Expected GREEN, exit status 0. If the first run identifies a real defect, fix
it with the cheapest focused gate, then rerun this long scenario once.

**Step 3: Commit**

```bash
git add apps/desktop/qa crates
git commit -m "test: cover room people projection end to end"
```

## Task 11: Integrated verification, self-review, PR, and merge

**Step 1: Run focused integrated gates with exact exit statuses**

Run each command directly, capturing its own status:

```bash
node scripts/check-sdk-submodule.mjs
cargo test -p koushi-state --test mention_candidates_state
cargo test -p koushi-core --lib mention_candidates
cargo test -p koushi-core --lib people_facing_label
cargo test -p koushi-core --lib state_delta
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact
npm --prefix apps/desktop run test -- --run src/i18n/messages.test.ts
npm --prefix apps/desktop test -- src/domain/appStore.test.ts src/components/composer.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "room people labels|room mention candidates|Home rail tooltip|Space rail tooltip|timeline action tooltip" --workers=1
```

Every reported exit status must be 0. Do not infer success from a pipe.

**Step 2: Run the full relevant crate/frontend gates**

```bash
cargo test -p koushi-state
cargo test -p koushi-core --lib
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm --prefix apps/desktop test
```

Expected GREEN. Run the long local homeserver scenario from Task 10 only once
at this final reviewed state unless its own evidence requires a correction.

**Step 3: Self-review the complete branch**

```bash
git diff --check origin/main...HEAD
git diff --stat origin/main...HEAD
git diff origin/main...HEAD
git status --short
```

Review against `REPOSITORY_RULES.md`, all three architecture/policy canon files,
the approved design, privacy constraints, state-delta mirrors, and every
untracked file. Correct findings and rerun the affected gates.

**Step 4: Push and open one draft PR**

The PR title is:

```text
Fix room-scoped people projections and Home tooltip
```

The body summarizes Rust ownership and RED/GREEN evidence and contains:

```text
Closes #380
Closes #355
Closes #374
```

Push the branch and open one PR. Do not create per-issue PRs.

**Step 5: Monitor all required checks**

Wait for every required check to finish. If CI fails, inspect the failing job's
actual log, reproduce locally where possible, add/retain a failing regression
check, fix, rerun the relevant gates, self-review the new diff, and push.

**Step 6: Merge with a merge commit**

After all required checks are green, merge using GitHub's non-squash merge
method. Verify:

- the PR is merged;
- the merge commit is present on `origin/main`;
- issues #380, #355, and #374 are closed;
- the local main workspace's unrelated `HANDOFF.md` remains untouched.
