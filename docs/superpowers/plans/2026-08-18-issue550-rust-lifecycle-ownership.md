# Issue #550 — Rust lifecycle ownership and leak cleanup

Status: implemented and verified locally (2026-08-18)

## Scope and ownership

This change makes lifecycle ownership explicit and repairs the concrete leaks
found by the repository-wide audit.

- Rust owns product-operation pending/retry/cancellation state, SDK subscriptions,
  actor children, and background tasks that can outlive one mounted DOM owner.
- React owns only browser/DOM resources whose lifetime is exactly one mounted
  presentation owner; the same effect or controller must cancel them on key
  change and unmount.
- Tauri and QA adapters may own platform/process/file handles only while adapting
  a Rust-owned intent or running one bounded scenario, with explicit settlement.

No Matrix state-machine, command/event wire shape, or UX change is in scope.

## Verify-first checks

1. A threads-list active subscription with pending relay/update tasks must settle
   every task during replacement and shutdown; the pre-fix owner detaches them.
2. A FIFO credential writer must time out without leaving a blocked open, open
   descriptor, or late payload write after a reader appears.
3. Timeline acknowledgement retry timers must not survive a signature replacement
   or unmount.
4. Search actor shutdown must acknowledge actor completion after cancelling its
   query/crawler/timer children.

## Implementation sequence

1. Amend the durable ownership canon before production code.
2. Add focused regressions and record their pre-fix failure.
3. Give `ThreadsListActorHandle`, `ActiveSubscription`, and `SearchActorHandle`
   retained task ownership with explicit cancel-and-await shutdown barriers.
4. Replace the uncancellable FIFO `open()` race with one shared nonblocking,
   absolute-deadline writer used by Linux and macOS QA scripts.
5. Cancel an existing TimelineView acknowledgement timer before replacing its
   slot and keep unmount/key-change cleanup authoritative.
6. Run focused tests, full layer gates, preflight review, and CI before merge.

## Acceptance

- No raw `JoinHandle` drop is used as teardown on the changed actor paths.
- Close, replacement, failure, account shutdown, and unexpected owner drop all
  settle or abort retained work without leaving a live subscription/task tree.
- FIFO timeout is fail-closed for credential delivery and private-data-free.
- React retains only DOM/render lifecycle resources; product retries remain Rust-owned.
- The full diff passes the repository preflight checklist and required CI.

## Verification and review record

The four focused regressions were observed RED before the fixes: both Rust
shutdown checks timed out, the FIFO test observed a 17-byte late credential
write, and the TimelineView test retained one timer after unmount. The same
checks pass after the implementation.

Local green gates:

- `cargo test -p koushi-core --lib`: 1,018 passed, 8 ignored.
- `cargo test --workspace`: 2,389 passed, 13 ignored.
- Core QA binary tests: 129 passed.
- Desktop Vitest: 1,361 passed; browser-headless Playwright: 248 passed.
- Tauri lib: 149 passed, 1 ignored; SDK lib: 143 passed; state lib: 38 passed.
- Desktop typecheck, lint, build, secret scan, adapter/domain boundary checks,
  wasm check, agents-doc check, SDK-submodule check, rustfmt, and diff check pass.

The workspace gate exposed two pre-existing `origin/main` fixture-backend
breakages: newly added production-only `AppEffect` variants were absent from the
historical fake executor, and one room-order expectation predated the canonical
label fallback. The branch explicitly ignores those effects only in
`koushi-backend` and aligns that synthetic expectation; production paths are
unchanged.

Preflight traced the production ownership paths, found no command/event/DTO or
state-machine change, and confirmed that unexpected drops abort children while
orderly close/replacement/shutdown await settlement. Browser-headless required
`CHOKIDAR_USEPOLLING=1` locally because the host inotify watch limit was already
exhausted; all 248 tests then passed.
