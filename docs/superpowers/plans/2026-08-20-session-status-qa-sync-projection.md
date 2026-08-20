# Headless QA projection stabilization

## Scope

Fix the pre-existing disposable-homeserver QA race exposed while verifying Issue #551. This is a separate behavioral repair based on `origin/main`; it must merge before the move-only headless QA decomposition is rebased.

## Verify-first evidence

On unchanged `origin/main`, the focused command

```bash
node scripts/desktop-headless-local-qa.mjs --run --server=tuwunel --core \
  --scenario=session_status --timeout-ms=600000 --cargo-profile=release
```

intermittently exits 1 after printing `sync_a=running`. A temporary private-safe diagnostic split proved the rejected status detail was `sync_state`, not `authentication_method`. The same failure occurs on the Issue #551 decomposition and its immutable pre-move base.

## Root cause

`wait_for_sync_started_and_running` currently treats the raw `CoreEvent::Sync(SyncEvent::Running)` notification as sufficient. The helper can therefore return before the corresponding Rust-owned `AppState.sync == SyncState::Running` projection is observable. `run_session_status_stage` immediately snapshots that product state and correctly reports the still-transitional sync fact.

## Change 1: synchronize on the state projection

In `crates/koushi-core/src/bin/headless-core-qa.rs`, keep waiting until both conditions hold:

1. the request-scoped `SyncEvent::Started` was observed; and
2. `CoreConnection::snapshot().sync` is `SyncState::Running`.

The raw `SyncEvent::Running` remains useful wake-up evidence but is no longer itself the completion boundary. Check the current snapshot after each received event so either event ordering is accepted without a sleep, retry, new abstraction, timeout change, production-runtime change, or relaxed assertion.

## Change 2: align the native-attention QA badge expectation

After Change 1 exposed the next stage in `--scenario=all`, unchanged `origin/main` consistently failed `native_attention` at the per-room notification-mode assertion. The fixture has unread counts 8, 3, and 1. Its modes make the 8-count room effectively muted, preserve the 3-count room's raw badge under `Mentions`, and leave the 1-count mention room unchanged. The product and its focused state tests therefore correctly project badge count `4`; the headless QA still expects the pre-Issue-543 total `12`.

Change only that stale expected badge value from `12` to `4`. Keep its unread, highlight, candidate, and mode assertions unchanged. This restores the QA contract rather than changing product behavior or weakening coverage.

## Ownership and compatibility

- Rust product state and lifecycle behavior are unchanged.
- Only the existing private QA event waiter and one stale expected badge value change.
- No public API, wire/serde contract, scenario/token registry, secret handling, cleanup order, timeout, or Tauri/frontend behavior changes.
- The later Issue #551 move must carry this exact helper body into `headless_core_qa/event_wait.rs`.

## Verification

1. `cargo test -p koushi-core --features qa-bin --bin headless-core-qa`
2. Run the focused disposable Tuwunel `session_status` command at least three consecutive times.
3. Run focused disposable Tuwunel `native_attention`.
4. Run disposable `--server=both --core --scenario=all`.
5. Run repository Rust/static/full gates required by CI once after review.
6. Review the full diff with the selected read-only cross-model reviewer before merge.
