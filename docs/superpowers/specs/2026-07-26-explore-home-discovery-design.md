# Explore as the Home-only discovery surface (issue #330)

- Date: 2026-07-26
- Issue: #330
- Status: approved

## Problem

Three navigation problems share one cause: the sidebar mixes account-global
actions with space-scoped ones, so the scope of an action is not visible from
where it lives.

1. Joining by address works but is invisible. `resolveDirectorySubmission`
   already classifies `#alias:server`, `!roomid:server`, and `matrix.to` links
   into a preview target, but the only way to reach it is to paste into a field
   labelled "Search public rooms".
2. `Explore` and `Invites` render in every sidebar, including a selected
   space's, which makes account-global actions look space-scoped.
3. `Directory server` does not say what it does. Blank already means the
   logged-in homeserver, but nothing on screen says so.

## What the code actually does today

The issue's premises differ from `main` in two ways that change the scope, so
they are recorded here.

**Navigation is close to the inverse of the proposal.** `Shell.tsx` renders
`accountHomeActive ? Activity : Threads`, then `Explore` and `Invites`
unconditionally. So Home has Activity/Explore/Invites and a space has
Threads/Explore/Invites. Home has no `Threads` entry at all.

**`Threads` is room-scoped, not space-scoped and not account-wide.**
`onOpenThreads` reads `navigation.active_room_id` and calls
`openThreadsList(roomId)`; `ThreadsListState` is keyed by `room_id`. The entry
shows the *selected room's* threads and silently does nothing when no room is
selected. There is no account-wide or space-wide thread aggregation in Rust.

The issue's Threads acceptance criteria therefore describe new backend
functionality, not a filter over existing data.

**The room header already has a Threads button** (`panes.tsx`, third of four
icon buttons) that opens the same room-scoped list — but only when
`showThreadsHeader` is true, i.e. only when the room has unread thread
attention. With zero thread activity the sidebar entry is the only way in.

## Decisions

**Thread aggregation is out of scope.** Account-wide and space-scoped thread
lists move to a follow-up issue, and #330's Threads criteria move with them.
This spec does not ship a misleading `Threads` entry in the meantime.

**The room-header Threads button becomes unconditional**, and the sidebar
`Threads` entry is removed. The header is where "this room" is already implied,
so the entry point explains its own scope without label copy. Removing the
`showThreadsHeader` condition preserves today's ability to browse a room's
threads when nothing is unread; the attention counts keep rendering as badges
when non-zero, so no Rust contract changes.

**Space sidebars keep no navigation entries.** After `Explore`, `Invites`, and
`Threads` move out, a space sidebar is the room list for that space. Space-scoped
actions already live in the sidebar header icons (space info, new DM, create
room), so nothing is lost. Mixing account-global and space-scoped entries was
the problem; separating them is the fix.

**The Home rail badge total is Rust-owned.** The issue asks for
`unread + invites` combined "only in the rail presentation", but canon requires
rail badges to render a value `compose_sidebar` produced. Both hold if the
*separation of meaning* is achieved with separate fields and the *total* is
computed in Rust. React renders the total and passes the two counts separately
to the accessible label.

**Explore keeps its Rust state machine.** Splitting the form is a presentation
change. `resolveDirectorySubmission` stays the single classifier for both
fields, and directory query/preview/join keep their existing Rust-owned
substates.

## Phase A — Rust

`AccountHomeItem` gains two fields:

```rust
pub struct AccountHomeItem {
    pub display_name: String,
    pub unread_count: u64,     // unchanged meaning: unread messages
    pub highlight_count: u64,  // unchanged
    pub invite_count: u64,     // pending invites for the account
    pub attention_count: u64,  // unread_count + invite_count
    pub is_active: bool,
}
```

`compose_sidebar_with_room_notification_settings` takes the pending invite count
(`AppState.invites.len()`). The three-argument `compose_sidebar` wrapper keeps
its signature and passes `0`, so its ten-odd test call sites do not churn.

`SpaceRailItem` is untouched: space rail badges stay unread-only. Muted rooms
are already excluded from `unread_count`; invites are account-level and are not
filtered by room notification settings.

Mirrors updated in the same change, per canon: `apps/desktop/src-tauri/src/dto.rs`,
`apps/desktop/src/domain/types.ts`, `browserFakeApi.ts`, `tauriIpcMock.ts`,
`appHarnessMain.tsx`, `desktopModel.ts`, and the
`frontend_app_state.json` golden.

## Phase B — GUI

### Navigation

`Shell.tsx` gates `Explore` and `Invites` on `accountHomeActive` and drops the
space `Threads` `NavButton`. `panes.tsx` drops the `showThreadsHeader`
condition.

| entry | Home | Space |
| --- | --- | --- |
| Activity | yes | no |
| Explore | yes | no |
| Invites | yes | no |
| Threads | no (room header) | no (room header) |

### Explore

Two sections replace the single overloaded form.

```
main[aria-label="Explore"]
  section: join a room or space
    input   — Matrix address or link       ("#room:server" or a matrix.to link)
    button  — Preview
    helper  — room aliases, room IDs, and matrix.to links are supported
    notice  — user-ID paste, or input that is not an address
  section: search public rooms and spaces
    input   — search term
    input   — search on server             (placeholder: your server)
    helper  — searches the selected server's public directory only
    button  — Search
  section: results                         (Room / Space badge on every row)
```

Submission routing, both fields through `resolveDirectorySubmission`:

| field | classification | action |
| --- | --- | --- |
| address | `join` | `previewJoinTarget(roomIdOrAlias, viaServers)` |
| address | `user` | notice: this is a user ID, start a DM from New DM |
| address | `search` | notice: this does not look like an address |
| search | `search` | `queryDirectory({ term, server_name })` |
| search | `join` | `previewJoinTarget(...)` — forgiving paste |
| search | `user` | same notice as the address field |

Joining never happens directly: both paths land in the existing preview dialog
and join only on confirmation.

Accessible names are made distinct. Today the search input and its submit button
share the name "Search public rooms", and WebDriver only tells them apart by tag
name. The inputs become "search term" / "search on server" / "Matrix address or
link" and the buttons take their visible text, "Search" and "Preview".

### Copy

`directory.searchServer` becomes "Search on server" and its placeholder says
"Your server", making the blank-means-homeserver behaviour visible. Behaviour is
unchanged: `server_name` is already `null` when the field is blank.

New keys cover the two section titles, the address label/placeholder/helper, the
preview button, the two notices, the search-term label, the server helper, and a
`Room` badge to sit beside the existing `Space` badge. English and Japanese
catalogs and `messages.test.ts` are updated together.

### Home rail badge

`Shell.tsx` renders `data-count={account_home.attention_count || undefined}`. The
accessible label goes through the catalog with both counts as parameters, e.g.
"Home, 12 unread messages, 2 invites", rather than concatenating Rust's
`display_name`. Space rail buttons are unchanged.

## Verification

Build each check to fail first, then make it pass.

**Phase A** — `cargo test -p koushi-state --test navigation_state`: invites raise
`invite_count` and `attention_count` without changing `unread_count`; a muted
room adds nothing to unread while invites still count; `space_rail` unread
excludes invites. `cargo test -p koushi-desktop` covers DTO serialization, and
the golden is regenerated with `UPDATE_GOLDEN=1`.

**Phase B** — Playwright (a CI gate):

- Home shows Explore and Invites; a selected space shows neither.
- A selected space shows no Threads nav entry.
- The room-header Threads button is visible with zero thread attention.
- Address field with `#alias:server` invokes `preview_join_target` and never
  `join_directory_room`.
- Address field with `@user:server` shows the notice and invokes nothing.
- Search field with an ordinary term invokes `query_directory` with
  `serverName: null` when the server field is blank.
- Search field with a full address routes to preview, not `query_directory`.
- The Home rail badge shows the total and its label exposes both counts
  separately; the space rail badge excludes invites.

**Vitest** — catalog parity in `messages.test.ts`.

**Not run here** — the `--scenario=local-explore` Linux virtual-display lane
needs a local Conduit or Tuwunel binary, and neither exists on this machine. Its
selectors are updated in this change (`input[aria-label="Search public rooms"]`
and the matching button are both renamed), but the lane is not executed, and no
evidence is claimed from it.

## Out of scope

- Account-wide and space-scoped thread aggregation (follow-up issue).
- Invite lifecycle semantics. `AppState.invites` stays the Rust-owned global
  list; this change only counts it.
- Space rail invite badges: `InvitePreview` carries no reliable parent-space
  scope, so a per-space invite count cannot be derived.
- Moving `Activity` anywhere. It is account-global and already Home-only, which
  is what this spec's principle requires.
