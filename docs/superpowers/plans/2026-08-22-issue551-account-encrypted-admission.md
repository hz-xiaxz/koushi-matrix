# Issue #551 account encrypted-content admission ownership

Status: design review pending. Scope is the final behavior-preserving runtime residual ownership move.

## Baseline

- Base: `db2e98cd4b023381d2a987f4aab2e343f8376a30` after reducer-support PR #623.
- `runtime.rs`: 6,532 newline-delimited lines / 280,234 bytes / SHA-256 `40d4286d21bfea04f4435932fcd46ff5011d1ec178d210169f8ff773d604ac73`.
- `account/routing.rs`: 875 newline-delimited lines / 34,156 bytes / SHA-256 `b963dfa093e647126258bc1016bedeb3b24263a32a6513e113625aa4762db7b5`.
- Focused baseline: secure-backup route source guard1, unknown-encryption admission1, encrypted admission1.

## Ownership decision

Move exactly these two adjacent identities from `runtime.rs` to `account/routing.rs`, immediately before `impl AccountActor` and its `route_timeline_command_with_permit_and_formatting_options` owner:

1. `EncryptedUserContentTarget<'a>`
2. `encrypted_user_content_target`

Preserve struct field order, command variant coverage, branch order, request/room/submission references and helper body exactly. Demote the struct, its three fields and the function from `pub(crate)` to private because the sole caller becomes same-module.

The classifier belongs to AccountActor routing: it maps eight user-content TimelineCommand variants to the authoritative secure-backup admission target used immediately before `admit_secure_backup_user_content`. Runtime/AppActor has no caller and owns no part of the barrier.

## Ordering and invariants

Keep `route_timeline_command_with_permit_and_formatting_options` byte-exact except replacing `crate::runtime::encrypted_user_content_target` with the same-module function call. Preserve:

1. composer account/session fence before encrypted-content classification;
2. promoted session requirement before secure-backup admission;
3. authoritative room encryption admission before policy mutation or TimelineActor forwarding;
4. submission commands emit correlated `SubmissionRejected`; other sends emit correlated `TimelineOperationFailed`;
5. all rejection returns precede link-preview policy mutation and message construction;
6. command variant/DTO/wire and privacy behavior unchanged.

No state, actor, task, channel, timer, map, counter, subscription, shutdown or public API moves.

## Imports and compatibility

`account/routing.rs` already imports `TimelineCommand`, `RequestId`, and `TimelineKey`; add no import. Removing the two runtime identities creates no runtime orphan binding because those types remain used throughout `runtime.rs`.

No re-export or compatibility shim preserves the old `crate::runtime` path: it was `pub(crate)` only and had exactly one first-party caller, which moves atomically. The destination identities are private; no visibility widening, wrapper, trait, alias, glob, path attribute or duplicate logic.

## Tests and exactness

Move no tests. Preserve all 1,029 core lib test identities/paths.

The existing `secure_backup_barrier_covers_normal_and_scheduled_user_content_routes` source guard already reads `account/routing.rs` and asserts that the normal route contains `admit_secure_backup_user_content` before timeline forwarding; its route boundaries remain unchanged. Unknown-encryption fail-closed and encrypted admission tests remain in `account/scheduled_send.rs`.

A temporary `syn` verifier compares immutable base with source + destination:

- identities 2/2 moved, runtime parent0;
- struct/function bodies, fields and order exact after normalizing only approved private visibility and the one call qualification;
- route method exact after normalizing only `crate::runtime::` removal;
- all 1,029 tests exact; source guard unchanged;
- imports/public API/wire/resources/dependencies delta0;
- duplicate/missing/excess identities0.

## Verification

Run the three focused baselines, full core library, account/timeline routing integration owners, `cargo check -p koushi-core --all-targets --all-features`, exactness/source/order/privacy checks, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main` if required, run the complete repository matrix, PR CI7/7 and merge. Then rerun the final runtime residual audit; no runtime checkbox closes before that verdict.

## Review gate

- Read-only residual audit found this sole cross-owner edge and rejected all other prospective runtime splits.
- Formal `reviewer-flash` verdict pending; implementation prohibited until `Correct-to-implement`.
