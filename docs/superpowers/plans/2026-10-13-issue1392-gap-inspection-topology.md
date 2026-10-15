# Issue #1392 — Topology-first gap inspection (no full-history decryption)

Date: 2026-10-13. Governs the #1392 fix. The issue text is the agreed contract;
this plan records the implementation decisions and the evidence obligations.
Rev 2 incorporates the independent pre-review (gpt-6.1-sol) findings.

## Problem

`handle_all_rooms_response_committed` fans a commit out to every resident room
timeline. A room without a matching per-room checkpoint schedules a `LiveEdge`
inspection (`GlobalCommitFence::observe`). The inspection calls
`RoomEventCache::inspect_timeline_gaps`, which runs `load_all_chunks`: SQLite
selects `events.content` for **every event chunk**, decrypts and JSON-parses
every returned event, even when the room has no gap chunk. On macOS 0.23.0
this produced recurring ~40% main-process CPU peaks with 1,904 healthy
inspections consuming full-history reads.

The full chain, as referenced by the issue:

- Koushi fan-out: `crates/koushi-core/src/timeline/gap_repair.rs`
  `handle_all_rooms_response_committed` → per-room
  `TimelineActorMessage::GlobalResponseCommitted` → `GlobalCommitFence`.
- SDK call: `crates/koushi-sdk/src/timeline.rs` `inspect_room_timeline_gaps` →
  `RoomPagination::inspect_timeline_gaps`.
- SDK implementation:
  `vendor/matrix-rust-sdk/crates/matrix-sdk/src/event_cache/caches/room/pagination.rs`
  `inspect_timeline_gaps` → `load_all_chunks` → `order_persisted_chunks` →
  `inspect_ordered_chunks`.
- Costly reads:
  `vendor/matrix-rust-sdk/crates/matrix-sdk-sqlite/src/event_cache_store.rs`
  `load_all_chunks` → `rebuild_chunk` → `load_events_content` →
  `decode_event` (serde_json of the decrypted `events.content`).

## Decision

Add a **topology-first store path** and route `inspect_timeline_gaps` through
it. Healthy chains never touch `events.content`; gapped chains decode only
boundary events. Core's global-commit/room-checkpoint semantics are unchanged;
`rooms_from_response=0` still schedules LiveEdge inspections.

Scope of the zero-read guarantee: the **inspection operation itself** on a
room whose event cache is initialized and whose store lease is clean. Room
initialization (`room.event_cache()` loads the last chunk once per room) and
cross-process dirty-lease reload are one-time/rare paths outside the repeated
inspection loop; they are not part of the 1,904-inspection amplification this
issue measures. The benchmark pre-warms rooms exactly like the real resident
room actors, then measures repeated inspections.

Two PRs, split at the real repository boundary:

1. `shinaoka/matrix-rust-sdk-work` fork PR — the store API + pagination change
   with tests.
2. `shinaoka/koushi-matrix` PR — submodule bump, koushi-sdk DTO counters,
   koushi-core diagnostics record, tests, benchmark evidence, docs.

## SDK fork change

### New store API (`matrix-sdk-base`)

New `EventCacheStore` method with a default implementation, so memory and
IndexedDB stay correct without per-store work:

```rust
/// Topology-first snapshot of a persisted linked chunk for gap inspection.
/// Event bodies are read only where a gap's boundary event IDs must be
/// resolved; a healthy chain with no gap chunk decodes zero events.
#[doc(hidden)]
async fn load_chunk_topology(
    &self,
    linked_chunk_id: LinkedChunkId<'_>,
) -> Result<PersistedChunkTopology, Self::Error> {
    // Default: whole-chunk fallback (memory / IndexedDB / DelayingStore).
    let event_bodies_decoded = <count events in loaded chunks>;
    Ok(PersistedChunkTopology { chunks: self.load_all_chunks(linked_chunk_id).await?, event_bodies_decoded })
}
```

New public type in `matrix-sdk-base::event_cache::store`:

```rust
#[doc(hidden)]
pub struct PersistedChunkTopology {
    /// Every persisted chunk, in storage (id) order. Event chunks carry only
    /// their boundary events (first/last ID-bearing event per neighboring
    /// gap, ascending position order; empty `Items(Vec::new())` for a healthy
    /// chunk); gap chunks carry their opaque token. The caller validates and
    /// orders the chain exactly as it does for `load_all_chunks` today.
    pub chunks: Vec<RawChunk<Event, Gap>>,
    /// Number of event bodies decoded while building this snapshot.
    pub event_bodies_decoded: usize,
}
```

The new type implements `Clone` and a **manual** `Debug` that prints only
structural counts (chunk count, event-bodies-decoded), never gap tokens. It
implements **no** equality traits: `RawChunk`, `ChunkContent`, `Gap`, and
`TimelineEvent` do not implement `PartialEq`/`Eq`, so a derived equality impl
would not compile and a hand-written one would expand SDK equality contracts
for a diagnostic snapshot. Tests compare descriptors and counters instead. An
unrestricted `Debug` is forbidden: gap tokens are opaque pagination secrets
and `TimelineEvent`'s debug exposes event IDs.

Because `EraseEventCacheStoreError<T>`
(`matrix-sdk-base/src/event_cache/store/traits.rs`) explicitly forwards every
store method, the new method **must be explicitly forwarded there** to the
inner store — otherwise production would silently keep the slow path through
the default. `DelayingStore` (redecryptor.rs) needs no change: its default
delegates to the forwarded `load_all_chunks` and stays correct.

### SQLite implementation (`matrix-sdk-sqlite`)

One read transaction (same consistent-snapshot mechanism `load_all_chunks`
already uses — `with_transaction` on a read-pool connection, writes on a
separate connection, WAL):

1. Select `id, previous, next, type` from `linked_chunks` (metadata only).
2. Empty chain → empty topology.
3. No `G` row → return chunks with `Items(Vec::new())` for every event chunk
   and `event_bodies_decoded == 0`; **no token/body reads at all**.
4. Gaps exist → decode each gap chunk's `prev_token`; for each gap walk the
   links outward in both directions to the nearest event chunk that yields an
   ID-bearing decoded event:
   - fast path: one row
     (`event_chunks JOIN events`, `ORDER BY position` toward the boundary,
     `LIMIT 1`), decode, keep if `event.event_id().is_some()`;
   - fallback: decode that chunk's rows in the same direction until an
     ID-bearing event is found (needed when the raw JSON lacks an
     `event_id` — see invariant note below); if none, **continue outward** to
     the next non-gap chunk (empty chunks and all-ID-less chunks are both
     skipped, matching `inspect_ordered_chunks`'s `find_map` walk);
   - for a chunk shared between two gaps, keep `[first, last]` in ascending
     stored position (deduplicated when identical) so the unchanged
     `.iter()`/`.iter().rev()` scans pick the same boundaries as today.
   - The walk is bounded by the number of chunk rows and visits are tracked;
     a broken/cyclic link terminates the walk as exhausted **without raising
     an error**, so the authoritative `order_persisted_chunks` validation in
     pagination still produces today's `InvalidLinkedChunkMetadata` error
     class instead of a store `Backend` error.
5. `event_bodies_decoded` counts every decoded body. There is no universal
   `2 × gaps` oracle (gap-only fixtures can have no boundary events); tests
   assert counts per shaped fixture.

Invariant note (no longer claimed airtight): all three store writers
(`PushItems`, `ReplaceItem`, `save_event`) reject events whose
`TimelineEvent::event_id()` cache is `None`, and deserialization re-parses the
ID from the raw JSON (`kind.parse_event_id`), so store-written normal data
round-trips with an ID. However `kind` is publicly mutable while the cached
ID is `#[serde(skip)]`-serialized, so a cached-ID/raw-ID mismatch is possible
in principle. The fallback walk keeps inspection descriptors exactly equal to
today in that case.

### `pagination.rs` change

```rust
pub async fn inspect_timeline_gaps(&self) -> Result<RoomTimelineGapInspection> {
    let started = Instant::now();
    let state = self.0.cache.state.read().await?;
    let topology = state.store
        .load_chunk_topology(LinkedChunkId::Room(&state.state.room_id)).await?;
    let chunks = order_persisted_chunks(topology.chunks)?;   // unchanged validation + link order
    let mut inspection = inspect_ordered_chunks(
        &state.state.room_id, state.gap_snapshot_id(), state.gap_topology_generation(), &chunks);
    inspection.diagnostics = RoomTimelineGapInspectionDiagnostics {
        scanned_chunks: chunks.len(),
        event_bodies_decoded: topology.event_bodies_decoded,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    Ok(inspection)
}
```

`order_persisted_chunks` and `inspect_ordered_chunks` are unchanged **in
behavior**; their two `RoomTimelineGapInspection` construction literals
(pagination.rs ~L346 and ~L398) gain `diagnostics: Default` and stay untouched
for repair re-inspection call sites. `RoomTimelineGapInspection` gains one
field, `diagnostics: RoomTimelineGapInspectionDiagnostics` (default `0`s).
Descriptor identity, handle fields, revision, continuity classification, and
the `Unknown` empty-chain keep their exact current meaning. The repair path
(`repair_timeline_gap_inner`) keeps full-chunk `load_all_chunks` reads and is
unchanged.

## Koushi integration

- `koushi-sdk`: `MatrixTimelineGapInspection` gains
  `MatrixTimelineGapInspectionDiagnostics { scanned_chunks, event_bodies_decoded, duration_ms }`
  mapped straight through `inspect_room_timeline_gaps`; update the one
  construction literal in its tests.
- `koushi-core`: in `handle_timeline_gap_inspection_finished`, emit one record
  `core.timeline_gap_inspection` with stage/trigger/outcome plus the three
  content-free counters (aggregate counts only; no room IDs, tokens, message
  content, or paths — the diagnostics privacy checker stays green). Existing
  `core.timeline_gap_repair` records remain untouched.
- Coalescing (already present, verified against source — the plan only pins
  it): one in-flight inspection per room via `TimelineGapRepairTracker`
  `active_serial` exclusion; pending triggers combined by
  `pending.max(trigger)` (priority, existing tests `gap_repair/tests.rs`
  pin it); `GlobalCommitFence` retains the newest eligible commit and
  `LiveEdge` triggers are only admitted when the room lacks a matching
  checkpoint. Add one combined test: an in-flight `LiveEdge` inspection plus a
  newer committed response must neither duplicate work nor drop the newest
  commit — without changing scheduling behavior.

## Evidence and test obligations (mapped to the issue)

| Acceptance item | Evidence |
| --- | --- |
| Healthy room: zero event-body reads/decodes per inspection | SQLite-level test writes a healthy multi-chunk chain, corrupts `events.content` in place; `load_chunk_topology` succeeds with `event_bodies_decoded == 0` while `load_all_chunks` fails on the same data. Same assertion through the erased `EraseEventCacheStoreError` wrapper. |
| Gapped room: only gap tokens + minimal boundary events; empty chunks, ID-less events, shared boundary chunks handled | SQLite tests: gaps with empty boundary chunks walk outward; a chunk shared between two gaps reconstructs `[first, last]` in position order; boundary events equal the canonical `inspect_ordered_chunks` result; tokens preserved; decode counts asserted per shaped fixture. |
| Synthetic multi-room idle-sync benchmark incl. zero-room committed responses; p50/p95 duration + decode counts | (a) fork-level deterministic perf test: sqlite-backed client, large healthy history, N repeated inspections on pre-warmed rooms, assert zero decodes, report p50/p95 duration; (b) `headless_core_qa` scenario `idle_sync_gap_inspection` emitting inspection counters from committed responses (incl. zero-room responses). Baseline = current pinned fork (`a8c2b7cc7`); after = fix; numbers recorded in the koushi PR body. |
| Regression: omitted-room/live-edge catch-up, limited sync, offline reconnect, gap repair, concurrent writes, restart, malformed topology | Existing Core unit tests (`gap_repair/tests.rs`) plus SDK integration tests (`event_cache/mod.rs`, `store/integration_tests.rs`) must stay green; new tests: healthy/gapped/malformed `load_chunk_topology`, erased-wrapper forwarding, coalescing pin. |
| Fix in pinned fork + integrated; Core coalescing correctness-preserving | Submodule bump commit `chore(sdk): pick up the gap-inspection topology path from fork PR #N`; Core diff keeps only counters + a new record. |

## Gates before opening the PRs

Fork PR: `cargo test -p matrix-sdk-base -p matrix-sdk -p matrix-sdk-sqlite`
plus the SDK integration target `cargo test -p matrix-sdk --features testing
--test integration` (the `testing` feature is required by that target and
absent from default features — it runs the existing gap-inspection/repair
assertions in `tests/integration/event_cache/mod.rs`), fmt/clippy for touched
crates, and a wasm compile check of `matrix-sdk-indexeddb` when the
`wasm32-unknown-unknown` target is installed.
Koushi PR: `cargo fmt`, `cargo clippy`, `cargo test` for koushi-sdk and
koushi-core, `node scripts/check-agents-docs.mjs`, diagnostics privacy checks,
and the headless scenario. GPU is not involved. Independent review: pre (rev 2
of this plan) and post (integrated diff) through the `independent-review`
skill on `openai/gpt-6.1-sol`.

## Verification of claims

- SQLite read snapshot: `with_transaction` on the read-pool connection is the
  mechanism `load_all_chunks` already uses; reads in one transaction cannot
  mix topology with concurrent writer updates.
- `event_id` is a one-way key-hash (`encode_key`), so raw IDs cannot be read
  from `event_chunks`; boundary IDs must come from decoded content — hence the
  minimal boundary-event reads, never full history.
- Metadata work scales with chunks, not events: the implementation uses only
  the `linked_chunks` metadata select plus per-gap `LIMIT 1` boundary queries
  (PK `(linked_chunk_id, chunk_id, position)`). It deliberately does **not**
  reuse `load_all_chunks_metadata`, whose `COUNT ... GROUP BY` over
  `event_chunks` is event-scale work. Only the defensive fallback (corrupt
  or ID-less content, which a cached-ID/raw-ID mismatch can in principle
  produce) scans an event chunk.

## Independent review disposition (rev 1 → rev 2)

- BLOCKER — `EraseEventCacheStoreError<T>` explicit forwarding: added as a
  mandatory item; a test asserts the erased path decodes zero events.
- IMPORTANT — dirty-lease reload / room-init reads: zero-read guarantee scoped
  to the inspection on initialized, clean-cache rooms; benchmark pre-warms.
- IMPORTANT — cached-ID/raw-ID mismatch: fallback retained, invariant claim
  softened, walk continues past all-ID-less chunks.
- IMPORTANT — shared-chunk boundary ordering: `[first, last]` ascending
  position specified and tested.
- IMPORTANT — error class: topology validation stays authoritative in
  pagination; the store walk terminates as exhausted on broken links instead
  of erroring.
- IMPORTANT — gate coverage: `matrix-sdk-sqlite` unit tests named explicitly;
  wasm check conditionally included.
- MINOR — construction literals: three sites listed and updated.
- MINOR — coalescing description fixed to `active_serial` /
  `pending.max(trigger)` / newest-commit replacement.
- Delete list applied: no parallel chunk-description types, no boundary-Event
  map, no reconstruction adapter (partial `Vec<RawChunk>` + one counter
  expresses the same contract); no second topology validator in SQLite; no
  `duration_ms` inside the topology snapshot; no duplicated coalescing tests.