# Issue #375 Authoritative Trust Recheck Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure every production `CheckCurrentDeviceTrust` effect performs a generation-safe, authoritative own-device trust recheck and always settles the verification gate.

**Architecture:** `koushi-sdk` will own the Matrix keys-query semantics and return the trust projection after the query updates SDK state. `AccountActor` will own one cancellable recheck task, correlate its completion with `trust_generation`, and reuse the existing authoritative trust transition path. Both AppActor effect lanes will route the reducer effect to that actor message; an already-pending transition will absorb the redundant initial-login effect.

**Tech Stack:** Rust, Tokio actors, matrix-rust-sdk encryption API, existing `koushi-core` unit/runtime tests.

---

### Task 1: Reproduce the dropped effect and stale cached-read trap

**Files:**
- Modify: `crates/koushi-core/src/runtime.rs`
- Modify: `crates/koushi-sdk/src/lib.rs`

- [x] **Step 1: Add a runtime structure guard for both effect lanes**

Add `runtime_routes_current_device_trust_rechecks_in_both_effect_lanes`, bounded to `handle_app_effects` and `handle_post_projection_effects`, and require an explicit `AccountMessage::CheckCurrentDeviceTrust` route in each lane.

- [x] **Step 2: Add an SDK behavior test for the authoritative request**

Use `MatrixMockServer::mock_query_keys().ok().expect(1)` and call `MatrixClientSession::recheck_current_device_trust()`. Assert that the method settles with a product trust value and that the mock verifies exactly one `/keys/query` request.

- [x] **Step 3: Run the focused tests and confirm RED**

Run:

```bash
cargo test -p koushi-core --lib runtime_routes_current_device_trust_rechecks_in_both_effect_lanes
cargo test -p koushi-sdk --lib recheck_current_device_trust_queries_own_identity
```

Expected: the core guard fails because both lanes discard the effect; the SDK test fails to compile because the authoritative recheck API does not exist.

### Task 2: Add the SDK-owned authoritative query

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`

- [x] **Step 1: Implement `MatrixClientSession::recheck_current_device_trust`**

Subscribe before the request, query the signed-in user's identity through `Encryption::request_user_identity`, then map the updated verification subscriber value into `CurrentDeviceTrustState`. Return a coarse SDK error without exposing raw server data.

- [x] **Step 2: Run the focused SDK test**

Run:

```bash
cargo test -p koushi-sdk --lib recheck_current_device_trust_queries_own_identity
```

Expected: PASS, with the mock observing one own-user keys query.

### Task 3: Route and own the recheck in AccountActor

**Files:**
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/runtime.rs`

- [x] **Step 1: Add actor messages and owned task state**

Add `CheckCurrentDeviceTrust` and a generation-tagged completion message. Add one `trust_recheck_task` field, abort it during provisional runtime teardown, and ignore stale completions through the existing `trust_generation` decision path.

- [x] **Step 2: Add actor behavior tests**

Cover: a recheck result reaches `AuthoritativeDeviceTrustChanged`; a stale generation cannot promote or lock; query failure projects `Unknown`, which the reducer maps to the existing retryable SDK gate failure; and a pending authoritative transition absorbs the redundant initial-login recheck.

- [x] **Step 3: Run the new actor tests and confirm RED**

Run:

```bash
cargo test -p koushi-core --lib authoritative_trust_recheck
```

Expected: FAIL before the actor implementation, then PASS after the minimal task/message implementation.

- [x] **Step 4: Route both production effect lanes**

Replace `CheckCurrentDeviceTrust` in each ignored-effect catch-all with an explicit send of `AccountMessage::CheckCurrentDeviceTrust`.

- [x] **Step 5: Run focused regression gates**

Run:

```bash
cargo test -p koushi-core --lib runtime_routes_current_device_trust_rechecks_in_both_effect_lanes
cargo test -p koushi-core --lib authoritative_trust_runs_through_app_actor_ack_and_restarts_real_children
cargo test -p koushi-core --lib authoritative_trust_recheck
```

Expected: PASS. In particular, the existing initial-promotion regression test remains green.

### Task 4: Integrated verification and standalone PR

**Files:**
- Review all modified and untracked files in this worktree.

- [x] **Step 1: Run formatting and package gates**

Run:

```bash
cargo fmt --all -- --check
cargo test -p koushi-sdk --lib
cargo test -p koushi-core --lib
```

Expected: all commands exit 0.

- [x] **Step 2: Run the local Conduit regression scenario**

Run the issue reproduction against the probed core backend:

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$PATH npm --prefix apps/desktop run qa:headless-local -- --server=conduit --scenario=media --core --core-backend=probed --timeout-ms=240000
```

Expected: exit 0 without a `phase=rechecking_trust` login timeout.

- [x] **Step 3: Self-review and publish**

Inspect `git diff origin/main...HEAD` and `git status --short`, commit only #375 files, push `codex/issue-375-trust-recheck`, open a standalone PR referencing `Fixes #375`, and explicitly inspect the non-required Core homeserver QA result before merge.
