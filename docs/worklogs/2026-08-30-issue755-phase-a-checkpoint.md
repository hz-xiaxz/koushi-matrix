# Issue #755 Phase A checkpoint

Status: Phase A2a implemented and locally verified; additional different-model integration checkpoint pending.

## Scope

This checkpoint covers the Core request-outcome service plus the Phase A
session, local-data, navigation, and search adapter migrations. It does not
implement phases B–E. Phase A adds runtime settlement infrastructure only; it
adds no reducer transition or AppState/AppAction change.

## Canon and design references

- Approved design: `docs/superpowers/plans/2026-08-30-issue755-thin-tauri-adapter.md`
- Architecture: `docs/architecture/overview.md` (Core request outcomes)
- State machine: `docs/architecture/state-machine.md` (Phase A, no reducer transition)
- Ownership inventory: `docs/architecture/frontend-ownership-inventory.md`
- Repository verification: `docs/agents/verification.md`

## Implementation

- `crates/koushi-core/src/runtime/request_outcome.rs` owns the closed,
  non-serde correlation, expectation, outcome, and error types plus the
  versioned snapshot/event waiter.
- `CoreConnection::select_room_and_wait` delegates to the service.
- Core-only test support and focused outcome tests cover the approved matrix.
- Session login/discovery, local-data reset, focused navigation, and search
  commands now capture a baseline, submit through the command connection, and
  settle through Core's typed request-outcome service on an attached wait
  connection. Search guards exact request, account, query, and scope values;
  search lag is terminal after Core's final snapshot check.
- Superseded adapter loops, search event-source traits, search timeout handling,
  and their adapter-only test are deleted. The diagnostic recovery-prompt
  waiter remains outside product settlement.

## Verification

- `cargo test -p koushi-core --test request_outcome`: 10 passed.
- `cargo test -p koushi-core --test request_outcome_a2a`: 10 passed.
- `cargo test -p koushi-desktop`: 126 library tests passed on full rerun; 5 integration tests passed. One pre-existing global diagnostic-context count test failed in the first parallel run and passed focused plus full rerun without changes.
- Strict Rust test-structure checker, checker tests, rustfmt, and `git diff --check`: passed.

## Integration checkpoint

- A1 reviewer: `deepseek-brainstormer` (read-only), `VERDICT: CORRECT-TO-CONTINUE`.
- A2a reviewer round 1: `deepseek-brainstormer`, `VERDICT: FINDINGS`. Real-runtime audit showed local reset, focused close, and search close do not emit the synthetic terminals used by initial unit tests. The service now has explicit `allow_projection_only` admission for these idempotent commands, still requiring a newer exact guarded snapshot; adapter callers opt in and RED/GREEN tests prove settlement without unavailable events. Search/auth/account guards and outcome mapping remain typed.
- A2a reviewer round 2: `deepseek-brainstormer`, `VERDICT: FINDINGS`. It found `SnapshotWake::SnapshotChanged` did not re-evaluate projection-only expectations, causing 10–60 second deadline settlement. The loop now rechecks authoritative projection at every iteration; a deterministic `now_or_never` test proves reset/focused-close/search-close settle on the first matching watch update without any unavailable terminal event.
- A2a reviewer round 3: `deepseek-brainstormer`, `VERDICT: CORRECT-TO-CONTINUE`; immediate watch settlement and non-opt-in/foreign guards verified.
- This is an additional same-design slice checkpoint, not a restarted pre-implementation gate.
- A2 must implement and RED-test every currently declared expectation before its adapter waiter is migrated; unimplemented variants may not ship silently.
- A2 must use operation-specific room guards (`RoomForgotten` settles on authoritative absence; leave uses its actual projected terminal) rather than the generic known-room predicate.
- A2 also tightens authenticated session state, submission account/target guards, exact account keys for adapter calls, and search correlation. The vacuous lag-loop assertion found in review was removed immediately.
