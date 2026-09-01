# Issue #780 DM Space-membership readiness plan

**Goal:** A partial Matrix member store must not erase a previously known DM-to-Space association, and selecting a Space—not opening its members panel—must request direct Space-member hydration.

## Design

- Do not modify `vendor/matrix-rust-sdk` or add an upstream SDK API. Use the existing public `Room::are_members_synced`, `Room::members`, and `Room::members_no_sync` APIs through `koushi-sdk`/Core.
- `MatrixRoomListSnapshot` carries a transient set of Space IDs whose direct JOIN-member input is complete. This does not cross into `AppState`, Tauri, or React.
- The room-list observer retains the last projected `dm_space_ids` for the current account session. Normalization always adds currently observed positive associations. For an incomplete Space input it also retains prior positives; only a complete Space observation may remove one.
- `SelectSpace` reliably queues direct membership hydration on the existing room-list observation owner. The observer refreshes the selected Space with `Room::members(JOIN)`, then reprojects from the same live `RoomListService`; opening Space members remains a consumer only.
- The Rust `People` room-list projection uses the same `dm_space_ids` Space predicate as `SidebarModel`.
- Diagnostics expose only complete/partial Space counts and hydration stage/outcome, never Space IDs, room IDs, user IDs, or members.

## Verification-first evidence

1. Core normalization RED: partial input preserves a known positive association; complete input removes a stale association.
2. State RED: active-Space `People` items equal the sidebar DM scope.
3. Core observer tests: selection hydration uses the existing observer owner and reprojection path.
4. Focused `koushi-state`, `koushi-sdk`, and `koushi-core` tests, then relevant lint/type checks.

## Non-goals

- No vendored SDK changes.
- No React-owned membership/readiness state.
- No new retry loop, persisted membership store, or second room-list/sync owner.
- No member IDs in public diagnostics.

## Review record

2026-08-31 — reviewed before implementation by the session's frontier model against `REPOSITORY_RULES.md`, `docs/architecture/overview.md`, `docs/architecture/state-machine.md`, `docs/policies/engineering-rules.md`, and issue #780. Verdict: **Correct-to-implement**. The design keeps membership semantics Rust-owned, uses existing SDK APIs, preserves last-known positives only for explicitly incomplete inputs, and gives complete observations sole removal authority.
