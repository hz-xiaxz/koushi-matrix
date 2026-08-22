# Issue #634 Browser Fake Composer Lease Revocation

## Status

Design approved by `reviewer-flash`: `Correct-to-implement`.

## Problem

`clearSessionViews` clears draft maps but neither retires `composerRendererGeneration` nor clears `composerLeases`. After logout and return to the same synthetic account, an old renderer can write/release through its old lease or acquire another lease under its pre-session generation. Rust revokes the live generation at account-runtime teardown.

## Verify-first RED proof

Add one public-API test:

1. fresh account A and selected main composer; begin generation and acquire lease;
2. logout, then switch back to saved account A and select the same room;
3. require old lease `setComposerDraft` and `releaseComposerDraftLease` to reject;
4. require `acquireComposerDraftLease` under the old renderer generation to reject;
5. begin a fresh renderer generation and prove a fresh lease is acquired and releasable.

Baseline fails because old generation and lease survive. The test uses no private fields. The explicit fresh begin avoids relying on the unrelated UI renderer lifecycle.

## Implementation boundary

At the start of `clearSessionViews`, alongside session-owned private cleanup:

- increment `composerRendererGeneration` by `1n`;
- clear `composerLeases`.

Keep `nextComposerLeaseId` process-monotonic; do not reset it. Do not add a helper merely to share two statements with `beginComposerDraftRendererGeneration`.

`submitSoftLogoutReauth` remains the explicit exception: locked fake instances are construction-only and have no prior leases. All six reachable teardown callers retain ordering and behavior.

No draft-map, submission-ledger, prepared-upload, API/DTO, lease shape, error text, timer, map field, export, or module change. Review found the same stale-lease pattern in `appHarnessMain.tsx`; that separate #551 harness owner is recorded for its own verify-first task and is not hidden inside this browser-fake PR.

## Verification and exactness

- exact RED before production change and GREEN three consecutive runs;
- browser fake/client full tests;
- renderer-generation increments occur at exactly renderer begin and session clear;
- lease clears occur at exactly renderer begin and session clear;
- `nextComposerLeaseId` assignment/reset delta0;
- all six `clearSessionViews` callers unchanged;
- fields/methods/maps/timers/declarations/exports/API/DTO/wire deltas0;
- full frontend/Playwright/workspace/Tauri/Headless/wasm and policy/audit matrix;
- post-implementation `reviewer-flash` full-diff approval and CI7/7.

## Implementation evidence

- RED: after logout and return to saved A, the old lease still wrote instead of rejecting.
- Production: session clear increments renderer generation once and clears the lease map once; next lease ID remains untouched.
- GREEN: exact test passed three consecutive runs; browser fake91 and client25 passed.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`. A static-count minor suggested90, but the exact focused runner confirms the recorded browser fake91 and client25.
- Final local matrix: Vitest1,377, Playwright248, workspace all-targets, Tauri149/1 ignored plus keyring5, Headless Core QA130, wasm state/search, typecheck/lint/build, Tauri/domain/IPC boundaries, secret/release/version, SDK/docs, rustfmt, `cargo deny`, `cargo machete`, and diff checks green without reruns.

## Delivery

One PR linked to #634. After merge update #634/#551; #634 remains open for prepared-upload lifecycle and batch-bound parity.
