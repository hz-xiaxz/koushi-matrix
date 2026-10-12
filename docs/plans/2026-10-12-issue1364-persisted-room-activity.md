# #1364 - persisted per-room conversation activity for the Recent sidebar order

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Sidebar Sections And Low Priority"; `REPOSITORY_RULES.md` (canon-first,
security, tests).

## Problem

`RoomSummary.conversation_activity` is the only Recent-first ordering key
(`crates/koushi-state/src/state/room.rs`, `compare_conversation_activity`), and
it is present only when the newest event the client can see is an
`m.room.message`, an `m.room.encrypted` event, or a thread reply
(`crates/koushi-sdk/src/room_projection.rs`, `matrix_conversation_activity_source`).

Two gaps make the key disappear in ordinary use:

1. The room-list Sliding Sync subscription requests `timeline_limit = 1`
   (`vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`,
   `DEFAULT_LIST_TIMELINE_LIMIT`), so for a room the user has not opened the
   in-memory event-cache window holds a single event. The fallback scan
   (`matrix_room_cached_conversation_activity`) therefore cannot find an older
   message behind a sticker, poll, call, reaction, redaction, or state event.
2. Nothing carries the value across sessions. `RoomSummary` is rebuilt from the
   SDK on every projection, so a cold start re-derives the order from whatever
   single event per room is in memory.

Rooms without the key fall behind every room that has it and then sort by
case-folded `display_label`, so a section with many activity-less rooms reads as
alphabetical. Reported in the field with the "Recent messages" sort selected, no
unread badge on the misordered rows, and no change after opening the room.

## Design

Two independent ordering inputs, both Rust-owned:

- **Conversation activity** (unchanged meaning): a message-like event the canon
  accepts. Memberships, state, receipts, typing, presence, redactions, edits,
  and reactions still never create it.
- **Server recency stamp** (new ordering use): the SDK's opaque
  `Room::recency_stamp()` value, already projected onto `RoomSummary.recency_stamp`
  and already documented upstream as the sort fallback when `LatestEventValue`
  is unavailable or inaccurate. It is consumed **only inside the activity-less
  group**, so a newly joined room still cannot outrank a room with a real
  conversation. This removes the alphabetical collapse even before any store
  exists.

On top of that, the client remembers the newest conversation activity it has
ever seen per room:

- `koushi-state` owns a serializable `RoomActivityState` (room ID to timestamp
  and source) inside `AppState`, and the room-list reducer merges
  `max(live, persisted)` into each installed `RoomSummary`, advances the map
  from the live snapshot, and prunes entries for rooms the authoritative
  snapshot no longer contains (plus `RoomLeftLocally`).
- `koushi-core` loads the map at session start and persists it through the
  existing `StoreActor` conventions: a dedicated HKDF domain in `koushi-key`, an
  account-scoped file under `account_root_dir`, a versioned magic
  (`KOUSHI-ROOM-ACTIVITY-V1\0`), `koushi_store` envelope encryption, an empty
  state deleting the file, and `atomic_replace_file` for the write.
- The persist effect is emitted only when the map advances, so a room-list
  update that changes no activity writes nothing.

## Consequence for #542

With the ordering input fixed, the sidebar default sort can move from
`activity` (attention-first) to `recentFirst` without regressing read rooms into
name order. #542 tracks that flip, the `active` to `recent` preference
migration, the Recent/新着順 labels, and the activity-classification regression
gate.

## Slices

1. `crates/koushi-state`: `recency_stamp` fallback in
   `compare_conversation_activity`; tests in
   `crates/koushi-state/tests/navigation_state/sidebar_sorting.rs`.
2. `crates/koushi-state`: `RoomActivityState` in `AppState`, reducer merge,
   advance, prune, and the persisted-activity effect; tests beside the room list
   reducer tests.
3. `crates/koushi-core`: `store/room_activity.rs` load/save with the round-trip
   and corruption tests, `koushi-key` derivation and its domain test, runtime
   session-start load and effect handling.
4. Canon: `docs/architecture/state-machine.md` ordering key and the persisted
   store; `docs/architecture/overview.md` if the ownership statement needs it.

## Not in scope

- Changing the `latest_event` preview so a sticker, poll, or call still renders
  a preview row. The same guard discards it today, and the fix belongs with the
  preview contract, not ordering.
- Cross-device or cross-account activity history: the record is a local cache of
  what this client observed, never a source of truth about the room.
