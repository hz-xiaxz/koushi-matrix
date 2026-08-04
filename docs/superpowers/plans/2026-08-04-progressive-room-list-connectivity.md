# Progressive Room-List Connectivity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep a working Sliding Sync runtime connected while its growing all-rooms range continues loading, without granting partial room lists removal authority.

**Architecture:** Extend the RoomActor reconciliation acknowledgement with a `Projected` result. A delivered response-correlated partial projection releases SyncActor's liveness wait, while RoomActor retains reconciliation metadata and independently promotes the projection to authoritative only after the complete range arrives.

**Tech Stack:** Rust, Tokio oneshot channels, Matrix Rust SDK RoomListService, Koushi actor state machines, Tauri DMG tooling.

## Global Constraints

- A partial list must never be authoritative or remove cache-only rooms.
- A delivered response-correlated projection may establish connectivity.
- Fixed page-loading duration must not terminate SyncService, encryption, timelines, or messaging.
- Closed channels, failed AppState delivery, and invalid generation/sequence correlation remain failures.

---

### Task 1: Partial Projection Acknowledgement

**Files:**
- Modify: `crates/koushi-core/src/room.rs:266`
- Modify: `crates/koushi-core/src/room.rs:3220`
- Modify: `crates/koushi-core/src/sync.rs:680`
- Test: `crates/koushi-core/src/room.rs:6530`
- Test: `crates/koushi-core/src/sync.rs:1450`

**Interfaces:**
- Produces: `RoomListReconcileAck::Projected { backend_generation, room_generation, response_sequence }`.
- Preserves: `RoomListReconcileAck::Reconciled` for a complete authoritative range and `Superseded` for newer incomplete correlation.

- [ ] **Step 1: Write failing tests**

Add a RoomActor reconciliation test proving a partial delivered projection emits `Projected`, retains pending reconciliation, and stays non-authoritative. Extend the SyncActor ACK-classification test to accept matching `Projected` and reject mismatched generation or sequence.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p koushi-core --lib partial_projection_acknowledges_connectivity_without_authority`

Expected: FAIL because the `Projected` variant and retained-ack behavior do not exist.

Run: `cargo test -p koushi-core --lib projected_room_list_ack_is_connectivity_evidence`

Expected: FAIL because SyncActor cannot classify `Projected`.

- [ ] **Step 3: Implement retained reconciliation state**

Change pending reconciliation to retain `(backend_generation, response_sequence, Option<oneshot::Sender<_>>)`. After successful partial AppState delivery, take only the sender and emit `Projected`; retain generation and sequence. On complete delivery, remove pending state, mark authority, and emit `Reconciled` only if its sender was not already consumed.

- [ ] **Step 4: Implement SyncActor classification**

Add `RoomListReconcileResult::Projected` and accept a matching positive room generation and response sequence. Treat it like `Reconciled` for liveness while leaving RoomActor authority unchanged.

- [ ] **Step 5: Verify GREEN**

Run both focused tests and the adjacent `committed_response_becomes_authoritative_only_after_matching_full_range` test; expect zero failures.

### Task 2: Fast DMG Verification

**Files:**
- Build: `target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`
- Copy: `/Users/hiroshi/projects/Element-dev/matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

**Interfaces:**
- Produces: installable arm64 DMG containing token migration and progressive connectivity.

- [ ] **Step 1: Run formatting and focused tests**

Run `cargo fmt --all -- --check` and the Task 1 focused tests.

- [ ] **Step 2: Build the DMG without broad preflight**

Run: `npm --prefix apps/desktop run build:dmg -- --skip-preflight`

- [ ] **Step 3: Verify and publish locally**

Run `hdiutil verify`, calculate SHA-256, copy to the canonical path, and verify the copied checksum matches.

### Task 3: Formal Coverage And PR

**Files:**
- Modify: `crates/koushi-core/src/sliding_sync_diagnostics.rs`
- Modify: `crates/koushi-core/tests/sliding_sync_diagnostics.rs`
- Modify: existing diagnostics transport files on the branch
- Modify: `docs/upstream/matrix-rust-sdk-feedback.md`

**Interfaces:**
- Produces only coarse privacy-safe fields for first projection acknowledgement, full-range authority, and reconciliation progress.

- [ ] **Step 1: Add failing diagnostic and delayed-range integration tests**

Model a committed partial range lasting longer than ten seconds and prove Running/message owners survive until later authoritative promotion. Assert copied diagnostics contain no IDs, tokens, positions, URLs, or raw errors.

- [ ] **Step 2: Implement fixed-enum diagnostic plumbing**

Record projection acknowledgement and authority separately without free-form values.

- [ ] **Step 3: Run affected Rust and frontend diagnostic suites**

Run targeted Matrix SDK, `koushi-core`, Tauri DTO, and frontend diagnostic tests with zero failures.

- [ ] **Step 4: Review, commit, push, and create a ready PR**

Inspect the complete diff, run the repository secret scan, commit logical scopes, push `codex/sliding-sync-runtime-diagnostics`, and open a ready-for-review PR against `origin/main`.
