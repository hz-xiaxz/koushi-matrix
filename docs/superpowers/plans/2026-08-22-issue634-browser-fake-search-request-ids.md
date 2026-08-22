# Issue #634 Browser Fake Search Request IDs

## Status

Design approved by `reviewer-flash`: `Correct-to-implement`.

## Problem and RED proof

`BrowserFakeApi.submitSearch` uses `Date.now()` as `request_id` in both `tooShort` and `results`. Two accepted searches in one millisecond collide, unlike the fake's existing instance-owned monotonic `nextRequestId()` and Rust's correlated request model.

Add one focused public-API test in `browserFakeApi.test.ts`:

1. create a fresh fake;
2. spy on `Date.now()` and freeze it to one value, restoring the spy in `finally`;
3. submit two valid searches sequentially;
4. capture each returned `state.domain.search.request_id` and require distinct increasing IDs.

The baseline deterministically fails because both IDs equal the frozen wall clock.

## Implementation boundary

Inside `submitSearch`, after the synced-view guard, allocate exactly one `const requestId = this.nextRequestId()` for each admitted call and use it in either terminal projection (`tooShort` or `results`). Remove only the two `Date.now()` identity uses. Timestamp uses elsewhere remain wall-clock timestamps.

No extraction, helper, counter change, API/DTO/wire change, search semantics, result ordering, trim/minimum behavior, snapshot ownership, timer, map, or cleanup change.

## Verification and exactness

- exact new test RED before production change and GREEN three consecutive runs afterward;
- full browser fake and client tests;
- `submitSearch`: `Date.now`0, `nextRequestId`1, one local request ID, both projection assignments use it;
- Date/time calls outside `submitSearch` unchanged;
- declarations, class fields/methods/maps/timers, exports, API, DTO/wire deltas0;
- full frontend, Playwright, workspace/Tauri/Headless/wasm, boundaries/security/release/SDK/docs/audit/diff matrix;
- independent post-implementation full-diff review and CI7/7.

## Implementation evidence

- RED: with `Date.now()` frozen to `1_700_000_000_000`, both public search calls returned that same ID.
- GREEN: exact test passed three consecutive runs; full browser fake reports88/88.
- Production: one local `nextRequestId()` allocation; both branches reuse it; `Date.now()` identity uses removed.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`; full matrix pending.

## Delivery

One independently mergeable PR linked to #634. Update #634 and #551 after merge; #634 remains open until all listed lifecycle defects are complete.
