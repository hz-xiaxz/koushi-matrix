# Issue #551 runtime connection transport extraction

Status: full diff approved; delivery pending. Scope is one behavior-preserving ownership seam.

## Baseline

- Base: `71c6785b3080846714567d8c0844b3573db9c363` after Activity PR #617 and latest-main integration.
- `runtime.rs`: 9,643 newline-delimited lines / 394,030 bytes / SHA-256 `51898f8e36e5d0bfd6a0e5538d79d795dbd94f2eb1bdd259614a2749255cccfc`.
- Focused baseline:
  - `standalone_composer_command_permit_outlives_activation_lease`: 1/1;
  - `core_connection_command_handle_clones_submit_path`: 1/1;
  - `timeline_sender_label_and_reaction_sender_preview_follow_people_facing_policy`: 1/1;
  - `runtime_core` integration: 4/4.

## Ownership decision

Create private direct child `crates/koushi-core/src/runtime/connection.rs`. Move exactly these four public types in original relative order:

1. `CommandSubmitError`
2. `EventStreamLag`
3. `CoreConnection`
4. `CoreCommandHandle`

Move the complete associated-item ownership:

- `CoreCommandHandle`: nine methods in exact order — `command`, `begin_composer_draft_renderer_generation`, `acquire_composer_draft_lease`, `release_composer_draft_lease`, `acquire_composer_draft_command_permit`, `command_with_composer_lease`, test-hook `command_with_composer_lease_after_admission`, private `validate_request_id`, private `admit_composer_command`.
- `CoreConnection`: thirteen methods in exact order — `connection_id`, `command_handle`, `next_request_id`, `command`, `begin_composer_draft_renderer_generation`, `acquire_composer_draft_lease`, `release_composer_draft_lease`, `acquire_composer_draft_command_permit`, `command_with_composer_lease`, `recv_event`, private exhaustive `project_event_for_consumer`, `snapshot`, `versioned_snapshot`.
- `CoreRuntime::attach` only, as a child-owned `impl CoreRuntime` block with one exact method. This lets the child construct private connection fields without a constructor wrapper or field visibility change.

The leaf owns consumer identity/request allocation, bounded command submission/admission, composer lease calls, event-lag handling, exhaustive consumer-side display-label projection and latest snapshot access. The parent retains `CoreRuntime` startup/tasks/shutdown, bounded channel creation, `CoreCommandEnvelope`, AppActor inbox/dispatch and all task/channel/timer lifecycle ownership.

## Imports and visibility

Production leaf has exactly seven import statements:

1. `std::sync::{Arc, atomic::{AtomicU64, Ordering}}`;
2. `tokio::sync::{broadcast, mpsc, oneshot, watch}`;
3. `super::{CoreCommandEnvelope, CoreRuntime}`;
4. `crate::command::CoreCommand`;
5. composer lease/permit types from `crate::composer_draft_lifecycle`;
6. event/snapshot/projection types and functions from `crate::event`;
7. IDs from `crate::ids`.

Parent declares private `mod connection;` and explicitly `pub use connection::{CommandSubmitError, CoreCommandHandle, CoreConnection, EventStreamLag};`. This preserves both `koushi_core::runtime::*` and existing crate-root re-exports in `lib.rs` without exposing `runtime::connection`.

All four moved types and their existing fields/methods retain exact visibility. Parent removes exactly the eleven production bindings made orphaned by the completed Activity/connection moves: `BTreeMap`, `Ordering`, `ActivityTab`, `RoomSummary`, `ComposerDraftLeaseFailure`, `ComposerDraftLeaseId`, `ComposerDraftScope`, `ComposerRendererGeneration`, `AppStateSnapshot`, `project_room_event_display_labels`, and `project_timeline_event_display_labels`. Parent retains `AtomicU64`, `BTreeSet`/`HashMap`, and all other runtime owners. `CoreCommandEnvelope` stays parent-private and is accessible to the descendant module. `CoreRuntime` fields stay private: the moved descendant `attach` method accesses its six connection resources, while existing parent-owned shutdown/barrier tests continue reading `snapshot_rx` directly. No `pub(super)`, new constructor, wrapper, alias, trait, compatibility shim or public namespace is added.

## Test ownership

Move exactly three tests in original relative order to the leaf's `#[cfg(test)] mod tests`:

1. `standalone_composer_command_permit_outlives_activation_lease`
2. `core_connection_command_handle_clones_submit_path`
3. `timeline_sender_label_and_reaction_sender_preview_follow_people_facing_policy`

The first directly constructs the private command handle and pins lease-permit retention. The second is a source-characterization test for this owner. Change only its source input from `include_str!("runtime.rs")` to `include_str!("connection.rs")`; continue reading the owner file individually and do not concatenate source. The third directly constructs private `CoreConnection` fields and pins consumer-side label projection, so it must move with that owner rather than widening fields or adding a test constructor.

Leaf tests use existing test-only `use super::*` plus four explicit import statements: `BTreeMap/BTreeSet`; event timeline/diff/item/thread DTOs; state reducer/profile/session/alias/`ComposerTarget` types; and `AccountKey/TimelineKey/TimelineKind`. Do not import the already fully qualified `CurrentDeviceTrustState`.

Parent test imports remove exactly ten bindings now owned only by the moved projection test: `ThreadSummaryDto`, `TimelineDiff`, `TimelineItem`, `TimelineItemId`, `LocalUserAliasUpdateState`, `OwnProfile`, `ProfileState`, `RoomNotificationModeOperation`, `RoomNotificationSettings`, and `reduce`; add one direct test-only `BTreeMap` import for the two retained alias-map tests. No parent helper moves or visibility changes. All other runtime unit tests remain parent-owned.

## Invariants

- Four type declarations, 9 handle methods, 13 connection methods and one attach method retain exact attrs/cfg/signatures/bodies/order apart from module imports and the approved source-test path.
- `CommandSubmitError` variants/messages and composer lease fail-closed admission remain exact.
- Request IDs remain connection-owned and use relaxed atomics; command submission still awaits the same bounded sender.
- `recv_event` lag/closed behavior and skipped counts remain exact.
- `project_event_for_consumer` remains exhaustive over every `CoreEvent`, with timeline/room label projection and snapshot reads unchanged.
- `CoreRuntime::attach` clones the same sender/lease/watch receivers and subscribes to the same event broadcaster.
- `CoreCommandEnvelope`, AppActor command routing, channel capacities, task guards, media reconciliation and ordered shutdown remain parent-owned.
- No command/event/state/serde/wire/API path, privacy/logging, resource bound, test config or dependency changes.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- types 4/4, parent 0;
- methods keyed by `(self type, method, name)`: handle 9/9, connection 13/13, runtime attach 1/1 and retained parent CoreRuntime methods unchanged;
- tests 3/3, parent 0, bodies/attrs exact except the one approved `include_str!` path;
- all 1,029 lib test identities bidirectionally equal after normalizing only the three owner paths;
- public re-export 4/4, leaf production imports 7/7, parent production orphan bindings 11/11, parent test orphan bindings 10/10 plus direct `BTreeMap` 1, child test extra bindings 0, zero visibility deltas and zero duplicate/missing/excess item;
- exhaustive event match order and public/crate paths exact;
- no path attribute, production glob, wrapper, alias, TODO or source concatenation.

## Verification

Run the same focused 1 + 1 + 1 + 4 tests before and after, then:

- `cargo test -p koushi-core --lib`;
- runtime session/device/e2ee/timeline/search/intent integration suites;
- `cargo check -p koushi-core --all-targets --all-features`;
- source/exactness verifier, rustfmt and diff checks.

After full-diff approval, integrate current `origin/main`, obtain delta approval if it moved, and run the full repository matrix before PR merge.

## Review gate

- Design round 1: `reviewer-flash` recorded `Changes-required` because the consumer-projection test directly constructs private `CoreConnection` fields and the CoreRuntime-field invariant was overstated.
- Both findings were corrected; round 2 verified the complete type/method/test/import/privacy/public/lifecycle graph and recorded `Correct-to-implement`.
- Implementation exposed additional compiler-proven orphan imports from the completed Activity/connection moves; the exact production/test closure was amended and `reviewer-flash` recorded `Correct-to-continue-implementation`. The unapproved worker draft omitted three doc attrs and added one unused binding; parent restored the exact docs and removed the binding before integration.
- Implementation exactness: types 4/4, handle methods 9/9, connection methods 13/13, attach 1/1, tests 3/3, leaf imports 7/7, public re-export 4/4, visibility deltas 0; all 1,029 lib test identities match after normalizing the three owner paths.
- Full-diff round 1 found moved docs/`Clone` attrs left on `AbortOnDrop` and `ComposerDraftLoadStatus`; parent removed both, expanded exactness to all retained top-level types, and round 2 recorded `Correct-to-merge` with no blocker.
- `runtime.rs` 9,643 → 9,003 newline-delimited lines; `runtime/connection.rs` is 665.
- Focused post-move 1+1+1+4; core lib 1,021/8 ignored; session 8, device 2, E2EE 2, timeline 21, search 1 and intent 5; all-targets/all-features check, rustfmt, exactness and diff checks green.
- Final local evidence: focused 1+1+1+4, core lib 1,021/8 ignored, Vitest 1,370, Playwright 248 with polling, workspace all-targets, desktop 149/1 ignored and Headless Core QA 130; all-targets/all-features, typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/format/exactness/diff gates green.
- Delivery: latest-main integration/delta review if required, PR CI and merge pending.
