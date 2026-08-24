# Issue #570 Redaction and Edit Convergence

## Goal and authoritative boundaries

Deleted/redacted events and edit relation events must never own Activity,
unread/navigation, thread attention/latest, or conversation-activity semantics.
A non-redacted edited message remains one original event identity and projects
its latest valid effective content.

This is not a React filter. The fix joins five existing Rust authorities:

1. SDK canonical timeline/event-cache aggregation (redaction/edit ordering);
2. Core per-room Activity cache;
3. Core timeline navigation eligibility;
4. Core thread attention and root-summary projection;
5. SDK/Core room-latest and conversation-activity projection.

Redacted timeline rows may remain renderable according to product display
settings. “Visible timeline row” and “eligible activity/unread event” are
separate contracts.

## Execution split and gate status

This is an umbrella, not one implementation approval. The first design review
rejected a one-PR implementation because the assumed client-recomputed thread
summary did not exist and the root+vendor+fake surface was not reviewable as one
diff. Execute three independently reviewed, mergeable tasks in order:

1. **Task A — SDK relation aggregate spike/patch**:
   [2026-08-23-issue570-sdk-thread-aggregate-spike.md](2026-08-23-issue570-sdk-thread-aggregate-spike.md).
   Prove relation APIs, implement one vendor-owned aggregate resolver, cut
   ThreadListService over, commit vendor + root gitlink, review and merge.
2. **Task B — Core convergence**: after Task A merge, write/review a dated design
   for canonical Activity replacement, resolution provenance/invalidation,
   shared eligibility, unread/navigation, thread attention, and
   ThreadRootProjectionService consuming Task A's resolver.
3. **Task C — room-latest DTO + frontend/fake/QA**: after Task B, write/review a
   dated design for redaction-aware room latest/conversation activity,
   state/Tauri/TypeScript/golden mirrors, Browser Fake cutover, and both-server QA.

Each task needs its own pre-design verdict, RED/GREEN evidence, exact full-diff
verdict, CI7/7, and merge. #570 closes only after all three and an acceptance
audit. No task may implement a later task’s semantics early.

## Shared eligibility

Add one Core helper for semantic Activity/unread/thread eligibility:

```rust
fn is_attention_eligible_event(item: &TimelineItem) -> bool {
    matches!(item.id, TimelineItemId::Event { .. })
        && !item.is_redacted
        && !item.is_hidden
        && has_user_visible_content(item)
}
```

Use it from:

- Activity row derivation;
- `is_unread_navigation_item` and own-visible marker advancement;
- thread-reply matching/activity promotion;
- thread-root activity derivation;
- thread-attention valid-reply reconciliation.

Own-user exclusion remains an additional unread/attention guard, not part of
general eligibility. Transaction/synthetic items, standalone relations,
redacted/hidden items, and bodyless nonrenderable items are ineligible.
Undecryptable live replies remain pending until a later renderable Set, preserving
existing late-decryption behavior.

## Canonical Activity reconciliation

The current append-only `ActivityProjection.rows_by_event_id` cannot observe an
empty room, Remove, redacted Set, or reset eviction. Split provenance:

```rust
struct ActivityProjection {
    canonical_rows_by_room: BTreeMap<String, BTreeMap<String, ActivityRow>>,
    resolution_rows_by_event_id: BTreeMap<String, ActivityRow>,
    cleared_event_ids: BTreeSet<String>,
    cleared_placeholder_room_ids: BTreeSet<String>,
}
```

Add an internal AppAction (not serialized across Tauri):

```rust
CanonicalActivityWindowReconciled {
    room_id: String,
    rows: Vec<ActivityRow>,
    invalidated_event_ids: Vec<String>,
}
```

Rules:

- replace only that room’s complete canonical eligible window;
- empty rows delete that room window;
- explicit invalidations remove IDs from canonical, resolver, and cleared maps;
- snapshot merges resolver observations first and canonical rows second by event
  ID, so current canonical effective content wins;
- generation-guarded activity resolution remains point observation and uses the
  separate resolver map;
- ordinary canonical window eviction is not global invalidation: only explicit
  redacted/hidden identities invalidate resolver provenance.

### Reliable publication

Publish the full eligible room window after every accepted generation-fenced
canonical commit, never merely from incoming diff rows and never only when
nonempty:

- initial subscription (including empty);
- normal live/pagination/Set/Remove/Reset and restore-buffered batches;
- relay overflow/recovery replacement;
- ignored-user reprojection;
- idempotent replay/resubscribe;
- send-queue lag replacement;
- actor startup/replacement and empty canonical state.

Reserve reducer-channel capacity before taking the generation lease, then send
the replacement inside the accepted commit publication boundary. Do not use a
lossy `try_send` for the sole authoritative replacement. A retired actor publishes
neither timeline state nor Activity replacement.

For each accepted SDK diff batch, derive `invalidated_event_ids` from stable event
identities whose post-diff canonical item is explicitly redacted or hidden.
Index-only Remove/Truncate/Clear uses full-room replacement for disappearance;
it is not a redaction invalidation unless the removed stable identity is known
from the pre-commit canonical window.

## SDK timeline resolution and edits

`MatrixTimelineSubscription::current_items` already maps SDK-aggregated message
items to their original event IDs and effective bodies; redacted/non-message
items become Remove. Keep that authority.

Activity resolution:

- filters redacted/non-message rows through SDK mapping;
- retains original event identity with latest effective edited body;
- remains request-generation fenced;
- cannot resurrect an explicitly invalidated canonical event in the resolver
  map.

Add SDK tests for edit-before-original, redaction-before-original, pagination,
replay, and redacted-edit fallback. Do not create a second edit ledger in Core.

## Timeline unread/navigation

`derive_timeline_navigation_snapshot`, `newer_unread_event_count`, and
first-unread selection all use shared eligibility. Therefore:

- redacted/hidden/edit-relation events contribute zero;
- `first_unread_event_id` promotes to the next eligible original event;
- `unread_event_count` and newer-event count recompute from the same set;
- effective edits retain original ordering/identity.

Add reset, pagination, out-of-order, own-user, redacted, hidden, and edited
navigation tests.

## Thread attention

At the start of every `ThreadAttentionTracker::reconcile_batch`, build the
current eligible reply-ID set for the exact root. Retain
`attention_event_ids` only in that set before receipt ordering and emit a count
transition if it shrinks. Keep `observed_reply_event_ids` as replay dedupe; do not
intersect it with the loaded window.

Consequences:

- redacted/hidden/removed reply immediately loses attention;
- replay cannot count it again;
- bodyless encrypted reply can become eligible exactly once on later decryption;
- total reply count remains independent from attention.

## Authoritative thread total/latest

Never infer total replies from loaded navigation items or attention IDs. The
initial design incorrectly assumed a client-recomputed root summary existed;
bundled summaries are server data and are stripped at persistence. Task A must
first prove exact thread/replacement/redaction relation queries and expose one
vendor-owned aggregate resolver. Task B consumes that concrete resolver with a
Core-owned monotonic summary revision; this umbrella does not authorize the
speculative implementation below until Task A evidence exists.

### Vendored SDK ThreadListService

Patch the vendored `matrix-sdk-ui` `ThreadListService` and add upstream-style
unit/integration coverage:

- process each event-cache batch by authoritative final root revision per tracked
  root;
- set `num_replies` exactly from the root thread summary;
- resolve the summary latest event through event cache; clear latest on
  absent/zero summary;
- when a root revision is present, do not separately increment from reply/edit/
  redaction append events in that batch;
- fallback reply-only batches dedupe by stable reply ID and ignore replacements,
  redacted content, reactions, and replay;
- effective edit preview uses the edited content but normalizes identity to the
  original reply target;
- redacting latest promotes the prior valid reply; redacting the final reply
  produces count zero and no latest.

The submodule commit and root gitlink are part of this reviewed change. Do not
fork a second Core thread-list counter.

### Core ThreadRootProjectionService

Add an internal authoritative aggregate to each root record:

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

A loaded activity event remains a placement/ownership key, never a total-count
source. On selected activity revision, redaction, reset, and replay:

1. read the root through RoomEventCache (loaded + stored);
2. consume its recomputed thread summary/count;
3. resolve latest through cache and normalize an edit to original identity;
4. project effective content;
5. fence completion by actor generation, root activity revision, and a monotonic
   summary revision;
6. set projected summary exactly (remove current `max(1)` inference);
7. zero clears latest fields and removes/clears stale projection.

Refresh is cache-only after the existing permitted root hydration; no pagination,
network retry, sleep, or repeated fetch loop is introduced.

## Room latest and conversation activity

Add `is_redacted` to:

- SDK `MatrixRoomLatestEventSummary`;
- state `RoomLatestEventSummary` with `#[serde(default)]`;
- Core normalization;
- TypeScript `RoomLatestEventSummary` as a Rust-owned projection field.

For remote/cached Matrix timeline events, detect
`unsigned.redacted_because` from raw event JSON before classifying conversation
activity. Local send values are not redacted.

- retain a typed redacted latest summary when useful, marked redacted;
- never create conversation activity from it;
- reverse cache scan skips redacted candidates and promotes the previous valid
  message/thread reply;
- Activity latest fallback returns None for redacted;
- placeholder timestamp uses non-redacted latest, otherwise conversation
  activity;
- fully-read marker fallback never selects redacted latest;
- m.replace/m.annotation/redaction relations never advance conversation ordering.

Frontend may refuse a redacted latest display anchor using the Rust field, but
must not repair Activity/thread/unread state.

## Browser Fake boundary

Update Rust-shaped fixture types/defaults for `is_redacted`. Browser Fake may
record typed commands and install supplied snapshots/events; it must not locally
recompute redaction, Activity, unread, thread count/latest, or conversation
activity. Tests seed authoritative Rust-shaped before/after projections and prove
no local optimistic repair.

## Verify first RED matrix

Before production wiring, add deterministic tests (no timing thresholds):

### Activity cache/runtime

- room A/B canonical window → B redacted/omitted promotes A in Recent/Unread;
- empty replacement clears stale room rows;
- resolver historical row coexists with unrelated canonical live row;
- explicit canonical invalidation removes duplicate resolver row;
- same original ID Set old→edited produces one row with edited preview;
- restore/replay repairs a deliberately stale Activity cache;
- mute/excluded room behavior remains.

### SDK timeline/out-of-order

- edit before target then target converges to one effective original row;
- redaction before target then target/replay remains absent;
- latest valid edit redacted promotes prior valid effective content;
- live/pagination/current_items produce equivalent output.

### Timeline navigation

- marker, redacted A, valid B, own C → first B and unread/newer counts 1;
- hidden/standalone edit relation contributes zero;
- edited original retains identity/order;
- reset/pagination/replay yield same snapshot.

### Thread attention/summary

- attention 1→0 on reply redaction and replay stays 0;
- late decryption 0→1→1 remains;
- latest B/count2 redacted → A/count1;
- final A redacted → None/count0;
- edit of A keeps identity/count1 with latest effective preview;
- repeated replay does not increment;
- old-root loaded window one + cached summary four projects count4;
- stale aggregate completion cannot overwrite newer summary revision.

### Room latest/conversation

- raw redacted message produces `is_redacted=true`, no conversation activity;
- cache valid A + newer redacted B promotes A activity;
- redaction/edit/reaction events do not reorder room;
- runtime redacted latest cannot create Recent/Unread fallback/placeholder.

### Frontend/fake

- TS redacted room latest returns no display anchor;
- browser fake installs authoritative after-state only and performs no local
  activity/thread repair.

Capture behavioral RED commands for each production defect before edits. Some
new typed-field fixture tests may compile only after adding the field; add the
field/default first, then record behavioral RED before filtering/wiring.

## Expected files by task

Task A is limited to the vendored SDK files/gitlink and its plans as specified in
its own reviewed design.

Task B may include:

- `crates/koushi-core/src/runtime/activity.rs`, `runtime.rs`,
  `activity_resolution.rs`;
- `crates/koushi-core/src/timeline/{actor,relay,navigation,item_projection,
  thread_projection,outbound_send,display_projection}.rs` and focused tests;
- `crates/koushi-core/src/threads_list.rs` if aggregate transport requires it;
- `crates/koushi-sdk/src/{timeline,room_projection}.rs` and tests;
- `crates/koushi-state/src/{action.rs,state/room.rs}` plus constructors/tests;
- vendored matrix SDK ThreadListService tests/source and root gitlink;
- architecture/state-machine/state-ownership docs and the Task B plan/index.

Task C includes all contract mirrors and fake behavior explicitly:

- `crates/koushi-sdk/src/room_projection.rs`,
  `crates/koushi-core/src/room/normalization.rs`,
  `crates/koushi-state/src/state/room.rs`;
- `apps/desktop/src-tauri/src/dto.rs`, serialization contract tests, and
  regenerated `frontend_app_state.json` golden with a real
  `is_redacted: true` value;
- `apps/desktop/src/domain/types.ts`, `TimelineView` projection tests;
- `apps/desktop/src/backend/browserFakeApi.ts`, `browserFakeApi.test.ts`, and all
  affected browser-headless specs/authoritative seeding fixtures;
- QA scenario/registry/docs with a fixed private-data-free token such as
  `redact_edit_convergence=ok`, on tuwunel and synapse;
- architecture/state-machine/state-ownership docs and Task C plan/index.

Do not touch React Activity sorting/filtering, timeline display-redaction policy,
search semantics, Tauri command registration, or persistence beyond serde default
compatibility.

## Full gates

Run focused SDK/Core/state/frontend tests, submodule guard, vendor targeted tests,
workspace/all-targets, Tauri, wasm, QA binary, full Vitest/Playwright,
headless local scenario with event-driven A/B edit/redact convergence on both
servers, docs/generated/boundary/security/dependency/rustfmt/diff checks, exact
full-diff review including submodule commit, CI 7/7, merge, and issue closure.

## Acceptance mapping

| Contract | Evidence |
| --- | --- |
| Recent/Unread promotion/removal | canonical room replacement + invalidation tests |
| counts/first unread | shared eligibility navigation matrix |
| edits under original identity | SDK aggregation + Activity replacement tests |
| out-of-order/live/pagination/replay | SDK/Core equivalence matrix |
| thread attention/latest/count | valid-set retention + authoritative root summary tests |
| conversation ordering | raw redaction-aware cache/latest tests |
| Browser Fake parity | authoritative fixture install/no-local-repair tests |
| no duplicate semantics/privacy | source review, redacted Debug, full gates |

Only the currently active task may begin after its own
`reviewer-flash-opencode-go` `Correct-to-merge` verdict. Exact root/submodule
diffs and RED/GREEN evidence require post-implementation review before each PR.

## Umbrella review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. The assumed
  recomputed SDK root summary/revision was absent; the aggregate APIs require a
  proof spike. Required three separately reviewed PRs, explicit Browser Fake
  migration files, Tauri DTO/golden mirrors, upstream Element comparison, and a
  registered QA token/lane contract.

## Task A execution evidence

- Task A added a live relation-query/redaction discovery test and a behavioral
  ThreadListService regression test before service wiring. The latter was RED
  on the pre-patch append-only counter.
- The pre-amendment redaction-before-target observation showed that the cache
  dropped a pending redaction; the target delivered later remained unredacted.
  The approved post-stop design changed that contract and renamed the flipped
  test to `test_redaction_before_target_is_replayed_by_cache` so its name and
  assertion now match the required convergence behavior.
- After the post-stop design re-review (`Correct-to-merge`),
  `cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk --features testing --test integration test_redaction_before_target`
  was run before implementation: **RED** (exit 101), with retention and
  same-batch passing while later-batch, duplicate-replay, and store-reopen
  failed because absent-target redactions were not reapplied.
- The resumed implementation now has focused GREEN evidence: the same command
  is **GREEN** (exit 0), 4 passed/0 failed/434 filtered; `cargo test -p
  matrix-sdk-ui --lib thread_list_service` is **GREEN**, 17 passed/0
  failed/353 filtered; the relation-query discovery filter is **GREEN**, 1
  passed/0 failed/437 filtered; the renamed multiple-valid-edits event-cache
  filter is **GREEN**, 1 passed/0 failed/437 filtered; and
  `cargo check -p matrix-sdk-ffi`, vendor rustfmt check, and root/vendor
  `git diff --check` all exit 0. The exact commands and no-commit boundary are
  recorded in the Task A plan.
