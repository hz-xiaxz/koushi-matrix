# Issue #551 runtime reducer/deferred support extraction

Status: design approved. Scope is the final planned behavior-preserving runtime seam before residual audit.

## Baseline

- Base: `136109ffa7d182e110a9a66397d2159a03b4af4e` after scheduled-send PR #622.
- `runtime.rs`: 7,035 newline-delimited lines / 300,436 bytes / SHA-256 `7f0878384109b098678fc66ae6da4f4cca3bf92effec64a9f8f77aa442423b42`.
- Focused baseline: room-list diagnostics 1 passed/1 ignored, native-attention diagnostics 1 passed/1 ignored, runtime intent lifecycle 5, scheduled-send 12, navigation 1, Activity 9.

## Ownership decision and immutable order

Create private `crates/koushi-core/src/runtime/reducer_support.rs`. Move exactly these five production identities in global immutable-source order:

1. `reduce_with_unread_diagnostics`
2. `DeferredReducerSideEffects`
3. `AppActor::reduce_app_action`
4. `AppActor::reduce_app_action_state`
5. `AppActor::apply_deferred_reducer_side_effects`

The leaf is one orchestration hub for the instrumented authoritative reducer gateway, automatic cross-domain persistence derivation and ordered async application. It adds no product state, reducer, actor, task, channel, timer, map, cache, store or dispatcher owner.

## Exact reducer/deferred order

Preserve `reduce_app_action_state` phases exactly:

1. classify explicit navigation preference mutation and composer transition policy;
2. snapshot destructive state and prior Activity/session/draft/target/navigation/scheduled-send projections;
3. invoke `reduce_with_unread_diagnostics` exactly once;
4. record space-order projection diagnostics;
5. cancel stale pending composer persistence after a destructive transition;
6. reconcile active composer targets;
7. derive deferred work in field/order: Activity cancellation, navigation persistence, composer persistence, scheduled-send persistence/loaded-marker clearing.

Preserve `apply_deferred_reducer_side_effects` order exactly:

1. send `AccountMessage::CancelActivityResolution`;
2. persist navigation or suppress an automatic write after failed load while allowing explicit preference retry;
3. schedule composer-draft persistence;
4. clear scheduled-send loaded marker or persist scheduled sends.

Never parallelize/spawn these operations. Reducer completion, post-projection effects, deferred persistence, UI effects and publication ordering remain observable.

## Parent-owned residual

Keep every `AppActor` field and these boundaries in `runtime.rs`:

- `AppActor::run`, channel batching, timer/select arms, pending maps/queues, state publication and shutdown;
- `handle_command`, command admission/routing and all account/timeline forwarding;
- `handle_app_effects` and `handle_post_projection_effects` as distinct exhaustive registries;
- `intent_outcome_token`, `app_loop_trace`, space-member rejection/rollback helpers;
- room-preference loading/persistence, `next_internal_request_id`, Activity resolver dispatch;
- consumer projection, verification allowlist, account speculative projection and public runtime façade.

The parent action transaction remains byte-exact: capture facts → moved reducer method → terminal correlation/post-projection effects → moved deferred application → composer acceptance release → Activity refresh/UI effects/domain loads → one publication.

## Visibility, sibling edges and imports

No public API/re-export is added. Parent declares private `mod reducer_support;` and imports no moved top-level identity. `reduce_with_unread_diagnostics` remains private to the leaf. `DeferredReducerSideEffects` is `pub(super)` because two parent-called method signatures carry it opaquely; its fields remain private and the parent neither constructs nor destructures it.

All three moved `AppActor` methods become `pub(super)`: parent `run` calls state reduction/deferred application and parent commands plus sibling scheduled-send dispatch call `reduce_app_action`. No AppActor field visibility changes.

Leaf uses explicit sibling paths for:

- composer transition/session/target reconciliation and persistence scheduling;
- navigation session/load-status/persistence;
- scheduled-send session/deferred enum/persistence;
- profile/display diagnostics;
- Activity state, diagnostics, reducer and unread tracing.

Remove exactly ten parent production bindings made orphaned by the move: `active_composer_targets`, `ComposerDraftTransitionPolicy`, `composer_draft_transition_policy`, `navigation_session_key`, `DeferredScheduledSendPersist`, `scheduled_send_session_key`, `live_receipt_profile_diagnostic_event`, `profile_resolution_diagnostic_event`, `record_native_attention_recomputed`, and `crate::unread_trace`. Retain `composer_draft_session_key`, `NavigationPersistenceStatus`, `scheduled_send_id`, `ActivityState`, `ComposerDraftStore`, `NavigationState`, `reduce`, diagnostics and all actor/event imports used by the residual.

No production glob, wrapper, trait, alias, path attribute, compatibility shim, callback registry or duplicated reducer path.

## Test ownership

Move exactly four owner tests in existing order:

1. `room_list_applied_records_through_real_reducer_with_trace_env_unset`
2. `room_list_applied_records_without_trace_environment`
3. `native_attention_recomputed_diagnostic_records_private_safe_fields`
4. `native_attention_recomputed_diagnostic_records_private_safe_fields_child`

Preserve attrs/bodies except update the two subprocess `--exact` paths from `runtime::tests::…` to `runtime::reducer_support::tests::…`. Leaf tests use `super::*`, import `super::super::tests::unread_diagnostic_room`, and explicitly import `koushi_state::{RoomLatestEventSummary, SessionInfo, SessionState}`; `SessionAuthenticationMethod` remains fully qualified. The shared fixture remains one parent-owned `pub(super)` copy because Activity/profile/navigation mailbox tests also consume it.

Remove only `RoomLatestEventSummary` from the parent test imports; `SessionInfo` remains required. All 1,029 core lib test identities remain equal after normalizing only four owner paths.

Existing persistence and effect-dispatch source tests remain parent-owned and require no owner-path rewrite. `runtime_intent_lifecycle` and search source contracts continue inspecting parent registries unchanged.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- production identities 5/5 in global order, parent 0;
- top-level items 2/2 with one private function and one `pub(super)` struct, AppActor methods 3/3 keyed by `(AppActor, method, name)` with exactly three approved `pub(super)` changes;
- moved tests 4/4 with attrs/bodies exact except two approved subprocess path strings; parent 0;
- all 1,029 lib test identities equal after four owner-path normalizations;
- parent production orphan bindings10, parent test orphan binding1, public/resource/wire/dependency deltas0;
- parent fields/run/action transaction/commands/effect registries/timers/channels/maps/queues/publication/shutdown byte-exact;
- deferred field/order, diagnostic source/stage/fields/tokens and reducer invocation exact;
- duplicate/missing/excess identities and alternate reducer paths0.

## Verification

Run focused baselines, full core library, Account/Activity/navigation/scheduled-send integration owners, `cargo check -p koushi-core --all-targets --all-features`, exactness/source/order/privacy checks, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main` if required, obtain delta approval, run the complete repository matrix and PR CI7/7. Then perform the separately reviewed final runtime residual audit.

## Review gate

- Read-only reconnaissance traced reducer, diagnostics, cross-domain persistence and central-registry boundaries.
- `reviewer-flash` round 1 found the private-type-in-`pub(super)` interface defect; the struct visibility/verifier contract was corrected.
- Round 2 traced the opaque pass-through, sibling edges, imports, tests and ordering and recorded `Correct-to-implement`.
