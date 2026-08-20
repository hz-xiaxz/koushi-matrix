# Issue #551: AccountActor feature-seam decomposition

## Status

- Design: `reviewer-flash` approved the base design and all bounded visibility amendments as `Correct-to-implement`, including the final seventeen-field scope through `ef46184`; non-listed fields are narrowed and verified owner-private. Compile work may resume.
- Implementation: blocked until the design reviewer records `Correct-to-implement`.
- Full-diff review, final repository gates, PR CI, and merge: pending.

## Objective

Replace the 19,819-line `crates/koushi-core/src/account.rs` with a small private-module façade and twelve existing feature/ownership modules. Move existing declarations, actor methods, tests, attributes, cfg gates, strings, state fields, task/subscription owners, cancellation, generation/request fencing, reliable settlement, and call order without changing behavior or existing root paths.

This is source ownership only. It does not redesign AccountActor, split its state into new objects, change Matrix/session/trust/E2EE semantics, alter command/event/reducer/DTO/wire contracts, change diagnostics, or repair any behavior.

## Immutable baseline

- Commit: `c8cedb8895449efa3b45bc1aac43df57947194dd`
- Source: `crates/koushi-core/src/account.rs`
- Size: 19,819 newline-terminated lines; 775,269 bytes
- SHA-256: `7a50ba636f35d3f1c773d96243e47c815382050122c61f7efcb5cf1301b0fac7`
- Named production/cfg-test top-level items plus associated methods outside the inline test module: 391 (171 top-level declarations, including the existing `trace_restore` macro, + 220 type-qualified associated methods)
- `AccountActor` methods: 200
- Account unit tests: 150 unique names
- Inline source-characterization tests/sites: 32/32; one additional external account-source site exists in `renderable_thumbnail.rs`
- Core lib baseline, default and `test-hooks`: 1,014 passed, 8 ignored
- Focused integration baselines: session 8, device-session 2, E2EE 2, scheduled-send 12, activity 9, search 1, timeline 21, room-list 6, intent-lifecycle 5, residency 25; all green

Every extraction and body/token comparison uses `/tmp/account-baseline.rs`, copied from this immutable commit before any edit. Line ranges are navigation hints only. The normative per-owner key, test, source-site, and visibility inventories are in [2026-08-21-issue551-account-actor-inventory.md](2026-08-21-issue551-account-actor-inventory.md); its production counts sum to 391 and its test counts sum to 150.

## One ownership-area PR

AccountActor is one state and lifecycle ownership area. Its sibling inherent impls share one actor field set, one mailbox, one session generation, and one ordered teardown barrier. Separate PRs would require temporary wrappers, duplicate state, widened interfaces, or incomplete dispatch. Deliver the private module graph atomically and review the whole resource graph once.

This exception permits no behavior fix. If extraction requires changed state, ordering, retry, timeout, fencing, cleanup, security, privacy, or public contracts, stop and handle it separately under verify-first discipline.

## Target layout

```text
crates/koushi-core/src/
├── account.rs                         # private declarations + exact flat façade
└── account/
    ├── account_management.rs          # devices, password, deactivation, UIA
    ├── actor.rs                       # contracts, fields, spawn, mailbox/command dispatch
    ├── local_data_cleanup.rs          # encryption health, device cleanup, local reset
    ├── profile.rs                     # presence, profile, aliases, avatar, ignore/report
    ├── recovery_backup.rs             # recovery, backup, room-key and identity-reset SDK work
    ├── routing.rs                     # child forwarding, activity/search/event-cache routing
    ├── runtime_children.rs            # child/task ownership and ordered shutdown
    ├── scheduled_send.rs              # delayed/local scheduled-send admission and settlement
    ├── session_lifecycle.rs           # login/OIDC/restore/switch/logout/persistence
    ├── sliding_sync.rs                # capability discovery/admission/revalidation
    ├── trust_gate.rs                  # provisional trust gate, status, method discovery
    ├── verification.rs                # incoming/outgoing verification, SAS, auth continuation
    └── test_source.rs                 # cfg(test)-only brace-aware source item helper
```

No `mod.rs`, public feature module, glob import/re-export, wrapper service, one-implementation trait, compatibility alias, duplicate helper, per-feature state object, callback registry, macro dispatch, or new dependency.

`account.rs` retains the existing ownership/security module docs and only private module declarations plus explicit re-exports. It contains no actor field, production function body, test body, task owner, diagnostic registry, or behavioral constant.

## Existing façade/API compatibility

Preserve exactly these flat names and cfg gates:

- public: `AccountActor`, `AccountActorHandle`, `VerificationMethodDiscoveryResult`;
- test-only public: `SyntheticVerificationTerminal`;
- crate-only: `AccountMessage`.

Preserve all existing associated paths, including `AccountActor::spawn`, crate-only `spawn_with_diagnostics`, `AccountActorHandle::{send, admit_navigation_projection, for_app_actor_test}`, and every `test-hooks` residency method. Callers continue using `crate::account::{...}` and do not learn private child paths. Do not narrow the two currently public test-facing result enums merely because production callers are absent.

## Ownership map

### `actor.rs`

Owns the irreducible actor composition boundary:

- `AccountMessage`, `AccountActorHandle` and its existing impl, and `AccountActor` with the complete unchanged field set;
- `spawn`, `spawn_with_diagnostics`, `run`, and exhaustive mailbox routing;
- `handle_command` and exhaustive `AccountCommand` routing;
- the existing `trace_restore` diagnostic macro, imported by routing/session siblings through one `pub(super) use`, plus common `trace_account_request`, `send_actions`, `emit`, `emit_failure`, `emit_event_cache_status`, `active_account_key`, and `current_epoch_ms` helpers.

Mailbox and command matches remain direct and exhaustive. The module may call leaf methods directly at `pub(super)` scope; it must not add forwarding wrappers, function tables, feature traits, or a second dispatcher.

### `routing.rs`

Owns cross-child forwarding and non-session feature routing:

- room, timeline, leased-composer, search, threads-list, sync, activity-resolution, event-cache repair, timestamp-open, and crawler-notification routes;
- feature failure projection and `timeline_event_by_timestamp_request`;
- search-scope, room-route, and active-session composer-target helpers.

`route_sync_command` stays whole here because it exhaustively forwards one child contract; provisional/session code invokes that same implementation rather than duplicating its match.

### `scheduled_send.rs`

Owns scheduled-send admission, dispatch, retry, cancel, reschedule, delayed-event send/update, and acceptance settlement. It also owns authoritative room-encryption classification, secure-backup user-content admission, message-content construction, and active-session targeting.

It reads the existing session/key/backup fields and uses the existing timeline/reducer routes. It creates no second queue, timer owner, durability fence, or per-session policy.

### `runtime_children.rs`

Owns normal child/task startup and the canonical ordered shutdown units:

- `shutdown_owned_runtime`, `stop_current_session_runtime`, sync/search/threads/timeline/RoomActor start/stop/clear, read-persistence worker stop, and `stop_normal_runtime_children`;
- read-persistence constants, generation, diagnostic helper, and worker.

The full shutdown methods move intact. Teardown ordering is not reconstructed as per-feature callbacks and no leaf independently drops shared SDK/session ownership.

### `session_lifecycle.rs`

Owns login discovery/password/OIDC, restore/last restore, account switch, soft-logout reauth, provisional install/runtime, logout/change-homeserver, pending teardown/retry, store persistence/restore/clear/lookups, and session-change observation.

It owns `RestoreOutcome`, session teardown continuation/state, pending OIDC flow, bounded server logout, login/auth classifiers, session-key conversion, restore diagnostics, and device-name outcome diagnostics. It invokes existing runtime-child and trust/admission methods directly.

### `sliding_sync.rs`

Owns all Simplified Sliding Sync capability discovery, positive evidence, retry, admission, revalidation, persistence, correlation, and cancellation. It owns `PendingSlidingSyncAdmission`, `PendingSlidingSyncRetry`, `StoredSlidingSyncAdmissionContext`, their methods, and capability-result/diagnostic helpers.

It does not probe or select a legacy backend. Completion invokes the existing session install/restore path with unchanged request/account epoch fencing.

### `trust_gate.rs`

Owns authoritative current-device trust, provisional encryption sync, trust projection acknowledgement, verification-method discovery, current-session status refresh, recovery-state observation, and secure-backup send admission derived from the trust gate.

It owns `RecoveryStateObservation`, `TrustLifecycleDecision`, `PendingTrustTransition`, `OwnedVerificationMethodDiscoveryTask`, `VerificationMethodDiscoveryResult`, trust/status/method-discovery currentness helpers and diagnostics, and the QA-only exact-device refresh helper.

`set_secure_backup_send_admitted` has one owner here because trust is its authoritative input; timeline/scheduled-send code only consumes the field.

### `recovery_backup.rs`

Owns recovery submission/completion/trust settlement, session bootstrap, secure-backup inspection/monitor/observer, cross-signing/backup operations, room-key import/export, backup passphrase changes, and identity-reset SDK operation/result projection.

It owns pending recovery/completion types, monitor/inspection constants, recovery and backup classifiers/projections, and recovery diagnostics. Identity-reset auth-handle/timer ownership remains in `verification.rs`; direct sibling calls preserve the current continuation without a wrapper.

### `verification.rs`

Owns incoming/outgoing verification request observation, own-user and other-user SAS, observer stop/join, timeout, settlement, identity-reset auth continuation, and all verification/SAS state/token diagnostics.

It owns `VerificationObservation`, `IncomingVerificationObservation`, pending request/SAS types, terminal/wait/adoption/incoming classifications, `SyntheticVerificationTerminal`, stop-aware mailbox delivery, and bounded incoming-observer shutdown. Reliable observer messages continue racing the owned stop signal so shutdown cannot deadlock.

### `profile.rs`

Owns presence, display name, local alias, avatar upload/download/cache/single-flight, account hydration, ignore/unignore, and report-user. It owns avatar/hydration constants, profile/account-data projection helpers, error classifiers, and the existing actor-owned `JoinSet` lifecycle.

### `account_management.rs`

Owns device query/rename/delete, account-management capabilities, password change, deactivation, account-management UIA submission, failure projection, `PendingUiaOperation`, and `AccountManagementUiaError`.

### `local_data_cleanup.rs`

Owns local-encryption health, device-cleanup UIA/remote/local phases, erase-anyway, local reset, pending cleanup types/stages, timeout, and closed diagnostic tokens. Secrets and raw SDK continuations remain actor-private and never enter reducer state or diagnostics.

## Cross-module visibility and dependency direction

- Modules remain private; existing `pub`/`pub(crate)` root APIs retain their exact visibility.
- The actor fields remain one struct. All fields become only `pub(super)` because the twelve sibling inherent impls operate on the unchanged field set; this restores the former `account`-module scope without exposing it outside the private subtree.
- The immutable call graph pins exactly 147 `AccountActor` methods for `pub(super)` sibling use and leaves the other 53 at their original public/crate visibility or owner-private. The appendix lists all 147 by owner.
- The immutable top-level/type/helper/macro/associated-method graph pins 65 cross-owner names in the appendix, including `trace_restore`, `PendingSlidingSyncAdmission::key_id`, and `AuthoritativeRoomEncryption` as the unchanged return type of the cross-owner admission function. Existing public/crate names retain their visibility; only a listed private name may become `pub(super)`, with its cfg unchanged.
- Only the appendix's seventeen concretely cross-read or cross-constructed shared-private-struct fields become `pub(super)` to restore the old parent-module scope; every other field remains owner-private and enum variant fields continue following their enum visibility. Test-only use never promotes production visibility.
- An unlisted required sibling edge is not a compile-fix opportunity: stop, amend this design/appendix, and re-review before broadening it.
- Leaves call the existing actor methods directly. They do not route through façade re-exports or introduce wrapper/helper methods solely to cross a module boundary.
- `actor.rs` may depend on every leaf for dispatch; leaves may depend on `actor` contracts and on explicitly proven siblings. No leaf re-exports another leaf, and no circular state owner is introduced.
- Record every visibility change with its concrete sibling field/caller in the exactness report.

## Lifecycle, ordering, and security invariants

Preserve exactly:

1. One AccountActor mailbox, one active SDK session/key, one account epoch, and one owner for each child/task/subscription.
2. Normal shutdown stops timeline activity, threads list, search, read persistence, sync/provisional encryption sync, session observers/tasks, and RoomActor session ownership in the same existing order before dropping SDK session handles.
3. `PendingSessionTeardown` retains the session until child/RoomActor/store barriers settle; generation-fenced retries reject stale wakes and acknowledgements are not emitted early.
4. Account switch preserves account credentials/store; logout uses bounded best-effort server logout and the existing startup-pointer/persistence continuation.
5. Session replacement joins old recovery, verification, trust, backup, session-change, hydration, avatar, activity, retry, and status owners before replacements can become authoritative.
6. Incoming verification observer shutdown remains stop signal → bounded join → abort-and-await fallback → SDK observer shutdown.
7. Avatar `JoinSet` abort increments `avatar_session_generation`; already-enqueued stale completions remain rejected.
8. Read persistence remains generation-fenced, debounced, latest-wins, and bounded on shutdown.
9. Sliding Sync admission/revalidation remains account-epoch/request fenced and preserves positive evidence; no backend fallback vocabulary returns.
10. Trust/recovery/method-discovery/status completions remain session/generation/transition/request fenced, and reliable reducer projection acknowledgement remains the promotion barrier.
11. Room-subscription residency remains TimelineManager-owned. AccountActor preserves exact session pointer identity, install→`SessionEstablished`, admitted operation draining, and replacement ordering.
12. Scheduled user content retains the secure-backup gate and active-session/composer-permit correlation.
13. Raw SDK errors, secrets, tokens, PKCE/UIA material, Matrix IDs, message content, filenames, and paths do not enter diagnostics, Debug, snapshots, or QA output.

## Test redistribution

Move all 150 unit tests exactly once beside their production owner. Each production module uses a private cfg-test child module. A single private cfg-test support module may own a fixture/parser only when at least two owner suites consume the exact existing helper; single-owner fixtures move with their owner. No helper is copied.

The normative appendix lists every test name exactly once and pins per-owner counts: account-management 1, actor 2, local-data-cleanup 5, profile 5, recovery-backup 22, routing 6, runtime-children 6, scheduled-send 10, session-lifecycle 35, sliding-sync 5, trust-gate 33, verification 20. The sum is 150.

Four ignored diagnostic child tests and the event-cache ignored child keep their parent/child behavior. Their five `--exact`/prefix literals are retargeted only to the final owner-local test paths. Test bodies and ignored cfg remain unchanged.

### Source-characterization migration

The 32 inline source tests currently read `include_str!("account.rs")`. The normative appendix lists all 32 by test owner and explicit production source set. A single-owner contract reads that owner file; a cross-owner contract reads each listed file separately and applies the private brace/string/comment-aware `item_body` helper per item. No source strings are concatenated. The global reliable-delivery negative contract scans all twelve production files independently and combines only boolean results. All 150 test functions and every assertion remain intact; approved changes are limited to explicit source paths, brace-aware item boundaries, and the five ignored-child exact path literals.

The external `renderable_thumbnail.rs` account-source guard is retargeted to `account/profile.rs`. No façade concatenation, generated omnibus source, self-satisfying test literal, split/duplicated test, or public test hook is allowed. Source assertions remain structural guards, not behavioral proof.

## Mechanical integration

After design approval, Luna/low write-capable workers copy complete AST items from the immutable baseline into isolated worktrees and disjoint destinations:

- A: `actor`, `routing`, `runtime_children`;
- B: `session_lifecycle`, `sliding_sync`;
- C: `trust_gate`, `recovery_backup`, `verification`;
- D: `scheduled_send`, `profile`, `account_management`, `local_data_cleanup`.

Workers do not edit `account.rs`, another worker's destination, shared manifests/contracts, or behavior. They copy only the exact owner keys/tests in the normative appendix, report body hashes and listed sibling visibility, and stop on any missing/extra/ambiguous item or edge rather than inferring ownership or changing logic.

One integration owner alone replaces the parent once, writes the façade/test-source support, resolves explicit sibling imports/visibility, retargets the one external source guard, and performs the exactness audit. Never cut from a line-shifted intermediate file.

## Exactness evidence

A temporary non-repository `syn` verifier compares the immutable baseline with the integrated tree:

1. All 391 named production/cfg-test keys exist exactly once: 171 top-level declarations (including `trace_restore`) plus 220 type-qualified associated methods, including all 200 `AccountActor` methods. Associated keys include the self type, so the same method name on `AccountActorHandle` and `AccountActor` remains distinct.
2. All 150 unit-test names exist exactly once; attrs and bodies match except the enumerated source-path/item-boundary and exact-child-path plumbing.
3. Public/crate declarations, method signatures, fields, enum variants, cfg/target gates, docs, derives, timeout values, strings, match arms, diagnostic arrays, and call bodies match bidirectionally.
4. Root named exports equal the five-name baseline façade set exactly, with `SyntheticVerificationTerminal` retaining its test cfg.
5. Non-`AccountActor` impls match whole-token form; split AccountActor methods match individually.
6. Moved production bodies match after normalization limited to required `super`/sibling qualification and `pub(super)` scope restoration.
7. Root contains no production/test body or behavioral constant; no glob, wrapper, duplicate helper, compatibility alias, TODO, or newly introduced dead code exists.
8. The sibling dependency/visibility report matches the appendix's 147 AccountActor method and 65 top-level/type/helper/macro/associated-method cross-owner names exactly; every promoted name and each of the seventeen shared-struct fields has a concrete edge and no unlisted visibility broadening exists.
9. All 32 inline and one external source guard match the appendix's explicit source sets; no concatenation exists and only the approved test plumbing differs.

Pre-existing warnings are baseline artifacts, not permission to add or suppress warnings.

## Verification

### Baseline and focused post-move

```bash
cargo test -p koushi-core --lib
cargo test -p koushi-core --lib --features test-hooks
cargo test -p koushi-core --test runtime_session
cargo test -p koushi-core --test runtime_device_session
cargo test -p koushi-core --test runtime_e2ee
cargo test -p koushi-core --test runtime_scheduled_send
cargo test -p koushi-core --test runtime_activity
cargo test -p koushi-core --test runtime_search
cargo test -p koushi-core --test runtime_timeline
cargo test -p koushi-core --test runtime_room_list_sync
cargo test -p koushi-core --test runtime_intent_lifecycle
cargo test -p koushi-core --test room_subscription_residency --features test-hooks
cargo check -p koushi-core --all-targets --all-features
cargo fmt --all -- --check
git diff --check
```

Run owner-name filters during integration for session, sliding-sync, trust, recovery/backup, verification/SAS, scheduled send, profile/avatar, device/account management, cleanup/reset, read persistence, child shutdown, and reliable routing.

### Formal review then final repository gates

After the read-only full-diff reviewer records `Correct-to-merge`:

```bash
cargo test --workspace --all-targets
cargo test -p koushi-desktop --lib
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
npm --prefix apps/desktop run lint:tauri-boundary
npm --prefix apps/desktop run lint:domain-deps
npm --prefix apps/desktop run qa:secret-scan
npm --prefix apps/desktop run qa:release-gates
npm --prefix apps/desktop run test:ipc-contract
node scripts/check-sdk-submodule.mjs
node scripts/check-agents-docs.mjs
cargo deny check
cargo fmt --all -- --check
git diff --check
```

Run generated/wire checks despite the prohibition on contract changes. A local homeserver/GUI lane is required only if compilation, exactness, tests, or formal review identifies runtime-path ambiguity; no behavior change may be waived into this decomposition.

## Stop conditions

Stop and amend/re-review the design before proceeding if:

- an item/test has ambiguous ownership;
- extraction needs a forwarding wrapper, duplicate state/helper, new state object, public feature module, callback registry, trait, or macro dispatch;
- any production/test body, order, cfg, public path, command/event/DTO/wire shape, timeout, retry, cleanup, privacy, or diagnostic token changes beyond approved qualification/visibility/test-source plumbing;
- any session, task, observer, subscription, timer, continuation, child actor, teardown, secure-backup, residency, or reliable-settlement owner changes;
- a worker needs to edit the parent or another worker's destination;
- exactness cannot prove all 391 production keys, 200 actor methods, 150 tests, five façade names, 33 source sites, 147 sibling methods, 65 shared top-level/type/helper/macro/associated-method names, and the seventeen shared-private-struct field scopes;
- a test exposes a behavior defect.
