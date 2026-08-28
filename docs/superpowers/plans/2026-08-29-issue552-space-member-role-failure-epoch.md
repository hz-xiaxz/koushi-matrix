# Issue #552 Space-member role failure epoch

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI.

## Scope

Phase 4.3e reconciles `spaceMembersRoleRequestRef`. Rust already owns role-update admission and settlement through request id, Space generation, power-level revision, expected/current power level, target, permission and confirmation guards. The App ref currently also gates returned snapshots, so a rapid duplicate can increment it, be rejected by Rust, and cause React to drop the first admitted result.

This PR changes no SDK, Rust, Tauri, IPC/DTO or panel confirmation behavior. It does not generalize request management.

## Ownership decision

Rename the ref to `spaceMembersRoleFailureEpochRef` and restrict it to local transport-failure presentation.

Semantic success uses the Space navigation epoch captured at dispatch plus full current/result account, Space and generation fences. Closing and reopening the same panel does not invalidate a valid Rust result; selecting a Space, room or Home does.

A returned Rust snapshot is authoritative for the first admitted operation:

- `updatingRole` remains pending without an optimistic member patch;
- `roleUpdateFailed` preserves the correlated Rust failure and exact retry/reload behavior;
- successful settlement applies the Rust projection;
- non-failed settlement advances the failure epoch before clearing local transport failure, so a duplicate catch cannot reintroduce an alert regardless of React flush ordering.

A transport rejection presents the fixed localized failure only while its failure epoch, navigation epoch and full fence are current and the target still has the dispatched expected power level. Navigation advances the failure epoch and clears local role transport failure. With a duplicate, the later dispatch intentionally makes the first invoke's catch failure-epoch-stale; the Rust-owned `roleUpdateFailed` snapshot and/or current duplicate rejection still present the same failure.

## Deterministic RED evidence

Extend `App.spaceMembers.test.tsx` with deferred promises and no sleeps:

1. rapid duplicate, duplicate rejection first: first admitted success applies and clears failure;
2. rapid duplicate, success first: later duplicate rejection cannot restore failure;
3. close/reopen the same Space-members panel: valid first result still applies;
4. same-Space room navigation: late success and rejection are ignored and unlogged;
5. one current single-dispatch transport rejection shows fixed localized failure and privacy-safe diagnostics;
6. source contract: success admission compares navigation, never failure equality; catch retains failure equality, navigation and target guards.

Existing tests continue proving confirmation, no optimistic patch, pending projection, authoritative success, forbidden/stale/network retry and stale-role reload.

## Implementation

1. Rename `spaceMembersRoleRequestRef` to `spaceMembersRoleFailureEpochRef`.
2. Capture `spaceNavigationRequestRef.current`, increment the failure epoch at dispatch and keep clearing the prior local transport failure.
3. Gate returned snapshots by navigation epoch and full current/result fences, not latest click.
4. Advance failure epoch on non-failed settlement, then clear transport failure and apply the snapshot.
5. Gate catch by failure epoch, navigation epoch, full fence and target expected power level.
6. Advance/clear role failure on navigation.
7. Update the ownership inventory and Phase 4 records.

## Rejected alternatives

- Keep latest-click success authority: contradicts Rust first-admitted command ownership.
- Move panel visibility/confirmation state to Rust: renderer-only and unnecessary.
- Add a generic request manager: distinct family lifetimes do not justify it.
- Change Rust/IPC: existing admission and settlement authority are sufficient.

## Local verification evidence

- focused Space-members: 53/53;
- full Vitest: 1511/1511;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs and production build: passed;
- secret scan, Tauri adapter boundary, SDK submodule sync, diagnostic isolation and domain-crate platform guards: passed.

No Rust/Tauri/SDK source or contract changes in this leaf; the exact PR head runs the complete Rust/Tauri/QA/dependency CI matrix.

## Acceptance

- First Rust-admitted role settlement wins over a rejected duplicate in either order.
- Same-Space panel close/reopen accepts a still-valid result; navigation rejects it.
- Local transport failure is current, private, retryable and cannot outlive success/navigation.
- Rust `roleUpdateFailed`, retry/reload and confirmation semantics are unchanged.
- No optimistic role mutation, timer, compatibility shim or generic abstraction is added.
