# Issue #551 runtime navigation support extraction

Status: full diff approved; delivery pending. Scope is one behavior-preserving navigation persistence/cleanup/projection seam.

## Baseline

- Base: `a9f14a9297891e9dfa9093750215f93e5f79be26` after composer lifecycle PR #620.
- `runtime.rs`: 7,715 newline-delimited lines / 323,444 bytes / SHA-256 `3e843204030627203bf3e0c05224d8c46dae3833fd7eb0d866cf9a1714b1468c`.
- Focused baseline: moved unit tests 6/6, `runtime_navigation` 1/1, state navigation 55/55.

## Ownership decision and immutable order

Create private `crates/koushi-core/src/runtime/navigation.rs`. Preserve the global immutable-source order below; do not regroup by item kind:

1. `NavigationPersistenceStatus`
2. `PendingFocusedNavigation`
3. `take_acknowledged_focused_navigation`
4. `anchored_action_after_projection_ack`
5. `focused_navigation_outcome_after_reduce`
6. `AppActor::load_navigation_for_current_session`
7. `AppActor::persist_navigation`
8. `AppActor::current_focused_context_timeline_key`
9. `AppActor::unsubscribe_replaced_focused_context_timeline`
10. `unsubscribe_replaced_focused_context_timeline_key`
11. `unsubscribe_replaced_timeline_key`
12. `cancel_replaced_room_timeline_pagination_key`
13. `cancel_replaced_room_timeline_link_previews_key`
14. `NavigationReplacementRoomForCleanup`
15. `NavigationReplacementRoomForCleanup::room_id`
16. `navigation_replacement_room_for_cleanup`
17. `navigation_session_key`
18. `effects_open_focused_timeline`

Inventory: 3 top-level types, 10 free functions, one associated enum method and four `AppActor` methods. Preserve derives, fields, variants, bodies, diagnostics, tokens and ordering exactly except approved descendant-module visibility.

The leaf owns encrypted navigation load/save, focused owner/key projection acknowledgment, replacement timeline key decisions and previous-room cleanup calculation. It adds no actor, task, channel, timer, queue, generation, store or reducer owner.

## Parent-owned orchestration and order

Keep every `AppActor` field and these registries in `runtime.rs`:

- `AppActor::run`, action batching and publication;
- `pending_select`, `navigation_projection_generation`, `pending_focused_navigation`, `pending_date_navigation_request_id`;
- `handle_command`, the complete `OpenFocusedContext`, `OpenAnchoredTimeline`, `AcknowledgeTimelineProjection`, `OpenTimelineAtTimestamp` and close arms;
- `reduce_app_action_state`, `DeferredReducerSideEffects`, `apply_deferred_reducer_side_effects`;
- both exhaustive effect dispatchers and latest-wins `NavigationProjectionIntent` admission.

Preserve these orderings byte-exact:

1. load navigation before same-batch actor projections can derive persistence;
2. capture active/current keys before reduce, then reduce, calculate replacement cleanup, increment projection generation, correlate pending select, dispatch post-projection effects, apply deferred persistence, publish;
3. cleanup keys travel through the latest-wins lane before replacement projection, bypassing a saturated ordinary mailbox;
4. focused cache repair and replacement-unsubscribe decisions precede new subscribe effects;
5. AccountActor projection acknowledgment completes before anchored action classification; reducer application precedes terminal lifecycle classification; target-missing unsubscribe precedes terminal emission;
6. `OpenTimelineAtTimestamp` checks local `activity_projection.event_at_or_after` before `AccountMessage::OpenTimelineAtTimestamp` fallback;
7. failed navigation load suppresses automatic persistence but explicit reorder/removal may retry; all store I/O remains awaited `executor::spawn_blocking`.

Do not move thread wrappers/methods/commands. The generic `unsubscribe_replaced_timeline_key` moves but remains a `pub(super)` sibling dependency for retained `unsubscribe_replaced_thread_timeline_key`.

## Visibility and imports

No public API exists or is added. Parent declares private `mod navigation;` with the other private modules and explicitly imports 11 `pub(super)` top-level identities: all moved top-level items except private `take_acknowledged_focused_navigation` and leaf-only `unsubscribe_replaced_focused_context_timeline_key`. All four moved `AppActor` methods and `NavigationReplacementRoomForCleanup::room_id` become `pub(super)`.

`PendingFocusedNavigation` has five `pub(super)` fields because retained command/action orchestration constructs and reads all five. Other moved fields/variants require no separate widening. Remove only one parent production binding made orphaned by the move: unqualified `FocusedContextState`; retained parent code already uses its fully-qualified path. Keep `NavigationState`, `IntentNoOpReason`, `IntentOutcome`, `TimelineKey`, `TimelineKind`, diagnostics, `executor`, `StoreActor` and `session_key_id_from_info` for retained production/tests.

Leaf production imports are explicit and may reference parent `AppActor` and sibling `composer_draft_session_key`; no production glob, wrapper, trait, alias, path attribute or compatibility shim.

## Tests and source contracts

Move exactly six unit tests, bodies/attrs/order unchanged:

1. `focused_projection_ack_requires_same_owner_and_key_and_is_idempotent`
2. `focused_anchor_action_is_impossible_before_actor_acceptance`
3. `focused_navigation_lifecycle_uses_the_reduced_state`
4. `replacement_focused_helper_preserves_same_key_and_unsubscribes_different_key`
5. `select_space_cleanup_targets_previous_room_only_when_active_room_changes`
6. `select_room_cleanup_still_uses_explicit_target_room`

Move their two private helpers `focused_projection_fixture` and `focused_key`. Leaf tests use `super::*`, one explicit `crate::ids::{AccountKey, RuntimeConnectionId}` import and one explicit `koushi_state::SessionInfo` import. Parent test import orphans: zero.

Keep parent-owned source/order tests and the saturated-mailbox behavior test in `runtime.rs`. Only `app_actor_persistence_uses_blocking_store_port` changes owner-file plumbing:

- parent `runtime.rs` continues checking scheduled-send, room-preference and settings sections; scheduled-save now ends at `async fn persist_room_preferences`;
- separate `runtime/navigation.rs` source checks load-navigation through `async fn persist_navigation` and persist-navigation through `fn current_focused_context_timeline_key` for `executor::spawn_blocking`;
- never concatenate source strings and never weaken/remove an assertion.

The focused replacement/cache-repair, previous-room pagination/link-preview, timestamp local-before-fallback and thread ordering source tests continue reading parent command/action orchestration unchanged. `committed_room_cleanup_bypasses_a_saturated_account_mailbox` remains parent-owned.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- global production inventory 18/18 and parent 0;
- top-level types/functions 13/13, `NavigationReplacementRoomForCleanup::room_id` 1/1, `AppActor` methods 4/4 keyed by `(self type, item kind, name)`;
- moved tests 6/6 and helpers 2/2, parent 0; relevant retained source tests 6/6 and behavior test 1/1;
- all 1,029 lib test identities equal after normalizing only six owner paths;
- top-level `pub(super)`11, method edges4, associated edge1, field edges5, parent explicit imports11, production orphan binding1;
- parent fields/run/action transaction/timer/channels/maps/generations/reducer/deferred/command/effect registries and timestamp lookup order byte-exact;
- source-test owner paths/boundaries are the only approved retained-test body delta;
- duplicate/missing/excess items, public/wire/resource/dependency deltas 0.

## Verification

Run moved unit tests, retained source/order tests, saturated-mailbox test, `runtime_navigation`, state navigation55, full core library and `cargo check -p koushi-core --all-targets --all-features`; then exactness/source/order/public-path, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main` if required, obtain delta approval, run the complete repository matrix and PR CI7/7.

## Review gate

- Read-only reconnaissance traced persistence, action transaction, cleanup lane, focused acknowledgment and timestamp lookup boundaries.
- `reviewer-flash` independently traced all 18 identities, six tests/two helpers, visibility/import closure, persistence source boundaries and parent ordering/ownership and recorded `Correct-to-implement`.
- Implementation re-confirmed immutable hash/bytes, integrated by `luna-implementer`, and parent-audited.
- Exactness: global production18/18, top-level13/13, AppActor methods4/4, associated method1/1, moved tests6/6, helpers2/2, all1,029 test identities, top-level `pub(super)`11, method edges4, associated edge1, field edges5, parent imports11 and orphan binding1; public/wire/resource deltas0.
- Full-diff review found one leaf-only helper unnecessarily imported/widened. The parent import was removed and `unsubscribe_replaced_focused_context_timeline_key` restored to private.
- Delta review independently verified the exact 11-edge closure and recorded `Correct-to-merge-after-finding-fix`.
- Delivery: final repository gates, latest-main integration if required, PR CI and merge pending.
- `runtime.rs` 7,715 → 7,149 newline-delimited lines; `runtime/navigation.rs` 593.
- Focused moved tests6, relevant retained source/order tests6, saturated-mailbox behavior1, runtime navigation1, state navigation55, core lib1,021/8 ignored and all-targets/all-features check green.
- Full diff and delivery pending.
