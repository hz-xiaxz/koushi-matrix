# #1481 - Room-scoped typing lane (keep pending-send publication live)

Status: implementation plan. Canon: `docs/architecture/overview.md`, "Async
Design Rules" rule 14, and `docs/architecture/state-machine.md`, "Live Signals"
-> "Room-Scoped Typing Lane". Related: #725/#823 (SDK-local-echo fallback),
#1064 / `42648884` (admission no longer waits on the actor), `24d23088`
(echo-handover visibility). The October 16 session's exact blocking operation
stays unconfirmed; typing HTTP latency is the structurally confirmed risk.

## Problem

`TimelineActor::run` awaits every selected handler. `handle_set_typing`
(`crates/koushi-core/src/timeline/read_state.rs`) awaited
`Room::typing_notice`, and the vendored SDK performs HTTP in
`send_typing_notice` (`vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/mod.rs`).
While it waits, the actor cannot service pending-send refreshes, canonical diff
batches, or cleanup, so the renderer sees no timeline update until convergence.
Admission (#1064) and echo handover (`24d23088`) are already correct; the
missing piece is that publication must not queue behind unrelated outbound HTTP.

## Handler inventory (audit required by the issue)

| Handler | Blocking dependency | Class | Action |
| --- | --- | --- | --- |
| `SetTyping` | `Room::typing_notice` -> `send_typing_notice` HTTP | outbound HTTP await (always) | **move to lane (this PR)** |
| `RedactReaction` (removing a remote reaction on a remote event) | `matrix-sdk-ui` `toggle_reaction_local` -> `RoomDataProvider::redact` -> `Room::redact` HTTP | outbound HTTP await | follow-up issue |
| `EditText` (remote target) | `Timeline::edit` -> `Room::make_edit_event` -> `Room::get_event`, which can fetch `/event` on a cache miss | network-capable preparation | follow-up issue |
| `DownloadMedia` | `spawn_media_download` worker -> actor `MediaDownloadFinished` | spawned worker with retained `JoinHandle`, not the directly-polled lane | keep |
| `LoadLinkPreviews` | `spawn_link_preview_fetch` worker -> actor `LinkPreviewsFetched` | spawned worker with retained `JoinHandle` | keep |
| `SendReaction` (add) / `Redact` / `EditText` (local target) | `send_queue` local enqueue (HTTP happens inside the SDK queue) | local projection | keep |
| `RetrySend` / `CancelSend` | SDK `SendHandle` state | local projection | keep |
| receipt / fully-read | manager-owned `ReadWorkerSupervisor` | separate lane | keep |
| sends | manager-owned `SendEnqueueWorkerSupervisor` + terminal observer | separate lane | keep |
| `ApplyReadSuccess`, `RefreshPendingSendProjection`, `DisplayPolicyChanged`, `SendQueueUpdate`, decrypt retry | local projection / mailbox | local | keep |

The verification of this table is the review finding that the first draft was
wrong: `room/mod.rs:2109` proves typing, `timeline/controller/mod.rs:780`
proves remote-reaction removal, and `vendor/matrix-rust-sdk/crates/matrix-sdk/
src/room/edit.rs:128` proves edit preparation. Typing is the confirmed risk from
the October 16 diagnostic and the first reviewable slice; the other two are
filed as a tracked follow-up (below), not silently declared local.

## Design

New module `crates/koushi-core/src/timeline/typing.rs`:

- `TypingLane` is owned by `TimelineManagerActor` (session-scoped), like
  `ReadWorkerSupervisor`. It holds `tasks: FuturesUnordered<...>` polled by the
  manager run loop (both `run` and the inline `await_driving_manager_workers`
  helper), so the same loop that already polls read workers polls the lane; no
  new task and no detached work.
- Admission is keyed by room id derived from `TimelineKey` (Room, Thread, and
  Focused keys share `room_id`). A key whose account is not the owning session's
  account is rejected with `NotSubscribed`. State per room: at most one
  in-flight request plus a `VecDeque` of admitted requests bounded by
  `TYPING_LANE_QUEUE_CAPACITY`, and the session-wide set of rooms holding
  unsettled notices is bounded by `TYPING_LANE_MAX_ROOMS`. A request beyond a
  bound settles with `TimelineFailureKind::QueueOverflow`.
- One in-flight request per room, popped FIFO on completion, so an older `true`
  cannot be submitted after a newer `false`; rooms do not order each other. Each
  room tracks the exact identity of its active request; a completion whose
  identity does not match the active request is inert, so duplicate or retired
  completions cannot settle a newer request. No separate generation registry is
  needed.
- Each task calls `Room::typing_notice(is_typing)` under one absolute
  `TYPING_LANE_TIMEOUT`. The SDK throttle/retry stays authoritative.
- Completion emits `CoreEvent::LiveSignals(LiveSignalsEvent::TypingSet {
  request_id, key, is_typing })` or a correlated `OperationFailed { Sdk }`.
- `TypingLane::cancel_all_and_settle()` cancels in-flight tasks and emits a
  correlated failure for every admitted-but-unsettled request. It is called on
  the manager shutdown drain like `read_workers.cancel_all()`, before the
  shutdown acknowledgement. The manager `Drop` fail-safe calls the same
  synchronous settlement (broadcast send never blocks), so a panic path still
  settles admitted requests instead of leaking them with no terminal.
- Test-only `TypingNetworkContext::Synthetic` mirrors the read-worker synthetic
  channel so a test can hold one typing operation behind a barrier and answer it
  explicitly.

Wiring: `TimelineCommand::SetTyping` stops routing `TimelineActorMessage::
SetTyping`; the manager admits into the lane. `TimelineActorMessage::SetTyping`
and `TimelineActor::handle_set_typing` are deleted. `spawn_with_session`
constructs the matrix-backed lane; the test/default constructors use
`TypingLane::unavailable()`.

Lifecycle: a room switch already sends an ordered `false` through the same
room-scoped lane (`App.tsx`), so composer exit needs no new stop signal; actor
replacement does not touch the manager-owned lane, so a replacement view keeps
the room's ordered sequence; logout and session clear drop the manager, which
cancels and settles. Unmount without a room switch leaves the SDK/server expiry
contract authoritative, as today.

Diagnostics: `trace_typing_operation` in `timeline/diagnostics.rs` records
`core.typing` events with stage (`admit`, `submit`, `complete`), a
private-data-free operation kind, an opaque local correlation, queue wait,
duration, and outcome. No room/user/event ids, content, or raw SDK errors.
Pending-projection publication latency (coordinator insertion vs actor
publication vs renderer receipt) is a separate diagnostic surface: the actor
publication stage needs the send lifecycle correlation threaded into
`PendingSendProjection`, and a renderer receipt has no ack channel today. The
canon therefore does not claim a publication stage here; it is filed as #1482.

## Acceptance tests (RED first)

1. Held typing operation: admit typing with the synthetic network held, then
   drive a pending-send refresh; the actor receives and publishes the refresh
   before the barrier is released. The RED version keeps the inline await.
2. Ordering: `true -> false -> true` for one room submits in order, one in
   flight at a time; two rooms progress independently.
3. Bound: the `N+1`th queued request settles with `QueueOverflow`; a rejected
   final `false` is covered explicitly, with the SDK/server expiry contract as
   the documented fallback (React does not retry after advancing its
   deduplication ref).
4. Failure and staleness: a failed operation emits one correlated
   `OperationFailed`; a duplicate completion for an already-settled request is
   inert; a completion cannot settle a newer active request.
5. Owner shutdown and manager `Drop` settle every admitted request and leave no
   task, each with its own manager-level test.
7. A key for another account is rejected with `NotSubscribed`; the session-wide
   room map is bounded by `TYPING_LANE_MAX_ROOMS`.
6. `live_signals` QA scenario (`typing=ok`) stays green on local Tuwunel; the
   existing Rust-shaped browser-headless coverage is unchanged because React
   neither owns nor repairs typing state.

## Follow-up (filed, not in this PR)

`RedactReaction` remote removal and `EditText` remote preparation still await
network on the actor loop; and the pending-send diagnostic still cannot
distinguish actor publication from renderer receipt. They are tracked in #1482 because each needs its own ordering/result contract and the renderer
receipt needs a frontend ack channel that does not exist yet. #1481 stays open
for them.

## Out of scope

Receipt/fully-read lanes, the SDK send queue, and React typing timers are not
touched. Voice/video and recorded voice messages stay deferred.

## Review record

Independent pre-implementation review (gpt-6.1-sol, read-only, 2026-10-16)
against `origin/main`. Disposition:

- BLOCKER "reaction removal awaits HTTP" — **verified** in
  `vendor/.../timeline/controller/mod.rs:780`; the false "keep" classification
  was corrected and the fix filed as #1482.
- "remote edit preparation can fetch" — **verified** in
  `vendor/.../room/edit.rs:128`; same disposition.
- "typing completions must be polled in `await_driving_manager_workers`" —
  **verified** (`manager.rs:2291`); the plan now requires it.
- "room generation is not request identity" — **adopted**; the lane matches the
  exact active request and drops non-matching completions.
- "abort-only `Drop` leaks terminals" — **adopted**; `Drop` uses the same
  synchronous settlement as shutdown.
- "diagnostics do not distinguish publication and renderer receipt" —
  **verified**; deferred to the follow-up issue rather than half-implemented.
- "composer exit unspecified" / numeric bound — **adopted**; see Design and
  test 3.
- Delete list: a separate room-generation registry and the false blanket
  "keep/local" classification were removed from the plan.

## Post-implementation review record

Independent post-implementation review (gpt-6.1-sol, read-only, 2026-10-16) of
`git diff origin/main...HEAD`. Disposition:

- IMPORTANT "account identity discarded in `admit`" — **verified** (no account
  fence applies to `SetTyping`); the lane now carries the owning session's
  account and rejects a foreign key with `NotSubscribed`, with a test.
- IMPORTANT "no session-wide bound on the room map" — **adopted**;
  `TYPING_LANE_MAX_ROOMS` bounds rooms with unsettled notices and rejects new
  rooms beyond it with `QueueOverflow`, with a test.
- IMPORTANT "teardown hooks untested" — **adopted**; manager shutdown and
  manager `Drop` each have a test that asserts one correlated failure per
  admitted request.
- MINOR "canon claims a publication stage the code does not emit" — **adopted**;
  the canon now lists only admission/submission/completion and records the
  publication/receipt deferral to #1482.
- MINOR "state diagram leaves `InFlightQueued` early / impossible edge" —
  **adopted**; the diagram now keeps the queued self-transition while the queue
  is non-empty.
- Delete list — **adopted**; `typing_room_id` was removed in favor of
  `TimelineKey::room_id()`, and the redundant `state.active = None` was removed.

All findings were verified against the source or a failing test before acting;
no blocking finding was dismissed.
