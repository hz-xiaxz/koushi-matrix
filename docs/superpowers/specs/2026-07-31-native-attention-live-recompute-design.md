# Native Attention Live Recompute Completion Design

**Issue:** #353  
**Date:** 2026-07-31  
**Status:** Approved under the user's goal-wide automatic approval

## Problem

Main already contains the first half of the #353 fix:

- `RoomListUpdated` recomputes the Rust-owned `AppState.native_attention`.
- The persistent desktop adapter applies the Rust-owned badge count.
- Transient sound and notification effects consume only the Rust-owned
  candidate.
- Existing reducer and adapter tests cover initial-sync suppression, live
  activity in another room, badge application, and sound dispatch.

Two acceptance gaps remain:

1. Room-list recomputation hard-codes `window_focused = true`. This correctly
   suppresses the selected room while focused, but also suppresses a later live
   event in that room while the window is not focused.
2. The frontend adapter records private-data-free application tokens, but core
   does not record the recomputation decision that produced the badge and
   candidate.

The fix must preserve `native_attention` as the only product-semantic source
for badge, sound, and notification behavior. React must not inspect room lists
or infer focus suppression.

## Considered Approaches

### 1. Close #353 against the existing main implementation

This is the smallest action, and the original stale-badge symptom is already
covered. It is rejected because the hard-coded focus value contradicts the
issue's explicit rule: only a focused window showing the active room may
suppress that room's live candidate.

### 2. Complete the existing reducer pipeline

Add a Rust-owned, process-local focus context; feed it from Tauri window focus
events through a typed core command; use it during the existing room-list
recompute; and record a private-data-free recompute diagnostic. This is the
recommended approach because it fixes the reproduced/named invariant without
changing the room-list transport or duplicating attention policy.

### 3. Add detailed event provenance to every room-list projection

Thread initial/live/backfill/self provenance from both SDK backends through
RoomActor, `AppAction`, reducer state, DTOs, and browser fakes. This is deferred.
Timeline backfill does not currently project room-list snapshots, and a
self-sent event does not raise the server unread metric used by the existing
recompute gate. No failing behavioral proof currently justifies that
cross-layer machinery.

## Architecture

### Rust-owned focus context

Add a small `NativeAttentionContext` to `AppState` with
`window_focused: bool`. It is process-local reducer input, not product content,
and is excluded from serialized WebView state. Its conservative default is
focused, matching current behavior until the native window reports its real
state.

Tauri's main-window `Focused(bool)` event dispatches a typed
`ObserveNativeWindowFocus` core command. The AppActor projects the command to a
`NativeWindowFocusChanged` action. No React listener or React-local focus state
is introduced.

On focus change, the reducer:

- updates the process-local context;
- recomputes badge totals from the current rooms;
- suppresses transient candidate creation for the focus transition itself;
- emits `NativeAttentionChanged` only if the serialized attention projection
  actually changes.

A later room-list update uses the stored focus fact. Therefore:

- focused + active room + live increase: badge updates, candidate suppressed;
- focused + another room + live increase: badge and candidate update;
- unfocused + active room + live increase: badge and candidate update.

### Existing observation boundary

Keep the current room-list observation classification:

- no attention increase is a non-transient/initial-style projection;
- an attention increase is live.

The completion adds explicit tests for the existing invariants that make this
safe:

- historical timeline pagination does not emit `RoomListUpdated`;
- a self-send does not raise the server unread metric that gates candidate
  creation.

If either invariant is later disproved by a headless reproduction, event
provenance must be added at the RoomActor boundary rather than guessed in
React.

## Diagnostics

After a real `RoomListUpdated` reduction, core records one diagnostic event:

- source: `native.attention`
- stage: `recomputed`
- fields: observation token, unread count, badge count, candidate kind or
  `none`, suppression token or `none`, window-focused boolean, and
  active-room-match boolean.

The event must not contain room IDs, event IDs, user IDs, room display labels,
message bodies, or raw errors. Existing frontend tokens remain responsible for
badge application, sound outcome, notification outcome, and adapter failures.

## Error Handling

Window focus observation is best-effort:

- a missing runtime or closed command channel does not crash the native event
  loop;
- the focus event carries no private Matrix data;
- focus observation never prompts for notification permission and never calls
  a platform notification API directly.

Native badge, sound, notification, overlay, and tray failures remain
best-effort adapter outcomes using their existing fixed diagnostic tokens.

## Verification

Build the checks before implementation and keep them headless:

1. Reducer test: an unfocused active room with a live unread increase produces
   a candidate and badge.
2. Reducer test: the same update while focused suppresses only the candidate.
3. Reducer test: changing focus does not replay an existing candidate.
4. Runtime test: the typed focus command reaches the reducer and the
   recomputation diagnostic contains only allowlisted fields.
5. Tauri unit/structure test: main-window `Focused(bool)` routes the typed
   command.
6. Existing native-attention reducer, DTO, frontend adapter, typecheck, lint,
   browser-headless, workspace, and local Conduit gates remain green.

No manual Dock or GUI inspection is acceptance evidence.

## Scope

This PR closes #353. It does not add message previews, notification permission
prompts, React-owned unread aggregation, a new notification policy store, or a
general room-list provenance protocol.
