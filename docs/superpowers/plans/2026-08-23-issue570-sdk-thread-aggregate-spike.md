# Issue #570 Task A — SDK Thread Relation Aggregate Spike

## Status and dependency

This is the first independently mergeable task under the #570 umbrella. It must merge before Core Activity/thread convergence and before DTO/Browser-Fake migration. It proves and implements the missing authoritative thread relation aggregate in the vendored Matrix SDK; no Koushi product state changes are allowed.

Bundled thread summaries are server data and are stripped from event-cache persistence. This task uses tested relation-index APIs instead of assuming a recomputed summary exists.

## Objective and public API

Expose one vendor-owned resolver from the narrow `matrix_sdk_ui::timeline::thread_list_service` module:

```rust
pub struct ThreadRelationAggregate {
    pub latest_event: Option<ThreadListItemEvent>,
    pub num_replies: u32,
}

pub async fn resolve_thread_relation_aggregate(
    room: &Room,
    root_event_id: &EventId,
) -> Result<ThreadRelationAggregate, ThreadListServiceError>;
```

`latest_event.event_id` is always the original stable reply ID. Its content is the latest valid effective edit. `num_replies` counts valid original thread replies only. Reactions, edits, redactions, duplicate replay, and unsupported relations never increment it.

The resolver is the sole aggregate-semantic owner. `ThreadListService` consumes it instead of maintaining an increment counter; later Koushi Core consumes the same public resolver rather than duplicating relation aggregation.

## Verify-first API spike

Before changing the service, add real event-cache/store tests with raw Matrix events:

1. Root + A/B thread replies: exact Thread relation query returns two stable originals; aggregate count2/latest B.
2. Redact B: observe that applying redaction strips B's thread relation before subscribers see the Set; aggregate filters/removes B and returns count1/latest A.
3. Redact sole A: count0/latest None.
4. Edit A: count remains1, identity A, effective body is the edit's `m.new_content`, never the edit event's `* ` fallback body.
5. Multiple edits + redacted latest edit: choose latest valid direct replacement by corrected timestamp/event ID without changing original reply ordering.
6. Edit-before-reply then original/replay converges to in-order output.
7. Redaction-before-reply currently diverges: the cache stores the redaction event but drops its application when the target is absent. After the pending-redaction amendment below, redaction-before-target then target/replay/restart must equal in-order delivery.
8. Duplicate/replayed batches are idempotent.
9. Cache reload/persistence produces the same aggregate as live memory once the redaction has been applied to its target.

Behavioral service tests RED on the current `saturating_add(1)`. Raw relation-query tests may be early-green discovery evidence.

**Stop condition:** if Thread/Replacement relations plus persisted redaction
facts cannot supply stable live/restart aggregation, stop, record exact evidence,
amend, and re-review. No loaded-window or process-only persistence approximation.

## Pending redaction amendment

Do not add a new persistence schema. The redaction event itself is already a
persisted event-cache fact. Add a room-cache-local pending-redaction index whose
authority is those stored events:

- when a redaction arrives and its target is absent, retain
  `target_event_id -> redaction Event` in the room state instead of discarding the
  semantic application; live insertion and rebuild use the identical latest
  `(corrected timestamp,event_id)` comparator for multiply-redacted targets;
- rebuild the index on room-cache initialization/reset by scanning existing
  persisted/in-memory `m.room.redaction` events via the current
  `get_room_events` store API; dedupe by redaction event ID and choose a
  deterministic latest `(timestamp,event_id)` if multiple target the same event;
- when any target event is inserted, mutate the local batch value to its redacted
  raw form first, then replace chunk/store state, **before** relation extraction/
  thread indexing/subscriber publication. Replacing only chunk/store is
  insufficient because the same-loop local event would still expose m.thread;
- rebuild is total over every stored redaction: absent target → pending; present
  unredacted target → apply immediately; present already-redacted target → skip
  idempotently. Clear/rebuild the derived index with room-cache clear/reload; the
  persisted redaction event remains the source of truth;
- same-batch redaction-before-target, later-batch delivery, duplicate replay, and
  process/store reopen all converge to the same redacted raw target;
- the in-memory map is an index over already-stored redactions, not a second
  semantic ledger. It carries no logging/diagnostics. Its cardinality is
  bounded by distinct persisted redaction target IDs for the room, with one
  newest redaction per target. Same-batch redaction is applied correctly, but
  an entry may remain until target re-delivery, rebuild/reopen, or room reset;
  optimize only with measured need.

Flip the existing spike observation to require the late target is redacted;
the renamed test is `test_redaction_before_target_is_replayed_by_cache`. Before
implementation it is the primary RED. The test also directly proves
redaction-event retention via `find_event` and `get_room_events`. Add same-batch,
later-batch, duplicate replay, and store-reopen convergence tests before
changing the aggregate service. If the store does not retain the event, stop and
amend for persistence rather than process-only memory.

## Aggregate algorithm

For one root:

1. Query `m.thread` relations across loaded and persisted cache.
2. Keep message/encrypted original replies whose exact thread target is the root and whose raw event is not redacted (`unsigned.redacted_because`).
3. Deduplicate originals by event ID.
4. Query `m.replace` relations per original. Relation queries are transitive, so keep only replacements whose exact edit target equals that original; deduplicate by event ID and reject cycles.
5. Validate replacement candidates with the SDK's public edit-validity helper. Choose the latest valid replacement using `serde_helpers::extract_timestamp(raw, MilliSecondsSinceUnixEpoch::now())` then event ID, matching TimelineEvent timestamp capping.
6. Apply its `m.new_content` using the existing timeline message-edit machinery. Preserve original reply ID, sender, profile, timestamp, and ordering. Never display the replacement's fallback `body`.
7. Count valid originals. Choose latest reply by original timestamp then original ID.
8. Build `ThreadListItemEvent` with the existing profile/content projection.

If encrypted edit content is unavailable, retain the original valid projection; a later cache update re-runs the resolver.

## Redaction affected-root rule

Applying redaction replaces the target raw event and strips `content.m.relates_to`; the subscriber can no longer derive its thread root. Therefore any event-cache batch containing an `m.room.redaction` value or redacted Set value triggers reconciliation of **every currently tracked root**. Reuse the same full-root reconciliation for subscriber lag. Thread/edit-only batches derive and deduplicate exact affected roots normally.

This dense path is deliberately O(tracked roots × local relation query) on redaction/lag; correctness outweighs this bounded panel-owned cost. Optimize only after measurement. No reply-ID→root counting ledger is added.

## ThreadListService cutover

For each cache batch:

- collect affected roots, using full tracked-root reconciliation for redaction/lag;
- call the shared resolver once per root after the complete batch is applied;
- re-find vector position after awaits;
- replace latest/count exactly from aggregate;
- zero clears latest; latest redaction promotes prior valid reply;
- delete `saturating_add(1)` and append-event counting.

The listener is serial. If a newer batch queues while resolution awaits, apply the current result then consume and re-resolve the newer batch. A deterministic interleaving test uses the existing delayed mock/response precedent to hold the first resolver await while queuing the newer batch, proving the later batch wins without wall-clock assertions.

## Bundled/local proof state

Initial pagination may use server bundled summary. Both bundled `latest_event` and `num_replies` remain unchanged until local proof is complete. Local relation evidence replaces both only when:

- no bundled proof exists; or
- local valid count is at least bundled count.

Example: bundled4, local evidence1→4 keeps bundled latest/count through1 and replaces both at4. After local evidence has reached/proved4, later redaction may reduce4→3→0. Track proof in a private service-side map keyed by root ID (do not add a private field to the public all-fields-public `ThreadListItem`, which would break external struct literals). Bundled/local counts are not competing owners. Reset clears the map. Tests pin the full progression.

## Downstream revision contract

Resolver is stateless. Later Core owns a per-root monotonic `summary_revision`, incremented when aggregate resolution is scheduled after an accepted actor-generation batch. Completion is fenced by actor generation + root + captured summary revision. This task adds no Core state.

## Minimal files

Vendored SDK only:

- `matrix-sdk-ui/src/timeline/thread_list_service.rs` or one exported sibling module;
- focused unit tests;
- `matrix-sdk/src/event_cache/caches/room/state.rs` for the pending index,
  live/rebuild/apply/clear hooks and local-batch mutation;
- `matrix-sdk/tests/integration/event_cache/threads.rs` for store retention and
  same/later/replay/restart equivalence;
- integration tests for relations/redaction/edit/replay/persistence/interleaving;
- necessary public exports/docs.

Root:

- vendor gitlink;
- Task A plan, umbrella amendment, plans index;
- submodule guard evidence.

No Koushi state/Tauri/TS/Fake changes. Koushi Core changes only the exhaustive
`ThreadListServiceError::EventCache` compatibility mapping to the existing
coarse `OperationFailureKind::Sdk`; it adds no aggregate state or behavior.
Out-of-band `save_events` used for
bundled latest replies bypasses the pending hook; bundled data remains provisional
under the proof-state rule and is not claimed by pending-redaction equivalence.
`maybe_add_live_related_events` also precedes the hook and is documented as an
out-of-scope provisional-cache boundary.

## Privacy and upstream compatibility

Never trace/debug-print aggregate items, raw content, IDs, or bodies. Keep existing public Debug shapes for upstream compatibility and add no logs containing them.

Element comparison:

- Element X Android PR #5595 (merged 2025-10-30) handles received-thread event/notification redaction;
- Element Web PR #29605 hides redacted-event notifications;
- Element Web issues #24392/#26933 record historical stuck thread/unread indicators.

These establish direction but not all ordering edges; cache tests remain authoritative. Record exact inspected revisions/links and divergences in implementation evidence.

## Gates

- focused vendor resolver/ThreadListService tests;
- vendor matrix-sdk-ui/event-cache suites;
- matrix-sdk FFI thread-list wrapper compile/tests (DTO unchanged);
- root submodule guard;
- root workspace/all-targets and existing thread list/replay tests;
- vendor/root rustfmt/docs/diff;
- exact vendor commit + root gitlink review;
- CI7/7.

## Acceptance

| Requirement | Evidence |
| --- | --- |
| concrete APIs proven | live/redaction/edit/persistence tests |
| exact count/latest | A/B→A→None |
| original identity/effective edit | validated m.new_content, ID A |
| redaction discovery | full tracked-root reconciliation test |
| proof state | bundled4/local1→4→3→0 |
| replay/interleaving | duplicate + queued-newer tests |
| one owner | service and later Core use public resolver |
| downstream source | public aggregate + revision contract |

Implementation begins only after `reviewer-flash-opencode-go` records `Correct-to-merge`. Exact vendor/root diffs require post-review.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. Relation APIs exist, but redaction strips root discovery before subscriber delivery; required full tracked-root reconciliation, explicit bundled/local proof state, and original-identity projection from validated `m.new_content`. Also required exact-target dedupe, honest redaction-before-target stop behavior, stable API/error module, FFI/privacy gates, and serial interleaving evidence.
- Round 2: `Correct-to-merge`. The resolver and relation APIs, full-redaction reconciliation, bundled/local proof state, validated original-ID edit projection, stop conditions, FFI/privacy, and serial interleaving design were verified against vendored source.
- Implementation spike stop: real integration evidence proved a missing-target
  redaction is not applied when its target later arrives. Implementation stopped
  before claiming GREEN. This amendment derives a pending-target index from
  persisted redaction events and requires live/replay/restart equivalence before
  service cutover continues.
- Post-stop re-review: `Correct-to-merge`. Store retention, arrival funnels,
  initialization/reset, no-schema persistence, bounds/privacy, and compatibility
  with the partial service implementation were verified. Required local-batch
  mutation, total rebuild, flipped RED/retention test, and one comparator for
  live/rebuild are incorporated above.

## Implementation evidence

### RED before service wiring (2026-08-23)

- `cargo test -p matrix-sdk-ui --lib test_redaction_of_latest_reply_reconciles_exact_aggregate`: **RED** (exit 101). The pre-patch `ThreadListService` retained `num_replies == 2` after the latest reply was redacted; expected exact aggregate count was 1 and the prior reply as latest. This is the reproduced `saturating_add(1)`/append-only defect.
- `cargo test -p matrix-sdk --features testing --test integration test_thread_relation_query_and_redaction_state_for_aggregate_spike`: **GREEN discovery evidence** (exit 0). The live cache returned the two direct `m.thread` originals; redaction delivered a redacted Set whose raw `content.m.relates_to` no longer exposed the root and whose `unsigned.redacted_because` was present; the post-redaction relation query retained only the unredacted reply.
- The first discovery invocation without the required `testing` feature was rejected by Cargo (exit 101); the feature-qualified command above is the authoritative result.

The red test was added and run before changing `ThreadListService`; no Koushi product code was changed.

### RED — pending-redaction convergence matrix (2026-08-23)

- `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing --test integration test_redaction_before_target`: **RED** (exit 101). The retention and same-batch checks compiled and the same-batch check passed, while the later-batch, duplicate-replay, and store-reopen checks failed because an absent-target redaction was not retained or reapplied. This is the primary RED evidence for the approved pending-redaction amendment.
- The retention assertion also proved the redaction event itself was present in the room cache/store before the fix; only its semantic application was missing.

### Superseded STOP observation (2026-08-23)

- The pre-amendment observation proved that a redaction received while its target was absent was not replayed when the target later arrived; that assertion was intentionally flipped and the test renamed to `test_redaction_before_target_is_replayed_by_cache` because the old name/claim no longer described the required contract.
- The post-stop design re-review was `Correct-to-merge`; implementation resumed with the persisted-event pending-redaction index. No loaded-window or process-only approximation was retained.

### GREEN after pending-redaction implementation (2026-08-23)

- `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing --test integration test_redaction_before_target`: **GREEN** (exit 0): 4 passed, 0 failed, 434 filtered. This covers retention, same-batch, later-batch, duplicate replay, and store-reopen convergence.
- `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-ui --lib thread_list_service`: **GREEN** (exit 0): 17 passed, 0 failed, 353 filtered. Resolver edit identity/effective content, redaction promotion/full-root reconciliation, serial batches, and the four post-review matrices passed.
- `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing --test integration test_thread_relation_query_and_redaction_state_for_aggregate_spike`: **GREEN** (exit 0): 1 passed, 0 failed, 437 filtered.
- `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing --test integration test_multiple_valid_edits_update_thread_summary`: **GREEN** (exit 0): 1 passed, 0 failed, 437 filtered. The former invalid-edit claim was stale; Matrix permits multiple valid edits, so the observation was renamed and now asserts latest-edit summary semantics.
- `cd vendor/matrix-rust-sdk && cargo check -p matrix-sdk-ffi`: **GREEN** (exit 0); the FFI thread wrapper compiles with the unchanged DTO surface and explicit event-cache error mapping.
- `cd vendor/matrix-rust-sdk && cargo fmt --all -- --check`: **GREEN** (exit 0). Stable rustfmt emitted only pre-existing nightly-option warnings.
- `git diff --check` and `git -C vendor/matrix-rust-sdk diff --check`: **GREEN** (exit 0).

Vendor/root commits and the root gitlink remain intentionally uncreated in this continuation.

### Post-review Round 1 repair evidence (2026-08-23)

- Round 1, `reviewer-flash-opencode-go`: **Not correct-to-merge**. Production
  semantics were accepted; the blocking evidence gaps were a deterministic
  queued-newer resolver fence, bundled4/local1 held at bundled4 then exact
  displayed `4 → 3 → 0` proof,
  edit-before-original/replay convergence with original identity and effective
  `m.new_content`, live/reopen aggregate equivalence after pending redaction,
  and exact renamed/full gate evidence. All findings are fixed below with
  vendor tests and a `cfg(test)` resolver barrier only; no production API or
  Koushi file changed.
- `test_queued_newer_batch_wins_after_first_resolver_is_released`: holds the
  first resolver await on a watch barrier, proves the newer cache batch is
  queued before release, and asserts final count/latest are from the newer
  batch; no sleep or yield controls correctness.
- `test_bundled_proof_keeps_latest_and_count_until_local_count_is_proven`:
  asserts bundled4/local1 keeps bundled4, local4 proves and replaces it, then
  redaction gives exact 3 and exact 0 with no latest event.
- `test_edit_before_original_replay_matches_in_order_aggregate`: compares
  edit-before-original plus duplicate replay against in-order delivery and
  asserts the original reply ID with effective `m.new_content`, not the edit
  fallback body.
- `test_relation_aggregate_matches_after_persistent_reopen`: captures the live
  aggregate with a pending redaction, reopens the persistent event-cache store,
  delivers the late target, and compares the complete aggregate projection.
- `cd vendor/matrix-rust-sdk && RUST_LOG=off cargo test -p matrix-sdk-ui --lib thread_list_service`:
  **GREEN** (exit 0), 17 passed, 0 failed, 353 filtered; exact log:
  `/tmp/570-thread-list-tests-evidence.log`.
- The renamed test is present and ran as
  `event_cache::threads::test_redaction_before_target_is_replayed_by_cache`.
  `cd vendor/matrix-rust-sdk && RUST_LOG=off cargo test -p matrix-sdk --features testing --test integration test_redaction_before_target`:
  **GREEN** (exit 0), 4 passed, 0 failed, 434 filtered; exact log:
  `/tmp/570-redaction-before-target-evidence.log`.
- Focused relation/edit integration gates are also **GREEN** (exit 0):
  `test_thread_relation_query_and_redaction_state_for_aggregate_spike` 1
  passed/437 filtered (`/tmp/570-relation-query-evidence.log`) and
  `test_multiple_valid_edits_update_thread_summary` 1 passed/437 filtered
  (`/tmp/570-multiple-edits-evidence.log`). The latter initially exposed
  same-timestamp fixture nondeterminism; explicit fixture timestamps (100 and
  200) make the comparator proof deterministic without changing production.
- `cd vendor/matrix-rust-sdk && RUST_LOG=off cargo check -p matrix-sdk-ffi`:
  **GREEN** (exit 0), log `/tmp/570-ffi-evidence.log`; DTO/thread wrapper
  surface is unchanged.
- `cd vendor/matrix-rust-sdk && cargo fmt --all -- --check`: **GREEN** (exit 0),
  log `/tmp/570-fmt-evidence.log`; nightly-option warnings are pre-existing.
  Root/vendor `git diff --check` were both **GREEN** (exit 0), logs
  `/tmp/570-root-diff-evidence.log` and `/tmp/570-vendor-diff-evidence.log`.
  The submodule guard was **GREEN** (exit 0), log
  `/tmp/570-submodule-evidence-final.log`.
- No test or hook prints event content or identifiers. No commit, amend, rebase,
  push, or root gitlink update was performed.
- First CI exposed one downstream exhaustive-match compatibility omission:
  Koushi Core's thread-list error classifier did not yet cover the new vendor
  `EventCache` variant. It now maps that closed cache failure to the existing
  coarse SDK failure, with no new product state or private error propagation.
- The second CI invitation run then exposed a QA output guard interaction: a
  new test-helper signature triggered an `unused_qualifications` warning that
  printed the raw `matrix_sdk_base::` path, correctly rejected by the public QA
  scanner on both servers. The helper now uses the already-imported
  `TimelineEvent` alias; test behavior and production bytes are unchanged.

### User-approved substitute exact review and bounded-index decision (2026-08-24)

- `reviewer-flash-opencode-go` exhausted its monthly quota. The user explicitly
  approved `reviewer-flash` as the mandatory substitute for all remaining
  design/exact gates. The substitute exact review decomposed root identity,
  persisted-redaction, and thread-aggregate scopes, then synthesized one verdict.
- The review confirmed every production semantics scope and resolved the
  suspected `Event`/`TimelineEvent` listener mismatch as a false positive
  (`Event` is the `TimelineEvent` alias). It returned `Not correct-to-merge`
  pending parent checksum confirmation and an explicit decision for the
  persisted pending-redaction index's memory ceiling.
- Parent checksum confirmation on the reviewed pre-resolution artifacts was
  root `582a0d19d3c6a184dd09aad92aa91f5192cef0bf249325f89ab27fa8c20f587e`
  and vendor `ecc37fb66fdf861f98e78786073bb07ac559d8476f401128132647b86335d350`.
  The repaired root artifact checksum is recorded externally in the PR and
  exact-review request (embedding a file's own changing hash is impossible);
  the vendor artifact is unchanged.
- **Explicit bounded-index decision:** retain the reviewed store-derived map
  without an arbitrary fixed cap. Its cardinality is bounded by distinct
  persisted redaction target IDs for this room, with one newest redaction per
  target. Same-batch redaction is correct, but an entry may remain until target
  re-delivery, rebuild/reopen, or room reset. A fixed eviction cap would silently
  lose restart convergence, contradicting the issue contract. No process-only
  ledger or second persistence schema exists. Replace this index only if
  measured large-room memory shows a problem and the replacement preserves all
  persisted redaction facts.
- A current-state `RUST_LOG=off cargo check -p matrix-sdk-ffi` re-capture is
  GREEN at `/tmp/570-ffi-evidence-current.log`; it contains no
  `matrix_sdk_base::` qualification warning. Other workspace warnings are
  pre-existing and outside this Task A diff.
- The review finding that `is_supported_thread_reply` narrowed aggregate
  admission to `m.room.message` and `m.room.encrypted` was valid: it excluded a
  valid `m.sticker` thread reply. The filter was deleted while redaction, exact
  `m.thread` root matching, and event-ID deduplication remain. The focused
  `test_sticker_thread_reply_contributes_to_exact_aggregate` was RED with the
  filter (count 0 instead of 1) and GREEN after removal (count 1 and the sticker
  reply as latest).
