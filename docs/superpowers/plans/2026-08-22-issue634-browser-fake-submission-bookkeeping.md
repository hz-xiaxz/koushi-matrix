# Issue #634 Browser Fake Submission Bookkeeping

## Status

Design approved by `reviewer-flash`: `Correct-to-implement`.

## Ownership seam

One independently mergeable submission-idempotency seam:

- `submissionLedger` is the fake's private replay owner and must be session-scoped like Rust's session-owned `TimelineManager` ledger;
- each main/thread `ComposerState.accepted_submission_ids` is the public bounded acceptance tombstone projection and must share Rust's 128-entry bound.

Both are written only by `acceptSubmission`; reachable session teardown is centralized by `clearSessionViews`. `submitSoftLogoutReauth` is the explicit exception: the fake reaches `locked` only at construction with an empty ledger, so reauth resumes no prior fake session ledger. Fixing the two artifacts together makes private replay ownership and public tombstone ownership converge at the same acceptance/session boundary. Composer leases and prepared-upload bytes are separate #634 PRs.

## Verify-first RED proofs

### Session replacement

Add one public-API test:

1. fresh account A, begin generation/lease, send submission ID `session-reuse`;
2. switch to saved account B, then back to saved account A;
3. begin a fresh generation/lease for A;
4. record the fresh A fixture timeline length, send `session-reuse` with a new body, and require one appended item carrying the new body.

Baseline replays the stale private ledger entry and appends nothing. Do not inspect private fields.

### Bounded main and thread histories

Extend the existing 129-main-submission bound test to require the main composer history length128 and eviction of `bounded-0`.

Add one thread equivalent: open a fixture thread, send129 unique IDs through one valid thread lease, and require thread composer history length128 and eviction of its oldest ID.

These assertions fail on the current unbounded array append. Existing global settled-registry assertions remain unchanged.

## Implementation boundary

- In `clearSessionViews`, clear `submissionLedger` alongside other session-owned private maps.
- In `acceptSubmission`, replace raw `composer.accepted_submission_ids = [...filter, submissionId]` with the existing `rememberSubmissionRegistryId(composer.accepted_submission_ids, submissionId)`, which deduplicates and evicts oldest entries at128.

No new helper, wrapper, owner object, map, cap, API, DTO, transaction format, lease behavior, draft behavior, timer, export, or source module. Keep the private ledger's existing128 FIFO bound and `$browser-${submissionId}` transaction IDs.

## Verification and exactness

- all three RED assertions demonstrated before production changes;
- exact tests GREEN three times; browser fake/client full tests;
- `acceptSubmission`: one ledger set, one global active-registry remember, one composer-history remember, no raw accepted-history assignment;
- `clearSessionViews`: exactly one ledger clear; all six existing callers unchanged;
- caps exactly128 for private ledger, global settled history, main history, and thread history;
- duplicate accepted IDs remain in their existing tombstone position instead of moving to the end, matching both Rust reducers;
- fields/methods/maps/timers/declarations/exports/API/DTO/wire deltas0;
- full frontend/Playwright/workspace/Tauri/Headless/wasm and policy/audit matrix;
- post-implementation `reviewer-flash` full-diff approval and CI7/7.

## Implementation evidence

- RED: stale submission ID replayed after A→B→A; main history retained129 IDs; thread history retained129 IDs.
- Production: raw composer-history assignment replaced by the existing bounded remember helper; `clearSessionViews` adds exactly one ledger clear.
- GREEN: all three focused tests passed three consecutive runs; browser fake90 and client25 passed.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`.
- Final local matrix: browser fake90, client25, Vitest1,376, Playwright248, workspace all-targets, Tauri149/1 ignored plus keyring5, Headless Core QA130, wasm state/search, typecheck/lint/build, Tauri/domain/IPC boundaries, secret/release/version, SDK/docs, rustfmt, `cargo deny`, `cargo machete`, and diff checks green without reruns.

## Delivery

One PR linked to #634. After merge, update #634/#551. #634 remains open for composer lease and prepared-upload lifecycle fixes.
