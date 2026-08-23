# Issue #660 Transient Settlement and Trust-Loss Reset

## Scope and diagnosed seams

This change fixes three reducer contracts without decomposing the atomic session
reset:

1. `has_session_projection_context` is currently identical to
   `is_session_ready`, despite the normative state machine allowing correlated
   projection/settlement traffic while the authenticated session is transiently
   `Locked` or `SwitchingAccount`.
2. `SessionLocked` resets `current_session_status` in the top-level reducer, but
   `CurrentDeviceTrustChanged(non-Verified)` and
   `AuthoritativeDeviceTrustChanged(non-Verified)` enter the same Ready → Locked
   path without that reset.
3. `clear_session_views` resets visible `invite_workflow` and `focused_context`
   projections without tracking them or returning matching UI-change effects.

Rust remains the sole owner. No frontend repair, delay, retry, new timer, or
state-machine decomposition is permitted.

## Admission split

Keep two predicates with intentionally different names and uses:

- `is_session_ready`: exactly `SessionState::Ready(_)`; command admission and
  fresh normal projection sources use this.
- `has_session_projection_context`: `Ready`, `Locked`, or `SwitchingAccount`;
  correlated terminal actions and explicitly documented actor projections use
  this.

Audit every current call site. At minimum, change fresh room/invite list
producers to `is_session_ready` so broadening the projection context cannot
repopulate cleared normal views:

- `navigation::handle_invite_list_updated`;
- `room::handle_room_list_updated` itself, before its current
  `Uninitialized → Ready { Cache, 0 }` readiness bump, plus
  `handle_room_list_updated_with_crawler`;
- room-list bootstrap entry and the shared room-list update container;
- `handle_room_list_snapshot_provisional` and
  `handle_room_list_snapshot_authoritative` at function entry, before either can
  assign `state.invites`. The provisional `(Cache, 0)` match is valid from
  `Uninitialized`, and a hand-built authoritative readiness can also match in a
  transient state, so delegation-only guarding is insufficient;
- `room::handle_room_tags_updated`, `handle_room_tag_set`, and
  `handle_room_tag_removed`, whose canon admits tag projections only in Ready.

Retain the wider helper for correlation-owned terminal paths (pin/unpin,
encryption-debug settlement, mark-read/unread settlement) and for
`RoomPinnedEventsUpdated`, whose existing canon explicitly admits projection
traffic in transient Locked/SwitchingAccount states. Existing key/request/room
and pending-operation guards remain authoritative; the wider session predicate
must not weaken them.

Do not include SignedOut, Restoring, Authenticating, Provisional/recovery,
Rejecting, LoggingOut, or CapabilityBlocked in normal session projection
context. Verification-gate projection has its separate existing predicate.

## Trust-loss status reset

At the top-level reducer, reset `current_session_status` only when the action is
an admitted exit from Ready:

- `SessionLocked` while Ready;
- `CurrentDeviceTrustChanged(Unknown | Unverified)` while Ready;
- `AuthoritativeDeviceTrustChanged(Unknown | Unverified)` while Ready.

Then invoke the existing session handler. A Verified observation performs no
reset. A duplicate/non-Ready trust-loss observation performs no reset and emits
no transition. This preserves exact-once semantics without moving status logic
into multiple nested handlers. Existing logout/switch reset behavior remains.

The resulting action remains atomic: the same reducer call changes Session to
Locked, resets current-session status to Idle, clears session views, and returns
the ordered effects. No intermediate public state is published.

## Visible reset effects

Add internal `UiEvent` variants:

- `InviteWorkflowChanged`;
- `FocusedContextChanged`.

They are reducer/runtime-internal effects, not serialized DTO or TypeScript wire
changes. In `clear_session_views`, capture before mutation:

- `had_invite_workflow = invite_workflow != InviteWorkflowState::default()`;
- `had_focused_context = focused_context != FocusedContextState::Closed`.

After the existing atomic reset, emit each new effect only when its projection
was visible/non-default. Keep one fixed ordering:

1. existing unconditional `RoomListChanged`;
2. `InviteWorkflowChanged` when required;
3. existing timeline/thread effects;
4. `FocusedContextChanged` immediately after `ThreadChanged` when required;
5. all remaining existing effects in their current relative order.

A default projection produces no new effect. Repeated cleanup therefore cannot
produce duplicate invite/focused notifications. No existing effect is removed
or reordered relative to another existing effect.

## Verify first: reducer RED

Add public reducer tests before production edits.

### Projection-context matrix

Build correlated pending pin/unpin states under Ready, Locked,
SwitchingAccount, CapabilityBlocked, and SignedOut. For exact request + room +
operation:

- Ready, Locked, and SwitchingAccount settle;
- CapabilityBlocked and SignedOut are inert;
- stale request, wrong room, and opposite operation stay inert in every state.

Add `RoomPinnedEventsUpdated` characterization for Locked/SwitchingAccount and
prove the expected room interaction projection applies. In
`message_interactions_state.rs`, rename/flip the existing
`pin_completion_is_ignored_after_session_leaves_ready`: exact completion now
settles in Locked/Switching, while stale/wrong/opposite terminals remain inert.

Separately assert that RoomListUpdated and InviteListUpdated remain inert in
Locked/SwitchingAccount in `navigation_state/room_list.rs` and
`invite_state.rs` (or `navigation_state.rs`) respectively. The RoomListUpdated
fixture starts from `room_list.readiness = Uninitialized` and requires whole-state
equality, proving the action entry cannot perform its current pre-delegation
readiness bump. Add whole-state Locked/Switching rows for provisional
`(Cache, 0)` and matching authoritative snapshots, proving neither can mutate
`invites`, readiness, spaces, or rooms before delegation. In
`room_tag_state.rs`, require RoomTagsUpdated, RoomTagSet, and RoomTagRemoved to
remain inert in both transient states. Broad context must not recreate ordinary
views or diverge from tag canon. The current code REDs the transient
correlated/projection cases.

In `package_a_state.rs`, rename/flip
`mark_as_read_success_is_ignored_while_session_is_locked`: with the hand-built
room retained, exact success is a correlation-owned terminal and clears counts;
a real post-cleanup Locked state with no room remains inert through `room_exists`.
Mark-read/unread failures are likewise correlation-owned terminals and may emit
the coarse `ErrorChanged` only when their hand-built retained room-existence
and session-context guards still match.

### Trust-loss matrix

First add the internal `InviteWorkflowChanged` and `FocusedContextChanged`
`UiEvent` variants without wiring them into cleanup. Then add the reducer matrix,
so the initial missing-effect failures are behavioral vector mismatches rather
than compile-only RED.

Start from Ready with non-Idle `current_session_status`, a visible invite
workflow, and open focused context. Assert for both current and authoritative
non-Verified trust actions:

- Session becomes Locked and sync Stopped;
- status becomes Idle in the same reducer call;
- invite/focused projections reset;
- exact ordered effects contain StopSync, SessionChanged, RoomListChanged,
  InviteWorkflowChanged, and FocusedContextChanged once each.

Characterize explicit `SessionLocked` identically. Add a separate duplicate
`SessionLocked` row: while already Locked, install an artificial non-Idle newer
status, dispatch `SessionLocked`, and require no reset/effect. Verified
observations and second current/authoritative trust-loss actions after Locked are
likewise inert. Current baseline REDs status reset, duplicate explicit-lock
exact-once behavior, and the two missing effects.

After each admitted trust-loss reset, inject both
`CurrentSessionStatusRefreshed` and `CurrentSessionStatusRefreshFailed` with the
pre-lock request ID and require status to remain Idle; stale status work cannot
revive the cleared projection.

### Cleanup idempotence

Call cleanup-driving transitions with default invite/focused projections and
assert neither new UI event appears. The visible fixture has an active timeline
room, no open thread, a non-default invite workflow, and open focused context.
Its exact trust-loss effect vector is therefore `StopSync`, `SessionChanged`,
`RoomListChanged`, `InviteWorkflowChanged`, `TimelineChanged { room_id }`, then
`FocusedContextChanged`; no error effect is expected. With default projections,
new effects are absent and all existing effects retain their prior order.

Focused reducer commands:

```bash
cargo test -p koushi-state --test session_state
cargo test -p koushi-state --test session_status_state
cargo test -p koushi-state --test message_interactions_state
cargo test -p koushi-state --test navigation_state
cargo test -p koushi-state --test invite_state
cargo test -p koushi-state --test room_tag_state
cargo test -p koushi-state --test package_a_state
cargo test -p koushi-core --test runtime_session
```

Commands whose new/flipped assertions are behaviorally RED must exit non-zero:
`session_state`, `session_status_state`, `message_interactions_state`,
`navigation_state` (entry-readiness and both snapshot pre-mutation rows),
`package_a_state`, and `runtime_session`. InviteListUpdated and the three
Ready-only tag rows are early-green characterization tests; record them as such
rather than fabricating RED. Every transient fresh-projection row snapshots the
full AppState and asserts whole-state equality, not only one field.

Concrete files are `crates/koushi-state/tests/session_state/lifecycle.rs` and
`support.rs`, `tests/session_status_state.rs`,
`tests/message_interactions_state.rs`, `tests/navigation_state/room_list.rs`,
`tests/invite_state.rs`, `tests/room_tag_state.rs`, `tests/package_a_state.rs`,
and `crates/koushi-core/tests/runtime_session.rs`. Record every RED command's own
non-zero exit. Do not accept compile-only RED, sleeps, weakened ordering, or
subset-only assertions.

## Runtime GREEN evidence

Add one focused CoreRuntime integration test using test hooks and public actions:

1. establish a Ready room/session and wait for all setup/account actor traffic to
   quiesce before the measured action;
2. install non-Idle current-session status, visible invite workflow, and focused
   context through reducer actions;
3. observe the state-event stream;
4. inject authoritative trust loss;
5. consume StateDelta events with a bounded event-driven wait until one delta
   contains all four required changed slices (unrelated housekeeping deltas may
   precede it), then require that one delta to contain: Session Locked,
   status Idle, default invite workflow, and focused context Closed. Other
   housekeeping slices may legitimately fold into the same delta and are not
   compared for exact equality; later StopSync cascades are ignored by this
   atomic-publication assertion.

Also verify a late matching correlated settlement under a hand-built transient
state at reducer level; CoreRuntime must not synthesize or repair that state.

## Minimal expected files

- `crates/koushi-state/src/reducer/mod.rs` — admission split, Ready-exit reset,
  reset tracking/effect ordering;
- `crates/koushi-state/src/reducer/navigation.rs` and
  `crates/koushi-state/src/reducer/room.rs` — fresh-projection Ready-only call
  sites;
- `crates/koushi-state/src/effect.rs` — two internal UI events;
- focused `koushi-state` tests and one `koushi-core` runtime integration test;
- `docs/architecture/state-machine.md` — exact projection-context states,
  Ready-only fresh projections, trust-loss reset/effect order, bounded pinned
  consequence, and invite/focused reset effects for every cleanup-driving
  transition (trust loss, logout, switch, rejection, recovery-required);
- this plan and `docs/agents/plans.md`.

No AppAction, AppState field, StateDelta schema, Tauri command, TypeScript type,
SDK API, or persistence format change is planned.

## Preservation and risks

- Session identity does not travel on room actions; therefore no uncorrelated
  fresh projection may use the wider predicate.
- Actual lock/switch cleanup may remove a pending projection before its terminal
  arrives. The terminal then remains inert through its existing pending/key
  guard; widening session context must not recreate absent operation state.
- `RoomPinnedEventsUpdated` is an explicit canon exception and may recreate its
  bounded room-interaction projection in transient context. Because profile
  state was cleared, sender-label resolution can use the safe raw-id fallback,
  and the orphan entry may remain until the next authoritative pinned projection;
  record this bounded consequence explicitly in
  `docs/architecture/state-machine.md`. Ordinary room/list/tag state remains
  cleared.
- New UiEvents trigger no special Core side effect today; they make the reducer
  contract explicit and remain available for future consumers without changing
  serialized state delivery.
- Status reset occurs once at reducer dispatch, never both dispatch and session
  handler.

## Full gates

After focused GREEN, run state lib, focused Core runtime, workspace/all-targets,
Tauri, wasm, QA binary, full frontend Vitest/Playwright/typecheck/lint/build,
SDK/diagnostic/docs/boundary/security/dependency/rustfmt/diff gates, relevant
headless lanes, exact full-diff review, and CI 7/7.

## Acceptance mapping

| Requirement | Evidence |
| --- | --- |
| transient settlements preserved | Locked/Switching exact correlation tests; stale/wrong cases inert |
| Ready admission remains narrow | Locked/Switching RoomList/InviteList rejection tests |
| trust loss resets status once | current/authoritative/explicit lock matrix plus duplicate inert case |
| visible resets notify exactly | ordered invite/focused UiEvent tests, absent when default |
| clear remains atomic | one reducer call and one Runtime state generation containing all reset slices |
| privacy/compatibility | no IDs in errors, no serialized contract change, full matrix |

Implementation starts only after `reviewer-flash-opencode-go` records
`Correct-to-merge`. The final exact diff and RED/GREEN evidence require the same
reviewer family again before PR creation.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. Required the
  three room-tag projections to remain Ready-only, concrete existing test
  targets/files, explicit duplicate-SessionLocked exact-once coverage, first
  StateDelta semantics, stale status completion fencing, and documentation of
  the bounded transient pinned-projection consequence.
- Round 2: `Not correct-to-merge`. Required explicit updates to the two existing
  old-canon tests (`message_interactions_state`, `package_a_state`) and a concrete
  `room_tag_state` host/command for all transient tag-inertness rows.
- Round 3: `Not correct-to-merge`. Required Ready-only admission at the
  `handle_room_list_updated` entry before its readiness bump, whole-state
  transient inertness evidence, and honest RED vs. early-green classification.
- Round 4: `Not correct-to-merge`. Required navigation_state to be classified as
  behavioral RED and Ready-only entry guards/whole-state tests for provisional
  `(Cache, 0)` and matching authoritative snapshots before their invites writes.
- Round 5: `Correct-to-merge`. Every broad-context call site, pre-guard mutation,
  trust reset, effect order, concrete RED/early-green target, runtime delta, and
  documentation obligation was verified against source.

## Implementation evidence

Behavioral REDs before production wiring:

- `session_state`: 76 passed / 2 failed;
- `session_status_state`: 9 passed / 1 failed;
- `message_interactions_state`: 13 passed / 2 failed;
- `navigation_state`: 55 passed / 2 failed;
- `package_a_state`: 18 passed / 2 failed;
- `runtime_session`: 8 passed / 1 failed.

Early-green characterization: `invite_state` and `room_tag_state` already
rejected transient fresh projections. After the approved production wiring, the
same focused targets passed respectively 78, 10, 16, 57, 21, and 9 tests;
invite passed 9, room-tag passed 4, and encryption-debug passed 10. Symmetric
transient pin/unpin failures, encryption-debug settle/fail, and mark-unread
success are directly covered. `koushi-state --lib` passed 39; rustfmt,
agent-docs, and `git diff --check` passed. Runtime shutdown uses the normal
awaitable path after the measured delta.
