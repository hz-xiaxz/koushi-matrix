# Sidebar Room Name Filter Design

**Issue:** #423

## Goal

Add an IME-safe, presentation-only name filter below the sidebar category and
sort controls. It narrows only the visible DM or room rows without changing
Rust-owned room state, category totals, unread counts, or the selected sort.

## Ownership And Scope

`Sidebar` owns the query as ephemeral React presentation state. Matching uses
the already projected `RoomListItem.display_name`; it does not inspect Matrix
IDs, aliases, members, or event content and does not issue a network request.
The existing `sortedSidebarRooms` result is filtered, preserving its order.

No Rust, Tauri, Matrix SDK, Sliding Sync, DTO, or persistence changes are
required. The query is deliberately not persisted.

## UI And State Flow

`RoomListControls` receives the current query, category-specific labels, and
change/clear handlers. Its text surface uses the shared IME-safe text input
primitive from `ImeTextControl.tsx`, with a search icon, accessible clear
button, and Escape-to-clear behavior.

`Sidebar` trims the query for matching and applies case-insensitive substring
matching to `display_name`. An empty normalized query returns the original
sorted array. Switching between DMs and Rooms or changing
`snapshot.state.ui.navigation.active_space_id` clears the query. Totals and
badges continue to use the unfiltered snapshot values.

When the selected category has rows but the filtered result is empty,
`RoomSection` receives a distinct localized no-match state rather than the
normal empty-category copy. English and Japanese catalog entries cover the
placeholder, accessible name, clear action, and no-match state.

## Verification

Extend `Shell.test.tsx` before implementation to reproduce and cover matching,
case folding, whitespace trimming, preserved sorting, clear/Escape, distinct
empty state, category switching, and active-space switching. Extend the i18n
catalog test and run the repository IME inventory and `ImeTextControl` gates.
No browser or native GUI lane is needed unless the focused component test
cannot prove one of these DOM-visible contracts.
