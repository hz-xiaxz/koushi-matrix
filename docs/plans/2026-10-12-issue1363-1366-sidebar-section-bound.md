# #1363 / #1366 - Sidebar section row bound with an expand control

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Sidebar Sections And Low Priority" (row bound subsection added by this change).
Issues: #1363 (DMs and the shared Rust mechanism), #1366 (Rooms acceptance).

## Problem

Every visible sidebar section rendered all of its rows. With dozens of DMs the
DM list pushed Low priority and not-joined below the fold, and the same happened
to DMs when a Space carried a long Rooms section. Both issues ask for the first
five rows plus a full-width control that reveals the rest and bounds it again.

## Design

One mechanism serves both issues. Whichever issue landed first implements it
once; the other only adds its section-specific acceptance.

1. **Persisted preference.** `SidebarSectionSettings` gains `expanded: bool`
   (default `false`), stored per scope x section next to `collapsed` and `sort`,
   and `SidebarSectionPatch` gains `expanded: Option<bool>`. A scope key is
   `__home__` or the Space id (`sidebar_scope_key`), and
   `SidebarSectionKind::scope_field` reads the section's entry so the reducer
   never re-implements the mapping. An older settings file without the field
   stays bounded.
2. **Rust projection.** `SidebarModel.section_bounds: SidebarSectionBounds`
   carries `rooms`, `dms`, and `low_priority` bounds.
   `SidebarSectionBound::project(total_count, persisted_expanded,
   active_room_index)` produces `visible_limit = 5`, `visible_count`,
   `hidden_count`, `expanded`, `expanded_by_active_room`, and
   `shows_expand_control`. The `SIDEBAR_SECTION_DEFAULT_VISIBLE_COUNT`
   constant is the single bound value.
3. **The open conversation is never hidden.** `expanded` is
   `persisted_expanded || active_room_index >= visible_limit`, and
   `expanded_by_active_room` marks the forced case. `shows_expand_control` is
   false while it is forced, because bounding the section again could not change
   what is visible, so the control would be inert.
4. **Collapsed-vs-bounded.** Collapse stays a separate axis: a collapsed section
   renders no rows and no control, and the bound control changes only `expanded`
   while the section toggle changes only `collapsed`. Every combination has a
   defined rendering (collapsed hides rows; otherwise `expanded` selects five
   rows or all of them).
5. **Reveal on entry.** #1366 requirement 8 covers entry from a notification or
   permalink into a scope whose section was collapsed. The bound rule above
   already keeps a bounded-open conversation visible, so the collapsed dimension
   needs a transition: `NavigationState.revealed_sidebar_section:
   Option<SidebarSectionReveal>` records the scope and section a settled
   conversation selection revealed. It is session state (`serde(skip)`, so it
   never enters the encrypted navigation payload), the projection applies it
   only while the open conversation is in that section and only in the scope it
   was recorded for, and the user's own collapse choice for the same scope x
   section clears it. The bound toggle is deliberately *not* part of that: it is
   the other dimension, so "Show more" on a revealed section with a stored
   collapse preference shows more rows instead of re-collapsing the section. A
   restart therefore honors the persisted preference rather than replaying a
   reveal.
6. **Renderer.** React slices the matching `sections` list with the Rust
   `visible_count` and renders `SectionBoundControl` only when Rust set
   `shows_expand_control`. The control is a normal button with `aria-expanded`,
   `aria-controls` on the section's row list, and an accessible name that names
   the action, the hidden count, and the section. The header count stays the
   pre-truncation list length, and the Rooms/DMs headers keep the Rust-owned
   whole-section unread badge.
7. **Filter exception.** The search box is renderer-owned, so a non-empty filter
   renders every matching row and hides the control; clearing it restores the
   bound. Ordering and truncation stay in Rust.
8. **Scope of the bound.** `sections.rooms`, `sections.people`, and
   `sections.low_priority` are bounded. `sections.favourites` is a derived
   convenience list, not a visible section, and not-joined keeps its current
   behavior until it has its own issue, per both issue recommendations.

## Upstream comparison

#1363 records that Element Web's room list V3 does not truncate sections (it
virtualizes with `react-virtuoso` and keeps per-section expansion state) and
that Element X relies on list scrolling. Truncating a section with an explicit
expand control is therefore a deliberate divergence, chosen for scanability with
dozens of DMs. This plan is the recorded divergence. Koushi keeps the two axes
Element Web does not separate: a persisted per-scope bound (`expanded`) next to
the existing per-scope collapse, so a bounded section can also be collapsed.

## Fake agreement

The browser harness and the browser-fake snapshots must not compute a bound.
`crates/koushi-state/tests/sidebar_section_bound.rs` writes
`apps/desktop/src/test/sidebarSectionBounds.generated.json` for totals 0..=8,
both persisted-`expanded` values, and a set of active-row positions, and asserts
it against `SidebarSectionBound::project`. `apps/desktop/src/test/
sidebarSectionBoundsFixture.ts` only looks those cases up, the harness attaches
the looked-up bounds in one place, and
`apps/desktop/src/test/sidebarSectionBoundsFixture.test.ts` proves every
generated case resolves to its own bound and that the default mock snapshot
carries the generated numbers. The harness stores the user's settings patch and
looks bounds up; it does not replay the selection reveal, which is a reducer
transition covered headlessly, because a fake that replayed it would be
re-implementing reducer semantics. A shape the fixture does not cover renders
every row, and the renderer also ignores a bound whose `total_count` disagrees
with the list it is handed (the failure mode of a stale fake snapshot is showing
rows, not hiding conversations).

## Verification

- `cargo test -p koushi-state --test sidebar_section_bound` covers the bound,
  the hidden count, per-scope x per-section independence, the active-room
  override, the whole-section totals, the reveal transition and its clearing,
  and a synthetic newest-first order that the bound cannot mask.
- `apps/desktop/src/components/Sidebar.sectionBound.test.tsx` covers the
  renderer: five rows plus a control naming the hidden count, the whole-section
  header count and unread badge, both control directions dispatching `expanded`,
  a bound whose `visible_count` disagrees with the derivation rule (so a
  re-derived limit would fail), `aria-controls` resolving to the row list, no
  control at or below the bound, independent DMs and Low priority bounds, the
  filter exception, the collapsed combinations, and the forced-open case.
- `apps/desktop/e2e/sidebar-section-bound.spec.ts` covers the path in the
  browser tier: the fake attaches the Rust-authored bound, five of seven rows
  render, the control names the hidden count, pressing it dispatches one typed
  `expanded` patch and shows every row, and the filter shows every match with no
  control.
- `apps/desktop/src/test/withSectionBounds.test.ts` covers the fake's own
  helper, including the partial signed-out snapshot that carries no sidebar
  projection.
- The DTO golden (`apps/desktop/src-tauri/tests/golden/frontend_app_state.json`)
  gained the projected `section_bounds` and the persisted `expanded` values, and
  `SNAPSHOT_SCHEMA_VERSION` moved to 9 with the core-event contract artifact.

## Not in this change

Not-joined section bounds, any second truncation path in React, and moving the
filter exception into Rust. #542/#1364 ordering remains independent: the bound is
applied after sorting and its tests use known timestamps.
