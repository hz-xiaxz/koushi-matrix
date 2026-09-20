# #955 — Low priority section restoration and Rooms / DMs unread badges

Date: 2026-09-20. Status: implemented.
Base: `origin/main` = `3037e5756f5ef6583a7d7a04b03ceacb9ca93baa`.

Restores the Low priority sidebar section that
[2026-09-19-sidebar-sections-design.md](../specs/2026-09-19-sidebar-sections-design.md)
§3.1 removed, excludes low priority from every notification and unread
aggregate, and replaces the Rooms / DMs conversation-count meta with the
Rust-owned unread badge.

## Canon consulted and amended

- [state-machine.md](../../architecture/state-machine.md): new
  "Sidebar Sections And Low Priority" subsection under Room Tags; the Native
  Attention persistent-badge bullet now excludes low priority (ignored-user DMs
  still contribute their raw count); Settings records the
  `SidebarScopeSettings.low_priority` fallback.
- [overview.md](../../architecture/overview.md): the sidebar-projection
  paragraph points at that contract.
- The 2026-09-19 design spec is annotated as superseded in part.
- [rooms-and-spaces.md](../../help/rooms-and-spaces.md) gains a
  "Conversation list sections" section.

## Ownership

Rust owns the section split, the aggregates, and the collapse preference.
React filters the projected sections by the search box and renders the numbers.

- `koushi-state/src/sidebar.rs` — `sections.rooms` (non-low-priority rooms,
  favourites included), `sections.people` (non-low-priority DMs),
  `sections.low_priority` (in-scope low-priority rooms and DMs in the Rooms
  order), `low_priority_collapsed`, and `contributes_attention` gating the
  Home / Space rail and Rooms / DMs aggregates.
- `koushi-state/src/state/native_attention.rs` — low priority is skipped before
  the persistent Dock badge accumulates; ignored-user DMs keep their raw count.
- `koushi-state/src/state/settings.rs` — `SidebarSectionKind::LowPriority` and
  the optional `SidebarScopeSettings.low_priority`, resolved from the legacy
  `SidebarCollapsedSections.low_priority` flag and the scope's Rooms sort.
- `apps/desktop/src/components/Shell.tsx` — renders the three Rust sections,
  restores the Low priority heading and collapse, and shows
  `space_unread_count` / `dm_unread_count` as `.section-unread-count`.

Deliberately unchanged: the low-priority room's own raw counts and read
receipts, the Activity exclusion, invite counts, mute semantics, and the
absence of an independent Low priority sort.

## Verification

| Check | Command |
| --- | --- |
| Section split, aggregates, native badge, legacy collapse | `cargo test -p koushi-state --test sidebar_low_priority` |
| Updated section/attention expectations | `cargo test -p koushi-state` |
| Delta republication for collapse and tag changes | `cargo test -p koushi-core --lib sidebar_preferences` |
| DTO golden (populated with a low-priority room and a scoped preference) | `UPDATE_GOLDEN=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden`, then rerun without it |
| Workspace | `cargo test --workspace` |
| Frontend | `npm --prefix apps/desktop run typecheck`, `npm --prefix apps/desktop test -- --run`, `npm --prefix apps/desktop run lint`, `npm --prefix apps/desktop run build`, `npm --prefix apps/desktop run qa:secret-scan` |
| Browser | `npx playwright test` from `apps/desktop` |
| Docs | `node scripts/check-agents-docs.mjs`, `node scripts/user-help.mjs --check` |
| README screenshot | `npm --prefix apps/desktop run docs:screenshot` |

RED evidence: reverting only the behavioral parts of `sidebar.rs` and
`native_attention.rs` while keeping the new fields fails 7 of the 12
`sidebar_low_priority` tests; restoring them turns all 12 green.
