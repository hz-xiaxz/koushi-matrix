# Issue #551: RoomActor feature-seam decomposition

## Status

- Design: `reviewer-flash` (read-only, cross-model) recorded `Correct-to-implement`; the four naming/visibility clarifications were incorporated in `c7d65f3` and the bounded amendment review again recorded `Correct-to-implement` with no findings.
- Implementation: integrated from the immutable baseline after worker ambiguity was rejected; bidirectional AST/token exactness and all focused gates are green.
- Full-diff review: `reviewer-flash` (read-only, cross-model) reviewed `70e1ecb5..071550f` and recorded `Correct-to-merge`; no blocking findings.
- Delivery: all required local repository gates are green; PR CI and merge remain pending.

## Objective

Replace the 10,760-line `crates/koushi-core/src/room.rs` with a small private-module façade and ten existing feature/ownership modules. Move existing items, tests, attributes, cfg gates, strings, actor state, task handles, cancellation, fencing, reliable settlement, and call order without changing behavior or public paths.

This is source ownership only. It does not redesign RoomActor, Matrix semantics, command/event/reducer contracts, DTOs, room-list authority, residency admission, retries, diagnostics, or shutdown.

## Immutable baseline

- Commit: `70e1ecb56b860ee4194696491a9f1e84f06ac4c5`
- Source: `crates/koushi-core/src/room.rs`
- Size: 10,760 newline-terminated lines; 414,456 bytes
- SHA-256: `9a792f1157c03bd2c6bf940daab1ea860c63296d8361f958994dbd19e29201fa`
- Named top-level items plus associated methods: 255
- Room unit tests: 85
- Core lib baseline, default and `test-hooks`: 1,014 passed, 8 ignored

Every extraction and token/body comparison uses the immutable baseline blob, never a line-shifted intermediate file.

## One ownership-area PR

RoomActor is one actor ownership area and is delivered atomically. Separate PRs would require temporary duplicate actor fields, widened routing, or forwarding wrappers while sibling `impl RoomActor` blocks are incomplete. One integration owner replaces the parent once and audits the full actor/resource graph once.

This exception permits no behavior fix. If extraction requires changed state, ordering, retry, fencing, cleanup, or public contracts, stop and handle that separately under verify-first discipline.

## Target layout

```text
crates/koushi-core/src/
├── room.rs                    # private module declarations + minimal explicit façade
└── room/
    ├── actor.rs               # public contracts, actor/handle, routing, lifecycle
    ├── list_observer.rs       # one live RoomListService observer/reconcile loop
    ├── normalization.rs       # room/space/invite/profile projection and mappings
    ├── operations.rs          # create/invite/join/leave/forget/tags/read/report
    ├── directory.rs           # public directory query/preview/join
    ├── management.rs          # settings, permissions, roles, moderation
    ├── mentions.rs            # demanded joined-member autocomplete projection
    ├── space_members.rs       # demand/refresh/invite/cancel/diagnostics
    ├── pins.rs                # pin/unpin/load/raw-event projection
    └── encryption_debug.rs    # reshare/index-0 operations, fences, diagnostics
```

No `mod.rs`, public feature module, glob import/re-export, wrapper service, one-implementation trait, compatibility alias, duplicate helper, speculative state object, or new dependency.

`room.rs` retains the existing ownership/security module docs and only:

- private `mod` declarations;
- explicit re-exports required to preserve the existing `koushi_core::room::*` surface;
- no production body, actor field, test body, task owner, diagnostic registry, or behavioral constant.

## Existing façade/API compatibility

Preserve these flat public names exactly:

- `MissingSpaceChildLink`
- `RoomMessage`
- `RoomActorHandle`
- `RoomListReconcileAck`
- `RoomActor`
- `assign_dm_space_ids`

Preserve these crate-internal names and cfg gates exactly:

- `RoomOperationKind`
- `RoomOperationTestControl`
- `EncryptionDebugTestControl`
- `classify_room_error`

All existing `RoomActorHandle` and `RoomActor` method paths remain unchanged. External callers in `account.rs`, `sync.rs`, Tauri/QA code, and integration-test support keep importing `crate::room::{...}`; they do not learn private module paths.

## Ownership map

### `actor.rs`

Owns the actor contract and supervision boundary:

- `MissingSpaceChildLink`, `RoomMessage`, and `RoomListReconcileAck`;
- `TimelineResidencyBinding`, `RoomActorHandle`, `RoomActor`;
- spawn, main `select!` loop, command routing, action/event reliable emission;
- session installation/clear, residency watch binding, known-room book;
- ordered actor shutdown orchestration and test-hook façade.

The observation handle and its owned task stay wholly in `list_observer.rs`; `RoomActor` holds that owner type without duplicating it.

The 96 baseline `impl RoomActor` methods are split into sibling inherent impl blocks. `actor.rs` keeps only spawn/run/routing/lifecycle/common reliable-send methods. It does not add forwarding methods.

### `list_observer.rs`

Owns the single live `RoomListService` observation and reconciliation subsystem:

- `RoomListObservation`, its stop/join/command ownership, observation commands, and `LiveRoomListReconciliation`;
- dynamic entries, committed-response reconciliation, auxiliary direct/account-data wake;
- authority checks by distinct identity, range completeness, generation/source fencing;
- reliable visible-room and membership forwarding;
- test-only `LiveObserverTestEvent`, `LiveDirectEventTestSource`, and `emit_live_observer_test_event`, colocated with their only producer and harness;
- initial/direct-source diagnostics and observer exit diagnostics;
- refresh/current-room refresh/start/stop observer methods.

It must never construct a second `RoomListService`, replace a diff accumulator from an independent snapshot, or weaken the current generation/authority rules.

### `normalization.rs`

Owns pure SDK-to-state projection:

- room/space/invite normalization;
- parent/child relationships and missing-link detection;
- DM-space assignment and direct-classification mapping;
- room tags, avatars, room/member/profile/permission/value mappings shared by projections;
- `assign_dm_space_ids` and its unchanged flat re-export.

No actor mailbox, SDK write, task, retry, or cleanup owner moves here.

### `operations.rs`

Owns ordinary room mutations and their reliable settlement:

- create room/space, parent-child linking, invite, invite accept/decline;
- DM start, direct join, leave, forget;
- tags, mark read/unread, notification mode, content/room report;
- residency admission/permit handling and membership success acknowledgements, including the single `AdmittedRoomOperation` owner; its struct/methods are `pub(super)` only for directory join's proven sibling use;
- known-room guards, space-child repair dedupe, coarse failure classification and operation diagnostics;
- `RoomOperationKind`, `RoomOperationTestControl`, and `classify_room_error`.

The `TimelineSubscriptionResidencyPermit` stays held through the same SDK result and reducer/event settlement. Request IDs and private-safe failures stay unchanged.

### `directory.rs`

Owns public-directory query, preview, alias join, public-room creation, and directory DTO mapping. Query and join remain separate Rust-owned state machines; join success preserves select-before-joined event ordering.

### `management.rs`

Owns room settings load/update, member role changes, kick/ban/unban moderation, permission guards, and SDK/state mapping. Submitted settings continue to settle from the authoritative Rust projection; no UI repair or new cache is introduced.

### `mentions.rs`

Owns mention demand, joined-member snapshot refresh, membership/alias invalidation, query normalization, projection/failure publication, and mention diagnostics. Session, demand, request, query, surface, and refresh-generation fences remain exact.

### `space_members.rs`

Owns space-member demand/install/clear, child-scope refresh, one-in-flight plus pending coalescing, session/demand/refresh fences, invite/cancel reconciliation, projection mapping, profile-resolution observation, and private-data-free metrics.

No empty projection may be fabricated on SDK lookup failure. No additional task, timer, retry queue, or session owner is introduced.

### `pins.rs`

Owns pin/unpin, refresh, pinned-state update handling, raw event projection, known-room guard, and pending-before-reload settlement. Event order, metadata, thread relation, and retryable failure state remain unchanged.

### `encryption_debug.rs`

Owns manual room-key reshare and dangerous encryption-debug operations:

- SDK outcome mapping and bounded diagnostics;
- per-room/request/kind fences;
- lossless unbounded completion ingress;
- cancellation broadcast, actor-owned cancelled flag, session snapshot, and join handle;
- duplicate rejection, authoritative removal cancellation, completion revalidation;
- cancel/join/settle/reset ordering on session clear and actor shutdown;
- `EncryptionDebugTestControl`.

The completion receiver and fence map remain owned here but are `pub(super)` because `actor.rs::run` directly selects the lossless completion ingress and enumerates fences during shutdown/session clear. Do not replace those direct accesses with a polling or forwarding method.

Normal shutdown must still cancel and join every in-flight operation without abort before it stops observation or acknowledges session clear. The outer 30-second actor emergency timeout remains unchanged.

## Visibility

- New modules are private.
- Existing `pub` and `pub(crate)` APIs retain their exact root paths and cfg gates.
- Cross-feature actor fields/methods use only the minimum `pub(super)` needed by a proven sibling impl/caller. From a child module this restores the old `room`-module scope and does not expose the field outside the private subtree.
- Do not route sibling calls through root re-exports.
- Do not promote a helper merely for tests; owner-local tests use private access.
- Record every visibility promotion and its concrete sibling caller in the exactness report.
- `classify_room_error` and `EncryptionDebugTestControl` have no current root-path consumer, but the reviewed ten-name façade requires those existing crate-internal paths. Their root re-exports use a narrowly scoped `allow(unused_imports)` rather than routing production siblings through the façade or broadening them to public API; this is façade compatibility, not a dead-code allowance.

## Test redistribution

Move all 85 unit tests exactly once beside their owner. The baseline `pub mod tests` is cfg(test)-only and has no consumer; it is dissolved into private owner-local test modules and is not part of the retained production façade:

- actor/lifecycle and cross-feature routing contracts → `actor.rs`;
- observer/reconciliation tests → `list_observer.rs`;
- pure room/space/invite/profile mapping tests → `normalization.rs`;
- ordinary operation/error/residency tests → `operations.rs`;
- directory tests → `directory.rs`;
- settings/permission tests → `management.rs`;
- mention tests/contracts → `mentions.rs`;
- space-member demand/fence/projection tests → `space_members.rs`;
- pin tests → `pins.rs`;
- encryption-debug lifecycle tests → `encryption_debug.rs`.

Cross-feature source-order contracts remain under the actor composition owner; they may inspect explicitly named owner files but must not concatenate files into a false global order. The shared test helper `make_request_id` is defined exactly once in `actor.rs` test support and exposed only as `pub(super)` under `cfg(test)` to sibling owner tests; it must not be copied.

### Source-characterization migration

Seventeen tests originally read `include_str!("room.rs")`. They remain seventeen source-contract tests; the integrated tree has eighteen `include_str!` sites because the actor-owned missing-space-child contract now reads both `operations.rs` and `list_observer.rs` explicitly instead of relying on one monolithic source blob. Preserve every assertion and searched production token while changing only the source file and brace-aware item boundary:

- actor: command loop, one-live-service routing, lifecycle/repair ownership ordering;
- list observer: direct subscription ordering, missing-link relay, known-book-before-delivery;
- operations: mark-read ordering, tag no-stale-refresh, create/link ordering;
- directory: select-before-joined ordering;
- space members: membership refresh routing and failure/no-empty-projection contracts;
- pins: pending-before-reload and known-room guard.

Use one private `#[cfg(test)]` brace/string/comment-aware `item_body` helper. Integration selected the approved `room/test_source.rs` fallback because seven owner modules consume it; one private helper avoids duplicate parsers and keeps source-contract tests owner-local. No public test hook is allowed.

## Mechanical integration

Mechanical extraction may use Luna/low write-capable workers only after design approval. Workers operate in isolated worktrees or create disjoint destination files from the immutable baseline:

- A: actor + list observer
- B: normalization + mentions
- C: operations + directory + management + pins
- D: space members + encryption debug

Workers do not edit `room.rs`, shared contracts, Cargo manifests, another worker's destination, or behavior. They report item/test inventories, token/body hashes, required sibling visibility, and ambiguity. Any ownership or behavior ambiguity stops that worker.

One integration owner alone removes baseline items in one brace-aware pass, writes the façade, resolves direct sibling imports/visibility, and performs the exactness audit. Do not repeatedly cut from a line-shifted intermediate file.

## Exactness evidence

A temporary non-repository `syn` verifier compares the immutable baseline with the integrated tree:

1. All 255 named top-level items and associated methods exist exactly once, except imports/module declarations.
2. All 85 test leaf names exist exactly once; bodies/attrs match except the enumerated source-path/item-boundary plumbing.
3. Public and crate-internal declaration names, visibility, method signatures, cfg/target gates, docs, derives, serde/error strings, enum variants, fields, match arms, and diagnostic arrays match bidirectionally.
4. Root named re-exports equal the ten-name baseline façade set exactly; no extra public name exists.
5. All non-`RoomActor` impl blocks match whole-token form; split `RoomActor` methods match individually.
6. Moved production bodies match after normalization limited to required `super`/sibling qualification and `pub(super)` scope restoration.
7. Root has no production body/test body/behavioral constant; no glob, duplicate helper, wrapper, compatibility alias, TODO, or newly introduced dead code exists.
8. The sibling dependency graph and visibility report identify one owner per item and every cross-owner edge.

Pre-existing compiler warnings are baseline artifacts, not permission to add or duplicate dead code. This PR neither hides nor expands them.

## Lifecycle and security invariants

Preserve exactly:

- AccountActor owns `RoomActorHandle`; handle shutdown sends `Shutdown`, awaits, then aborts+awaits only on timeout.
- Every live observation replacement/stop/session clear cancels and awaits the owned observation task under source/generation fencing.
- RoomActor consumes the one SyncService-owned `RoomListService`; no alternate sync owner or snapshot reset path appears.
- Encryption-debug completion remains lossless and independently selected from the bounded actor mailbox.
- Encryption-debug cancel/join/settle/reset completes before session clear acknowledgement and normal actor shutdown exit.
- Timeline residency requires exact session pointer identity and holds one admitted permit through settlement.
- Space-member and mention results remain session/request/demand/generation fenced and fail closed.
- Reliable reducer actions and request-correlated events are awaited; no settlement becomes best-effort.
- Raw SDK errors, Matrix IDs, event IDs, member data, message content, and secret material do not enter diagnostics or Debug output.

## Verification

### Baseline and focused post-move

```bash
cargo test -p koushi-core --lib
cargo test -p koushi-core --lib --features test-hooks
cargo test -p koushi-core --test dm_space_ids
cargo test -p koushi-core --test room_subscription_residency
cargo test -p koushi-core --test runtime_room_list_sync
cargo test -p koushi-core --test runtime_room_preferences
cargo test -p koushi-core --test runtime_room_selection_scale
cargo check -p koushi-core --all-targets --all-features
cargo fmt --all -- --check
git diff --check
```

Run owner-name filters during integration for observer, normalization, operations, directory, settings, mentions, space members, pins, encryption debug, session clear, and shutdown.

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

Run wire/generated checks despite the prohibition on contract changes. A local homeserver/GUI lane is required only if compilation, tests, exactness, or review identifies runtime-path ambiguity; no behavior change may be waived into this decomposition.

### Final local evidence

- Bidirectional AST/token inventory: 227/227 production keys with matching multiplicity, 10/10 public/crate declarations and exact root exports, 85/85 unit-test names; the 17 reviewed source-test functions are the only body allowlist.
- Focused: Core lib default and `test-hooks` each 1,014 passed/8 ignored; DM-space 1, residency 25, room-list 6, preferences 2, selection-scale 4; all-target/all-feature check green.
- Rust workspace final rerun: 2,394 passed, 13 ignored, 0 failed across 97 suites; desktop lib 149 passed/1 ignored; Headless Core QA 129 passed.
- The first workspace run hit the existing five-second `runtime_room_list_sync` event deadline after about 8.12 seconds total wall-clock under concurrent suite load. The exact failed test passed focused in 7.21 seconds total wall-clock, and the complete workspace rerun was green; no expectation was changed or waived.
- Frontend: typecheck/lint green; Vitest 1,367 passed; UI-headless timeline store 76 passed and Playwright 248 passed with `CHOKIDAR_USEPOLLING=true`; production build green.
- Boundary/policy: Tauri adapter, domain dependencies, secret scan, release gates, SDK submodule, agents docs, IPC generated-wire contract, `cargo deny`, rustfmt, and diff checks green.
- Compilation, tests, exactness, and formal review found no runtime-path ambiguity, so the design's conditional local homeserver/GUI lane was not triggered.

## Stop conditions

Stop and amend/re-review the design before proceeding if:

- an item or test has ambiguous ownership;
- an extracted impl needs a forwarding wrapper, duplicate field/helper, new state object, or public feature module;
- any body, ordering, cfg, public API, command/event/DTO, retry, cleanup, privacy, or diagnostic token differs beyond approved qualification/visibility/source-test plumbing;
- observer, completion, residency, mention, space-member, or encryption-debug ownership changes;
- a worker needs to edit the parent or another worker's destination;
- a test exposes a behavior defect.
