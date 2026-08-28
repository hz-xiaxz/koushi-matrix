# Issue #552 alias mutation sequencing ownership

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI.

## Scope

Phase 5A reconciles the `alias:${userId}` lane of `latestTextMutationQueueRef`. This leaf changes no Rust, SDK, Tauri, IPC/DTO or alias behavior unless evidence disproves the retained-owner decision.

## Ownership decision

Retain the alias lane as a documented renderer-specific submission/result owner.

Rust remains the sole durable semantic owner of local aliases, normalized values, `Saving(request_id)`, SDK account-data writes, reconciliation, display projections and errors. The renderer queue owns only ordering from autosave intent to command submission and returned-snapshot application:

- alias intent is emitted on input changes before Tauri command submission;
- concurrent Tauri invokes allocate request IDs and submit through async boundaries whose completion is not the alias terminal;
- `set_local_user_alias` currently returns the latest snapshot immediately after command queue acceptance, not after `LocalUserAliasUpdateSucceeded/Failed`;
- browser fake results use equal `state_generation`, so app-store generation admission alone cannot reject an older returned snapshot;
- Rust cannot recover original renderer intent order if invokes reach the command channel out of order;
- the keyed queue is bounded to active keys, skips superseded not-yet-started work, serializes started writes, applies only the latest result and deletes its tail on settlement/invalidation.

Removing it safely would require a larger contract change: a Rust latest-intent sequence known before concurrent submission plus a correlated alias terminal returned by Tauri and mirrored by browser mode. That is not a deletion-only leaf and adds no product value while the bounded renderer owner is explicit.

## Deterministic evidence

Use deferred promises and no sleeps in an App-level alias interaction:

1. first alias autosave starts and remains pending;
2. a newer alias edit does not start its API operation until the first settles;
3. the first returned snapshot is not applied after supersession;
4. the second operation receives the latest alias and its snapshot becomes visible;
5. queue source tests continue proving superseded-before-start, rejected-tail continuation, invalidation and bounded cleanup;
6. source contract documents the alias key and rejects any direct unqueued `setLocalUserAlias` call in App.

## Implementation

1. Add the alias-specific ownership comment at `setLocalUserAlias`.
2. Add focused App-level deferred evidence without changing queue behavior.
3. Update ownership canon, inventory, umbrella plan and index.
4. Leave caption lanes for separate Phase 5B analysis.

## Rejected alternatives

- Delete the alias lane and trust app-store generation: browser generations are equal and Tauri returns pre-terminal snapshots.
- Treat Rust `Saving` as latest intent: it admits only one projected request and cannot observe renderer order before submission.
- Change the IPC/terminal contract in this leaf: cross-layer complexity without a demonstrated defect.
- Create a second alias-only queue: the existing keyed bounded primitive already provides the exact behavior.

## Local verification evidence

- focused App alias sequencing: 1/1; queue continuation is included in the full suite;
- full Vitest: 1515/1515;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs and production build: passed;
- secret scan, Tauri adapter boundary, SDK submodule sync, diagnostic isolation and domain-crate platform guards: passed.

No Rust/Tauri/SDK source or contract changes; the exact PR head runs the complete Rust/Tauri/QA/dependency CI matrix.

## Acceptance

- Alias durable/product semantics have one Rust owner.
- Renderer sequencing is narrowly documented as pre-terminal autosave transport/result ownership.
- Deferred evidence proves latest intent and no stale snapshot application.
- Queue entries remain bounded and cleanup behavior is unchanged.
- Caption sequencing is neither changed nor claimed complete.
