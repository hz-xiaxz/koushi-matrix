# Space Members Audit and Profile Cache Implementation Plan

> **For Luna:** Execute this plan task-by-task with `gpt-5.6-luna` at reasoning effort `max`. Follow test-driven development: add a failing focused test, run it, implement the minimum production behavior, rerun it, then run the task-level regression set. Preserve the existing diagnostics work and do not push or open a PR.

**Goal:** Add a Space-only Members navigation entry and a Rust-owned member audit that separates joined Space members, pending Space invites, and the union of child-room-only joined users; make child-room-only users directly invitable to the Space; improve local profile fallback and sanitized diagnostics; rebuild the macOS DMG.

**Architecture:** Matrix SDK code reads local membership state with explicit JOIN and INVITE filters and constructs raw Space membership facts. Koushi state/core owns classification, profile precedence, operation state, and generation fencing. Tauri exposes load/invite commands through the existing snapshot transport. React only renders the projection and dispatches commands. Existing room People behavior remains room-scoped.

**Tech Stack:** Rust, matrix-rust-sdk, serde, Tokio actors/reducers, Tauri 2, React 19, TypeScript 6, Vitest, Testing Library, Cargo.

**Design reference:** `docs/superpowers/specs/2026-07-31-space-members-profile-cache-design.md`

---

## Task 1: Add explicit Space membership facts in the SDK

**Files:**

- Modify: `crates/koushi-sdk/src/lib.rs`
- Test: inline `#[cfg(test)]` tests in `crates/koushi-sdk/src/lib.rs`

### Step 1: Add failing classification and source-guard tests

Add tests that exercise a pure classifier using anonymous fixtures:

```rust
#[test]
fn space_member_facts_separate_join_invite_and_child_only() {
    let facts = classify_space_member_ids(
        ["joined", "both"],
        ["invited"],
        [
            ("child-a", ["child-only", "both"]),
            ("child-b", ["child-only", "second-only"]),
        ],
    );

    assert_eq!(facts.space_joined_ids, vec!["both", "joined"]);
    assert_eq!(facts.space_invited_ids, vec!["invited"]);
    assert_eq!(facts.child_room_only_ids, vec!["child-only", "second-only"]);
}

#[test]
fn joined_only_helpers_do_not_use_active_membership() {
    let source = include_str!("lib.rs");
    let body = source
        .split("async fn matrix_space_members_projection")
        .nth(1)
        .expect("projection helper exists");
    assert!(!body.contains("RoomMemberships::ACTIVE"));
}
```

Also cover exclusion of leave/ban/knock indirectly by accepting only JOIN/INVITE inputs, duplicate child membership, and incomplete child rooms.

### Step 2: Run the tests and confirm RED

Run:

```bash
cargo test -p koushi-sdk space_member_facts --lib -- --nocapture
cargo test -p koushi-sdk joined_only_helpers_do_not_use_active_membership --lib -- --nocapture
```

Expected: failure because the classifier/projection does not exist.

### Step 3: Introduce SDK projection types

In `crates/koushi-sdk/src/lib.rs`, add public serializable raw facts with no UI prose:

```rust
pub struct MatrixSpaceMembersProjection {
    pub space_id: String,
    pub space_joined: Vec<MatrixSpaceMemberEntry>,
    pub space_invited: Vec<MatrixSpaceMemberEntry>,
    pub child_room_only: Vec<MatrixSpaceMemberEntry>,
    pub child_room_count: usize,
    pub complete_child_room_count: usize,
    pub incomplete_child_room_count: usize,
}

pub struct MatrixSpaceMemberEntry {
    pub user_id: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub power_level: Option<i64>,
    pub role: MatrixRoomMemberRole,
    pub child_room_ids: Vec<String>,
}
```

Use an internal user-ID-keyed accumulator so child users are deduplicated before profile work.

### Step 4: Load membership with explicit filters

Implement `matrix_space_members_projection(session, space_id)` using local SDK room/store state:

- Space joined: `RoomMemberships::JOIN`.
- Space invited: `RoomMemberships::INVITE`.
- Child union: each current child room's `RoomMemberships::JOIN`.
- Never use `ACTIVE` for these classified sets.
- Subtract joined and invited Space IDs from child-only.
- Retain all contributing child room IDs on child-only entries.
- Determine complete/incomplete counts from locally available room/member state; keep known members from incomplete rooms.
- Do not add per-user network profile calls.

When resolving the raw entry profile, use the Space room member first for Space sections and a contributing child room member first for child-only. A non-empty local member profile may update/reuse existing account-scoped profile observations; do not create a plaintext database.

### Step 5: Add sanitized SDK diagnostics

Emit `sdk.space_members_scope` with counts, explicit filter tokens, completeness counts, local lookup outcomes, and `network_member_sync_attempted=false`. Do not include IDs, names, MXC URIs, or raw errors.

### Step 6: Run SDK regression gates

Run:

```bash
cargo fmt --all -- --check
cargo test -p koushi-sdk space_member --lib -- --nocapture
cargo check -p koushi-sdk --lib
git diff --check
```

### Step 7: Commit

```bash
git add crates/koushi-sdk/src/lib.rs
git commit -m "feat: project classified Space membership facts"
```

---

## Task 2: Add Rust-owned Space member state, profile precedence, and invite transitions

**Files:**

- Create: `crates/koushi-state/src/state/space_members.rs`
- Create: `crates/koushi-state/src/reducer/space_members.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/lib.rs`
- Create: `crates/koushi-state/tests/space_members_state.rs`
- Modify: `crates/koushi-state/src/state/profile.rs`
- Modify: `crates/koushi-state/tests/profile_state.rs`
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/room.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Add or modify focused tests near the changed core modules

### Step 1: Add failing state tests

Define the intended serialized state and transitions first:

```rust
#[test]
fn loaded_projection_is_generation_fenced_and_keeps_three_sections() {
    // Start generation 2, deliver generation 1 then 2.
    // Assert generation 1 is ignored and all three generation-2 sections survive.
}

#[test]
fn invite_moves_child_only_person_to_pending_optimistically() {
    // Request invite for child-only person.
    // Assert one pending operation, no duplicate submission, and pending section placement.
}

#[test]
fn failed_invite_returns_person_to_child_only() {
    // Resolve operation as failed and assert authoritative classification is restored.
}
```

Add profile tests proving precedence:

```rust
#[test]
fn relevant_room_profile_precedes_global_profile_cache() {
    let resolved = resolve_people_label(ProfileResolutionInput {
        local_alias: None,
        relevant_room_label: Some("Relevant room"),
        space_room_label: Some("Space room"),
        payload_label: None,
        cached_label: Some("Cached"),
        local_homeserver_label: None,
    });
    assert_eq!(resolved.label, "Relevant room");
    assert_eq!(resolved.source, ProfileResolutionSource::RelevantRoom);
}

#[test]
fn global_profile_cache_prevents_unknown_when_payload_label_is_missing() {
    let resolved = resolve_people_label(ProfileResolutionInput {
        local_alias: None,
        relevant_room_label: None,
        space_room_label: None,
        payload_label: None,
        cached_label: Some("Locally cached"),
        local_homeserver_label: None,
    });
    assert_eq!(resolved.label, "Locally cached");
    assert_eq!(resolved.source, ProfileResolutionSource::GlobalCache);
}
```

### Step 2: Run and confirm RED

```bash
cargo test -p koushi-state --test space_members_state -- --nocapture
cargo test -p koushi-state --test profile_state global_profile -- --nocapture
```

Expected: missing types/actions/reducer behavior.

### Step 3: Add the state model

Add `SpaceMembersState` to `AppState.domain` with:

```rust
pub struct SpaceMembersState {
    pub selected_space_id: Option<String>,
    pub generation: u64,
    pub space_joined: Vec<SpaceMemberEntry>,
    pub space_invited: Vec<SpaceMemberEntry>,
    pub child_room_only: Vec<SpaceMemberEntry>,
    pub child_room_count: usize,
    pub complete_child_room_count: usize,
    pub incomplete_child_room_count: usize,
    pub operation: SpaceMembersOperationState,
}
```

Each entry must expose display label/original label, avatar URL, role, membership class, child room IDs/labels, and per-person invite pending state. Keep debug output sanitized.

### Step 4: Add actions and reducer transitions

Add actions for load requested/loaded/failed and invite requested/settled. Enforce:

- generation and selected-space fencing;
- stale results never replace the active projection;
- duplicate invite requests are ignored while pending;
- successful/already-invited moves the person to pending;
- authoritative JOIN moves the person to joined;
- failure returns the person to child-only and records the standard sanitized failure kind.

### Step 5: Implement profile fallback without changing room authority

Extend the existing account-scoped `ProfileState.users` use rather than adding storage. Add a pure resolver that implements:

```text
local alias
> relevant room member profile
> Space room member profile
> embedded payload label
> account profile cache
> locally stored homeserver profile
> Unknown user
```

Use it for the Space projection and for Seen/reaction receipt labels where their payload label is absent. Room-specific labels must win over global cache values.

### Step 6: Add core commands and actor handlers

Add:

```rust
RoomCommand::LoadSpaceMembers { request_id, space_id, generation }
RoomCommand::InviteUserToSpace { request_id, space_id, user_id, generation }
```

Handlers call the SDK projection/invite APIs, map raw facts into state entries, use child room display labels from current domain state, reduce transitions, and emit snapshot events. Reuse the existing `invite_user_to_room` SDK behavior; do not auto-invite anyone.

Refresh/reconcile after invite settlement. If the server reports already joined/invited, reload and classify rather than surface a hard failure.

### Step 7: Add sanitized core/profile diagnostics

Emit `core.space_members_projection` and `core.profile_resolution` with only generation, trigger/outcome tokens, counts, booleans, source buckets, cache hit/miss/stale-hit counts, dedupe counts, and incomplete state. Add a focused test that serializes diagnostic events and rejects Matrix-style IDs, names, and raw errors.

### Step 8: Run Rust gates

```bash
cargo fmt --all
cargo test -p koushi-state --test space_members_state -- --nocapture
cargo test -p koushi-state --test profile_state -- --nocapture
cargo test -p koushi-core space_members --lib -- --nocapture
cargo check -p koushi-state
cargo check -p koushi-core --lib
git diff --check
```

### Step 9: Commit

```bash
git add crates/koushi-state crates/koushi-core
git commit -m "feat: own Space member audit in core state"
```

---

## Task 3: Wire Tauri/Desktop transport and browser fake API

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/room.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/client.test.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.test.ts`
- Modify: `apps/desktop/src/domain/types.ts`

### Step 1: Add failing IPC tests

Test exact command names and argument shape:

```ts
await api.loadSpaceMembers("!space:example.invalid", 4);
expect(invoke).toHaveBeenCalledWith("load_space_members", {
  spaceId: "!space:example.invalid",
  generation: 4
});

await api.inviteUserToSpace("!space:example.invalid", "@user:example.invalid", 4);
expect(invoke).toHaveBeenCalledWith("invite_user_to_space", {
  spaceId: "!space:example.invalid",
  userId: "@user:example.invalid",
  generation: 4
});
```

Add fake API fixtures with one joined, one invited, one child-only user and a deterministic invite transition.

### Step 2: Run and confirm RED

```bash
cd apps/desktop
npx vitest run src/backend/client.test.ts src/backend/browserFakeApi.test.ts
```

### Step 3: Implement the boundary

- Add TS domain types matching Rust serde exactly.
- Add API interface/client methods.
- Add Tauri commands and register them.
- Use the normal request/event wait pattern and timeout behavior already used by `load_room_settings` and `invite_user`.
- Make browser fake state behave like the Rust reducer, including pending and failure fixtures.

### Step 4: Run boundary gates

```bash
cd apps/desktop
npx vitest run src/backend/client.test.ts src/backend/browserFakeApi.test.ts
npm run typecheck
cd ../..
cargo test -p koushi-desktop load_space_members --lib -- --nocapture
cargo check -p koushi-desktop
git diff --check
```

### Step 5: Commit

```bash
git add apps/desktop/src-tauri apps/desktop/src/backend apps/desktop/src/domain/types.ts
git commit -m "feat: expose Space member audit to desktop"
```

---

## Task 4: Build the approved Space Members UI

**Files:**

- Modify: `apps/desktop/src/components/Shell.tsx`
- Modify: `apps/desktop/src/components/Shell.test.tsx`
- Modify: `apps/desktop/src/components/PeoplePanel.tsx`
- Modify: `apps/desktop/src/components/PeoplePanel.test.tsx`
- Modify: `apps/desktop/src/components/rightPanel.tsx`
- Modify: `apps/desktop/src/domain/rightPanel.ts`
- Modify: `apps/desktop/src/domain/rightPanel.test.ts`
- Modify: `apps/desktop/src/domain/contextMenus.ts`
- Modify related context-menu tests
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/styles.css` or the existing stylesheet that owns sidebar/People styles
- Modify: `apps/desktop/e2e/member-list.spec.ts`

### Step 1: Add failing component tests

Cover:

- Members entry renders above DMs/Rooms only when a Space is active.
- Count is joined count plus separate `+N` child-only warning.
- Panel section order is Space members, Invitation pending, Not in Space.
- Search placeholder is `Search space members` and search filters all sections.
- Child-only row renders child room context.
- Inline and context-menu actions call the same invite callback.
- Pending/permission-disabled buttons are accessible.
- Incomplete-sync notice is driven by Rust state.
- Existing room People tests remain unchanged and passing.

### Step 2: Run and confirm RED

```bash
cd apps/desktop
npx vitest run src/components/Shell.test.tsx src/components/PeoplePanel.test.tsx src/domain/rightPanel.test.ts
```

### Step 3: Add the Space Members navigation entry

In `Sidebar`, when `activeSpace` exists, render a `Members` nav button between the workspace header and `RoomListControls`. Use Rust-owned counts and open `PeoplePanelScope { kind: "space" }`. Do not render it in account Home.

### Step 4: Add the classified panel presentation

Keep room People using the existing flat `RoomSettingsSnapshot.members`. For Space scope, render the dedicated `SpaceMembersState`:

1. `Space members`
2. `Invitation pending`
3. `Not in Space`

Search all sections by label, alias/original label, and user ID. Reuse virtualization where practical, but preserve section headers and accessibility metadata. Show `Some child rooms are still syncing` when incomplete count is non-zero.

### Step 5: Add invite controls

For child-only rows:

- Inline `Invite to Space` button.
- Context-menu `Invite to Space` action.
- Same callback/command for both triggers.
- Disable while that user is pending or current user lacks permission.
- Optimistically show pending classification from Rust state, not React-only state.

### Step 6: Add UI diagnostics

Use the existing renderer diagnostic collector to emit `ui.space_members_panel` with open trigger, section/result counts, search boolean, invite trigger kind, button availability reason, and incomplete notice boolean. Never include IDs or labels.

### Step 7: Run UI gates

```bash
cd apps/desktop
npx vitest run src/components/Shell.test.tsx src/components/PeoplePanel.test.tsx src/domain/rightPanel.test.ts
npx vitest run src/backend/client.test.ts src/backend/browserFakeApi.test.ts
npm run typecheck
npm run lint
cd ../..
git diff --check
```

### Step 8: Commit

```bash
git add apps/desktop
git commit -m "feat: add Space member audit panel"
```

---

## Task 5: Integrated review, regression verification, and DMG

**Files:**

- Review all files changed since `origin/main`
- Update tests only if a real integration gap is found
- Update: `.superpowers/sdd/progress.md`

### Step 1: Review the implementation against the design

Check every goal/non-goal in the design spec, especially:

- JOIN and INVITE are distinct.
- Child-room-only is a deduplicated JOIN union minus both Space sets.
- Room People is unchanged.
- No automatic invite or profile network fan-out exists.
- Existing encrypted SDK state is the durable source.
- Diagnostics contain no private identifiers/content.
- All behavior/state semantics are Rust-owned.

### Step 2: Run the full focused verification set

```bash
cargo fmt --all -- --check
cargo check -p koushi-sdk --lib
cargo check -p koushi-state
cargo check -p koushi-core --lib
cargo check -p koushi-desktop
cargo test -p koushi-sdk space_member --lib -- --nocapture
cargo test -p koushi-state --test space_members_state -- --nocapture
cargo test -p koushi-state --test profile_state -- --nocapture
cargo test -p koushi-core space_members --lib -- --nocapture
cd apps/desktop
npx vitest run src/components/Shell.test.tsx src/components/PeoplePanel.test.tsx src/domain/rightPanel.test.ts src/backend/client.test.ts src/backend/browserFakeApi.test.ts
npm run typecheck
npm run lint
cd ../..
git diff --check
```

### Step 3: Build the release DMG

```bash
cd apps/desktop
npm run build:dmg
```

Expected artifact:

```text
target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
```

### Step 4: Verify the artifact

```bash
ls -lh target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
shasum -a 256 target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
hdiutil verify target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
```

Do not push or create a pull request.
