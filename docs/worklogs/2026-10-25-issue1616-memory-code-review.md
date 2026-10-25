# Code review: long-session memory ownership (#1616)

Baseline: `origin/main` at 552dc061, reviewed 2026-10-25. Static review only:
no new live-memory measurement was taken here, so nothing below is a retained-byte
attribution. Measurements quoted are the 28.5-hour snapshot recorded in
[#1616](https://github.com/shinaoka/koushi-matrix/issues/1616).

Purpose: list the state owners that could hold memory for a long session, screen
out places whose lifecycle is closed, and record what a code read cannot decide.

## Method

1. Enumerated the collection-valued fields of long-lived owners:
   `AccountActor`, `TimelineActor`, `TimelineManagerActor`, the runtime task,
   `AccountRuntimeManager`, `AppState`, `ThreadsListActor`.
2. For each field, compared every insertion site with the matching
   `remove`/`retain`/`drain`/`clear`/`Drop` path.
3. Checked channel capacities and lag handling, task and event-handler
   lifecycles, and cache caps.
4. Checked the SDK configuration that decides how much per-room state the app
   asks the SDK to keep.

## Measured context (snapshot; live heap and fragmentation are separate)

- Native footprint 366.2 MiB = live malloc 234.7 MiB + allocator fragmentation
  105.0 MiB + ~26.5 MiB other.
- WebContent footprint 705 MiB, of which 554.7 MiB graphics.
- 230 `WebKit LayerBacking` surfaces, including 84 `1024x1024` ones.
- 26 timeline actors, 2,010 resident event items; event cache 4,735 events over
  144 rooms with zero subscribers.

## Ranked leads

### 1. Per-room SDK event storage retained by timeline actors

`TimelineManagerActor::timelines` (`crates/koushi-core/src/timeline/manager.rs:617`)
keeps one actor per opened room across ordinary navigation. Each actor holds the
SDK `Timeline` (the app mirror is `crates/koushi-core/src/timeline/actor.rs:859`)
together with `navigation_items` (body, HTML, plain text), one event-id `String`
per item in `ranks`, plus `media_sources`, `media_gallery_items` and
`display_projection`.

Lifetime: ordinary navigation never releases the actor (documented deviation,
`docs/policies/engineering-rules.md:294`). A confirmed leave does:
`quiesce_left_room_timelines` unsubscribes every room key for that room
(`crates/koushi-core/src/timeline/residency.rs:804`,
`crates/koushi-core/src/timeline/manager.rs:1875`). The deviation is retention
across navigation, not every lifecycle.

Multiplier: a loaded event is held at least twice (SDK `EventTimelineItem`, app
projection); the same event may also be in the client event cache. Per-item cost
is well above the text length (sender profile, encryption info, push actions,
raw event), but no per-item byte figure has been measured.

### 2. Event-cache state is materialized per room and never evicted

`EventCacheInner` holds `by_room: HashMap<OwnedRoomId, Caches>`
(`vendor/matrix-rust-sdk/crates/matrix-sdk/src/event_cache/mod.rs:658`), and
`all_caches_for_room` inserts an entry for every room it is asked about (`:873`)
with no removal path. Entries are materialized by:

- `handle_room_updates`, for every left or joined room in each sync response
  (`mod.rs:788`, driven by the app's `EventCache::subscribe` at
  `crates/koushi-sdk/src/client_session.rs:527`),
- every timeline build, which asks for its room
  (`vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/timeline/builder.rs:169`,
  `:173`).

When the *last* subscriber of a room cache drops, the SDK shrinks that cache to
its last reloaded chunk (`event_cache/caches/room/state.rs:643`, `:658`): the map
entry stays and the newest chunk is kept. A room that never had a subscriber
never receives that message at all. This is client-owned state: it does not
depend on a timeline actor existing, and the app's actors keep subscribers for
the whole tab lifetime in any case.

What this does not show: how many bytes a chunk retains, or that the growth is
unbounded. Both need measurement.

### 3. Thread subscriptions created by a diagnostic

`desktop_client_builder_defaults` enables thread subscriptions
(`crates/koushi-sdk/src/client_session.rs:570`). `crates/koushi-core/src/timeline/relay.rs:815`
is marked `#1238 diagnostic (temporary)` and calls
`subscribe_thread_if_needed(&root, Some(latest))`, then separately reads the
thread cache (`sdk_thread_cache_counts`).

Three different lifetimes are involved:

- The *attempt* is one-shot and may be a no-op when a subscription already
  exists. The SDK documents that an automatic subscription can be skipped by the
  server while returning `Ok`
  (`vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/mod.rs:4470-4488`), so a
  successful call does not prove continuing delivery.
- The *thread cache entry* is client-owned (`Caches::threads`, an
  `HashMap<EventId, ThreadEventCache>` under the client event cache,
  `event_cache/caches/mod.rs:215`), not owned by the actor.
- *Server-side subscription state* persists until an unsubscribe or a server
  decision, which can outlive the actor.

The app never calls `Room::unsubscribe_thread`; the replaced-thread unsubscribe
path (`crates/koushi-core/src/runtime.rs:6239`) covers navigation replacement
only. Retained bytes are unmeasured.

### 4. `ThreadsListActor::thread_unread` keeps one entry per thread root

`crates/koushi-core/src/threads_list.rs:231` holds
`thread_unread: HashMap<(String, String), ThreadRootAttention>` plus
`thread_unread_signatures`; both are inserted per root (lines 341-342) and
released only when the room is confirmed left (`forget_room_attention`, line
266). Ordinary unsubscribe deliberately keeps attention for roots that scrolled
out of the window, so this is a contract, not an omission: it is not a candidate
for pruning on view disappearance. Each entry is two strings plus the attention
struct.

### 5. `media_sources` keeps entries that a diff removal no longer backs

`apply_sdk_media_cache_diff` (`crates/koushi-core/src/timeline/media.rs:596-630`)
adds on `Push*`/`Insert`/`Set`/`Append` and drops only on `Reset`/`Clear`;
`Remove`, `Truncate`, `PopFront` and `PopBack` are ignored.

Lifetime of a stale entry: until the next SDK `Reset`/`Clear`, an authoritative
recovery replacement (`replace_authoritative_cache`,
`crates/koushi-core/src/timeline/relay.rs:1170`, `:1432`) or actor teardown.
Ordinary replay does not replace the map: `handle_replay_initial_items`
(`crates/koushi-core/src/timeline/navigation.rs:1244`) re-emits the existing
projection.

`PrivateMediaEntry` holds source metadata, not image or file payload bytes, so
even an unbounded entry count is a small-metadata question.

Follow-up (same day): removal-bearing batches now reconcile the map against the
committed window and edits that drop media clear their stale entry; see
`2026-10-25-issue1616-media-reconciliation-and-residency-bytes.md`.

### 6. `LiveTailRefreshCoordinator::states` is never pruned

`crates/koushi-core/src/live_tail_freshness.rs` keeps a state per non-delayed
room and has no removal path (documented in engineering rules). Driver: rooms
observed; size unmeasured.

### 7. `latest_focused_projection_generation`

`crates/koushi-core/src/runtime.rs:1434` keys a map by `TimelineKey`; only
insertion and lookup were found. Small per entry (room and optional event IDs),
driver is the set of focused timelines ever opened.

## Screening evidence (not attribution)

These paths are closed or bounded as far as the code shows. That is evidence
against a trivial leak, not proof about retained bytes or runtime behavior:

- Direct task vectors prune before push or on abort
  (`crates/koushi-core/src/account/actor.rs:3112-3114`, `:3184-3235`).
- Event handlers: two registration sites, both stored and removed on shutdown
  (`crates/koushi-core/src/sync.rs:701`,
  `crates/koushi-core/src/room/history_keys.rs:130`).
- Avatar state: entries removed on demand change, capacity pressure, completion
  or failure (`crates/koushi-core/src/account/profile.rs:749`, `:882`, `:905`);
  `avatar_inflight` drained on settlement.
- `pending_select` removed when the select settles
  (`crates/koushi-core/src/runtime.rs:2169`); `COMMAND_INBOX_CAPACITY` is 256.
- Per-event side tasks release their map slot on completion: `reply_quote_tasks`
  (`crates/koushi-core/src/timeline/reply_quote_hydration.rs:889`, `:896`) and
  `link_preview_fetches`
  (`crates/koushi-core/src/timeline/item_projection.rs:329`).
- Event fan-out: one 16,384-capacity broadcast with an `EventStreamLag` resync
  path; the webview forwarder uses a dedicated connection.
- SQLite store pool is 4 (`crates/koushi-sdk/src/e2ee.rs:2708`); the client is a
  thin handle (`crates/koushi-sdk/src/client_session.rs:161`).
- Search keeps no message bodies resident (measured 0 bytes in the snapshot);
  the crawl queue is deduplicated per room
  (`crates/koushi-core/src/search.rs:1747`, `:1886`).
- View scopes: 64-scope cap with a 256 MB admission ledger; reservations are
  released with the scope entry
  (`crates/koushi-core/src/view_scope_lifecycle`). The ledger is an admission
  cap, not a measurement.
- `AppState` holds no per-room timeline items
  (`crates/koushi-state/src/state/mod.rs:376`).

## Renderer (sampled, not audited)

- Object URLs are revoked at their owners
  (`apps/desktop/src/components/dialogs.tsx:1814`,
  `apps/desktop/src/components/timeline/ReceiptReaders.tsx:306`,
  `apps/desktop/src/components/timeline/TimelineMedia.tsx:94`).
- One application-level interval exists (`apps/desktop/src/app/useNowMs.ts:14`).
- `timelineViewportSessionMemory` (`apps/desktop/src/components/timeline/TimelineViewportAnchors.ts:136`)
  accumulates one entry per timeline key ever displayed and is never pruned:
  an accumulating owner whose practical size is unmeasured.
- No `will-change` was found; the compositing triggers in
  `apps/desktop/src/styles.css` are the sticky `.timeline-navigation-bar` with
  `backdrop-filter` (line 2741), the masked Space rail (1326/1333), and transient
  fixed overlays.

Not audited: DOM/virtualization retention (`mountedTimelineRows`,
`mountedImages`), JS heap snapshots, and layer-to-node attribution. The observed
surface shapes suggest tiled and full-content backings but do not identify their
producer or prove which WebKit behavior retained them; that needs Web Inspector
-> Layers or `CA_COLOR_OFFSCREEN`.

## What the review can and cannot conclude

- It ranks owners and records which lifetimes are closed. It does not measure
  retained bytes, so it cannot say which owner dominates the 234.7 MiB live
  heap, and it does not attribute the 105.0 MiB fragmentation.
- The event cache and the timeline actors are independent retention surfaces;
  actor retirement does not bound the cache, and cache eviction does not bound
  the actors.
- None of the ranked items is established as the long-session leak.

## Next measurement

1. Separate live allocations from fragmentation and attribute both; do not add
   the two categories.
2. Measure room and thread cache chunks and retained bytes with and without
   timeline actors, including a room that was never subscribed.
3. Compare allocation generations across navigation, media preview and download
   with synthetic data (`MallocStackLogging=1` + Mark Generation, `dhat`, or
   Instruments).
4. Record when auto-shrink actually completes, and distinguish ordinary replay
   from authoritative recovery when following `media_sources`.

## Verification passes

- First pass (2026-10-25): corrected "chunks are never released" to the
  auto-shrink behavior, and bounded `media_sources` by recovery.
- Second pass (2026-10-25, external audit): the audit's six findings were checked
  against the checkout and accepted. This revision: replaced the
  fragmentation-double-counting arithmetic with the measured breakdown; removed
  "until the next replay" for `media_sources`; split the diagnostic's attempt,
  client-owned cache and server subscription lifetimes; recorded confirmed-leave
  actor teardown; retracted pruning `thread_unread` on view disappearance;
  recast "ruled out" as screening evidence; and marked the renderer conclusions
  as sampled.
