# Persisted Room Store Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore every persisted joined room and space after a Sliding Sync resume without deleting the existing database, while keeping each room ID unique and adding aggregate diagnostics.

**Architecture:** The Matrix SDK client room store is the cache-first display source. Committed-response metadata remains a liveness/loading signal, but response-local changed room IDs no longer filter the store. Koushi core deduplicates SDK entries by room ID at its projection boundary and emits privacy-preserving counts.

**Tech Stack:** Rust, matrix-rust-sdk, matrix-sdk-ui RoomListService, Tokio, Koushi diagnostics, Tauri release bundling.

## Global Constraints

- Do not delete, reset, migrate, or recreate the user's database.
- Do not log room IDs, room names, aliases, event contents, tokens, or response bodies.
- Distinct room IDs with the same display name remain distinct rows.
- Focused tests precede the rapid DMG build; broad test expansion follows user validation.

---

### Task 1: Restore cache-first RoomListService entries

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/room_list.rs`
- Test: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`

**Interfaces:**
- Consumes: `matrix_sdk::Client::rooms_stream()` and Matrix `RoomState`.
- Produces: `RoomList::current_entries_snapshot()` and `entries_with_dynamic_adapters()` whose membership is independent of response-local changed IDs.

- [ ] **Step 1: Write a failing resumed-response regression test**

Preload two joined rooms, including one space, process an incremental Sliding Sync response whose `rooms` object contains only one changed room, and assert both stored rooms remain in `current_entries_snapshot()`.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `cargo test -p matrix-sdk-ui resumed_incremental_response_preserves_persisted_rooms -- --exact --nocapture`

Expected: FAIL because the response-local ID filter hides the unchanged stored room.

- [ ] **Step 3: Remove response-local ID visibility filtering**

Make snapshots and dynamic entries consume all SDK rooms and retain only membership/filter-adapter checks. Preserve observed response sequence, range-loaded, and maximum-count metadata in `RoomListEntriesSnapshot` without using observed IDs to choose entries.

- [ ] **Step 4: Run focused SDK tests**

Run: `cargo test -p matrix-sdk-ui resumed_incremental_response_preserves_persisted_rooms -- --exact --nocapture`

Expected: PASS with both persisted rooms present.

Run the existing authoritative snapshot and local-room race tests that share this boundary; update expectations only where they incorrectly require unchanged cached joined rooms to disappear.

### Task 2: Enforce projection identity and diagnostics

**Files:**
- Modify: `crates/koushi-core/src/room.rs`
- Test: `crates/koushi-core/src/room.rs`

**Interfaces:**
- Consumes: live `Vector<RoomListItem>` from RoomListService.
- Produces: a room-ID-unique collection passed to `room_list_snapshot_from_sdk_rooms`, plus aggregate `core.room` diagnostics.

- [ ] **Step 1: Write failing identity tests**

Add tests proving repeated entries with the same room ID normalize once, while two IDs with the same display name remain separate. Verify diagnostics calculate input, unique-ID, duplicate-entry, membership, and name-collision counts without including identifiers.

- [ ] **Step 2: Run the focused core tests and verify failure**

Run the two new test names with `cargo test -p koushi-core <test-name> -- --exact --nocapture`.

Expected: FAIL before the deduplication/aggregate helper exists.

- [ ] **Step 3: Add one projection-input helper**

Collect entries by room ID in a `BTreeMap`, classify membership, and return joined/invited vectors plus aggregate counts. Emit those counts with response/range metadata already available at the observer boundary. Never emit identifier or display-name values.

- [ ] **Step 4: Run focused core tests**

Run the two new tests and existing partial/authoritative reconciliation tests.

Expected: PASS; a duplicate ID projects once and same-name different IDs project separately.

### Task 3: Build and verify the rapid DMG

**Files:**
- Build artifact: `matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

**Interfaces:**
- Consumes: focused-test-passing workspace.
- Produces: installable macOS arm64 DMG for testing against the unchanged database.

- [ ] **Step 1: Build release DMG without broad tests**

Run the repository's established release DMG command from `matrix-desktop`, preserving the existing SDK submodule working tree.

Expected: Tauri release build completes and writes the DMG.

- [ ] **Step 2: Validate the artifact**

Run: `hdiutil verify target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

Expected: `VALID`.

Run: `shasum -a 256 target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

Expected: one SHA-256 digest for handoff.

- [ ] **Step 3: User validation checkpoint**

Install over the existing application, retain the existing database, and confirm spaces and the full joined-room list return. If same-name rows remain, use the new collision and duplicate counts to determine whether they are distinct Matrix rooms.

### Task 4: Formalize and publish after local validation

**Files:**
- Test: SDK and Koushi core tests from Tasks 1 and 2
- Modify: only files changed by the validated fix

**Interfaces:**
- Consumes: successful user validation with the existing database.
- Produces: committed implementation, pushed branch, ready-for-review PR.

- [ ] **Step 1: Run the relevant package test suites and formatting checks**

Run focused package suites, `cargo fmt --check`, frontend diagnostics tests, and repository-required checks proportional to the changed files.

- [ ] **Step 2: Review the exact diff and commit intentionally**

Stage only the room-store projection, diagnostics, tests, and existing Sliding Sync fixes belonging to this branch. Preserve unrelated user changes.

- [ ] **Step 3: Push and prepare the PR**

Push `codex/sliding-sync-runtime-diagnostics`, update the PR description with the old-DB reproduction and test evidence, and mark it ready for review when checks pass.
