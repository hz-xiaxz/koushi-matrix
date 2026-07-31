# Space Members audit and profile cache design

Date: 2026-07-31

## Summary

Koushi will add a Space-scoped `Members` entry above the `DMs` / `Rooms`
switcher. The entry opens a right-side `Space Members` panel that distinguishes:

1. users joined to the Space room;
2. users invited to the Space room but not yet joined; and
3. users joined to at least one child room but neither joined nor invited to
   the Space room.

The third group appears last and exposes `Invite to Space` both as an inline
button and as a context-menu action. This makes forgotten Space invitations
visible without changing Matrix membership semantics.

Name resolution will use room-scoped profiles as authoritative input and an
account-scoped local profile cache as a fallback. The durable source remains
the existing encrypted Matrix SDK store; Koushi must not create a new plaintext
profile database.

## Problem

The existing People projection uses `RoomMemberships::ACTIVE`, which means
`JOIN | INVITE`. It presents invited users as ordinary members because the UI
model does not retain membership state. The current diagnostic labels the
result as `direct_space_members`, so it also cannot distinguish joined from
invited users.

Separately, Seen and reaction labels can become `Unknown user` when their event
payload omits a display label. A user may legitimately be joined to a child room
without being joined to its parent Space. Their name must be resolved from the
child room or a local fallback cache, never by requiring parent-Space
membership.

## Goals

- Show only `JOIN` users in the main Space member section.
- Show Space `INVITE` users in a separate pending section.
- Compute the union of `JOIN` users across all current child rooms.
- Show child-room-only users at the end of the panel, deduplicated by user ID.
- Make forgotten Space invitations visible and actionable.
- Resolve people-facing labels without requiring Space membership.
- Reuse the encrypted SDK store and keep an account-scoped runtime cache.
- Add rich, private-data-free diagnostics at SDK, core, and UI boundaries.
- Keep membership and profile semantics Rust/headless-owned.

## Non-goals

- Automatically inviting child-room members to the Space.
- Treating child-room membership as Space membership.
- Changing Matrix room or Space membership rules.
- Fetching every profile from the homeserver whenever the panel renders.
- Storing raw user, room, or event identifiers in diagnostics.
- Replacing local user aliases or room-specific display names.

## Membership model

The Rust projection will preserve membership state instead of flattening
`ACTIVE` into one list.

### Space joined

`space_joined` contains users whose membership in the Space room is `JOIN`.
These users appear in the first section.

### Space invited

`space_invited` contains users whose membership in the Space room is `INVITE`.
These users appear in `Invitation pending`. They do not count as joined Space
members.

### Child-room-only

For every current child room, collect users with `JOIN` membership. Build a
deduplicated union keyed by user ID, retaining the set of child room IDs in
which each user is joined.

The final set is:

```text
child_room_only = child_join_union - space_joined - space_invited
```

`LEAVE`, `BAN`, and `KNOCK` memberships are excluded from all three displayed
sections.

The projection also retains completeness facts per child room. A partially
synced child room contributes currently known joined users but increments an
incomplete-room count. The UI must disclose that the audit is still syncing
rather than presenting the result as complete.

## Rust-owned projection

Introduce a Space-members projection owned below React. Suggested shape:

```text
SpaceMembersProjection
  space_joined[]
  space_invited[]
  child_room_only[]
  child_room_count
  complete_child_room_count
  incomplete_child_room_count
  generation
  operation
```

Each projected person contains only the UI facts required by the panel:

```text
SpaceMemberEntry
  user_id
  display_label
  original_display_label
  avatar
  role
  membership_class
  child_room_ids[]
  invite_state
```

React receives an already classified and sorted projection. It must not infer
membership from room lists or merge member snapshots itself.

The projection is refreshed when:

- the active Space changes;
- Space membership changes;
- a child room is added or removed;
- a child-room membership snapshot changes;
- an invite operation settles; or
- a previously incomplete child-room member snapshot becomes complete.

Generation fencing prevents a late result for a previously selected Space from
overwriting the active projection.

## Local profile cache

Koushi already has an account-scoped `ProfileState.users` map and an encrypted
Matrix SDK state store containing room membership profiles. The design extends
their use rather than introducing a separate plaintext database.

### Durable and runtime layers

- Durable source: existing encrypted Matrix SDK room/member store.
- Runtime accelerator: account-scoped Koushi profile cache keyed by user ID.
- Room-specific profile observations update the runtime cache when they contain
  a non-empty display name or avatar.
- A cache entry records the latest usable label/avatar and a coarse freshness
  class. It does not replace room-scoped profile data.
- On a runtime-cache miss, core may perform a local-only SDK-store lookup in the
  relevant room(s). Network profile lookup is not on the render path.

### Resolution precedence

People-facing labels use this order:

1. user-defined local alias;
2. profile from the room in which the event, receipt, or membership is shown;
3. Space-room profile when rendering the Space member sections;
4. non-empty label embedded in the receipt, reaction, or event payload;
5. account-scoped local profile cache;
6. locally stored homeserver profile, if already available;
7. `Unknown user`.

Room-specific names remain authoritative because Matrix membership profiles may
differ between rooms. The global cache is a last-known fallback, not a source
that overwrites room identity.

Conflicting room labels are resolved by preferring the active/relevant room;
outside a room context, the most recently observed non-empty cached label is
used. Local aliases always win.

## User interface

The approved layout is Option A from the Visual Companion.

### Left sidebar

When a Space is active, add a `Members` entry between the Space heading and the
`DMs` / `Rooms` switcher.

The entry shows:

- joined Space member count; and
- a warning count for child-room-only users, when non-zero.

Example: `Members 26 · +3`.

No Space-specific Members entry is shown in account Home.

### Right panel

Clicking the entry opens `Space Members`, with these ordered sections:

1. `Space members`
2. `Invitation pending`
3. `Not in Space`

The search placeholder is `Search space members`, not `Search room members`.
Search spans all three sections and matches display label, local alias, and
user ID.

Each `Not in Space` row shows the child rooms in which the user participates,
using compact room labels or a count when the set is large.

### Invite action

Each `Not in Space` row has an inline `Invite to Space` button. The same command
is available from the row context menu.

- Hide or disable the action when the current user cannot invite to the Space.
- Disable duplicate submission while an operation is pending.
- On accepted submission, show `Invitation pending` immediately.
- Keep the user in the pending section until Space `JOIN` is observed.
- On failure, return the row to `Not in Space` and show the existing operation
  failure treatment.
- If the server reports that the user is already invited or joined, reconcile
  to the authoritative membership state instead of treating it as a hard error.

## Diagnostics

Diagnostics are private-data-free. They may contain only kinds, booleans,
counts, positions, coarse freshness classes, generations, and anonymous
operation tokens. They must never contain user IDs, room IDs, display names,
message text, MXC URIs, raw SDK errors, or secrets.

### SDK projection

Source: `sdk.space_members_scope`

Record:

- joined Space member count;
- invited Space user count;
- child room count;
- complete and incomplete child-room counts;
- deduplicated child-room joined-user count;
- child-room-only count;
- membership filter used for each set;
- local member-store lookup success/failure count;
- whether any network member sync was attempted.

### Core classification and cache

Sources: `core.space_members_projection` and `core.profile_resolution`

Record:

- projection generation and trigger kind;
- input and output counts for each membership class;
- deduplication count;
- stale-generation discard count;
- profile resolution counts by source: alias, relevant room, Space room,
  payload, global cache, locally stored homeserver profile, unresolved;
- cache hit, miss, and stale-hit counts;
- incomplete projection boolean;
- post-invite membership class and operation outcome.

### UI

Source: `ui.space_members_panel`

Record:

- open trigger;
- rendered section counts;
- search active boolean and result count;
- inline versus context-menu invite trigger;
- invite button availability reason;
- whether an incomplete-sync notice is visible.

## Failure and incomplete-state handling

- A failure to load one child room does not erase the last valid projection.
- The panel shows `Some child rooms are still syncing` when completeness is
  false.
- A missing profile does not remove a person from the membership audit.
- An unresolved profile renders `Unknown user` while preserving the actionable
  invite command.
- Cache corruption or unreadable local state falls back to empty cache and is
  reported only as a sanitized outcome.
- The panel remains usable offline from locally stored membership state.

## Performance

- Do not scan all child rooms from React or on every render.
- Coalesce membership changes and publish one generation of the projection.
- Use local SDK-store reads; do not issue one homeserver profile request per
  person.
- Deduplicate by user ID before profile resolution.
- Virtualize the final member list using the existing People panel pattern.
- Cache room labels and profile-resolution results for the projection
  generation.

## Testing

### SDK and core

- `ACTIVE` is not used when a joined-only Space set is required.
- Space `JOIN` and `INVITE` users are classified separately.
- Child-room union deduplicates users present in multiple rooms.
- Space joined/invited users are subtracted from child-room-only.
- `LEAVE`, `BAN`, and `KNOCK` users are excluded.
- Incomplete child rooms set the completeness flag without dropping known data.
- Late generations cannot replace the active Space projection.
- Profile precedence is room-first and cache-last.
- Local aliases override all remote/cache labels.
- Cache miss performs local-only room-store fallback.
- Invite requested, succeeded, already-invited, joined, and failed transitions
  produce the correct classification.
- Diagnostic events contain no raw Matrix identifiers or profile content.

### Desktop UI

- The Space Members entry appears above `DMs` / `Rooms` only in a Space.
- Counts show joined and child-room-only values separately.
- Sections render in the approved order.
- Search spans all sections and uses `Search space members`.
- Child-room-only rows show child-room context.
- Inline and context-menu invite actions dispatch the same command.
- Pending and permission-disabled states are accessible.
- Incomplete-sync notice reflects Rust-owned completeness.
- Room People remains room-scoped and unchanged.

### Integrated verification

- A user joined only to a child room appears under `Not in Space`.
- Inviting that user moves them to `Invitation pending`.
- Their eventual Space join moves them to `Space members`.
- A child-room receipt with no embedded display label resolves from the child
  room or local profile cache without requiring Space membership.
- Restart/offline behavior resolves previously stored room profiles from the
  encrypted SDK store.

## Local implementation and delegation

Implementation is local to the current development worktree. Do not push or
open a pull request unless separately requested.

The implementation worker is Luna (`gpt-5.6-luna`) with reasoning effort
`max`. The coordinating agent owns specification review, integration review,
privacy review, and final verification.
