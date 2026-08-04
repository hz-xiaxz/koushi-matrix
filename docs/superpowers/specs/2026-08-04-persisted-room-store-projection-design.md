# Persisted Room Store Projection Design

## Problem

Koushi currently treats the room IDs present in each Sliding Sync response's
`rooms` object as the complete visible room set. That object contains changed
room payloads, not the authoritative room-list membership. After restarting
with a persisted `pos`, an incremental response can contain only a few changed
rooms while the SDK store still contains the user's joined rooms and spaces.
Koushi then hides the stored rooms and spaces, projects a permanently
provisional list, and may show several distinct rooms with the same display
name as if they were duplicates.

The existing database must remain intact during repair and verification.

## Decision

Use the SDK's persisted room store as the immediate display source, matching
Element X's cache-first behavior. Sliding Sync responses update that store;
their `rooms` keys must never be interpreted as complete list membership.

Remove the response-local visible-ID filter from `RoomListService` snapshots
and dynamic entries. Continue filtering by Matrix membership state so joined
and invited rooms are visible while left, banned, and knocked rooms are not.
Keep committed-response sequence and range-loading metadata separate from the
room collection. These signals describe connectivity and loading progress,
not which stored rooms may be rendered.

## Projection invariants

- A room is keyed by Matrix room ID and appears at most once in a projection.
- Distinct room IDs may share a display name and remain separate rows.
- Spaces are classified from stored `m.room.create` state and projected to the
  space rail immediately when that state is available.
- A resumed incremental response with zero changed rooms must not erase or
  hide stored rooms or spaces.
- Provisional versus authoritative readiness must not change the collection's
  identity or introduce duplicate rows.
- No repair path deletes, resets, or recreates the user's database.

## Diagnostics

Add privacy-preserving aggregate fields at the SDK-to-core projection boundary:

- SDK store entries before membership filtering
- joined, invited, and excluded membership counts
- unique room-ID count and duplicate-entry count
- normalized room and space counts
- number of display-name collision groups and rows in those groups
- response-local changed-room count, maximum room count, range-loaded state,
  response sequence, and whether the sync resumed from a persisted position

Do not record room IDs, names, aliases, event contents, tokens, or homeserver
response bodies.

## Error handling

If projection delivery fails, retain the last state and report a coarse
diagnostic failure. A mismatch between the server's maximum room count and the
local store is diagnostic evidence, not permission to discard local rooms.
Normal incremental responses with no changed rooms are successful responses.

## Verification

The focused regression tests cover:

1. A pre-populated store plus a resumed response containing only a subset of
   changed rooms preserves every joined room and space.
2. A response containing no changed rooms preserves the prior projection.
3. Duplicate input entries with one room ID yield one projected row.
4. Separate room IDs with the same display name remain separate and are
   counted as a name collision, not an ID duplicate.
5. Joined, invited, and excluded membership states are classified correctly.
6. Range and connectivity signals can progress independently without changing
   the visible room identity set.

For the rapid local validation, run the focused Rust tests and build a signed
release DMG without deleting the existing application database. Broader tests
and PR preparation follow after the user confirms the repaired DMG behavior.
