# Direct-message classification projection design

## Problem

Koushi can restore a joined room from the Matrix SDK room store before that
room's `RoomInfo.dm_targets` has been populated. The live room-list projection
currently normalizes those rooms without passing the cached global `m.direct`
account-data map. Such a room is initially projected as a normal room and is
absent from both the Home DM list and a Space DM list. A later timeline event
causes another projection after `dm_targets` has caught up, making the DM appear.

The room store and global account data are valid independent asynchronous
inputs. Correctness must not depend on when one happens to update the other.

## Goals

- Restore every joined DM identified by cached `m.direct` at cold start, before
  any new timeline event arrives.
- Apply the same DM classification to Home and Space projections.
- Keep room and space display non-blocking when `m.direct` is unavailable.
- Reclassify rooms when Sliding Sync publishes a new `m.direct` event.
- Avoid redundant projections when the effective direct-room map is unchanged.
- Add privacy-preserving diagnostics sufficient to distinguish missing account
  data from projection or UI filtering failures.

## Non-goals

- Supporting legacy `/sync` or homeservers without the Sliding Sync account-data
  extension.
- Changing which rooms belong to a Space.
- Changing Matrix `m.direct` or room-membership state.
- Performing a blocking network request solely to classify the initial room
  list.

## Chosen design

### Stable projection context

The live Room observer will own a `DirectClassificationSnapshot` alongside its
current `RoomListService` entries. The snapshot contains the normalized mapping
from room ID to direct-message targets and a source marker:

- `local_store`: loaded from cached global `m.direct` account data;
- `sliding_sync_event`: replaced by a typed `DirectEvent`;
- `unavailable`: no explicit mapping has been observed yet.

Every room-list projection receives this snapshot explicitly. When a cached or
event-delivered `m.direct` map is available, it is the authoritative DM
classification: absence from that complete map means the room is not a DM.
Only while `m.direct` itself is unavailable may the SDK normalizer use the
room's `direct_targets`/SDK direct-room fallback for provisional
classification. It must not fetch global account data from the network in the
projection hot path.

### Race-free initialization

The observer subscribes to typed global `DirectEvent` updates before reading
cached `m.direct`. This ordering closes both sides of the startup race:

1. an event processed before subscription is visible in the local account-data
   store;
2. an event processed after subscription is delivered by the event stream.

The cached read initializes the projection context. Room entries can be
projected immediately even when the read returns no event or fails.

### Update flow

When a `DirectEvent` arrives, the observer normalizes its content into the same
room-to-target map and compares it with the current snapshot. If the effective
map changed, it replaces the snapshot and reprojects the current RoomListService
entries. That single projection feeds both Home and Space DM lists. If the map
did not change, no projection is performed.

Removal from `m.direct` is also an authoritative update: a room absent from the
new complete map is no longer classified as a DM, even if an older
`RoomInfo.dm_targets` cache has not caught up yet.

The DirectEvent stream is auxiliary metadata. If it ends, Room observation and
room display continue with the last known snapshot, and diagnostics record the
loss of live DM-classification updates.

## Component changes

### `koushi-sdk`

- Expose a local-only loader for the normalized cached `m.direct` map.
- Expose normalization from `DirectEventContent` to room-to-target mapping.
- Add a room-list normalization entry point that accepts either an authoritative
  direct map or an unavailable marker.
- Preserve fallback classification through `Room::direct_targets()` and the
  existing SDK direct-room predicate only while the direct map is unavailable.
- Do not add a network fallback to this path.

### `koushi-core` Room observer

- Create and retain the direct-classification snapshot.
- Subscribe to DirectEvent updates before the initial cached read.
- Pass the same snapshot to every projection trigger: RoomListService diffs,
  reconciliation, refresh commands, base-room updates, and DirectEvent updates.
- Coalesce identical DirectEvent content by comparing normalized maps.
- Continue operating if initial account-data loading or the DirectEvent stream
  fails.

No UI-specific repair is required: both affected lists consume the corrected
`RoomSummary.is_dm` projection.

## Diagnostics

Diagnostics contain aggregate values only; user IDs and room IDs are excluded.

Record initialization and effective-map changes with:

- snapshot source (`local_store`, `sliding_sync_event`, or `unavailable`);
- mapped room count and target count;
- whether the effective map changed;
- DirectEvent wake and applied-update counts;
- projected DM count;
- classification counts for explicit `m.direct`, SDK fallback, and non-DM;
- stream termination or cached-read failure reason as a bounded token.

The final diagnostic summary should include the current source, mapped-room
count, projected-DM count, and DirectEvent update count so a single report can
identify this failure class.

## Error handling

- Missing cached `m.direct`: record `unavailable`, project immediately using SDK
  fallback state, and wait for Sliding Sync account data.
- Cached-read error: record the bounded error class and continue as unavailable.
- Malformed IDs in account data: ignore only invalid entries, count them, and
  retain valid mappings.
- DirectEvent stream termination: retain the last snapshot, warn once, and keep
  the Room observer alive.
- Projection delivery failure: retain the existing Room observer delivery and
  authority behavior; this feature does not introduce a second retry owner.

## Tests

1. With empty `RoomInfo.dm_targets` and a cached `m.direct` mapping, the first
   projection sets `is_dm = true` without a timeline event.
2. With no initial mapping, a DirectEvent reclassifies the room and updates both
   Home and Space-derived DM projections.
3. Missing account data does not suppress normal rooms or spaces.
4. Repeating equivalent DirectEvent content does not trigger another projection.
5. Removing a room from an available `m.direct` map causes reclassification to
   non-DM even when the room's older `dm_targets` cache is still populated.
6. Initial subscription plus cached read covers events on both sides of startup
   ordering.
7. Diagnostics expose only counts, source markers, and bounded error classes.

Focused unit and observer tests are required before the implementation commit.
The existing room-list identity and Sliding Sync regression tests remain the
fast integration check before rebuilding the DMG.

## Acceptance criteria

- On a cold start with a healthy existing database, a joined DM mapped by cached
  `m.direct` appears in Home and any applicable Space without waiting for a new
  message.
- If the mapping first arrives through Sliding Sync, both lists update during
  the same Room projection cycle.
- Room/Space rendering remains available when direct account data is missing.
- Repeated unchanged account data causes no projection churn.
- A user-generated diagnostic report identifies the current DM-classification
  source and aggregate counts without exposing Matrix identifiers.
