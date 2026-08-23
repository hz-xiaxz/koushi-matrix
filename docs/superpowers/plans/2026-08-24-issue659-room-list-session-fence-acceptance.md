# Issue #659 Room-List Session Fence Acceptance

## Scope

Merged PR #673 / #660 implemented the production root fix that #659 requires:

- `handle_room_list_updated` checks Ready before the Uninitialized readiness bump;
- shared room-list update and bootstrap are Ready-only;
- provisional/authoritative snapshot handlers check Ready before invites writes;
- whole-state Locked/SwitchingAccount tests and full CI are merged.

#659 additionally names SignedOut explicitly. Complete the acceptance evidence
without another production change: extend the existing whole-state tests to
SignedOut, document that #660 supplied the fix, run the historical pre-#660 code
against the unchanged tests for RED, then current main for GREEN.

## Verify first

1. Extend both existing `navigation_state/room_list.rs` tests so their session
   matrix is `Locked`, `SignedOut`, and `SwitchingAccount`.
2. Whole-state equality includes readiness, invites, spaces, rooms, navigation,
   crawler effects and all other fields.
3. Keep those tests unchanged; temporarily restore only the pre-#660 production
   `reducer/room.rs` from pinned parent `cd8775bdf14d772439230a1c96ab1dd91b4bbf62`
   (parent of #660 implementation `26ff1c53`) and run:

```bash
cargo test -p koushi-state --test navigation_state transient_room_list -- --nocapture
```

The old entry/snapshot pre-guard mutations must RED. Restore current production
bytes and run the identical command GREEN.

Also run the full `navigation_state`, `invite_state`, and state-lib suites to
preserve Ready flow, generation ordering, provisional/bootstrap semantics and
exhaustive action dispatch.

## Files

- `crates/koushi-state/tests/navigation_state/room_list.rs` — SignedOut rows only;
- `docs/architecture/state-machine.md` — note that all non-Ready states,
  explicitly SignedOut/Locked/Switching, reject fresh room-list signals before
  mutation;
- this dated plan and plans index.

No production code, action, state, DTO, command, frontend or dependency change.

## Evidence and closure

The PR body links #659 and #673, records the historical RED/current GREEN and
full matrix, and closes #659 only after exact review + CI7/7. #551 evidence notes
that #660 supplied the shared production seam while this task pins the remaining
acceptance state.

Implementation starts only after `reviewer-flash-opencode-go` records
`Correct-to-merge`; exact final test/docs diff requires post-review.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Correct-to-merge`. Verified merged
  #660 guards cover every #659 production seam, SignedOut is the only missing
  whole-state matrix row, and the pinned historical-blob same-command RED is
  honest if it compiles and each row fails behaviorally.

## Acceptance evidence

- The two existing whole-state tests now cover the ordered session matrix
  `Locked`, `SignedOut`, `SwitchingAccount`; equality remains against the full
  cloned `AppState`, and every rejected action must return empty effects.
- Historical RED used only the pinned parent blob for
  `crates/koushi-state/src/reducer/room.rs`:
  `cd8775bdf14d772439230a1c96ab1dd91b4bbf62` (blob SHA-256
  `0ed15fa6263f6a5f2b1a65a9bcbd26d6eae7411556cc4e4619403d7120b70ea9`). The
  exact command `cargo test -p koushi-state --test navigation_state
  transient_room_list -- --nocapture` compiled and exited `101`: 2 tests ran,
  0 passed, 2 failed, 55 filtered. Both failures were behavioral assertions
  (the pre-guard entry/snapshot paths mutated transient state), not compile
  failures.
- Current `room.rs` was restored byte-exact (`cmp=equal`); its SHA-256 was
  `551e36a36f74c210753f670d36be6fc701799222f578c4a0f41983ab2ba835d6` both
  before and after the historical run. The identical command then exited `0`:
  2 tests ran, 2 passed, 0 failed, 55 filtered.
- Full gates passed: `cargo test -p koushi-state --test navigation_state`
  exited `0` (57 passed, 0 failed); `cargo test -p koushi-state --test
  invite_state` exited `0` (9 passed, 0 failed); `cargo test -p koushi-state
  --lib` exited `0` (39 passed, 0 failed); `cargo fmt --all -- --check`
  exited `0`; `node scripts/check-agents-docs.mjs` exited `0`; and `git diff
  --check` exited `0`.
- Final changes are tests/docs/evidence only: the reducer production file is
  unchanged.
