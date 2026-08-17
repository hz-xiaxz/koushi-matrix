# Issue #541 verification worklog

Candidate tree:

- outer: `fad3fa1`
- vendored SDK: `4f4bbc6`

The approved design was reviewed before implementation and received the
recorded GPT `Correct-to-merge` sign-off. The latest implementation review was
not `Correct-to-merge`: it still requests high-level executor/actor terminal
regression tests and live encrypted-room evidence.

## Fresh green evidence

Commands were run from the exact candidate tree unless noted otherwise:

- `cargo fmt --all -- --check` — pass.
- `cargo test --workspace --exclude koushi-backend --exclude sidebar-composition --exclude key-management` — **2337 passed, 12 ignored**.
- `cargo test -p matrix-sdk-crypto --lib` at SDK `cb164845` — **580 passed, 1 ignored**.
- `cargo test -p koushi-sdk --lib` — **143 passed**.
- `cargo test -p koushi-desktop --lib` — **148 passed, 1 ignored**.
- Tauri registration test `every_tauri_command_is_registered_in_generate_handler` — **1 passed**.
- `npm --prefix apps/desktop run typecheck` — pass.
- `npm --prefix apps/desktop run lint` — pass; IME and agent-doc checks pass.
- `npm --prefix apps/desktop run test -- --run` — **70 files, 1358 tests passed**.
- `npm --prefix apps/desktop run qa:secret-scan` — pass.
- `npm --prefix apps/desktop run qa:release-gates` — structural and release compile pass.
- `npm --prefix apps/desktop run qa:wasm-check` — pass.
- `node scripts/check-sdk-submodule.mjs` — pass.
- `node scripts/check-agents-docs.mjs` — pass.
- `git diff --check` — pass.
- `cargo test -p koushi-core --features test-hooks` — **1171 passed, 8 ignored**.
- `cargo test -p matrix-sdk --features testing --test integration encryption::index0_reshare` at SDK `4f4bbc6` — **7 passed**; this includes public `Room::resend_index0_room_key` success, claim failure, send-failure cleanup/retry, and controlled-deadline coverage.

## Non-green evidence

- `cargo test -p matrix-sdk --lib` has one unrelated existing Sliding Sync
  test failure: `sliding_sync::tests::test_extensions_to_device_since_is_set`
  observed `since=None` where the test expects `Some("depuis")`. The issue-541
  room focused test itself passes; the full vendored `matrix-sdk` suite is not
  green.
- The exact encrypted-room QA command
  `node scripts/desktop-headless-local-qa.mjs --run --server=tuwunel
  --scenario=encryption_debug --core --timeout-ms=600000` fails before the
  resend recipient stage. Private-data-safe stdout reaches the room token, and
  the A2 verification gate times out in `awaiting_verification`; no resend
  token is claimed.

## Remaining blockers

1. Extend deterministic high-level SDK executor coverage to partial-send/
   persistence paths; public resend claim failure, send-failure cleanup/retry,
   and controlled deadline coverage now pass, and Core actor duplicate
   admission, teardown cancellation, stale completion suppression, and
   exactly-one terminal event coverage now pass.
2. Resolve the A2 SAS prerequisite or obtain an accepted equivalent live
   encrypted-room evidence path; do not infer resend success from the current
   failure.
3. Triage the unrelated full `matrix-sdk` Sliding Sync test failure if the
   vendored SDK full suite is required for this merge.

No PR was created or merged because the implementation-review gate and live QA
gate are not satisfied.
