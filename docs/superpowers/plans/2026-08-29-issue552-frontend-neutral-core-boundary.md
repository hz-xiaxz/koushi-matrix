# Issue #552 frontend-neutral Core boundary proof

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI.

## Scope

Phase 6 proves the public `koushi-core` consumption boundary without Tauri/WebView. It adds integration evidence and documentation only unless a missing public seam is found. No IPC DTO is moved into Core.

## Existing contract

`CoreRuntime` and `CoreConnection` already expose transport-neutral Rust types:

- `CoreRuntime::start*`, `attach`, and awaited `shutdown(self)`;
- connection-scoped `next_request_id` and `command(CoreCommand)`;
- `recv_event() -> CoreEvent`, bounded broadcast lag error, `snapshot()` and `versioned_snapshot()`;
- `AppStateSnapshot`/`VersionedAppStateSnapshot` and command/event enums from Core/state crates;
- ordered AppActor → AccountActor/store shutdown and media-lifecycle join.

Existing `runtime_core` tests separately prove forged connection request IDs fail locally, result events preserve submission order, action batches coalesce, and a slow consumer observes lag then resyncs from the latest snapshot. Runtime/session/send-queue suites prove deeper resource shutdown.

## Deterministic evidence to add

One public integration test, importing no Tauri crate/type:

1. start `CoreRuntime` and attach a connection;
2. record initial versioned snapshot generation;
3. allocate a connection request id and submit `AppCommand::UpdateSettings`;
4. observe the matching Rust state event/delta;
5. assert the public versioned snapshot generation advanced and contains the setting;
6. attach/drop an additional consumer to prove consumer lifetime is independent;
7. drop connections and await `runtime.shutdown()` under a bounded timeout.

Also make the existing lag/resync test explicitly drop consumers and await shutdown, proving backpressure recovery and resource teardown compose.

No sleeps or logs are evidence. Timeout is only a deadlock bound around awaited shutdown/event receipt.

## Documentation

- Record the five public boundary capabilities and tests in architecture/state ownership/inventory.
- Keep `FrontendDesktopSnapshot` and all serde IPC wrappers in `apps/desktop/src-tauri`.
- Do not add a GPUI crate; optional Phase 8 remains out of #552 closure.

## Local verification evidence

- focused `runtime_core`: 5/5;
- Rust workspace: 2579 passed / 13 ignored;
- Tauri: 177 passed / 1 ignored;
- QA binary: 135/135;
- rustfmt, wasm state/search, cargo-deny and cargo-machete: passed;
- frontend typecheck/lint/IME/docs, secret scan, adapter/domain guards, SDK sync and diagnostic isolation: passed.

The exact PR head runs the complete CI matrix, including browser DOM and both invitation servers.

## Acceptance

- A non-Tauri integration test starts Core, submits a typed command, observes event/snapshot convergence and awaits shutdown.
- Connection request ownership, lag/resync and consumer teardown are covered.
- No Tauri type/import appears in the Core contract test.
- No production behavior, IPC/DTO, security/privacy or browser/Tauri behavior changes.
