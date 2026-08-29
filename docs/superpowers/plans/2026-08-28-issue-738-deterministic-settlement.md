# Issue #738 deterministic settlement — implementation design

Status: proposed; implementation is blocked until `reviewer-flash` records Correct-to-merge.

## Outcome

Complete Issue #738 Phases A–E without using retries or larger success-path timeouts to hide races:

1. product commands settle only after their authoritative versioned state is visible;
2. committed mutations identify the generation that published them;
3. room-selection publication is not blocked by cleanup/persistence transport;
4. stale Timeline viewport callbacks cannot override newer user intent;
5. focused tests use causal barriers/manual scheduling; and
6. CI reports attempt-level flake evidence separately from the required no-retry gate.

Rust continues to own Matrix/product state. React owns DOM measurement and viewport presentation only.

## Existing boundary and root cause

`AppActor` currently reduces a `SelectRoom`, emits `IntentLifecycle::Committed`, awaits post-projection and deferred effects plus persisted-state loads, and only then calls `publish_state_delta()`. Tauri's `wait_for_selected_room()` returns immediately for `Committed`, so `select_room()` can return an old snapshot.

`TimelineView` schedules viewport work through several independent frame refs. Although some call sites carry `viewportIntentRevisionRef`, scheduling and invalidation are not one contract. An older live-edge callback can therefore run after genuine user input.

## Phase A — narrow Tauri safety fix

### RED

Extend `apps/desktop/src-tauri/src/commands/navigation.rs` tests with a scripted `SelectEventSource` that emits matching `IntentLifecycle::Committed` while its snapshot still names the old room, then publishes the requested room. Assert the waiter does not resolve at the lifecycle event. Add lag recovery and benign-already-active coverage if not already explicit.

### Change

Keep `Committed` and `BenignNoOp` as progress telemetry only. Continue waiting until `event_conn.snapshot()` or a state event contains `selected_room_id`. Preserve typed early failures for `OperationFailed` and failed no-op reasons. Change the waiter to return the exact versioned snapshot that satisfied the predicate and have `select_room()` convert that snapshot directly; do not perform a separate `current_snapshot()` read that may return an unrelated newer generation.

No new timeout and no sleep.

### Gates

Focused Tauri navigation tests, `cargo test -p koushi-desktop --lib`, formatting, and relevant contract tests.

## Phase B — Core-owned typed settlement

### RED

Add Core connection tests proving:

- `Committed` before snapshot publication does not settle;
- matching versioned snapshot settles with its generation;
- broadcast lag recovers from the latest watch snapshot;
- failed/benign no-op classification remains typed; and
- unrelated request IDs or rooms do not settle; and
- when two different room selections reduce in one batch, only the request whose room exists in the published snapshot may settle committed; the superseded request receives a typed superseded/failure outcome rather than `Committed` for an unpublished intermediate state.

### Change

Add a Core-owned API on `CoreConnection` (final name chosen to fit existing naming), equivalent to:

```rust
pub async fn select_room_and_wait(
    &mut self,
    room_id: String,
    timeout: Duration,
) -> Result<VersionedAppStateSnapshot, SelectRoomError>;
```

The connection allocates the request ID, submits `RoomCommand::SelectRoom`, and waits on the watch-backed versioned snapshot predicate. Events may classify failure early but are not the reliable success transport. Outcome classification is finalized against the post-batch published snapshot, never per-action intermediate state; a same-batch selection superseded by a later room is a typed failure, not a committed settlement. Expose a typed error, not UI strings. Tauri delegates to this API and maps the typed error once; delete its duplicate settlement state machine and test-only abstraction when no longer used.

Do not add a general framework before a second command needs it.

### Gates

Core connection/runtime tests, Tauri command tests, workspace compile/test, public-item docs/doctests where required.

## Phase C — AppActor commit point and generation

### RED

Add deterministic actor tests proving:

- a selected room is published before a blocked/saturated cleanup mailbox is released;
- settlement carries the generation whose snapshot contains the selected room;
- benign/failed no-ops settle against the current generation without a fabricated delta;
- only one terminal outcome exists per request; and
- existing navigation cleanup still executes after commit.

No 20/50/100 ms success assumption: use a channel barrier/test hook that holds cleanup and separately observes the watch generation.

### Change

Make `publish_state_delta(&before_state)` return `Option<u64>`. Reorder the action loop into explicit stages:

1. reduce the action batch and synchronous derived reductions required for a consistent snapshot;
2. publish the versioned snapshot/`StateDelta`;
3. settle the correlated intent with `outcome` and `published_generation` (current generation for no-op);
4. run cleanup, persistence, refresh, and other non-authoritative post-commit work;
5. route asynchronous state-producing results back as typed actor actions that publish later generations.

Introduce the smallest typed `IntentSettlement { request_id, outcome, published_generation }` contract needed by Core. Keep `IntentLifecycle` diagnostic-only and do not make broadcast telemetry the reliable waiter transport. If a request-owned oneshot is necessary, AppActor completes it exactly once at the commit point; otherwise use the watch predicate plus typed failure event.

Classify each existing effect moved across the commit boundary in the design notes/code comments. Do not move an effect before commit unless the snapshot would be internally inconsistent without its synchronous reduction.

### Gates

Focused actor tests including 100 consecutive runs of `committed_room_cleanup_bypasses_a_saturated_account_mailbox` in single-threaded and normal parallel execution, Core workspace tests, headless core QA, Tauri tests, fmt/clippy-equivalent repository gates.

## Phase D — Timeline viewport epoch owner

### RED

Add pure scheduler/controller tests where epoch N live-edge work is queued, user intent advances to N+1, then N is flushed and cannot mutate range/scroll state. Cover room/projection replacement, cancellation, follow-up frames, and current-epoch execution. Reproduce the unread-marker failure without wall-clock polling.

### Change

Create one small Timeline viewport scheduler/controller beside `TimelineViewportVirtualization.ts`:

```ts
type ViewportEpoch = number;
interface TimelineViewportScheduler {
  currentEpoch(): ViewportEpoch;
  advance(): ViewportEpoch;
  schedule(epoch: ViewportEpoch, cb: FrameRequestCallback): TimelineScheduledFrame;
  cancelBefore(epoch: ViewportEpoch): void;
  dispose(): void;
}
```

Production uses the existing RAF-plus-timeout primitive internally. A test-only/manual implementation queues callbacks and exposes `flushNext()`/`flushAll()` without timers. `TimelineView` advances the epoch on timeline/projection replacement and genuine user viewport intent. Every viewport-mutating frame/follow-up captures an epoch and becomes inert when stale. Existing frame refs route through this owner; delete superseded revision/cancellation logic rather than layering a second mechanism.

Keep measurements, virtual-range calculation, anchoring, and scrolling in the frontend. Do not move semantic state into React.

Convert recurring unread-marker, top-pagination, and stale-live-edge tests to explicit scheduler advancement. Retain only an outer deadlock watchdog.

### Gates

Pure scheduler tests; 100 consecutive runs of the recurring unread-marker test under normal parallel Vitest configuration; full Vitest; full Playwright DOM tier; typecheck/lint/IME checks.

## Phase E — flake measurement

### Change

Keep retries disabled for required CI. Add a separate non-required flake-probe workflow/script that:

- runs the named Rust and Vitest timing-sensitive checks repeatedly against one SHA;
- records every attempt, SHA, test, mode, result, duration, and failure signature;
- uploads machine-readable JUnit/JSON plus a concise summary artifact;
- fails or reports honestly without converting retries into required-gate success; and
- supports scheduled/manual execution without secrets in logs.

Add a repository script that summarizes attempt-level results so the seven-day rate is reproducible, not hand-counted. Document commands and artifact interpretation in the existing agents verification/QA topic, with one owner for each fact.

### Gates and evidence window

- focused probe-parser tests;
- workflow syntax/static checks;
- ten consecutive full CI runs for one unchanged SHA with no rerun;
- seven days of attempt-level data below 1% failures.

Phase E implementation may land before the evidence window completes, but Issue #738 and the durable goal are not complete until both measurements are actually satisfied. The final PR may be opened for review only after implementation gates are green; it must not claim the seven-day acceptance criterion prematurely.

## Step-by-step integration and review

Luna implements one phase at a time on `fix/issue-738-deterministic-settlement`. After each phase:

1. run its RED→GREEN focused gates;
2. inspect the integrated diff and ownership boundaries;
3. obtain `reviewer-flash` Correct-to-merge for that phase diff, fixing and re-reviewing findings before the next phase;
4. commit the approved phase separately.

No implementation begins until this full design receives `reviewer-flash` Correct-to-merge.

## Full verification before PR

Run repository CI-equivalent gates: Rust workspace/Tauri/wasm tests, focused 100-run modes, typecheck, lint, all Vitest, full Playwright DOM, build, IME and SDK-submodule checks, formatting, agent-doc checks, secret/import/platform boundary checks, workflow checks, and `git diff --check`. Review the exact final diff, rebase onto `origin/main`, rerun affected gates, push, open one conflict-free PR, and monitor exact submitted-state CI.

## Explicit exclusions

No automatic retry on required CI, timeout inflation, telemetry-as-state, frontend Matrix semantics, compatibility shim, duplicated Tauri/Core waiter, or speculative generic scheduler/settlement framework.