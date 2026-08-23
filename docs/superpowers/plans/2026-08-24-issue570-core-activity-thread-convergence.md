# Issue #570 Task B — Core Activity, Unread, and Thread Convergence

## Dependency and scope

This independently mergeable task is based on merged Task A main
`2d321eed144ee45a492b7953fdbe8f03aa482397` and vendor gitlink
`56028a4ded016381d75bdd5ed978af380f0809a2`. The original reviewer exhausted
its monthly quota; the user approved `reviewer-flash` as the mandatory
substitute. Implementation must not start until this design has its recorded
`Correct-to-merge` verdict.

Task B makes Core/state consume canonical SDK truth. It does **not** add the
Task C room-latest `is_redacted` DTO, Tauri/TypeScript/Browser Fake fields, or
both-server convergence scenario, and it does not close #570.

## Contract boundaries

### One eligibility predicate

Add one private Core projection helper and derive the predicate from it:

```rust
fn eligible_activity_preview(item: &TimelineItem) -> Option<String> { /* existing safe body/media/formatted fallback */ }
fn is_attention_eligible_event(item: &TimelineItem) -> bool {
    matches!(item.id, TimelineItemId::Event { .. })
        && !item.is_redacted
        && !item.is_hidden
        && eligible_activity_preview(item).is_some()
}
```

Activity row derivation must consume the same returned preview; it may not apply
a second body-only filter that drops formatted-only renderable content.

Use exactly this predicate for canonical Activity event rows, timeline unread
navigation/first-unread/newer counts, thread reply activity/placement, and
thread-attention valid-set reconciliation. Own-user exclusion remains an
additional unread/attention condition. Transactions, synthetic rows,
standalone relations, redacted/hidden rows, and non-renderable bodyless rows are
ineligible. An undecryptable reply can become eligible exactly once after a
later renderable `Set`.

Do not change timeline display-redaction policy or React filtering.

### Canonical Activity provenance

Replace append-only `ActivityProjection::rows_by_event_id` with:

```rust
struct ActivityProjection {
    canonical_rows_by_room: BTreeMap<String, BTreeMap<String, ActivityRow>>,
    resolution_rows_by_event_id: BTreeMap<String, ActivityRow>,
    redacted_event_ids: BTreeSet<String>,
    hidden_event_ids_by_room: BTreeMap<String, BTreeSet<String>>,
    invalidated_placeholder_room_ids: BTreeSet<String>,
}
```

Add an internal, non-wire AppAction:

```rust
CanonicalActivityWindowReconciled {
    room_id: String,
    rows: Vec<ActivityRow>,
    redacted_event_ids: Vec<String>,
    hidden_event_ids: Vec<String>,
}
```

Rules:

- `rows` is the complete eligible bounded canonical room window and replaces
  only `canonical_rows_by_room[room_id]`; an empty vector deletes that room;
- duplicate IDs are last-in-window wins before storage;
- authoritative redaction invalidations remove the stable identity from
  canonical, resolution, and invalidation-cleared placeholder provenance;
- ordinary Remove/Clear/Truncate disappearance is represented by full-room
  replacement, not global invalidation;
- only an accepted post-diff **redacted** stable ID enters the persistent
  `redacted_event_ids` tombstones;
- hidden/ignored IDs replace only `hidden_event_ids_by_room[room_id]` and are
  reversible eligibility suppression, never permanent invalidation;
- ignored-user reprojection recomputes `is_hidden` from the immutable projected
  sender/content/redaction baseline plus the current ignored set (no irreversible
  `old_hidden || ignored`); ignore→unignore republishes and can restore an
  otherwise eligible canonical or resolver row;
- resolver observations enter only `resolution_rows_by_event_id`; snapshots skip
  IDs currently hidden in their room and can never resurrect IDs in
  `redacted_event_ids`;
- snapshot merge overlays resolver rows first and canonical rows second, so
  canonical effective content owns duplicate identity;
- existing room-unread placeholders, mute settings, mark-read and resolution
  generations remain closed and separately bounded.

Use explicit deterministic bounds:

- at most 512 canonical room slots, LRU by accepted replacement sequence;
- at most 120 canonical event rows per room (the existing
  `ROOM_REPLAY_INITIAL_ITEMS_MAX`), newest eligible activity first;
- at most 2,048 canonical event rows globally;
- at most 200 resolver rows (the existing Recent bound);
- at most 2,048 redaction tombstones, evicting oldest only when the ID is absent
  from active canonical/resolver maps;
- hidden-ID sets exist only for the retained 512 room slots and share each
  room's 120-ID bound.

Overflow of an unread event detail deterministically degrades to the existing
room-unread placeholder, preserving unread signal without retaining unbounded
event detail. Recent remains capped at 200; existing 201-row unread behavior is
preserved below these provenance limits. Bounds use monotonic ordinal/LRU
metadata and count-only diagnostics; never hash or log IDs/content.

### Reliable canonical publication

After every accepted generation-fenced canonical commit, derive and reliably
publish the full room Activity replacement, even when empty:

- initial subscription and empty initial state;
- live/pagination `Set`/Remove/Clear/Truncate/Reset;
- restore-buffered flush;
- relay overflow/recovery replacement;
- ignored-user and profile/display reprojection affecting eligibility/content;
- idempotent replay/resubscribe, send-queue lag replacement, and actor startup/
  replacement.

Reserve required reducer capacity before taking the actor-generation commit
lease, then publish timeline state and `CanonicalActivityWindowReconciled`
inside the same accepted commit boundary. Never `try_send` the sole canonical
replacement. A retired generation publishes neither. Profile-only label changes
may reuse the current canonical row identities; they must not synthesize Matrix
semantics.

Derive redaction tombstones and reversible hidden sets from the accepted
post-commit stable event-ID window. A post-diff explicitly redacted item is a
redaction tombstone; a hidden-but-not-redacted item enters only that room's
replaceable hidden set. Index-only disappearance is neither.

### SDK resolution and effective edits

Keep `MatrixTimelineSubscription::current_items` as SDK authority: originals
retain stable event identity, effective valid edit content is projected onto the
original, and redacted/non-message events map to absence. Add SDK regression
coverage for edit-before-original, redaction-before-original, pagination/replay,
redacted-latest-edit fallback, and live/current-items equivalence. Core adds no
edit/redaction ledger.

Activity resolution remains request-generation fenced, writes only resolver
provenance, and refuses explicit invalidated IDs.

### Timeline unread/navigation

Route `derive_timeline_navigation_snapshot`, `newer_unread_event_count`, marker
advancement and first-unread selection through shared eligibility:

- redacted/hidden/edit-relation rows contribute zero;
- edited originals retain identity/order;
- first unread advances to the next eligible original;
- own-user rows remain excluded from unread counts after eligibility;
- reset, pagination, replay and out-of-order application converge identically.

No receipt transport/retry change belongs in Task B.

### Thread attention

At the start of `ThreadAttentionTracker::reconcile_batch`, derive the current
eligible reply-ID set for its exact room/root and retain `attention_event_ids`
only in that set before receipt ordering. Emit a transition when pruning changes
count. Preserve `observed_reply_event_ids` as replay dedupe; do not intersect it
with the bounded loaded window.

A removed/redacted/hidden reply immediately loses attention and replay cannot
restore it. `acknowledge()` performs the same current eligible-set prune before
receipt ordering, so correctness does not depend on an actor calling
`reconcile_batch` immediately beforehand. Late decryption remains 0→1→1. Total
reply count is independent.

### Authoritative thread aggregate

Extend each `ThreadRootProjectionRecord` with:

```rust
struct AuthoritativeThreadAggregate {
    reply_count: u32,
    latest_event_id: Option<String>,
    latest_sender: Option<String>,
    latest_sender_label: Option<String>,
    latest_body_preview: Option<String>,
    latest_timestamp_ms: Option<u64>,
}
```

`ThreadRootProjectionService` owns checked per-root `activity_revision` and
`summary_revision` counters (u64; exhaustion retires the root rather than
reusing a value). `activity_revision` increments on every accepted effective
projection or eligibility change, including a same-ID/same-timestamp edit; full
`ThreadRootProjectionActivity` semantic equality, not timestamp/ID ordering
alone, decides that change. A loaded reply remains a placement key only.

On new selected activity, accepted redaction/removal/reset, replay handoff, and
initial hydration:

1. under the accepted commit boundary, capture the union of pre-commit and
   post-commit affected roots before reconciliation can drop a disappeared root;
   retain each root as pending-summary until its exact completion;
2. capture actor generation + room/root + root activity revision + next summary
   revision;
3. use the existing `matrix_sdk::Room`/RoomEventCache after the one permitted
   root hydration and call Task A's public
   `matrix_sdk_ui::timeline::resolve_thread_relation_aggregate`;
4. map Task A's `ThreadListItemEvent` with a concrete Koushi adapter in
   `threads_list.rs`: preserve its event ID/sender/timestamp, derive sender label
   from `TimelineDetails<Profile>`, and derive a privacy-safe preview from its
   `TimelineItemContent` using the existing thread-list `body_preview` rules for
   message/media/sticker/UTD/unsupported content. Do not call the incompatible
   `TimelineItem` adapter or stringify Debug/content;
5. complete through the actor control lane;
6. admit only exact actor generation, room/root, activity revision and summary
   revision;
7. set count exactly; zero clears every latest field and stale projection;
8. emit one replacement projection and release pending-summary retention; no
   `max(1)`, loaded-window count, network
   pagination, retry loop or sleep.

Capture an `AggregateRefreshCause` with each revision. If the cache resolver
returns a coarse error, keep the prior authoritative aggregate only for a
still-present root and current summary revision, then project the existing
closed failure path. For a root proven absent by the accepted pre/post-union
Remove/Clear/Reset/redaction cause, fail closed to an empty terminal summary and
release pending retention rather than leaving stale nonzero UI. This is an
explicit unavailable/cleared terminal, not a claim that a successful cache
query returned zero. Never infer from the selected reply. A newer scheduled
revision supersedes an older success/failure. Add success/error release tests for
both present and disappeared roots.

Task A already returns `u32`; preserve it exactly with no conversion or
saturation. Count zero is authoritative.

## Verify-first RED matrix

Add tests before production wiring and record behavioral REDs:

### Activity

- canonical A/B → B redacted/omitted promotes A;
- empty replacement removes stale room rows;
- resolver historical + unrelated canonical live coexist;
- explicit invalidation removes duplicate resolver row and prevents resurrection;
- same original `Set` old→edited yields one effective edited row;
- restore/replay replaces a deliberately stale cache;
- ignore→unignore restores reversible canonical/resolver eligibility while
  redaction remains tombstoned;
- 120-per-room, 512-room, 2,048-global, resolver and tombstone overflow obeys
  deterministic eviction and unread-placeholder degradation;
- mute/excluded room behavior remains.

### Reliable publication

- empty initial, Remove/Clear/Reset, restore flush, overflow replacement,
  ignored-user reprojection and actor replacement each publish one full-room
  replacement;
- a replaced actor blocked on reducer capacity publishes nothing;
- no sole-authority path uses `try_send`.

### SDK resolution

- edit before original, redaction before original, replay and pagination converge;
- redacted latest valid edit promotes prior valid effective content;
- original event ID/timestamp/sender are retained.

### Navigation

- marker, redacted A, valid B, own C → first B, unread/newer count 1;
- hidden and standalone edit rows count zero;
- effective edit keeps identity/order;
- reset/pagination/replay/out-of-order yield the same snapshot.

### Thread attention/aggregate

- reply redaction prunes attention 1→0 and replay remains 0;
- late decryption 0→1→1;
- count2/latest B → redaction gives count1/latest A → final redaction gives 0/None;
- edit A retains original identity/count1 with effective preview;
- bundled count4 with loaded one projects 4;
- delayed old completion cannot overwrite a newer summary revision;
- same-ID/same-timestamp edit advances activity revision and fences old work;
- final-reply Remove, relation-stripped redaction, Clear and Reset retain the
  pre/post union root until authoritative count0 completion;
- actor replacement and both serial exhaustion paths fence old work.

RED must show current append-only Activity, eligibility divergence, attention
retention, and `max(1)`/loaded-reply summary behavior. Type scaffolding may land
first, but compile-only failures are not behavioral RED.

## Files

Expected maximum:

- `crates/koushi-core/src/runtime/activity.rs` and `runtime.rs`; keep resolution
  logic in the existing activity module rather than extracting a speculative
  `activity_resolution.rs` file;
- `crates/koushi-core/src/timeline/{actor,relay,navigation,item_projection,
  thread_projection,display_projection,outbound_send}.rs` as proven by callers;
- `crates/koushi-core/src/threads_list.rs` for root aggregate records only;
- `crates/koushi-sdk/src/timeline.rs` and focused tests;
- `crates/koushi-state/src/action.rs` plus
  `crates/koushi-state/src/reducer/{activity,mod}.rs` for exhaustive Debug/
  dispatch/reduction of the internal canonical replacement;
- architecture/state-machine/state-ownership docs and this plan/index.

Do not touch state room-latest DTOs, Tauri, TypeScript, React, Browser Fake, QA
scenario registry, persistence schema, search semantics, or vendor code/gitlink
(the Task A gitlink is consumed unchanged).

## Gates and acceptance

Focused RED/GREEN; SDK timeline; Core Activity/runtime/timeline/navigation/thread
suites; state action dispatch; root workspace/all-targets; wasm; Tauri compile;
QA binary tests; fmt/docs/boundary/security/dependency/diff; exact review; CI7/7.
No server lane is required until Task C because Task B exposes no new command or
QA scenario.

| Requirement | Evidence |
| --- | --- |
| canonical Activity replacement | room-window/empty/invalidation/replay tests |
| one eligibility owner | shared helper callers + navigation/attention matrix |
| edit identity/effective content | SDK + Activity tests |
| exact thread total/latest | Task A resolver + summary revision tests |
| stale safety | actor/activity/summary revision fences |
| privacy/bounds | Debug/count diagnostics/bounded maps |
| task isolation | no Task C DTO/frontend/fake diff; unchanged gitlink |

Implementation begins only after the user-approved mandatory substitute
`reviewer-flash` records `Correct-to-merge` for this document.

## Implementation evidence

### RED before production wiring (2026-08-24)

The deterministic reproductions were added before production wiring and run against the current append-only Core paths. No compile-only failure was counted as RED.

- `cargo test -p koushi-core --test runtime_activity canonical_activity_window_replaces_omitted_event`: **RED** (exit 101), 0 passed / 1 failed / 10 filtered; the stale omitted event remained in the Activity snapshot.
- `cargo test -p koushi-core --test runtime_activity canonical_activity_empty_window_removes_stale_room_rows`: **RED** (exit 101), 0 passed / 1 failed / 10 filtered; the empty observation did not clear the stale room row.
- `cargo test -p koushi-core --lib timeline::navigation::tests::eligibility_skips_redacted_and_own_rows_for_first_unread_and_newer_count`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; the redacted row won first-unread eligibility and inflated the count.
- `cargo test -p koushi-core --lib timeline::navigation::tests::formatted_only_activity_rows_remain_eligible`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; Activity row derivation dropped formatted-only renderable content.
- `cargo test -p koushi-core --lib timeline::thread_projection::tests::thread_root_activity_requires_shared_attention_eligibility`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; redacted/hidden replies still produced root activity placement.
- `cargo test -p koushi-core --lib timeline::thread_projection::tests::thread_attention_prunes_redacted_reply_before_replay`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; attention remained nonzero after redaction.
- `cargo test -p koushi-core --lib timeline::thread_projection::tests::thread_attention_acknowledge_prunes_hidden_reply_without_reconcile`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; acknowledge retained hidden attention without a prior reconcile.
- `cargo test -p koushi-core --lib threads_list::tests::same_reply_identity_edit_advances_activity_revision_boundary`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; same-ID/same-timestamp effective edits did not advance the current activity comparison.
- `cargo test -p koushi-core --lib timeline::item_projection::tests::ignored_sender_suppression_is_reversible`: **RED** (exit 101), 0 passed / 1 failed / 1038 filtered; ignore suppression was irreversible.

`cargo test -p koushi-core --lib timeline::item_projection::tests::formatted_only_content_is_renderable_for_shared_eligibility` was **GREEN** (exit 0), 1 passed / 0 failed / 1038 filtered, confirming the existing renderability helper already covers formatted-only content; the RED is specifically the divergent Activity-row producer above.

### GREEN for Task B slice 2 (2026-08-24)

The five unchanged slice tests now pass with exit 0: navigation eligibility and formatted-only Activity rows (1 passed each), thread-root eligibility, redaction pruning/replay, and acknowledge pruning (1 passed each). Focused surrounding suites also pass: `timeline::navigation::tests::` 48 passed, `timeline::thread_projection::tests::` 68 passed, `timeline::item_projection::tests::` 54 passed, and `timeline::outbound_send::tests::` 48 passed.

The aggregate revision RED (`threads_list::tests::same_reply_identity_edit_advances_activity_revision_boundary`) remains intentionally deferred and was not changed by this slice.

## Design review record

- Mandatory Round 1, `reviewer-flash`: `Correct-to-merge`. All fourteen contract
  areas passed against merged Task A. Minor plan findings were corrected before
  implementation: the base/gitlink now name the merged commits, `acknowledge()`
  owns the same eligible-set prune instead of relying on actor ordering, and the
  speculative new/restructured activity-resolution file was removed from scope.

### GREEN for final aggregate wiring continuation (2026-08-24)

- `cargo test -p koushi-core --lib`: **GREEN** (exit 0), 1,045 passed / 0 failed /
  8 ignored.
- `cargo test -p koushi-core --test runtime_activity`: **GREEN** (exit 0), 11
  passed / 0 failed.
- Aggregate service focused tests for exact 2→1→0 completion, stale completion
  plus serial exhaustion, and disappeared-root error cleanup: **GREEN** (exit 0).
- `loaded_old_root_raw_event_projects_renderable_snapshot_with_latest_activity_identity`
  now asserts the raw bundled summary is provisional, then applies the
  authoritative Task A aggregate identity/count: **GREEN** (exit 0).
- The generation commit reserves manager capacity before its lease, reconciles
  the pre/post root union, schedules current/disappeared aggregate refreshes,
  and sends one `StartAggregateRefresh`; the existing capacity test proves
  hydration is not duplicated. The production source grep shows the schedule
  caller outside test code.
- Complete current-tree Rust gates are GREEN: Core all-targets 1,210/1,210 (8
  ignored), state all-targets 764/764, SDK lib 154/154, rustfmt, SDK submodule
  guard, agent-doc validation, and root diff check. This task adds no command,
  wire DTO, frontend behavior, or QA scenario, so the reviewed no-server/no-
  browser lane decision remains applicable.

## Exact-review follow-up (2026-08-24)

The mandatory exact-review findings for Task B are resolved in this continuation:

1. `apply_ignored_sender_suppression` recomputes hidden state only for
   `TimelineItemId::Event`; synthetic/date-divider and transaction items retain
   their prior value. The deterministic divider-plus-event ignore→unignore test
   proves the divider remains visible while eligible event content returns.
2. `handle_aggregate_refresh_start` first recognizes the exact aggregate worker
   for the current actor generation and summary revision. The manager/registry
   ordering test covers FetchFinished starting that worker before the stale FIFO
   StartAggregateRefresh and proves a failed hydration/aggregate terminal does
   not trigger another hydration.
3. Hydration-pending is separate from aggregate-pending through
   `ThreadRootProjectionRecord::is_hydration_pending`; `has_pending_attempt`
   uses only hydration state while the DTO-facing `is_pending` still includes an
   aggregate refresh.
4. The unreachable `record.activity != *activity` reconciliation branch was
   removed because `activity_is_newer` already means candidate inequality.
5. A zero-count Hydrated completion for an inactive root now clears/removes the
   record exactly like the Aggregate completion path, with focused coverage.

Remaining minor findings are explicitly adjudicated:

- Retired records are intentionally retained until room teardown so an exhausted
  activity or summary serial cannot be reused; `clear_room` is the lifecycle
  removal boundary.
- Legacy `ActivityRowsObserved([])` remains the explicit existing global-reset
  compatibility path only; canonical room-window replacement is the normal
  production path and no new caller is added.
- The redaction tombstone bound is enforced after every accepted replacement,
  and the exact bound/eviction behavior is covered by the existing deterministic
  redaction-bound test.
- An aggregate success cannot render a failed root without a root item: the
  aggregate-only completion preserves the failed hydration attempt, so failed
  hydration remains a truthful terminal rather than becoming a synthetic Ready
  projection.

This continuation changes only Core tests/implementation and this plan; it does
not touch frontend, vendor, Task C, command/wire DTOs, or QA scenarios. Final
focused and complete Core reruns are GREEN: the divider/event restoration,
FIFO aggregate-worker ordering, hydration-vs-aggregate pending split,
inactive Hydrated count-zero clear, canonical authority/reversible-hidden/
redaction convergence, and every reviewed provenance bound pass; Core lib is
1,045/1,045 (8 ignored) and Core all-targets is 1,210/1,210 (8 ignored). The
final exact-review Minor was also removed: `aggregate_refresh_cause` no longer
exists as redundant write-only record state because the pending
`AggregateRefresh` already carries the admitted cause.
- Final rebase onto merged #582/main `e80e32430d986c4078f1ca482cfcbed10c93e029`
  had a plans-index-only conflict; Core/state production hunks auto-merged and
  focused post-rebase Core lib, Core all-targets, state all-targets, fmt,
  submodule guard, docs, and diff checks are GREEN with the counts above.
