# #1344: SDK owner-side retention counters (measurement milestone)

Status: implemented and under review. Parent issue:
[#1344](https://github.com/shinaoka/koushi-matrix/issues/1344) (Reduce v0.21.0
resident memory). Predecessor: PR #1349 shipped the native process/allocator
audit. Precedent record: the #1150 resident-retention record
([2026-10-09](2026-10-09-issue1150-resident-retention.md)).

## What this PR does

Lands the maintainer's posted Mac-audit priority #1 ("Attribute and bound
search/history retention"): content-free owner-side counters in the vendored
SDK fork, sampled **at diagnostic export time** through Core, recorded as
`core.search_retention` records. **Observation only** — no eviction, no
reload/shrink while sampling, no writer budget change, no reclamation.

- SDK fork (branch `fix/1344-owner-counters`, commit `81ae185f5`):
  `LinkedChunk::memory_stats` (non-initializing), `DecryptedFileAccounting` +
  `TrackedDecryptedBytes` inside `OwnedBytes` (charge follows the allocation,
  not the `FileSlice`), `EncryptedMmapDirectory::open_or_create_with_accounting`,
  `RoomIndex::has_active_reader`, `SearchIndex::diagnostics()`, and
  `EventCache::memory_diagnostics()` (in-memory-only; never the store lock).
  See the dated section in
  `docs/upstream/matrix-rust-sdk-feedback.md`.
- koushi-sdk: `retention.rs` mapping the SDK snapshots into content-free DTOs.
- koushi-core: `SearchActorMessage::RetentionDiagnostics` (bounded SDK read),
  `AccountMessage::SearchRetentionDiagnostics` (forwards the reply channel,
  never awaits the measurement), a `CoreCommandEnvelope` internal variant,
  `RetentionDiagnosticsProbe` (captured under the manager lock, sampled after
  release), and the manager's per-tab enumeration. Every wait is bounded and a
  stalled read settles as unavailable (`None`).
- src-tauri: `get_diagnostic_snapshot` samples per live account and records
  the aggregate + per-account rows with `*_count` / `*_bytes` units and
  `search_scope=decrypted_index_allocations` /
  `event_cache_scope=room_timeline_linked_chunk_only` scope tokens.

## Claims (what the numbers do and do not say)

- The live decrypted figures are **sample-time tracked allocation residency**,
  settled at export time; transient reload/write overlap and the SDK's
  independent sync-time `search_indexing_task` are not separately attributed in
  this increment (crawl in-flight comes from app state as before).
- Atomic loads in the accounting are independent, not a transactionally
  coherent multi-field snapshot.
- Event-cache `events` are resident item lengths, not deduplicated event
  counts; an already-resident empty sentinel chunk counts as a chunk.
- No `RSS - live allocations` subtraction, no cross-platform totals.

## Design review

Two independent pre-implementation reviews (openai/gpt-6.1-sol) rejected the
round-1 design. All BLOCKER/IMPORTANT findings were verified against source and
resolved before implementation:

1. (BLOCKER) `FileSlice::Drop` does not track the allocation: readers convert
   to `OwnedBytes` (Delete → `AliveBitSet`). Resolved by tracking inside
   `OwnedBytes` through a `StableDeref` owner so the charge follows the last
   reference to the plaintext.
2. (BLOCKER) The event-cache `read()` path reloads from the store when dirty
   and reload unloads/shrinks history. Resolved with an in-memory-only
   `locked_state` read that never takes the store lock.
3. (IMPORTANT) The stale registry could not bound staleness (the SDK indexes
   during sync independently of SearchActor). Resolved by export-time sampling
   through Core, with bounded awaits at every hop and channel closure settling
   as unavailable.
4. (IMPORTANT) `rchunks()` also lazily initializes the sentinel chunk.
   Resolved with `LinkedChunk::memory_stats`, which walks only initialized
   links.
5. (IMPORTANT) Overlap attribution, the "non-trivial chunks" predicate, scope
   labels, verification commands, and lifetimed Debug were narrowed/labeled as
   listed above.

A post-implementation review of the finished diff runs before push.

## Verification

- Fork (in `vendor/matrix-rust-sdk`, from the design record's commands):
  `cargo test -p matrix-sdk-search --lib` 46 passed; `cargo test -p
  matrix-sdk --lib --features experimental-search search_index::` 21 passed;
  `event_cache::` 109 passed; `cargo test -p matrix-sdk-common --lib` 164
  passed; `cargo clippy -p matrix-sdk-search -p matrix-sdk-common -p matrix-sdk
  --features experimental-search --all-targets` exit 0 (one pre-existing
  unused-import warning in `sliding_sync`).
- App: `cargo fmt --check`; `cargo clippy --profile ci --workspace
  --all-targets --locked -- -D warnings`; `cargo clippy --release --workspace
  --locked -- -D warnings`; `cargo clippy --profile ci -p koushi-qa --features
  qa-bin --all-targets --locked -- -D warnings`; `cargo test --profile ci
  --workspace --exclude sidebar-composition --exclude key-management` (exit 0,
  172 suites ok); `cargo deny check`; `cargo machete`; the repo checkers
  (incl. `check-rust-test-structure` with the added `src/retention.rs`
  manifest entry, `check-domain-crate-platform-deps`,
  `check-diagnostic-test-isolation`, `check-agents-docs`, submodule guard
  after the gitlink bump). Frontend unchanged.

## Delivery

Fork branch → fork PR → fork `main`; the app PR bumps the
`vendor/matrix-rust-sdk` gitlink, updates `THIRD_PARTY_NOTICES.md` and this
record's status, rebases onto the latest origin/main before the PR, and is
merged after green CI.