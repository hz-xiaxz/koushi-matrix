# Local Space order preference implementation plan

## Goal

Make Space ordering a durable, account-local preference under Sliding Sync. A
partial or temporarily empty room-list snapshot must never erase saved order.
The visible Space list is a projection of the preference ledger, and explicit
successful room/Space leave removes the corresponding entry.

## Tasks

### 1. Add state-level preference helpers and tests

- Update `crates/koushi-state/src/reducer/mod.rs` with helpers that normalize
  and deduplicate the order ledger, append genuinely new Spaces, project the
  ledger onto visible Spaces, and reorder only currently visible entries while
  preserving hidden ledger slots.
- Add unit tests for duplicate normalization, missing/reappearing Spaces,
  hidden-entry slot preservation, invalid visible reorder rejection, and stable
  projection.
- Run `cargo test -p koushi-state` and confirm the new tests initially fail
  before adding production behavior.

### 2. Make reducer updates non-destructive

- Update `NavigationLoaded`, room-list updates, and `ReorderSpaces` handlers to
  use the non-destructive ledger helpers.
- Add a regression test that loads persisted order while `state.spaces` is
  empty, then applies a room-list snapshot and verifies the persisted order is
  recovered rather than replaced.
- Add a regression test for a Space missing from one snapshot and returning in a
  later snapshot.
- Run the focused state tests and the full `cargo test -p koushi-state`.

### 3. Remove the preference on explicit successful leave

- Add an internal reducer action for removing one Space-order preference entry.
- Dispatch it from the successful `RoomActor` leave path, without treating a
  failed leave as removal.
- Add reducer coverage for removal and no-op behavior for ordinary rooms.
- Run focused `koushi-core` and `koushi-state` tests.

### 4. Harden navigation persistence and diagnostics

- Change navigation persistence to write a temporary file, flush/sync it, and
  atomically rename it into place.
- Surface navigation load/save failures through runtime diagnostics and avoid
  overwriting a previously valid ledger with a projection derived from a load
  failure.
- Record compact `core.space_order` diagnostics: load source/result, ledger
  count, visible count, missing count, reorder result, and persistence result.
- Add store/runtime tests for atomic replacement and failure visibility.

### 5. Align the browser fake API and UI-facing tests

- Update `apps/desktop/src/backend/browserFakeApi.ts` to preserve hidden Space
  entries when reordering visible Spaces.
- Extend browser fake tests and the relevant desktop/E2E coverage for startup,
  transient omission, and hidden-entry reorder behavior.
- Run the targeted npm tests and the existing Rust tests.

### 6. Review and publish

- Inspect the complete diff and run formatting, focused tests, and the required
  desktop checks.
- Request a code review, address actionable findings, commit the implementation
  with the design and plan, push the branch, and open a draft PR.
