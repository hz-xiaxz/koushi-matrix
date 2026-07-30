# Design: Room People Projection and People-Facing Labels (#380, #355, #374)

Status: approved in chat on 2026-07-30
Issues:

- https://github.com/shinaoka/koushi-matrix/issues/380
- https://github.com/shinaoka/koushi-matrix/issues/355
- https://github.com/shinaoka/koushi-matrix/issues/374

## Goal

Resolve three related people-facing presentation defects in one PR:

1. remove the redundant visual tooltip from the Home rail button while
   preserving its accessible name and attention state;
2. prevent normal people-facing surfaces from using raw Matrix user IDs as
   primary labels when a local alias or room/member display name is available;
3. replace account-global mention autocomplete with a Rust-owned,
   room-scoped, joined-member-only query.

The shared boundary is room/member identity projection. Rust owns membership
eligibility, label precedence, Unicode/CJK matching, candidate ranking,
completeness, permission facts, and stale-result fencing. React owns only
popover visibility, focus, active-option presentation, and composer draft
input.

## Upstream Baseline

Element Web's `UserProvider` builds autocomplete from the current room. At
commit `27906ce6e7a5edb6d652468415f5b24b094eb032`, it uses
`room.getJoinedMembers()` but also appends invited members. Koushi deliberately
does not copy the invited-member behavior because #374 requires joined-only
eligibility.

Element X Android's `SuggestionsProcessor` at commit
`52d146a59d5cde9fb0400f0281a1b52921f27b58` consumes the active room's
`RoomMembersState`, filters `membership == JOIN`, matches room member display
names and Matrix IDs, and treats `@room` separately through a permission check.
That ownership and eligibility model is the closer baseline for Koushi.

Koushi intentionally differs from both clients in these respects:

- the current account may appear as a joined candidate, as required by #374;
- local aliases affect visible labels and search keys but never eligibility;
- Unicode/CJK normalization and deterministic ranking stay in Rust;
- incomplete lazy-loaded membership fails closed and is represented explicitly
  instead of falling back to an account-global profile cache.

## Existing Defects

### Account-global mention candidates

`selectMentionCandidates` reads `AppState.profile.users`, sorts it with
JavaScript `localeCompare`, and appends `@room`. It receives no room ID and
cannot distinguish joined members from invitees, departed members, or people
known only through another room.

The existing `ProfileState.users` population is not an authoritative room
member directory. The room-list snapshot primarily adds DM counterpart
profiles. Even when a user profile is known, that says nothing about the user's
membership in the active composer room.

### Lost room-specific display labels

The SDK timeline conversion already obtains a room-specific sender profile.
`sdk_item_to_timeline_item_with_send_states` stores that display name in
`TimelineItem.sender_label`, but
`project_timeline_item_display_labels` later overwrites the field using the
account-global profile resolver. This can discard the better room-specific
label.

Reaction aggregates retain only sender IDs. The GUI joins those IDs against
`ProfileState.users` and falls back to the raw ID when the profile is absent.
Several other GUI surfaces also use `sender_label ?? sender`, making an
identity field a normal display label.

### Home tooltip focus behavior

`WorkspaceRail` wraps the Home button in the shared `Tooltip`. The tooltip opens
on focus, and pointer activation focuses the button, so the visual `Home`
bubble can remain after selection. The button already has a complete dynamic
`aria-label`; Space buttons still require their display-name tooltips.

## Canon Amendment

Implementation changes the normative mention contract. Amend
`docs/policies/engineering-rules.md`,
`docs/architecture/overview.md`, and
`docs/architecture/state-machine.md` before production code:

- `ProfileState.users` remains an account-scoped profile cache and is not
  membership evidence.
- Mention candidates come from a room-keyed Rust projection containing only
  users proven to have `join` membership in that room.
- React does not filter membership, normalize queries, rank candidates, or
  append `@room`.
- Raw Matrix user IDs remain identity fields and may be shown as secondary
  account/detail text, but are not normal primary labels.
- A missing friendly label is represented as missing data; React renders a
  localized `Unknown user` label rather than promoting the raw ID.

The state-machine document gains the request, partial projection, refresh,
success, failure, stale completion, room switch, logout, and account-switch
transitions described below.

## Rust State Contract

Add a domain slice named `mention_candidates` to `AppState`. It contains a
bounded collection keyed logically by `(room_id, surface)`. A collection is
used instead of a JSON object with a compound key so the wire shape stays
explicit and portable. At most one entry exists for each room/surface pair;
main and thread composers in the same room may therefore hold independent
queries without overwriting one another. The collection retains only current
composer targets and a small fixed number of recently queried targets; it is
cleared on logout, account switch, lock, and session reset.

Each target entry contains:

```text
room_id
generation
request_id
query
surface=main|thread
completeness=loading|partial|complete|failed
candidates=[MentionCandidate]
room_mention_allowed=allowed|denied|unknown
failure_kind?
```

`MentionCandidate` contains:

```text
user_id
display_label?
original_display_label?
avatar
membership=joined
```

The raw `user_id` is retained for mention intent and may be secondary identity
text. `display_label` is local alias, then room member display name, then known
profile/own display name. It is absent when no friendly value is known.
`original_display_label` excludes the local alias. React renders a localized
unknown-user label when `display_label` is absent.

Normalized match terms stay inside the Rust query service rather than crossing
the WebView boundary. Candidate results are already matched and ranked by
Rust; React does not compare match terms.

## Query and Refresh State Machine

Add a typed command for mention candidate demand:

```text
QueryMentionCandidates {
  request_id,
  account_key,
  room_id,
  surface,
  query
}
```

The core path is:

1. validate the active account/session and room;
2. increment the target projection generation and record the exact request,
   room, surface, and query;
3. read `members_no_sync(JOIN)` and project the known joined subset;
4. inspect `are_members_synced()`:
   - if true, publish `complete`;
   - if false, publish `partial`/`loading`, then run the SDK's deduplicated
     `members(JOIN)` refresh;
5. publish the refreshed complete result or a coarse failed state;
6. ignore any completion whose account, room, request, generation, surface, or
   query no longer matches.

The partial result is fail-closed: it contains only members already proven
joined. It never adds `ProfileState.users` entries to eligibility.

Room membership updates invalidate the RoomActor's internal room member
directory and refresh every demanded target entry for that room using the
existing base-room update observation. A join or leave can therefore update an
open main and/or thread popup without restarting the app. Each recomputation
uses a new target generation and its own latest recorded query. Room changes
immediately select the new `(room_id, surface)` entry, so a late result from
the old room cannot appear in the new composer.

The SDK already exposes `members_no_sync(JOIN)`, `are_members_synced()`, and
`members(JOIN)`. No SDK patch is planned. If implementation uncovers a missing
public primitive, the separately approved SDK-change workflow applies:
minimal upstreamable patch, deterministic RED/GREEN SDK test,
`docs/upstream/matrix-rust-sdk-feedback.md`, and submodule gitlink update.

## Matching and Ranking

Rust performs case/normalization-insensitive substring matching over:

- local alias;
- room-scoped member display name;
- known original/profile display name;
- full Matrix user ID and localpart.

It reuses the existing Unicode/CJK normalization policy. It does not use fuzzy
or edit-distance matching.

Ranking is deterministic:

1. exact normalized match;
2. token or prefix match;
3. general substring match;
4. existing Rust-owned collation/display order;
5. user ID as the final stable identity tie-breaker.

This preserves matches such as `Hiroshi Shinaoka` by either name component and
Japanese full names by surname or given-name substring regardless of display
order. A strong textual match never makes a non-member eligible.

`@room` is a synthetic result, not a member. Rust includes it only for a room
composer when the current power-level projection permits room notification.
Unknown permission does not become allowed. Its result remains separate from
the joined-member list.

## People-Facing Label Policy

Add a shared Rust resolver for primary people-facing labels that returns an
optional friendly label:

```text
local alias
  ?? room/upstream member display name
  ?? account profile display name
  ?? own profile display name
  ?? none
```

Keep the existing identity-oriented resolver for places where an MXID fallback
is deliberately part of account/detail UI. Do not globally change every
identity label into `Unknown user`.

Apply the friendly resolver or an equivalent producer-owned room label to:

- timeline message/emote senders;
- reply quote senders;
- thread summary and Threads list senders;
- reaction sender previews;
- typing users;
- Files, media-gallery, pinned-event, and related sender metadata;
- read-receipt display DTOs without regressing their existing Rust-owned
  projection.

Specific transport changes are purpose-built:

- preserve the SDK-projected room sender label when reapplying aliases;
- replace reaction sender-ID-only previews with structured previews carrying
  identity plus an optional Rust-projected display label;
- project typing users as structured Rust-owned display entries;
- add optional `sender_label` fields to list/result DTOs that currently carry
  only `sender`;
- refresh already loaded timeline/thread/list labels from Rust-owned patches
  after local alias or profile changes.

React renders `display_label`/`sender_label` when present and localized
`Unknown user` otherwise. It must not use `?? user_id`, `?? sender`, or a
profile-map join for primary display text. Raw IDs remain available for:

- account/session identity;
- explicit profile detail secondary text and copy/verification;
- source/debug/diagnostic views;
- identity-entry/search flows where the ID itself is the subject.

## Frontend and DTO Boundary

Mirror the new state and label fields through all hand-maintained boundaries in
one change:

- `apps/desktop/src-tauri/src/dto.rs`;
- `apps/desktop/src/domain/types.ts`;
- `apps/desktop/src/domain/coreEvents.ts`;
- `apps/desktop/src/domain/coreEvents.generated.json`;
- `apps/desktop/src/backend/browserFakeApi.ts`;
- `apps/desktop/src/test/tauriIpcMock.ts`;
- `apps/desktop/src/test/appHarnessMain.tsx`;
- `apps/desktop/src-tauri/tests/golden/frontend_app_state.json`.

The maximally populated golden includes independent main and thread entries,
partial/complete state, a candidate with a friendly label/avatar, a candidate
without a friendly label, and a room-mention permission value. Empty
collections are insufficient proof.

Replace `selectMentionCandidates(state)` with a room/surface/query projection
selector. It returns the exact Rust result for the current target and preserves
reference identity when unrelated state slices change. The composer dispatches
typed query demand when the active `@` token or target changes. It may render a
loading indicator for `loading`/`partial`, but it cannot add global profiles or
locally reorder results.

## Home Tooltip

Remove only the Home button's `Tooltip` wrapper and tooltip trigger props.
Preserve:

- `accountHomeLabel(...)` as its dynamic `aria-label`;
- active styling and click behavior;
- attention badge/count;
- native button keyboard focus and visible focus ring.

Do not add a `title` attribute. Keep Space tooltips, timeline action tooltips,
and the shared `Tooltip` component unchanged.

## Diagnostics and Privacy

Mention lifecycle diagnostics use source `mention.candidates` and only these
private-data-free facts:

```text
stage=requested surface=main|thread
stage=projected completeness=complete|partial candidate_count=N
stage=member_refresh_started
stage=member_refresh_settled outcome=success|failed candidate_count=N
stage=stale_result_ignored
```

Never log room IDs, user IDs, queries, labels, aliases, avatar URIs, event IDs,
or raw SDK errors. Public DTO `Debug` implementations expose only kinds,
booleans, generation/request facts safe under repository rules, and counts.

## Verification

All behavior changes use RED-first checks.

### Rust state/reducer

- joined members included; invited, knocked, left, banned, and global-only
  profiles excluded;
- partial membership publishes only the known joined subset;
- room/surface/query/request/generation stale completions ignored;
- switching rooms immediately changes scope;
- local aliases change labels/search terms but not eligibility;
- exact, token/prefix, substring, Matrix localpart, and CJK normalization
  ranking;
- `@room` allowed, denied, and unknown permission;
- logout/account switch/session clear removes projections;
- missing friendly labels remain `None`, never raw-ID primary labels.

### SDK/core

- no-sync partial projection does not make a network request;
- incomplete membership starts one deduplicated refresh;
- complete membership skips refresh;
- membership update refresh removes departed members and adds new joined
  members;
- late room-A refresh cannot settle room B;
- timeline room-specific sender labels survive profile projection;
- aliases override room labels;
- reaction, typing, thread, file/media, pinned, and receipt display projections
  use friendly labels or missing-label state;
- local Conduit/Tuwunel headless QA proves ordinary room, two-person DM, group
  DM, membership change, and structured `m.mentions.user_ids`.

### Tauri/TypeScript contracts

- maximally populated frontend golden;
- CoreEvent checked-in contract artifact;
- state-delta slice serialization/application;
- selector reference stability for unrelated state changes;
- typecheck and IPC mocks cover the new shape.

### Browser headless

- ordinary room excludes a matching non-member;
- two-person DM and group DM show only joined members;
- main and thread composers consume the same room-scoped source;
- room switching and stale completion cannot contaminate the popup;
- partial/loading UI never falls back to global profiles;
- reaction known/missing-profile tooltip uses friendly/unknown labels;
- at least one thread/list and one file/media/pinned surface avoid raw IDs;
- read-receipt display labels remain correct;
- Home has no tooltip on hover, pointer focus, or keyboard focus;
- Home accessible naming, focus, selection, badge, Space tooltips, and timeline
  tooltips remain intact.

No manual or visual GUI inspection is acceptance evidence.

## Commit and PR Shape

Use one branch and one non-squash PR for #380, #355, and #374. Keep commits
reviewable:

1. approved canon and state-machine contract;
2. Rust mention directory/query state and tests;
3. core/SDK room member refresh and headless evidence;
4. people-facing label transport and tests;
5. DTO/frontend mention wiring and browser tests;
6. Home tooltip regression fix and component tests;
7. integrated verification and documentation.

The PR closes all three issues only after every issue-specific acceptance check
is green.

## Out of Scope

- Using account-global profile presence as room-membership evidence.
- Inviting users or changing Matrix membership from autocomplete.
- Fuzzy/edit-distance person search.
- Showing a visual Home tooltip through another component or native `title`.
- Removing Space or timeline action tooltips.
- Hiding raw Matrix IDs from explicit account, profile-detail, source, or debug
  views where identity is intentionally displayed.
- A repository-wide generic person DTO migration unrelated to the named
  surfaces.
