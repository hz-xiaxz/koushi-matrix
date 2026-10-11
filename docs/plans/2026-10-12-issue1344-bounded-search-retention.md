# #1344: bounded search/history residency and durable crawl progress

Status: **revision 3 — Stage A and Stage C implemented; Stage B is not.** Parent:
[#1344](https://github.com/shinaoka/koushi-matrix/issues/1344); authority is the
maintainer's 2026-10-12 direction comment on that issue. Precedents:
[#1150 index-first search](2026-10-06-issue1150-search-index-first.md),
[#1150 resident retention](2026-10-09-issue1150-resident-retention.md),
[#1344 owner counters](2026-10-11-issue1344-owner-counters.md).

Two independent `gpt-6.1-sol` pre-implementation reviews ran. Revision 1's 12
findings are dispositioned below and produced revision 2's staging. Revision 2
was then reviewed; its findings are dispositioned in the revision-2 section,
which is also the contract Stage B must satisfy. Every finding was re-verified
against the source before acting.

Implemented so far:

- **Stage C** — operation-scoped search readers, in fork PR
  [#22](https://github.com/shinaoka/matrix-rust-sdk-work/pull/22) (branch
  `fix/1344-release-search-readers`), pinned by this branch's gitlink.
- **Stage A** — `koushi_sdk::persisted_room_event_page` and the crawler source
  switch, with the crawler canon amended.
- **Stage B** — not implemented. Its contract is the revision-2 section.

## Review disposition (revision 1 → 2)

| # | Finding | Verified | Disposition |
| --- | --- | --- | --- |
| 1 | `SearchIndexMessage` feeds only the in-memory Files store, `SearchMaintenanceQueue` has no production consumer, and the SDK subscriber drops failures — so "completed" plus a latest-event check cannot cover older-event changes (late UTD resolution rewrites a stored event in place and does not notify observers) | yes: `search.rs:1237-1273`, `koushi-search/src/maintenance.rs` (tests only), `redecryptor.rs:560-640` | **accepted** — Stage B adds per-room change detection and durable pending work; completion no longer means "nothing left to index" |
| 2 | Skipping the crawl drops historical Files rows (`handle_attachments` reads only `SearchDocumentStore`, which starts empty) | yes: `search.rs:1259` | **accepted** — Stage B rebuilds Files rows per query from the Stage A store cursor (`#1150` M3 did exactly this; `732ef484` removed it with the commitments) |
| 3 | Completion-only persistence does not resume, and replay after an interruption is unbounded | yes | **accepted** — the Stage A cursor is persisted per room and per batch |
| 4 | `SEARCH_INDEX_CONTRACT_VERSION` does not identify the index *instance*; a recreated directory would keep a matching record; awaited writes are not a session fence | yes: `store.rs:1093-1106` | **accepted** — Stage B binds the record to an index-instance marker and settles late saves; no `SearchProgressWriter` wrapper |
| 5 | `completed = … || (messages.is_some() && chunk_len == 0)` treats a nonterminal empty page as historical completeness | yes: `search_crawler.rs:307-308` | **accepted** — Stage B records only `reached_start`/catch-up-boundary completion, with the empty-page case retried under a bound |
| 6 | `subscribers == 0 && pagination Idle` does not cover targeted gap repair (which holds `pagination_operation_lock` and does not set the shared status) and auto-shrink deliberately discards its diffs | yes: `room/pagination.rs:658,952,1097`, `event_cache/tasks.rs:184-199` | **accepted** — Stage D uses the operation lock and states its publication rule; Stage D is not in the first increment |
| 7 | `shrink_to_last_reloaded_chunk` can `Update::Clear` persistent room storage on a load error | yes: `room/state.rs:566-600` | **accepted** — Stage D must not call it as a release primitive without a non-destructive failure contract |
| 8 | Per-client length budgets and deferred global history bounds do not meet the direction's global byte requirements | yes | **open scope decision #1** — see below; not silently narrowed |
| 9 | Creation-time accounting deltas are not sound LRU entry costs; reload grows an entry; global deltas are not attribution | yes | **accepted** — revision 2 drops the idle LRU entirely (Stage C has no cache in the first cut) |
| 10 | Removing the `contains` fail-open needs failed-batch rollback and reload-failure handling | yes: `index/mod.rs:224-227,513-528`; `uncommitted_adds` cleared only in `commit` | **accepted** — Stage C resets batch bookkeeping on the failed path and handles a failed reload |
| 11 | Canon amendments missing; the crawler canon already describes mechanics the current code no longer uses | yes | **accepted** — explicit list below |
| 12 | Evidence section combined normal-run counters with instrumented allocator buckets; shrink retains events; `SearchIndex::new` has SDK test callers | yes | **accepted** — corrected below |

## What the measurements do and do not say

Kept separate, never added or subtracted across the two runs:

- **Normal packaged run (owner counters):** 153 room indexes with 153 cached
  readers; 139.17 MiB live decrypted length / 191.29 MiB capacity; 70,120
  resident history items in 1,015 chunks across 160 rooms; 151 zero-subscriber
  rooms, 146 with resident events.
- **Separate instrumented run (`MallocStackLogging=lite`):** 602.21 MiB live
  malloc, of which 227.80 MiB search-origin and 294.05 MiB history-origin
  (222.52 MiB whole-file decryption buffers, 274.52 MiB decoded history events,
  16.01 MiB chunk containers).

These are different measurement types from different runs. Both point at the
same two owners; neither is a process footprint, a steady idle plateau, or a
number to combine.

## Stage A — read persisted history without hydrating it (SDK fork + app)

**Why first.** The 274 MiB history bucket is produced by the crawler, not by the
crawl bookkeeping: `run_backwards_once` inserts every scanned page into the
shared room-timeline linked chunk, so a full crawl leaves ~160 rooms hydrated
regardless of who triggered it. Fixing the source of the read removes that
bucket while keeping today's indexing behavior *identical*.

- **SDK fork:** reinstate the bounded persisted-event page reader that #1150's
  M3 had and `732ef484` removed (`koushi_sdk::persisted_room_event_page`,
  commit `5e8f6ca6`): read one bounded page of a room's persisted chunks,
  newest chunk first, release the store lock between pages, return an opaque
  cursor. It does not insert into the resident linked chunk and does not
  subscribe.
- **App:** `run_history_crawl_page` reads its page from that cursor instead of
  `event_cache.pagination().run_backwards_once`. Everything else about the page
  (extraction, redaction handling, `contains` dedupe, the Tantivy commit
  acknowledgement in `index_page_events`) is unchanged, so the indexed document
  set stays the same.
- **Consequence accepted by the direction:** the crawl no longer warms the
  timeline cache for a room the user has not opened; a later open re-reads from
  the encrypted store, which is the intended "history stays on disk" behavior.
- **This stage changes no search result, no Files row and no crash recovery
  property.** It is the one piece that is safe to land independently, and it is
  what makes the later stages cheap to reason about.

## Stage B — durable progress, Files reconstruction, change detection

**Why.** With Stage A the startup crawl costs no memory but still re-reads all
history every launch. Stage B stops re-reading unchanged rooms and rebuilds what
the skipped crawl used to supply.

- **Record** (`crates/koushi-core/src/store/search_progress.rs`, account-file
  pattern, `derive_search_progress_key()`, `<account_root>/search/progress.v1.enc`,
  `atomic_replace_file`, identifiers and cursors only):
  file-level `identity { index_contract_version, index_instance: Option<String>,
  include_media_captions, include_filenames }` and per-room
  `{ backfill_state: Incomplete | Complete, newest_scanned: Cursor,
  oldest_committed: Cursor, latest_event_id: Option<String>, store_revision: u64,
  processed: u64, indexed: u64 }`.
- **Index instance fence (finding 4).** `identity.index_instance` is a marker
  file written beside the index directory the first time a record is written and
  re-read on load; if the marker or the index directory is missing while a record
  exists, the record is dropped and the rooms are crawled. This closes the
  "recreated `search-index.v2` with a matching contract version" hole without
  asking the SDK for a generation type.
- **Honest completion (finding 5).** Only `reached_start` (historical backfill)
  and reaching a catch-up boundary (incremental) mark a room complete. A page
  with zero events and `reached_start == false` is a nonterminal outcome: the
  room keeps `Incomplete` and is retried under a bounded attempt count, so it can
  never be durably marked complete from an empty page.
- **Cursor-based resume (finding 3).** `oldest_committed` advances only after
  the page's Tantivy commit has been acknowledged, in the same ordering as
  `index_page_events`; a crash between commit and the record write replays at
  most that page. Two accounts never share a record or a cursor.
- **Files reconstruction (finding 2).** A Files query rebuilds each queried
  room's rows from the Stage A cursor in bounded pages, releasing the store lock
  between pages, and replaces them per room (the `forget_room` semantics #1150
  M3 used) so a redacted or removed row cannot survive. A failed read leaves
  that room's rows untouched and the next query retries.
- **Older-event changes (finding 1).** Completion is bound to a per-room
  `store_revision`. **Decision:** one cheap per-room persisted-revision counter
  in the SDK fork, bumped by `save_events` and by chunk updates, so an in-place
  late-UTD resolution is visible without a fork notification protocol. On
  restart a completed room is revalidated only when that revision moved, and it
  is re-read from the cursor in bounded batches, never hydrated. This replaces
  "completed means nothing left to index" with "completed means backfill was
  committed; a store change since then reopens it". A throttled revalidation
  sweep was the alternative and is rejected: it re-reads every completed room at
  a fixed cost regardless of whether anything changed.
- **Invalidation.** `InvalidateCrawlerCache`, a content-policy change,
  `RebuildIndex`, an index-contract bump, a room leaving the joined set, and a
  failed write all have defined behavior: the affected scope is dropped in memory
  and durably, and the room returns to `Incomplete`.
- **Bootstrap.** No record → one full crawl (today's behavior), then a record.
  Never a repeated bootstrap.
- **Session fence (finding 4).** The record write is ordered inside the actor and
  a late `spawn_blocking` save cannot outlive the actor: the writer is owned by
  the actor task and the actor's shutdown path awaits in-flight saves before
  exiting, mirroring the read-outbox fence.

## Stage C — operation-scoped search readers (SDK fork)

**Decision (review-directed alternative, no idle LRU in the first cut).** A
`RoomIndex` no longer keeps a permanent `IndexReader`. One reader is acquired per
synchronous operation and released when that operation ends.

- `RoomIndex` loses `reader: OnceCell<IndexReader>`. `bulk_execute` resolves one
  reader for the whole batch and passes it to `contains`,
  `events_to_be_removed` and the commit; `search` resolves one per call. A
  per-event reopen is therefore not introduced, which is what made the rejected
  "no cache at all" variant expensive.
- `ReloadPolicy::Manual` is kept; a freshly created reader reads current metadata
  and needs no reload. `commit_and_reload` becomes a commit plus (when a reader
  is held) a reload of that reader.
- **Failed-batch rollback (finding 10).** `uncommitted_adds` / `uncommitted_removes`
  are cleared on every exit from a batch, not only on commit, so a batch that
  fails before committing cannot leave `contains` reporting an uncommitted event
  as present. A failed reload propagates rather than leaving a stale reader.
- `contains` stops failing open. Reader acquisition and query errors propagate,
  so a page fails and is retried instead of being silently treated as
  "already indexed". This is what makes the Stage B commit-then-progress ordering
  meaningful.
- No reader-count or byte budget is introduced, because an unenforced budget is
  worse than none; if measurement shows reopening latency is a problem, an idle
  cache is a separate, measured follow-up (the revision-1 LRU design is dropped,
  finding 9).

## Stage D — enforceable idle release (deferred, not in the first increment)

The direction's global idle-history allowance needs an owner that this
increment does not create, and the review showed the obvious primitive is not
safe as written (findings 6, 7). Deferred deliberately, with the trigger
recorded: land if the post-Stage-A/B/C measurement still shows resident history
for zero-subscriber rooms. Requirements if it is picked up: use
`pagination_operation_lock` rather than an observable status, define the diff
publication rule, and define non-destructive behavior when the last-chunk load
fails instead of clearing persistent storage.

## Revision-2 review disposition (Stage B contract)

The reviewer rejected revision 2 as written and named these. All were verified.

| # | Finding | Disposition |
| --- | --- | --- |
| 1 | The durable record would lose gap evidence and traversal mode, so a resume could reach the oldest stored chunk with `crossed_store_gap = false` and declare success over an unfilled gap; a post-switch resume would restart the store walk at the tail | **accepted** — the record persists the traversal mode and the cumulative gap evidence, not just a cursor |
| 2 | Catch-up accepted a boundary found across a gap, skipping events inside it | **fixed in Stage A** (see below): the page reports where continuity breaks, and a boundary behind a gap hands the rest to back-pagination |
| 3 | `store_revision` had no coverage invariant: a page could read a UTD event, skip it, and the revision sampled at completion would claim coverage of a resolution it never indexed | **accepted** — the record stores a *coverage* revision captured before the traversal, and a change to already-scanned chunks or during revalidation reopens the room; an active-session trigger is required, not only a restart comparison |
| 4 | A sibling marker does not identify a recreated index directory, and says nothing about one missing room index | **accepted** — identity is coupled to actual index creation and per-room index presence; a record whose rooms or directory do not match is invalid, not trusted |
| 5 | Files reconstruction from persisted room chunks can delete a valid row admitted from a focused/thread projection, which is not persisted in room chunks; a concurrent tail re-insert can also be missed | **accepted** — reconstruction preserves verified non-store rows unless there is positive invalidation evidence, and publishes a replacement only with adequate scan coverage |
| 6 | Awaiting a save in the Shutdown arm is not the session fence: `SearchActorHandle` aborts after its timeout and drops the actor, and an in-flight `spawn_blocking` save survives that | **accepted** — reuse the read-outbox fencing (session/save generations, per-path write serialization, timeout invalidation), not an await-only argument |
| 7 | Switching to the network does not position SDK pagination at the gap the walk found: it starts from the resident position, may re-read and hydrate already-scanned disk chunks, and the preserved empty-page rule can stop before older gaps are repaired | **accepted, recorded as a Stage A limit** — the switch repairs gaps but is not gap-targeted. Stage B's honest completion rule stops the empty-page shortcut from marking completion; gap-targeted continuation is deferred with the store cursor work |
| 8 | The increment has no byte bound at all, not even per account, and the first-page resident read clones the whole resident window | **open maintainer decision** (below). Stage C gives a zero idle-reader allowance by construction; no history byte bound is claimed |
| 9 | A batch reloaded the reader it was about to drop | **fixed** — `commit_batch` commits without reloading |
| 10 | The non-hydration test exercised only resident history, so a reader that also hydrated would pass | **fixed** — `crawler_page_indexes_history_that_is_not_resident` backfills 300 events across several linked chunks, releases the resident window through the public subscriber path, and then requires the deciding crawl to reach every stored document with no network and no growth of the resident counters |
| 11 | The crawler canon still described `CrawlFinished`, per-room cancel flags and drain-while-await | **partly accepted** — the history-source, catch-up, preemption and shutdown-history text was corrected in place; the older `CrawlFinished`/cancel-flag text is a pre-existing inaccuracy outside this change and is left for its own correction |

Delete list: `newest_scanned` is dropped (a chunk cursor is not a newest-event
boundary); `index_instance` is non-optional in a valid record; the disposable
reload is gone; the cloned rollback snapshots stay (they are the correct rollback
and the batch is bounded); the standalone shutdown-drain test and the unused
session construction in the non-hydration test are pre-existing/in-test scope and
are handled with the Stage A acceptance test.

### Stage A as implemented

`koushi_sdk::persisted_room_event_page` walks a room's persisted chunks newest
chunk first under the store lock, releases the lock between pages, returns a
resumable numeric chunk cursor, reverses each chunk's items so a page is newest
first, and reports `gap_at`: the position at which continuity with the previous
page breaks. A crawl starts in the store, and switches its checkpoint to SDK
back-pagination when the walk ends on a gap, or when the catch-up boundary sits
behind one. Catch-ups ignore the resident read (it flattens the linked chunk and
cannot see a gap) and decide from pages only; full crawls still merge it for
#996. Diagnostics name the source.

Known limits, recorded rather than claimed away: the network switch is not
gap-targeted (finding 7), the first-page resident read is unbounded in size
(finding 8), and no history byte bound exists.

## Rejected variants

- Idle reader LRU with creation-time accounting costs (finding 9).
- Durable completion inferred from `chunk_len == 0` (finding 5).
- `SearchIndex::new` constructor restructuring — not needed by the reader
  lifetime change alone (delete-list item 5).
- Per-page shrink, Room timeline-actor retirement (#1230 rejected), plaintext
  spill files, SQLite FTS migration, a new on-disk index format.

## Canon to amend with the implementation

- `docs/architecture/state-machine.md`: the crawler section's "Completed rooms are
  NOT persisted across restarts" sentence, the durable-completion projection,
  historical vs incremental completion, pending/invalidation/membership churn,
  and the Restore-gap/Shutdown text, which still describes per-room cancel flags,
  `CrawlFinished` messages and drain-while-await mechanics the current page task
  no longer uses.
- `docs/architecture/overview.md`: progress-persistence ownership, the
  persisted-cursor read path, shutdown write settlement, and the preserved Files
  behavior.
- `docs/agents/state-ownership.md`: the bounded-batch owner and the
  canonical-navigation vs search-traversal distinction.
- `docs/agents/plans.md`: index entry for this record.

## Delivery split

1. **fork PR (`shinaoka/matrix-rust-sdk-work`)**: Stage A cursor read, then Stage
   C reader lifetime — two commits, one PR, with SDK tests.
2. **app PR (`shinaoka/koushi-matrix`)**: Stage A crawler switch, Stage B, the
   gitlink bump, canon amendments, and the app-side regressions.

## Acceptance and verification

Headless; no sleeps as assertions; each item names its deciding check.

1. **No hydration from indexing.** Scanning a room's full persisted history
   through the Stage A cursor leaves the event-cache resident counters
   (`rooms`/`chunks`/`events`) at their pre-scan values while search reaches the
   same documents. Fails today: `run_backwards_once` inserts every chunk.
2. **No re-crawl after restart.** A completed, unchanged fixture runs a second
   session with zero page work and search still returns the indexed history;
   Files rows for historical attachments are still present via the
   reconstruction path.
3. **Interrupted backfill resumes from its cursor.** Fault injection between the
   Tantivy commit and the record write loses no document and replays at most one
   page; a mid-backfill restart resumes at `oldest_committed`, not from the tail.
4. **Completion cannot be faked.** A page with no events and
   `reached_start == false` never marks a room complete.
5. **Change detection.** A late UTD resolution / edit / redaction of a
   non-latest event in a completed room reopens that room's indexing; an
   unrelated room is not re-read.
6. **Invalidation matrix.** Content-policy change, index-contract bump, lost
   index directory/marker, `RebuildIndex`, a departed room, and a failed save
   each re-crawl the right scope and nothing else; a pure speed change does not.
7. **Reader residency.** With the owner counters, repeated cross-room queries and
   repeated index cycles leave `rooms_with_active_reader` at zero between
   operations and `live_decrypted_len_bytes` at zero after the last operation,
   instead of 153 readers / 222.52 MiB. Counter assertion, not RSS.
8. **Correctness unchanged.** Japanese substring/normalization, edits,
   redactions, dedupe, post-commit search visibility, account isolation,
   `search_memory`'s body-retention claim, and the crawler redaction scenarios on
   the headless lane.
9. **Latency reported, not promised.** Warm/cold first-query and per-page crawl
   timings reported alongside memory; a regression is a finding.

Not claimed: process RSS, desktop/iOS equivalence, a global idle-history byte
bound, a bound on one very large room, or allocator trim as a substitute for
dropping live objects.

## Open scope decisions (need the maintainer)

1. **Global byte allowances (finding 8).** The direction asks for idle history
   and readers bounded by allocated bytes *across accounts*, plus separately
   bounded writer scratch/overlap. This increment bounds them per account only
   (Stage C removes the reader charge entirely; Stage B bounds what is re-read).
   Either the maintainer accepts these as per-account guarantees for this
   increment, or Stage D must also design a cross-account allowance owner — which
   changes the deliverable's size and needs its own review.
