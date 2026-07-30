# New Thread Draft Lifecycle Design

Date: 2026-07-31
Status: approved through the user's standing authorization for issue work
Issue: #304

## Problem

The thread-open command currently carries only a room ID and root event ID.
That erases the distinction between:

- opening a thread that is known to contain replies, and
- opening a root with no known replies in order to compose the first reply.

Both cases subscribe the same SDK thread timeline. When the initial projection
is empty, two independent React effects can request backward pagination. The
generic viewport policy can repeat the request after later settlement events,
and the special empty-thread effect bypasses the automatic-history setting.
Core also publishes `Paginating` before it owns the account scheduler permit,
so queued work appears as an indefinitely active request.

## Upstream Behavior

Element Web's `ThreadView` obtains an existing thread when one is known and
otherwise creates a thread object immediately from the root event. The thread
view and composer do not wait for historical replies to prove that the thread
exists. Element X uses the Rust SDK direction but does not expose a more
specific product contract that should override this desktop behavior.

Koushi follows the same user-visible rule—first-reply composition is
immediately usable—but keeps the intent and lifecycle in Rust rather than
creating Matrix semantics in React.

## Decision

Add a serializable Rust-owned `ThreadOpenIntent`:

- `ExistingThread`: the entry point knows the root has replies. Subscribe and
  permit one bounded automatic history load when the local projection is empty.
- `NewThreadDraft`: the entry point knows no replies for the root. Subscribe
  for live activity, display an immediately usable empty pane/composer, and
  reject all automatic backward-history requests until the intent is promoted.

The intent crosses the complete typed command path:

```text
Timeline/Threads entry point
  -> Tauri client open_thread
  -> AppCommand::OpenThread
  -> AppAction::OpenThread
  -> ThreadPaneState::{Opening, Open}
  -> frontend snapshot/backfill eligibility
```

The room timeline chooses `ExistingThread` only when its Rust-projected
`thread_summary.reply_count` is greater than zero. A Threads-list entry is
always existing. A room-timeline “Reply in thread” action without a known reply
summary is `NewThreadDraft`. React does not revise that decision from viewport
emptiness.

## Thread Pane State Machine

`ThreadPaneState::Opening` and `ThreadPaneState::Open` retain the intent.
Opening a draft still emits `OpenThreadTimeline`: the subscription is required
for live incoming activity and send routing. Presentation treats a matching
draft `Opening` state as an empty composer-capable pane rather than a loading
pane. Subscription success moves it to `Open` exactly as today.

A matching `ThreadSubmissionAccepted` promotes `NewThreadDraft` to
`ExistingThread`. This is the earliest authoritative local transition: the
core accepted the typed threaded send and will settle it through the normal
send lifecycle. A matching event-backed thread activity observation also
promotes the draft, covering a remote first reply without closing or reopening
the pane. Promotion is monotonic for a pane identity; stale room/root actions
are ignored.

Subscription failure retains the existing private-data-free failure behavior.
Close, room change, logout, and session clear still clear the pane.

## Backfill Admission

`evaluateTimelineBackfill` becomes the single automatic admission point. Its
snapshot includes a semantic eligibility value:

- ordinary room/focused timelines and existing threads are eligible;
- a new-thread draft is not eligible.

The policy checks semantic eligibility before viewport demand. Every trigger—
initial projection, layout settlement, pagination terminal, replay, gap-repair
release, reset, and setting change—therefore reaches the same guard.

Delete `emptyThreadBackfillRequestedRef` and its effect. An existing thread
with an empty local cache uses the generic underfilled path, whose in-flight
and transition fences already limit it to one request at a time. Turning off
automatic history loading blocks underfilled and near-top requests. A genuine
user top-scroll remains explicit only on eligible timelines; a draft has no
older thread history to request.

Promotion enables the ordinary policy for later evaluation. The accepted first
local reply itself does not force a backward request: the live/local-echo
projection fills the timeline. Incoming promotion may schedule one normal
evaluation after its projection settles.

## Pagination Scheduling

The current account-wide scheduler is authoritative; no endpoint-specific
backpressure gate is added.

For `TimelineActor::paginate_once_for`:

1. resolve the pre-request oldest edge;
2. begin waiting for `AccountWorkKind::ExplicitPagination`;
3. acquire the permit, while actor shutdown or replacement can drop the queued
   future;
4. verify the actor generation is still current;
5. publish `Paginating`;
6. execute one SDK pagination call;
7. publish the terminal state through the existing generation gate.

This prevents scheduler queue time from appearing as active pagination.
Room/thread replacement aborts or drops the old actor command path; generation
gating prevents late state from reaching the replacement timeline. The SDK
does not expose cancellation for an already running pagination request, so this
change does not add speculative vendored-SDK cancellation. The user-visible
guarantee is that queued work is cancellable and never displays `Paginating`;
an admitted SDK batch remains bounded by the existing event-count policy and
its completion is fenced if the actor is replaced.

Diagnostics remain private-data-free and use existing timeline/account-work
events. The ordering of `queued`, `started`, `Paginating`, SDK finish, and
terminal publication is sufficient to distinguish scheduler wait from SDK
work without adding room/event identifiers or raw errors.

## DTO and Compatibility Impact

Add the intent to:

- Rust `ThreadPaneState` variants and public exports;
- `AppCommand`, `AppAction`, and Tauri open-thread request;
- Tauri `FrontendAppState` conversion;
- TypeScript `ThreadPaneState` and API signatures;
- browser/Tauri fixtures and checked-in serialization artifacts.

The field is mandatory in new snapshots and commands. Checked-in maximally
populated fixtures exercise both enum values through focused tests rather than
defaulting a missing value and hiding wire drift.

## Verification

Build the failing checks before implementation:

1. Reducer tests prove draft intent retention, stale promotion rejection,
   accepted-send promotion, incoming-activity promotion, and existing intent.
2. Backfill-policy tests prove every settlement trigger rejects draft
   backfill, existing empty-cache requests once per normal fence, and disabled
   auto-load issues no request.
3. Timeline actor tests prove `Paginating` is absent while the scheduler permit
   is queued and stale/replaced actor publication is fenced.
4. Component/browser-headless tests open both entry types, assert an empty draft
   has a usable composer with no spinner or pagination command, then prove first
   send/incoming promotion and existing-thread bounded backfill.
5. Tauri wire, TypeScript typecheck, focused Rust suites, and the relevant
   browser-headless scenario form the final gate.

No manual GUI inspection is acceptance evidence.

## Alternatives Rejected

### Infer From Empty React State

This is a smaller patch, but it assigns Matrix operation semantics to React,
cannot distinguish an uncached existing thread, and can drift after replay.

### Probe The Server Before Opening

This is authoritative but makes composer availability depend on a network
round trip and recreates the unbounded loading failure. Ambiguous cache state
must not be resolved by viewport-driven pagination.
