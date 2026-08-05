# Sidebar Room Name Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an IME-safe local name filter for the selected DM/room sidebar list.

**Architecture:** `Sidebar` keeps an ephemeral query, filters the existing sorted `display_name` projection, and passes unchanged snapshot totals to `RoomListControls`. No Rust or transport changes.

**Tech Stack:** React, TypeScript, Vitest/Testing Library, shared IME controls, existing i18n catalogs.

## Global Constraints

- Match trimmed `display_name` only with case-insensitive substring semantics.
- Clear on category or active-space change; never persist or send the query.
- Use `ImeTextField`; raw composable inputs are prohibited.
- Keep category totals and attention counts unfiltered.

---

### Task 1: Reproduce Filtering Behavior

**Files:**
- Modify: `apps/desktop/src/components/Shell.test.tsx`

**Interfaces:**
- Consumes: existing `Sidebar` test snapshot/builders.
- Produces: tests locating the filter by category-specific accessible name.

- [ ] **Step 1: Add failing tests**

Render DMs named `Alice` and `Bob`, type ` ALI ` into the filter, and assert only
Alice remains while the DM total is unchanged. Add cases for Name sort order,
no-match copy, clear button, Escape, category switch, and a rerender with a new
`active_space_id`.

- [ ] **Step 2: Verify RED**

Run: `npm --prefix apps/desktop test -- src/components/Shell.test.tsx`

Expected: FAIL because the category filter control is absent.

- [ ] **Step 3: Commit the RED test**

```bash
git add apps/desktop/src/components/Shell.test.tsx
git commit -m "test: reproduce sidebar room filter behavior"
```

### Task 2: Add The Minimal Filter UI

**Files:**
- Modify: `apps/desktop/src/components/Shell.tsx`
- Modify: `apps/desktop/src/styles.css`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/i18n/messages.test.ts`

**Interfaces:**
- Consumes: `ImeTextField`, `RoomListItem.display_name`, `SidebarRoomCategory`.
- Produces: `filterSidebarRooms(rooms: RoomListItem[], query: string): RoomListItem[]` and controlled filter props on `RoomListControls`.

- [ ] **Step 1: Add catalog entries and catalog assertions**

Add English/Japanese IDs for DM/room filter labels and placeholders, clear, and
no matches. Assert both catalogs contain the exact new IDs.

- [ ] **Step 2: Implement matching and reset behavior**

Add query state to `Sidebar`, filter the result of `sortedSidebarRooms`, clear
inside `selectRoomCategory`, and clear in an effect keyed by
`snapshot.state.ui.navigation.active_space_id`.

```tsx
function filterSidebarRooms(rooms: RoomListItem[], query: string): RoomListItem[] {
  const normalized = query.trim().toLocaleLowerCase();
  return normalized
    ? rooms.filter((room) => room.display_name.toLocaleLowerCase().includes(normalized))
    : rooms;
}
```

- [ ] **Step 3: Render the IME-safe control**

Use `ImeTextField type="search"`; Escape clears only a non-empty query. Show
the distinct no-match copy when the unfiltered category has entries and the
filtered array is empty.

- [ ] **Step 4: Add only the required compact CSS**

Reuse existing input/icon/button tokens under `.room-list-controls`; do not add
a new component library or layout abstraction.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
npm --prefix apps/desktop test -- src/components/Shell.test.tsx src/i18n/messages.test.ts
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx
npm --prefix apps/desktop run typecheck
```

Expected: every command exits 0.

- [ ] **Step 6: Review and commit**

Read `git diff -- apps/desktop/src/components/Shell.tsx apps/desktop/src/components/Shell.test.tsx apps/desktop/src/i18n/messages.ts apps/desktop/src/i18n/messages.test.ts apps/desktop/src/styles.css`, run `git diff --check`, then commit those files.
