# Issue #433 — Element X-compatible unread and native attention projections

## Objective

Make plain unread-message counts authoritative for room/DM indicators while
keeping notification counts, mentions, manual unread marks, mute policy, and
native delivery as separate projections. Fix the regression described in
issue #433 and prove the behavior with headless Rust and browser tests.

## Design

1. Keep `RoomSummary.unread_count` as the raw unread-message count from the
   SDK. The room activity helper will derive content activity with
   `max(unread_count, notification_count, highlight_count)` plus the explicit
   `marked_unread` fallback. Marking a room unread will no longer fabricate a
   raw message count; marking it read still clears all counters after the
   successful SDK operation.
2. Add one Rust-owned room-list projection for `has_unread_content`,
   `is_attention_highlighted`, `has_unread_mention`, mute state, and the
   numeric display count. Unmuted rows display `notification_count`; muted
   rows display raw unread messages; a content-only row renders a dot. Mirror
   these fields in the TypeScript browser projection for headless parity.
3. Make native attention sum raw unread messages once per unique joined
   non-space room for the Dock badge. Candidate/banner/sound selection stays
   push-rule and notification-mode based; muted/ignored/low-priority rooms are
   excluded from candidates but do not erase the global unread badge.
4. Extend private diagnostics with raw counters, notification mode, derived
   booleans, selected display count/reason, and native badge totals/exclusions.
   Do not include room IDs, event IDs, message bodies, or other private data.

## Implementation tasks (TDD order)

- [x] Update the focused navigation, native-attention, SDK, reducer, unread
  trace, and desktop-model tests to encode the Element X expectations. Run the
  focused tests and record the expected RED failures.
- [x] Implement the SDK/raw-count and mark-unread reducer changes, the shared
  Rust room projection, sidebar fields, native unique-room badge calculation,
  and diagnostics.
- [x] Update the TypeScript model, browser fake, room-row rendering, CSS, and
  contract fixtures/golden state as required by the Rust-owned DTO.
- [x] Add coverage for plain unread 1→2→3 updates, DM/non-DM rows, muted plain
  unread, notification and mention emphasis, manual unread, read clearing,
  duplicate/space exclusion from the native badge, and privacy-safe
  diagnostics. Run the same tests GREEN.
- [x] Run repository gates: SDK submodule guard, focused Rust tests, desktop
  unit/type checks, IME inventory/lint when touched, formatting, and
  `git diff --check`. Read the complete diff including this plan.
- [ ] Commit on a branch based on `origin/main`, push, open one ready PR linked
  to #433, monitor required checks, address failures if any, and merge with a
  normal merge commit. Confirm the merged commit and clean branch state.

## Verification commands

```bash
node scripts/check-sdk-submodule.mjs
cargo test -p koushi-state --test navigation_state
cargo test -p koushi-state --test attention_surface
cargo test -p koushi-sdk --lib
npm --prefix apps/desktop run test -- --run src/domain/desktopModel.test.ts src/backend/browserFakeApi.test.ts src/components/Shell.test.tsx
npm --prefix apps/desktop run typecheck
cargo fmt --all -- --check
git diff --check
```
