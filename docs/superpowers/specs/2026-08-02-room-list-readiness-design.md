# Room-list readiness and cold-start projection design

## Scope

Issue #409 fixes the misleading cold-start state where the desktop renders an
authoritative-looking empty room list before any backend has proved that the
account's joined-room projection is complete. The change covers the Rust
state contract, room actor lifecycle, sync fallback fencing, Tauri/browser
mirrors, React rendering, diagnostics, and deterministic regression tests.

The existing Matrix SDK `RoomListService` remains the only SyncService room
list owner. The change adds readiness metadata around its projections; it does
not introduce a second room-list service or alter room membership semantics.

## State contract

Add a Rust-owned `RoomListReadiness` projection alongside the existing
`RoomListProjection`:

- `uninitialized`: no cache or authoritative backend projection exists;
- `loading`: bootstrap or a backend replacement is in progress;
- `ready`: the current generation has committed an authoritative projection;
- `failed`: the current generation failed after no usable projection existed.

The projection also carries a private-data-free source token (`cache`,
`syncService`, or `legacy`), the active backend generation, and bounded
diagnostic counters/timestamps. Room counts are scalar counts only. Existing
room rows remain in the normal `rooms`, `spaces`, and `invites` fields, so
React does not derive readiness or source locally.

The default state is `uninitialized`. A cache-backed joined-room snapshot may
move the state to `loading` with source `cache` and remains provisional. Only a
current-generation SyncService proof or a committed legacy response moves the
state to `ready`, including a genuine authoritative zero-room result.

## Projection and generation flow

RoomActor owns a monotonically increasing backend generation for each
`SyncStarted`/replacement. Every observation loop captures its generation and
source. Before starting a new observation, the actor stops the old loop and
marks the room-list contract as loading while retaining the last usable rows.

The initial session bootstrap requests the existing SDK store's joined-room
normalization path and projects it as provisional cache data. A SyncService
Reset is accepted as authoritative only after the existing connectivity proof
boundary is reached. An empty or incomplete unproven Reset is held and cannot
clear a non-empty cache projection. Legacy sync commits its first successful
response as the authoritative projection for its current generation.

Reducer actions carry source and generation metadata. The reducer rejects a
projection whose generation is older than the current room-list generation,
and accepts a zero-room projection only when the source is authoritative and
the proof flag is set. Late results from either replaced backend are therefore
no-ops.

The user-facing sync lifecycle remains `starting`/`reconnecting` until the
room-list proof is committed. The existing internal SyncService lifecycle and
fallback behavior remain intact; only the projected readiness and public
status are tightened at the proof boundary.

Search crawler admission is guarded by `RoomListReadiness::Ready`. Its existing
background scheduling and preemption policy remain unchanged.

## UI and transport

The Tauri `FrontendAppState`, TypeScript `AppState`, browser fake snapshots,
golden state fixture, CoreEvent contract, and IPC mocks mirror the new
Rust-owned fields. The sidebar renders cached rows with a non-alarming
provisional/loading label. It renders `0 rooms`/`0 DMs` as an empty-account
state only after readiness is `ready`. Loading and fallback copy use existing
i18n catalog patterns.

Diagnostics emit only fixed tokens, counts, source, generation, elapsed
durations, and decision counters. They never contain room IDs, user IDs,
event IDs, names, aliases, message content, or raw SDK errors.

## Verification

Tests are added before the implementation:

1. State/reducer tests cover default uninitialized state, cache loading,
   authoritative non-empty/empty results, held unproven empties, generation
   rejection, and retaining a usable cache after failure.
2. Core tests drive a production-shaped SyncService-running-before-proof,
   empty Reset, fallback-to-legacy, delayed first response, stale generation,
   and final authoritative room sequence without fixed sleeps.
3. Browser/React tests cover loading instead of false zero counts, cached rows
   during fallback, genuine empty accounts, no empty flash across replacement,
   and stable room selection after authoritative replacement.
4. Existing timeline continuity, sync fallback, DTO golden, CoreEvent, IME,
   lint, typecheck, build, and full CI checks remain green.
