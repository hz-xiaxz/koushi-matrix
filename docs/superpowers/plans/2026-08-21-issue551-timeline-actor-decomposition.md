# Issue #551: Timeline ownership decomposition

## Status

- Design: `reviewer-flash` reviewed the full design/inventory, required explicit empty-impl and cross-owner source-guard amendments, verified both correction rounds against the immutable source, and recorded `Correct-to-implement`.
- Implementation: integrated from immutable AST items; bidirectional exactness, warning parity, both Core lib configurations, focused integrations, all-target/all-feature check, rustfmt and diff checks are green.
- Full-diff review: `reviewer-flash` inspected the complete `origin/main...c935f377` diff, ownership/lifecycle graph, façade, tests and source guards and recorded `Correct-to-merge`; no finding remains.
- Delivery: final repository gates, PR CI and merge pending.

## Objective

Replace the 48,486-line `crates/koushi-core/src/timeline.rs` with a small private-module façade and fifteen existing ownership/verification modules. Move complete AST items and type-qualified inherent methods from the immutable source without changing product behavior, SDK call order, actor state, lifecycle ownership, command/event/reducer/DTO/serde/wire shapes, public paths, diagnostics, timeouts, retry, backpressure, cleanup, or security/privacy behavior.

This is source ownership only. It adds no production helper, state object, callback registry, service, trait, wrapper, fallback, compatibility shim, behavior repair, dependency, or public feature namespace. The only new helper is the cfg-test-only `test_source::item_body` plumbing required to preserve source-characterization tests after ownership moves.

## Immutable baseline

- Commit: `d7b3e268c7564fe691db49e42fe80cd2f67e06a1`
- Source: `crates/koushi-core/src/timeline.rs`
- Size: 48,486 newline-terminated lines; 1,847,055 bytes
- SHA-256: `6bac08386c5f431994345958b9ebf137f028fbb0f7e9012c9d5a4a4832d34e4b`
- Production/cfg-test inventory outside inline test modules: 1,050 keys = 594 named top-level declarations + 455 type-qualified associated items + the explicitly keyed empty `impl Eq for ReadRetryToken {}` container
- Impl-container exactness: all 72 impl identities, attrs and associated-item memberships are tracked, including the one empty impl
- Principal inherent methods: `TimelineManagerActor` 84; `TimelineActor` 112
- Unit tests: 410 unique names (71 gap-tracker module + 339 main test module)
- Source-characterization tests/sites: 55/55 `include_str!("timeline.rs")` sites
- Existing flat public/crate declarations: 30
- Core default baseline: first run 1,013 passed/1 unrelated Account verification ordering failure/8 ignored; the exact failing test passed 3/3 focused and the unchanged full rerun passed 1,014/8 ignored
- Core `test-hooks` baseline: 1,014 passed/8 ignored
- Focused integrations: timeline 21, send queue 13, residency 25, intent lifecycle 5, room-selection scale 4, activity 9, composer-draft lifecycle 7, scheduled send 12; all green

Every extraction uses the immutable blob captured before edits. Line ranges in the inventory are navigation/classification hints only; the extractor reads complete `syn` top-level or associated items and keys them by item kind, qualified self type, name, attrs, and token body. A line-sliced or brace-ambiguous draft is rejected.

The normative 1,050-key and 410-test owner inventory is [2026-08-21-issue551-timeline-actor-inventory.md](2026-08-21-issue551-timeline-actor-inventory.md).

## One atomic ownership-area PR

TimelineManager and per-key TimelineActor form one ownership area: they share one manager mailbox/control lane, actor-generation gate, accepted-send coordinator, subscription-residency owner, read-state supervisor, projection acknowledgement protocol, replay registry, and ordered teardown. Splitting intermediate PRs would require temporary root wrappers, duplicate dispatch/state, broader API, or an incomplete ownership graph. The fifteen private siblings land atomically and the full resource graph receives one design and one full-diff review.

This exception permits no behavior change. A discovered defect stops this PR and moves to a separate verify-first change.

## Target layout

```text
crates/koushi-core/src/
├── timeline.rs                         # private declarations + exact flat façade
└── timeline/
    ├── actor.rs                        # per-key actor fields, mailbox/control dispatch, cleanup
    ├── composer.rs                     # pure ComposerDocument/body/mentions Matrix content
    ├── diagnostics.rs                  # closed private-data-free timeline diagnostic projection
    ├── display_projection.rs           # canonical/display membership rope and validated diffs
    ├── gap_repair.rs                   # inspection/repair/live-tail causal fences and scheduler
    ├── item_projection.rs              # SDK item/HTML/relation/action projection and validation
    ├── manager.rs                      # manager contracts, fields, spawn, central routing
    ├── media.rs                        # download task state, media cache/gallery projection
    ├── navigation.rs                   # generation leases, initial/replay, pagination, anchors, unread
    ├── outbound_send.rs                # accepted enqueue futures, terminals, correlation, queue state
    ├── read_state.rs                   # receipt/fully-read/typing workers, retry, persistence
    ├── residency.rs                    # session-resident room set and membership-operation gate
    ├── room_key_recovery.rs            # key request/decrypt retry and bounded reshare owners
    ├── relay.rs                        # SDK diff subscription, restart/overflow/resync recovery
    ├── thread_projection.rs            # root hydration/replay and pane attention provenance
    ├── test_source.rs                  # cfg(test)-only brace-aware source item helper
    └── test_support.rs                 # cfg(test)-only genuinely shared existing fixtures
```

No `mod.rs`, public child module, glob import/re-export, barrel, wrapper service, one-implementation trait, compatibility alias, duplicate helper, new state object, macro dispatch, or dependency.

`timeline.rs` retains the existing module docs and only private declarations plus explicit flat re-exports. It contains no production/test function body, actor field, behavioral constant, registry, task owner, or diagnostic implementation.

## Existing façade/API compatibility

Preserve exactly these 30 names, visibility and cfg gates at `crate::timeline::*` / `koushi_core::timeline::*`:

- manager: `TIMELINE_DIFF_QUEUE_CAPACITY`, `TimelineMessage`, `TimelineManagerHandle`, `TimelineManagerActor`;
- residency: `RoomRemovalCause`, `RoomMembershipTransitionKind`, `RoomMembershipTransition`, `VisibleRoomObservation`, `TimelineSubscriptionResidencyPermit`, `TimelineSubscriptionResidencyHandle`;
- navigation: `TimelineProjectionAcknowledgement`, `NavigationProjectionIntent`, `NavigationProjectionCleanup`, `NavigationProjectionIngress`, `display_projection_reset_fallback_count`;
- read state: `ReadPersistenceIngress`, `ReadPersistenceRequest`;
- composer: `validate_composer_body_for_timeline_send`, cfg-test `build_room_message_content_from_composer_document`, `build_room_message_content_from_composer_body`, `build_room_message_content_from_composer_body_with_options`;
- item projection: `sdk_item_to_timeline_item`, `timeline_item_can_react`, `validate_send_reaction`, `validate_redact_reaction`, `timeline_item_can_redact`, `timeline_item_can_edit`, `validate_retry_send`, `validate_cancel_send`, `reaction_groups_from_sdk`.

All existing associated paths, including spawn/shutdown/test-hook methods, remain available through the same root types. External callers remain unchanged. Child paths are private implementation details.

## Ownership map

### `manager.rs`

Owns `TimelineMessage`, `TimelineManagerControl`, `TimelineManagerHandle`, `TimelineManagerActor` with its unchanged complete field set, spawn, central manager `run`, exhaustive `TimelineMessage` and `TimelineCommand` routing, subscribe/rebuild/actor replacement, actor lookup, failure/action emission, and ordered manager shutdown entry. It contains the irreducible dispatch matches directly; leaves are sibling inherent impls, not wrappers or function tables.

### `actor.rs`

Owns `TimelineActorMessage`, `TimelineActorControl`, `TimelineActorCleanupIngress`, `TimelineActorHandle`, `TimelineActor` with the unchanged complete field set, spawn, actor `run`, exhaustive actor message dispatch, control cleanup, common reliable emission, and `Drop`/stop behavior. Accepted send/session work is not moved into this presentation owner.

### `residency.rs`

Owns membership transition/removal/visibility DTOs, `MembershipOperationGate`, permit/handle, room lease/session leave state, sync-start/visible/membership/leave/rejoin handling, restored coverage, subscription reconciliation, existing-room rebuild, and membership-operation admission/drain. The residency set remains uncapped, in-memory and account-session-owned by TimelineManager; actor unsubscribe/navigation/replay never removes it.

### `outbound_send.rs`

Owns send terminal ingress/admission/handoff, enqueue contexts/payloads/futures, worker supervisor, admission ledger, completion coordinator/registration/tombstones, global terminal monitor, queue monitor, send lifecycle trace, manager send worker routing, actor retry/cancel/send-queue presentation, and exact terminal handoff. It does not move any accepted future or terminal correlation into TimelineActor.

### `read_state.rs`

Owns `ReadPersistenceIngress/Request`, read worker supervisor/network/apply/retry state, manager read routing/completion/waiter settlement, actor receipt/fully-read/typing updates, authoritative observation publication, receipt collection, and typing observer. Desired reads remain session/operation-generation fenced, bounded, persisted, and manager-owned.

### `room_key_recovery.rs`

Owns bounded post-send room-key reshare schedules/workers, decrypt-retry/key-request controller, recovery resume persistence/ticks, diagnostics and manager/actor handlers. Existing timeout, task ownership, replacement cancellation, stale-operation fencing and private-data-free tokens move intact.

### `relay.rs`

Owns relay restart/backoff/control/batch, `run_diff_relay`, authoritative overflow/stream-end resubscription preparation/commit, actor relay-control/diff/overflow methods and generation acceptance. It preserves one `Timeline::subscribe()` snapshot+stream boundary, lossless control lane, bounded restart timer, and actor shutdown cancellation.

### `gap_repair.rs`

Owns all historical/live-edge gap descriptors, tracker, scheduler selection/budget, causal projection/render/relay fences, manager committed-response/checkpoint/live-tail orchestration, actor inspection/repair/settlement methods, and the existing 71-test tracker suite except the two send-scheduler tests owned by outbound send. It must not split live-tail from the shared causal repair state or broaden automatic repair.

### `navigation.rs`

Owns projection ingress/ack, generation gate/lease, initial/replay settlement, pagination, anchor restore, committed room selection/foreground demand, unread/navigation projection, activity rows and stable replay windows. Request IDs remain correlation values; generation ordering and latest-desired control behavior remain unchanged.

### `thread_projection.rs`

Owns root fetch/replay registries, hydration preparation/terminal handoff, loaded-root/reaction projection, thread-attention provenance/tracker and missing-root hydration. Hydration workers remain manager-owned; attention remains event-origin/receipt-provenance based and does not infer liveness from vector shape.

### `media.rs`

Owns actor-local bounded media download tasks/results, private source cache changes, media gallery replacement/action projection and media download failure classification. Downloaded bytes and encrypted media data remain Rust-local and never cross events/diagnostics.

### `composer.rs`

Owns only the existing pure composer validation and Matrix message-content/mentions construction functions. It adds no product state or command routing and preserves literal slash/format/mention semantics.

### `item_projection.rs`

Owns SDK item conversion, sanitized formatted/plain/code/spoiler projection, state notices, media metadata mapping, send-state mapping, editable document/reply/reaction/action validation, link-preview/reply detail/message-source/forward/search-index item helpers and actor handlers. SDK aggregation remains authoritative; this module does not create a second timeline accumulator.

### `display_projection.rs`

Owns the canonical/display membership rope, bounded display state/context, canonical batch translation, diff validation/fallback, item-list diff application and display identity normalization. Only this owner advances the SDK-index accumulator; independent snapshots cannot replace it.

### `diagnostics.rs`

Owns the existing closed token/count/bucket diagnostic builders and trace producers. It holds no actor/product state and adds no free-form field. Moving it is a verification/privacy boundary, not a generic utility abstraction.

## Cross-module visibility and dependency rules

- All child modules are private. Existing public/crate root names retain exact visibility; no child module path becomes public.
- `TimelineManagerActor` and `TimelineActor` remain one struct each. Their existing field sets are unchanged; only fields proven read/constructed by a sibling may become `pub(super)` to restore former parent-module scope.
- Sibling inherent methods become `pub(super)` only when the immutable call graph has a concrete sibling caller. Owner-private methods stay private; existing public/crate methods retain exact signatures.
- The normative inventory keys every associated method as `(self type, item kind, name)`, so repeated names on different types cannot collide.
- An unlisted required visibility edge is a stop condition: amend the inventory and re-review instead of opportunistically widening it.
- Leaves call existing sibling methods directly. They do not add forwarding wrappers, façade-qualified internal calls, callbacks, traits or registries.
- `manager.rs` and `actor.rs` may depend on leaves for direct exhaustive dispatch. Leaves may depend on owner structs/contracts and explicitly evidenced siblings. No leaf re-exports another leaf.

## Lifecycle, ordering and security invariants

Preserve exactly:

1. One account-session TimelineManager mailbox/control lane, one per-key TimelineActor mailbox/control lane, and one retained owner for every task, future, subscription, timer and continuation.
2. TimelineManager alone owns accepted enqueue futures, the client-global terminal observer/correlation coordinator, room residency, read supervisor, root hydration workers and actor registry.
3. TimelineActor owns only its SDK timeline subscription/relay, presentation projection, pagination/link-preview/media/decrypt tasks and retry/cancel handles.
4. Unsubscribe/actor replacement stops presentation resources but never cancels an accepted send or loses its terminal correlation.
5. Accepted send order remains registration → permit-blocked worker → reducer acceptance → permit open → exact worker preflight signal → SDK enqueue/bind → global terminal → reliable reducer/event settlement.
6. Manager orderly shutdown keeps one count-independent five-second deadline, polls workers and observer, synchronously drops remaining futures while terminal admission exists, final-polls and drops observer, then stops actors and drains terminal ingress before ack.
7. Unexpected drop closes terminal admission, synchronously drops futures, then drops observer; raw task-handle drop never becomes orderly cleanup.
8. Membership shutdown closes admission and drains exact manager-instance permits before residency/session retirement; SDK session pointer identity and leave/rejoin observation ordering remain unchanged.
9. Diff relay replacement stops/settles the old owner, increments generation, publishes resync then authoritative InitialItems, and rejects stale batches. Overflow and stream-end controls remain lossless and bounded.
10. Canonical SDK positions and display positions remain distinct. Each SDK batch is applied once; invalid translation uses one validated Reset and the existing counter.
11. Gap repair/live-tail keeps exact response/subscription/actor/repair/publication/batch/render fencing, one bounded scheduler, work permits, budgets, queued-trigger priority, timeout recovery and no arbitrary gap substitution.
12. Navigation remains latest-desired/generation ordered; read state remains session/operation fenced, latest-wins and bounded; thread attention remains receipt/event-origin proven.
13. All channels, constants, timeouts, retry curves, queue bounds, tombstone bounds, cfg gates and reliable/lossy delivery choices remain byte-equivalent.
14. Commands/events/reducers/DTO/serde/wire/Tauri/QA registries are untouched.
15. Message bodies, filenames, Matrix identifiers, transaction IDs, SDK errors, key material and paths remain excluded from diagnostics, Debug and review evidence exactly as before.

## Test redistribution

Move all 410 tests exactly once beside the production owner. The inventory pins counts: actor 3, composer 12, diagnostics 13, display projection 17, gap repair 70, item projection 51, manager 7, media 6, navigation 46, outbound send 48, read state 34, residency 6, room-key recovery 18, relay 14, thread projection 65. Sum: 410.

The two scheduler/send tests textually located in the 71-test gap-tracker module move with outbound send; the remaining 69 tracker tests plus main-module `gap_repair_room_switch_cancels_completion` move with gap repair, for 70 total. Every attr, ignore flag, start-paused setting, body, assertion, literal and ordering stays intact.

Existing helper ownership:

- owner-local helpers move with their only consuming suite;
- `test_support.rs` may contain only the existing helpers consumed by multiple owners: `fake_rid`, `room_key`, `replacement_generation_fixture`, `replay_projection_services`, `timeline_item`, `test_timeline_actor_handle`, `gap_demand_test_actor_handle`, `live_tail_test_manager`, `timeline_media_item`, `focused_key`, and `thread_key`, plus their required existing fixture structs/impls;
- imports from support are explicit; no glob and no copied/generalized fixture;
- a helper found to have only one owner moves back to that owner rather than remaining shared.

### Source-characterization migration

The inventory names all 55 tests. Each reads the explicit owner source file(s) containing the guarded items. Single-owner contracts read one file; cross-owner contracts read every listed file separately and apply a private brace/string/comment-aware `item_body` helper per declaration. No source strings are concatenated, no omnibus/generated source exists, and no façade source can satisfy a child implementation assertion.

All assertions remain intact. All 55 source guards are explicitly authorized to replace positional `.split(...).nth(...)` delimiter slicing with brace-aware `item_body(source, qualified_item)` calls for the same declarations, in addition to changing `include_str!` paths. This is test plumbing, not assertion relaxation: each existing positive/negative predicate is applied to the same immutable item body or explicit set of item bodies. In particular:

- `replay_known_registry_lifecycle_helpers_cover_actor_refresh_paths` reads exact item bodies from `actor.rs` (`spawn`, `run`, `handle_msg`, and `emit_action_reliable`), `navigation.rs` (`handle_replay_initial_items` and `finish_anchor_restore`), `relay.rs` (`handle_diff_batch` and `handle_relay_overflow`), `outbound_send.rs` (`handle_send_queue_lagged` and `resync_send_queue_statuses`), and `item_projection.rs` (`handle_ignored_users_updated`). Assertions that name `commit_sdk_batch_for_generation` and `maybe_hydrate_missing_thread_roots` additionally read those exact bodies from `display_projection.rs` and `thread_projection.rs`. The old positional terminators `run`, `handle_diff_batch`, `resync_send_queue_statuses`, `emit`, `handle_ignored_users_updated`, `emit_action_reliable`, and `handle_paginate` are replaced by per-item boundaries rather than treated as asserted source.
- `media_gallery_and_thread_attention_projections_use_reliable_delivery` reads only `actor.rs` (the exact `spawn` item containing the InitialItems emission block), `relay.rs` (`handle_diff_batch`), and `media.rs` (`emit_media_gallery_if_changed`). Its old `emit_navigation_if_changed` positional terminator is replaced by the `emit_media_gallery_if_changed` item boundary and does not create a `navigation.rs` source dependency.

No source strings are concatenated. A cross-production negative assertion scans files independently and combines only booleans. Destination top-level items, associated methods and tests retain immutable-baseline relative source order per owner so any still-order-sensitive structural contract remains deterministic. If another guard cannot be expressed as the same per-item predicate without changing an assertion, implementation stops and amends/re-reviews this table.

## Mechanical integration

After design approval, Luna/low write-capable workers operate in isolated worktrees on disjoint destination files. Proposed extraction groups:

- A: `composer`, `item_projection`, `display_projection`;
- B: `residency`, `read_state`, `navigation`;
- C: `outbound_send`, `room_key_recovery`, `media`;
- D: `relay`, `gap_repair`, `thread_projection`;
- integration owner only: `manager`, `actor`, `diagnostics`, parent façade, test support/source helper, imports/visibility and exactness.

Workers copy complete immutable AST items/methods/tests by the normative key list. They do not edit the parent, another destination, manifests/contracts/canon, or behavior. They report body hashes, counts and unresolved edges; any missing/extra/ambiguous item or visibility edge stops instead of guessing.

One integration owner reconstructs split inherent impls from exact qualified method keys, replaces the parent once, and resolves only inventory-approved imports/scope. Never cut from a line-shifted intermediate tree.

## Exactness evidence

A temporary non-repository `syn` verifier must prove bidirectionally:

1. All 1,050 production/cfg-test keys exist once: 594 named top-level declarations + 455 qualified associated items + the empty `impl Eq for ReadRetryToken` container, including all 84 manager and 112 actor methods. All 72 impl containers also match identity, attrs, trait/self type and associated-item membership.
2. All 410 test names exist once with attrs/bodies matching except the enumerated source/path plumbing.
3. The 30 flat public/crate declarations, cfg gates, docs, derives, signatures, fields, enum variants, constants, strings, match arms and token bodies match.
4. Root contains only private declarations and explicit 30-name re-exports; no production/test body, behavioral constant, glob or child public namespace.
5. Non-split impls match whole-token form; split actor/manager methods match individually by qualified key.
6. Production bodies match after normalization limited to required sibling qualification and reviewed `pub(super)` scope restoration.
7. The generated sibling field/method/type visibility report has a concrete caller for every promotion and no unlisted widening.
8. All 55 source guards read explicit owner files, no concatenation exists, and only approved test plumbing differs.
9. No wrapper, duplicate, compatibility shim, new state object, trait, callback registry, TODO, dead-code allowance, dependency, diagnostic field/token, or behavior change exists.

Pre-existing warnings are baseline artifacts, not permission to add or suppress warnings.

## Integrated implementation evidence

- `timeline.rs`: 48,486 → 99 lines. It contains the unchanged module canon docs, fifteen private production declarations, two cfg-test support declarations, and the exact thirty-name flat façade; no production/test body or behavioral constant remains.
- Production exactness: 1,050/1,050 named keys and 72/72 logical impl identities/memberships, including 84/84 TimelineManagerActor and 112/112 TimelineActor methods.
- Test exactness: 410/410 tests and 44/44 existing helpers. All non-source test attrs/bodies match immutable tokens; the only twenty body-token deltas are source-characterization plumbing that replaces cross-file positional slicing with explicit per-item `item_body` predicates. All 55 source-contract tests read explicit owner files, and no source is concatenated.
- Façade: 30/30 baseline public/crate names, exact cfg gates, no public child module, glob, barrel or compatibility alias.
- Warning parity: fresh baseline/current `cargo check -p koushi-core --lib` both report 30 koushi-core warnings with the same categories; no warning or allow-list was added.
- Core lib default and `test-hooks`: 1,014 passed/8 ignored each. The pinned Account ordering test passed three focused runs before the post-move full suite.
- Focused integrations: runtime timeline 21, send queue 13, room-subscription residency 25, intent lifecycle 5, room-selection scale 4, activity 9, composer-draft lifecycle 7, scheduled send 12; all green.
- `cargo check -p koushi-core --all-targets --all-features`, `cargo fmt --all -- --check`, and `git diff --check` are green.

### Final local evidence

- Rust workspace final rerun: 2,393 passed/13 ignored/0 failed across 97 suites; desktop lib 149 passed/1 ignored; Headless Core QA binary 129 passed.
- The first workspace run hit the existing five-second room-list synchronization fence; the unchanged focused test passed, including its normal setup wall time beyond the internal fence. The next workspace run hit the pre-recorded Account action-ordering flake; the exact test passed 3/3 focused and the unchanged final workspace rerun passed. No timeout, expectation, source, or behavior was changed or waived.
- Frontend: typecheck and lint green; Vitest 1,367 passed; UI-headless timeline store plus Playwright 248 passed with `CHOKIDAR_USEPOLLING=true`; production build green.
- Boundary/policy: Tauri adapter, domain dependencies, tracked secret scan, release gates, IPC generated-wire contract, SDK submodule, agents docs, wasm domain check, `cargo deny`, `cargo machete`, rustfmt, exactness and diff checks green.

## Verification

Baseline and identical post-move checks. Before each full Core lib run, the unrelated ordering-sensitive Account test is pinned with three focused passes; if the subsequent full run fails it, rerun the exact focused test three times and the unchanged full suite once, recording both outcomes rather than changing its expectation:

```bash
for i in 1 2 3; do cargo test -p koushi-core --lib actor_sas_settlement_emits_exactly_one_terminal_and_clears_runtime; done
cargo test -p koushi-core --lib
cargo test -p koushi-core --lib --features test-hooks
cargo test -p koushi-core --test runtime_timeline
cargo test -p koushi-core --test send_queue_fast
cargo test -p koushi-core --test room_subscription_residency --features test-hooks
cargo test -p koushi-core --test runtime_intent_lifecycle
cargo test -p koushi-core --test runtime_room_selection_scale
cargo test -p koushi-core --test runtime_activity
cargo test -p koushi-core --test composer_draft_lifecycle
cargo test -p koushi-core --test runtime_scheduled_send
cargo check -p koushi-core --all-targets --all-features
cargo fmt --all -- --check
git diff --check
```

After `reviewer-flash` full-diff `Correct-to-merge`, run the complete repository gate matrix used by the Account/Room actor PRs: workspace all targets, desktop lib, QA binary, frontend typecheck/lint/Vitest/build, `CHOKIDAR_USEPOLLING=true` browser-headless, Tauri/domain/secret/release/IPC checks, SDK submodule, agents docs, `cargo deny`, rustfmt and diff checks. Run generated/wire checks despite the no-contract-change rule. Local homeserver/GUI lanes are required only if compile/exactness/tests/review expose runtime-path ambiguity; behavior may not be waived into this PR.

## Stop conditions

Stop and amend/re-review before implementation continues if:

- any production/test/helper/source guard has ambiguous ownership;
- extraction requires a wrapper, duplicate, new state object, trait, callback table, public child, glob/barrel or compatibility alias;
- any body/order/cfg/path/API/command/event/reducer/DTO/wire/timeout/retry/backpressure/cleanup/privacy/diagnostic token changes beyond approved import/scope/source plumbing;
- any task/future/subscription/timer/terminal/residency/read/gap/relay owner or teardown ordering changes;
- an unlisted sibling visibility edge is required;
- a worker needs the parent or another worker's destination;
- exactness cannot prove all 1,050 production keys, 196 principal actor methods, 410 tests, 30 façade names and 55 source guards;
- a test exposes a behavior defect.
