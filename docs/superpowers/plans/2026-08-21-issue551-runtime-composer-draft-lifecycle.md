# Issue #551 runtime composer-draft lifecycle extraction

Status: full diff approved; delivery pending. Scope is one atomic behavior-preserving lifecycle seam.

## Baseline

- Base: `ab280a388186933ab0bf192f2957173a6c1fc7e9` after profile/display PR #619.
- `runtime.rs`: 8,280 newline-delimited lines / 342,193 bytes / SHA-256 `5ed28b8067b05e10aab2404ef03d3fa29961e9834e349d02e1c0791ff00f3654`.
- Baseline focused: moved unit tests 3, source contract 1, `composer_draft_lifecycle` 7, `runtime_timeline` 21, `runtime_scheduled_send` 12 and `send_queue_fast` 13.

## Ownership decision

Create private `crates/koushi-core/src/runtime/composer.rs`. Move these 16 top-level identities in immutable-source relative order:

1. `COMPOSER_DRAFT_PERSIST_DEBOUNCE`
2. `composer_draft_account_matches`
3. `composer_draft_revision_for_target`
4. `active_composer_targets`
5. `composer_draft_acceptance_would_exhaust`
6. `timeline_submission_revision_exhaustion`
7. `ComposerAcceptanceIdentity`
8. `PendingComposerAcceptance`
9. `ForwardedComposerDraftPermit`
10. `ComposerDraftLoadStatus`
11. `PendingComposerDraftPersist`
12. `composer_draft_session_key`
13. `ComposerDraftTransitionPolicy`
14. `composer_draft_transition_policy`
15. `composer_acceptance_identity_for_timeline_command`
16. `composer_acceptance_identity_for_action`

Move the complete `ForwardedComposerDraftPermit` implementation with four methods (`new`, cfg-test `new_with_acceptance_probe`, `acceptance_projection_reached`, `acceptance_enqueued`) and its `Drop::drop` implementation. Preserve `#[doc(hidden)]`, every cfg and all method visibility exactly.

Preserve global immutable-source order in the leaf: top-level identities 1–11, these nine `AppActor` methods, then top-level identities 12–16. Move the methods in their original relative order:

1. `reconcile_composer_draft_lifecycle`
2. `reconcile_composer_draft_lifecycle_after_permit_change`
3. `reconcile_composer_draft_lifecycle_with_active`
4. `composer_draft_persistence_protection`
5. `forward_composer_draft_permit`
6. `load_composer_drafts_for_current_session`
7. `schedule_composer_draft_persist`
8. `composer_draft_persist_delay`
9. `flush_pending_composer_drafts`

This leaf owns fail-closed command acceptance identity/permit forwarding, active/leased/store-pending reconciliation, per-session encrypted load status, debounce projection and awaited blocking persistence. It adds no actor, task, channel, timer, cache, map or store owner.

## Parent-owned orchestration

Keep `CoreCommandEnvelope`, every `AppActor` field, `AppActor::run`, test-hook mutation plumbing, `reduce_app_action_state`, `DeferredReducerSideEffects`, `apply_deferred_reducer_side_effects`, `handle_command` and both exhaustive effect dispatchers in `runtime.rs`.

The parent remains the only owner of:

- bounded command/action/event/watch channels;
- lease registry/change receiver/rejection sender+receiver;
- pending acceptance map and pending-persist slot;
- timer polling/select arms, action batching, publication and shutdown;
- reducer ordering and deferred composer/navigation/scheduled-send persistence routing;
- account/timeline forwarding and all CoreCommand/CoreEvent/wire contracts.

Preserve these orderings exactly: actor-held permit clone before forwarding; acceptance match before reduction and release only after projection/deferred effects; previous-session flush before another load; pending-key flush before replacement; acquisition failure retains the existing pending save; persistence permits move into the awaited blocking save closure; shutdown flush precedes AccountActor teardown.

## Façade and visibility

Preserve both existing paths with explicit flat re-exports:

- `koushi_core::runtime::COMPOSER_DRAFT_PERSIST_DEBOUNCE`;
- `crate::runtime::ForwardedComposerDraftPermit` for AccountActor/TimelineActor internals.

Place private `mod composer;` with the existing private-module declarations before the explicit re-exports. Use `pub use composer::{COMPOSER_DRAFT_PERSIST_DEBOUNCE, ForwardedComposerDraftPermit};`; no glob or crate-root API expansion.

Exactly 13 other top-level identities become `pub(super)` because retained parent orchestration uses them: all moved items except the two explicit public re-exports and private `composer_draft_revision_for_target`. Exactly seven moved `AppActor` methods become `pub(super)`: `reconcile_composer_draft_lifecycle_after_permit_change`, `reconcile_composer_draft_lifecycle_with_active`, `forward_composer_draft_permit`, `load_composer_drafts_for_current_session`, `schedule_composer_draft_persist`, `composer_draft_persist_delay`, `flush_pending_composer_drafts`. The other two methods stay private to the leaf.

The parent explicitly imports the 13 `pub(super)` identities. Remove only six production bindings made orphaned by the move: `ComposerDraftProtection`, `ComposerDraftRevision`, `SubmissionId`, `ComposerDraftPersistencePermit`, `PersistedComposerDraftStoreV3`, and `persisted_projection as persisted_composer_draft_projection`. Retain `BTreeSet`, `Instant`, `ComposerDraftStore`, `ComposerTarget`, `ThreadPaneState`, `session_key_id_from_info`, diagnostics, `mpsc` and `oneshot` because parent production/tests still use them.

## Tests and source contract

Move exactly three owner tests with bodies/attrs/order unchanged:

1. `destructive_composer_draft_clear_does_not_schedule_resurrection`
2. `composer_revision_exhaustion_is_detected_for_room_and_thread_submissions`
3. `composer_revision_exhaustion_preflight_preserves_authoritative_draft`

Leaf tests use `super::*`, one explicit `crate::ids::{AccountKey, RuntimeConnectionId}` import and one explicit `koushi_state::SessionInfo` import. Parent test import orphans: zero (`SessionInfo` remains used broadly).

Keep `app_actor_persistence_uses_blocking_store_port` parent-owned. It must read owner files separately, never concatenate source strings:

- `runtime.rs` sections continue checking navigation/scheduled-send/room-preference/settings blocking persistence: replace the moved composer-loader delimiter with `async fn load_scheduled_sends_for_current_session` and the moved composer-schedule delimiter with `fn scheduled_send_delay`;
- `runtime/composer.rs` independently checks `flush_pending_composer_drafts` through the following `composer_draft_session_key` boundary for `executor::spawn_blocking`.

No assertion is weakened or removed. Keep all forwarded-permit/account/timeline tests in their current owners; the compatibility path is unchanged.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- top-level production 16/16, parent 0, original relative order;
- `ForwardedComposerDraftPermit` methods 4/4 and `Drop::drop` 1/1 keyed by `(self type, item kind, name)`;
- `AppActor` methods 9/9 exact modulo seven approved `pub(super)` changes, parent 0;
- moved tests 3/3, parent 0; source-contract test retained with only approved owner-path/boundary edits;
- all 1,029 lib test identities equal after normalizing only three owner paths;
- explicit public re-exports 2, parent imports 13, top-level `pub(super)` 13, method `pub(super)` 7, production orphan bindings 6;
- parent actor fields/run/select/timer/shutdown, reducer/deferred/effect/command registries and call order byte-exact;
- duplicate/missing/excess identities, public-path, wire/resource/dependency deltas 0.

## Verification

Run the same focused baselines after the move plus:

- full core library and the AccountActor/TimelineActor permit owner unit suites;
- `cargo check -p koushi-core --all-targets --all-features`;
- source/exactness/public-path/privacy checks, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main`, obtain delta approval if required, run the complete repository matrix, then PR CI7/7.

## Review gate

- Read-only reconnaissance traced admission, permit drop, reducer/deferred ordering, store blocking, timer/select and shutdown boundaries.
- `reviewer-flash` round 1 found the missing parent orphan `SubmissionId`; the plan corrected it, the moved-test count and exact source boundaries.
- Round 2 independently re-traced identities, paths, visibility, imports, tests and lifecycle order and recorded `Correct-to-implement`.
- `schedule_composer_draft_persist` requires `pub(super)` because retained parent `apply_deferred_reducer_side_effects` calls it; it is not leaf-only.
- Implementation integrated by `luna-implementer`, then parent removed its production glob and established explicit owner imports.
- Exactness: top-level16/16, AppActor methods9/9, permit methods4/4 + Drop1, moved tests3/3, all1,029 test identities, top-level `pub(super)`13, method `pub(super)`7, necessary `PendingComposerAcceptance::identity` field edge1, public re-exports2 and orphan bindings6; public/wire/resource deltas0.
- `runtime.rs` 8,280 → 7,715 newline-delimited lines; `runtime/composer.rs` 641.
- Focused post-move unit3, source1, lifecycle7, scheduled-send12 and send-queue13; core lib1,021/8 ignored and all-targets/all-features check green.
- Initial parallel `runtime_timeline` run observed `lock_unlock_retries_repaired_composer_payload` expected2/got3; exact rerun and complete21-test rerun were green, matching the diagnosed shared diagnostic-observation timing/isolation class rather than a persistence behavior delta.
- Full diff: `reviewer-flash` independently traced all identities, permit/reducer/store/timer/shutdown order, tests, visibility and public paths and recorded `Correct-to-merge`.
- The reviewer classified the initial diagnostic-count failure as non-blocking process-global observation isolation; harden the test only if CI reproduces it, never alter the lifecycle leaf to mask it.
- Delivery: final repository gates, latest-main integration if required, PR CI and merge pending.
