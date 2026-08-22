# Issue #634 Browser Fake Link-preview Fixture Isolation

## Status

Design revision 2 approved by `reviewer-flash` round 2: `Correct-to-implement`. Round 1 found that deleting the two explicit fixture traversals was insufficient because `snapshot.timeline` shallowly aliases `timelineMessages`; revision 2 requires copy-on-write.

## Problem and RED proof

`BrowserFakeApi.updateTimelineMessageLinkPreviews` mutates `this.snapshot.timeline` and the module-owned `timelineMessages` and `backwardTimelineMessages` fixture arrays. `createReadySnapshot` then shallow-filters `timelineMessages`, so one fake instance changes later instances and later account snapshots.

Add one focused test to `apps/desktop/src/backend/browserFakeApi.test.ts`:

1. create fake A and identify `$alpha-update` in `!room-alpha:example.invalid`;
2. prove its fixture `link_previews` is absent;
3. call A's public `hideLinkPreview` and prove A reports an empty preview list;
4. create fake B and prove the same fixture still has absent `link_previews`.

The unmodified baseline must fail at step 4 (`[]` instead of `undefined`). This is the required verify-first RED proof. Do not use private access or mutate returned snapshots.

## Implementation boundary

`createReadySnapshot` and `selectRoom` shallow-filter `timelineMessages`, so snapshot entries alias module fixture objects. In `updateTimelineMessageLinkPreviews`:

1. replace the in-place `forEach`/property assignment with a copy-on-write `this.snapshot.timeline = this.snapshot.timeline.map(...)` that returns `{ ...message, link_previews: linkPreviews }` only for the matching room/event and preserves every other object;
2. remove the traversals of module `timelineMessages` and `backwardTimelineMessages`.

This is the same existing copy-on-write idiom used by `editMessage` and `sendThreadReply`, not a new abstraction. A preview command resolves only a currently loaded message through snapshot-scoped `findTimelineMessage`. `backwardTimelineMessages` currently has no path into `snapshot.timeline`, so its explicit mutation is unreachable shared-fixture mutation, not runtime ownership.

No extraction, helper, broad fixture cloning, compatibility shim, fixture rewrite, timer change, API change, or unrelated cleanup. `loadLinkPreviews` keeps its 50ms synthetic delay and exact ready projection. Default fixtures contain no pending preview, so `loadLinkPreviews` remains a no-op unless future instance-owned state supplies one; this PR tests and fixes the reachable `hideLinkPreview` path. `hideLinkPreview` keeps its empty-list behavior in the active instance.

## Preserved contracts

- `DesktopApi` and `BrowserFakeApiContract` methods and signatures
- `DesktopSnapshot`, `TimelineMessage`, and `LinkPreview` shapes
- event IDs, room IDs, fixture order, pagination behavior, and synthetic preview fields
- all class fields/maps/timers/session cleanup and Rust ownership boundaries
- zero production exports or new source modules

## Deterministic verification

- Baseline RED: exact new test fails only because fake B observes `[]`.
- Post-fix focused test passes; repeat it at least three times.
- Full `browserFakeApi.test.ts` (81 lexical `test(` declarations; parameterization reports 86 baseline cases and 87 after this test).
- `client.test.ts`, full Vitest, Playwright with `CHOKIDAR_USEPOLLING=true`.
- Frontend typecheck/lint/build; workspace/all-target Rust and Headless Core QA.
- boundary/security/release/wire/SDK/docs/audit/rustfmt and `git diff --check` gates required by the repository.

## Exactness evidence

Record before/after:

- only one new test; production changes are one in-place traversal replaced by one copy-on-write traversal plus deletion of exactly two module-fixture traversals;
- `updateTimelineMessageLinkPreviews` has one snapshot `map`, one matching object copy, no direct message mutation, and no module fixture references;
- top-level declaration, class method/field/map/timer, API/export, DTO/wire deltas are zero;
- full diff reviewed read-only by `reviewer-flash` after all fixes.

## Implementation evidence

- RED before production change: the new public-API test failed because fresh fake B observed `link_previews: []`.
- Production fix: one in-place snapshot traversal plus two module-fixture traversals became one copy-on-write snapshot `map`; no fixture writes remain.
- GREEN: exact test passed three consecutive runs; full focused file reports 87/87.
- The first post-implementation review was invalidated after Prettier introduced whole-file churn. Both code files were reset to the immutable baseline and the same semantic change was minimally reapplied; the RED assertion was only line-wrapped, not changed.
- Fresh post-reset full-diff re-review: `reviewer-flash` `Correct-to-merge`.
- Final local matrix: browser fake87, client25, Vitest1,310, Playwright248, workspace all-targets, Tauri149/1 ignored plus keyring5, Headless Core QA130, wasm state/search, typecheck/lint/build, Tauri/domain/IPC boundaries, secret/release/version, SDK/docs, rustfmt, `cargo deny`, `cargo machete`, and diff checks green.
- The first workspace run hit unrelated `runtime_room_list_sync::normal_runtime_waits_for_full_all_rooms_reconciliation_and_reuses_one_sync_engine` deadline with a zero Rust diff. Its exact test passed three consecutive runs, its six-test file passed, and the complete workspace all-targets rerun passed; no failure was waived.

## Delivery

One independently mergeable fix PR linked to #634. Merge only after latest-main comparison, full local matrix, CI green, and recorded full-diff `Correct-to-merge`. Update #634 and #551 evidence after merge; do not close #634 until its remaining lifecycle findings are delivered.
