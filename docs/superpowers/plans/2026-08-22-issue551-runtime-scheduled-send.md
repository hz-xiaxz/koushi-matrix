# Issue #551 runtime scheduled-send extraction

Status: full diff approved; delivery pending. Scope is one atomic behavior-preserving local scheduled-send lifecycle seam.

## Baseline

- Base: `04938d29604dd99cb82ba6a508c7f37de0396bb1` after navigation support PR #621.
- `runtime.rs`: 7,148 newline-delimited lines / 304,019 bytes / SHA-256 `1b192b1f6421894c1cdb88683975067ceaf905e741959727287b353b506ac487`.
- Focused baseline: `runtime_scheduled_send` 12/12, core lib `scheduled_send` filter 16/16, state scheduled-send 9/9, persistence source contract 1/1.

## Ownership decision and immutable order

Create private `crates/koushi-core/src/runtime/scheduled_send.rs`. Preserve this global immutable-source order; do not regroup by item kind:

1. `DeferredScheduledSendPersist`
2. `current_epoch_ms`
3. `scheduled_send_id`
4. `AppActor::load_scheduled_sends_for_current_session`
5. `AppActor::persist_scheduled_sends`
6. `AppActor::scheduled_send_delay`
7. `AppActor::dispatch_due_scheduled_send`
8. `AppActor::dispatch_scheduled_send`
9. `scheduled_send_session_key`

Inventory: four top-level identities and five `AppActor` methods. Bodies, variants, diagnostics, transaction-ID generation and order remain exact except approved descendant-module visibility.

The leaf owns encrypted local scheduled-send load/save, local deadline calculation, one-due-item selection and fail-closed AccountActor dispatch. It adds no actor, task, channel, timer, counter, map, store or state owner.

## Parent-owned orchestration and order

Keep every `AppActor` field and these boundaries in `runtime.rs`:

- `scheduled_sends_loaded_for`, authoritative `AppState`, shared `next_internal_request_sequence` and AccountActor handle;
- `AppActor::run`, the one timer delay calculation/select arm, before-state clone and one publication;
- `reduce_app_action_state`, `DeferredReducerSideEffects` and its scheduled-send field;
- `apply_deferred_reducer_side_effects`, including `ClearLoadedMarker` handling;
- `next_internal_request_id` and the global saturating internal-ID owner;
- exhaustive schedule/cancel/reschedule command arms, slash-command validation, revision/permit admission and server-vs-local routing;
- action channels, both effect registries and shutdown.

Preserve these orderings exactly:

1. scheduled state loads once per session marker at the end of the action batch, after actor projection/effects and immediately before the one batch publication;
2. lock/logout/account switch clears only the loaded marker and never persists transient empty session views;
3. due dispatch requires `SessionState::Ready`, selects one non-server/non-dispatching due item, then recomputes the deadline on the next loop;
4. reduce `ScheduledSendDispatchStarted` and complete deferred persistence before AccountActor forwarding;
5. emit UI effects, allocate one shared internal request ID, then send with exact origin session key;
6. AccountActor rechecks the origin key and uses deterministic scheduled-ID transaction IDs;
7. closed mailbox emits `ShutdownFailed`, reduces a retry timestamp, persists through the normal deferred path and clears dispatching state;
8. cancel/reschedule and timer dispatch remain serialized by the one AppActor loop; no detached timer or batch dispatcher;
9. all filesystem I/O stays in awaited `executor::spawn_blocking`.

## Visibility and imports

No public API or re-export is added. Parent declares private `mod scheduled_send;` with the other private modules and explicitly imports three `pub(super)` top-level identities: `DeferredScheduledSendPersist`, `scheduled_send_id`, and `scheduled_send_session_key`. `current_epoch_ms` stays private.

Four moved `AppActor` methods become `pub(super)` because parent orchestration calls them: load, persist, delay and due-dispatch. `dispatch_scheduled_send` remains private to the leaf. Enum variants need no separate visibility edge.

Remove exactly three parent production bindings made orphaned by the move: `SystemTime`, `UNIX_EPOCH`, and `ScheduledSendStore`. Retain `Duration`, `ScheduledSendItem`, `ScheduledSendCapability`, `ScheduledSendHandle`, `SessionState`, `CoreFailure`, `AccountMessage`, `CoreEvent`, `executor` and composer session support for parent production/tests.

Leaf production imports are explicit; no production glob, wrapper, trait, alias, path attribute, compatibility shim or reuse of a second counter/timer. Consolidating the pre-existing epoch helper with `crate::scheduled_send::current_epoch_ms` is behavior redesign and out of this move-only scope.

## Tests and source contract

Move no unit tests: integration/reducer/account/store owners remain in their existing files, so all 1,029 core lib test identities and paths are byte-exact.

Keep `app_actor_persistence_uses_blocking_store_port` parent-owned and adjust only owner-file plumbing:

- separate `runtime/scheduled_send.rs` source checks load-scheduled through `async fn persist_scheduled_sends` and persist-scheduled through `fn scheduled_send_delay` for `executor::spawn_blocking`;
- parent `runtime.rs` continues checking room-preference through the retained `fn next_internal_request_id` boundary and the settings section;
- existing navigation and composer source checks remain separate;
- never concatenate source strings or weaken/remove an assertion.

The 12 runtime integration tests, 16 core scheduled-send-filter tests, nine state tests, AccountActor scheduled-send tests and encrypted store tests remain in their current owners.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- global production inventory 9/9 and parent 0;
- top-level identities 4/4, AppActor methods 5/5 keyed by `(AppActor, method, name)`;
- top-level `pub(super)`3, method edges4, parent explicit imports3, production orphan bindings3;
- all 1,029 core lib test identities and paths exact; source-contract test retains assertions with only approved owner-path/boundary edits;
- parent fields/run/timer/select/publication/reducer/deferred/command/effect/counter/shutdown and all lifecycle order byte-exact;
- duplicate/missing/excess identities, public/wire/resource/dependency deltas 0.

## Verification

Run focused baselines, full core library, AccountActor scheduled-send and store owners, `cargo check -p koushi-core --all-targets --all-features`, exactness/source/order checks, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main` if required, obtain delta approval, run the complete repository matrix and PR CI7/7.

## Review gate

- Read-only reconnaissance traced timer, reducer/deferred persistence, origin-session fence, retry and shutdown/resource boundaries.
- `reviewer-flash` independently traced all nine identities, visibility/import closure, timer/reducer/persistence/session-fence ordering and source-test boundaries and recorded `Correct-to-implement`.
- Shell-confirmed baseline is 7,148 newline characters (`wc -l`) / 304,019 bytes; editor line numbering includes the final non-newline-delimited display line.
- Implementation integrated by `luna-implementer` and parent-audited.
- Exactness: global production9/9, top-level4/4, AppActor methods5/5, top-level `pub(super)`3, method edges4, parent imports3, orphan bindings3, all1,029 test identities/paths exact; public/wire/resource deltas0.
- `runtime.rs` 7,148 → 7,035 newline-delimited lines; `runtime/scheduled_send.rs` 137.
- Focused post-move runtime12, core filter16, state9, source contract1, core lib1,021/8 ignored and all-targets/all-features check green.
- Full diff: `reviewer-flash` independently traced all nine identities, lifecycle order, visibility/imports, source assertions and parent resource ownership and recorded `Correct-to-merge`.
- Final local evidence: focused scheduled-send/source suites green, core lib1,021/8 ignored, Vitest1,370, Playwright248 with polling, desktop149/1 ignored and Headless Core QA130; typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/format/exactness/diff gates green.
- Initial workspace run reproduced the known process-global composer diagnostic observation in `corrupt_load_attempts_once_per_session`; exact test, complete runtime-timeline21 and complete workspace reruns were green. Scheduled-send persistence code has no edge to that diagnostic.
- Delivery: latest-main integration if required, PR CI and merge pending.
