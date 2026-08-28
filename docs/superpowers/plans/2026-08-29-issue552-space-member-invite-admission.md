# Issue #552 Phase 4.3c — Space-member invite admission ownership

Status: implemented, locally verified and exact-final-diff approved. `reviewer-flash` verified the first-admitted/latest-click inversion, deletion safety and monotone RED harness, then recorded `Correct-to-merge` before implementation after all Minor notes were incorporated. Exact-final-diff review passed all eight areas with no blocking findings and the verdict follow-up recorded `Correct-to-merge`. RED showed only the pre-existing invited member after rapid duplicate settlement; focused GREEN is 44/44.

## Decision

Delete `spaceMembersInviteRequestRef`. Rust's Space-member operation request/generation admission is the sole semantic owner of which invite is accepted; App keeps only the full account/Space/generation view fence from Phase 4.3a and fixed private-data-free transport diagnostics.

Cancellation and role epochs remain separate Phase 4.3d/4.3e decisions because they guard renderer-local failure presentation. They are known instances of the same latest-click/first-admitted inversion and must be resolved next, not treated as already-correct.

## Current problem

App increments `spaceMembersInviteRequestRef` for every dispatch and admits only the latest returned promise. A rapid duplicate can therefore invert Rust authority:

```text
first invite accepted by Rust -> Inviting(request A) -> eventual success
second click before Pending projection -> Rust rejects duplicate request B
App epoch now B, so request A's authoritative success snapshot is ignored
```

The local latest-click rule is not equivalent to Rust's first-admitted operation rule. It can leave the renderer on the old child-only projection until a later StateDelta despite a valid correlated command response.

## Rust/Tauri authority

- `admit_space_member_invite` checks exact selected Space/generation, rejects joined/invited/non-child targets and rejects any invite/cancel/role operation already in flight.
- the first accepted request reduces `SpaceMemberInviteRequested { request_id, space, user, generation }` before SDK settlement;
- reducer settlement accepts only the matching request/Space/user/generation and reconciles the authoritative projection;
- Tauri attaches before submit and waits only for the correlated `SpaceMemberInviteSettled`/`OperationFailed` under one deadline;
- returned snapshots enter monotone appStore admission.

## Renderer boundary

App still must:

- capture `SpaceMemberFence` with full homeserver/user/device + Space/generation;
- require live and returned snapshots to match that fence before `setSnapshot`;
- ignore/log no stale prior-account/navigation completion;
- catch transport or correlated admission rejection without raw error data and retain the historical fixed token `invite outcome=transport_rejected` when the original fence remains current; in the rapid-duplicate proof this token describes request-level rejection even though its name says transport.

It must not choose the latest click over Rust's accepted request. Duplicate suppression itself remains Rust admission; no new local in-flight boolean/queue is added.

## Verify-first checks

Before production edits:

1. behavioral RED: dispatch two same-target invites before any pending projection; reject the second, then resolve the first with an authoritative success snapshot whose `state_generation` is exactly equal to (or greater than) current appStore generation; assert the first success is applied and the user leaves child-only state. This isolates the RED to the ref check rather than a stale-generation rejection; current latest request ref makes it fail;
2. account/navigation stale completion tests remain GREEN through the shared full fence;
3. transport rejection emits only fixed private-data-free diagnostics;
4. source RED removes the ref declaration, navigation invalidation and response-id checks without adding another invite queue/boolean.

## Scope

- `apps/desktop/src/App.tsx`: delete invite request ref/increments/checks; preserve exact shared fence and fixed diagnostic;
- `apps/desktop/src/App.spaceMembers.test.tsx` and source contract;
- ownership inventory/canon and Phase 4 plan/index.

No Rust/Tauri/API/DTO/IPC/BrowserFake change. Cancel/role refs and failure UI are untouched.

## Local verification evidence

- focused Space-members: 44/44;
- full Vitest: 1502/1502;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter/domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2537 passed/12 ignored), Tauri tests (177 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

The exact PR head will additionally run both Tuwunel/Synapse invitation lanes; no local real-homeserver scenario is added because Rust/Tauri/SDK semantics are unchanged and the changed renderer response-admission branch is deterministic in App tests.

## Acceptance

- Rust first-admitted request, not latest renderer click, determines invite settlement;
- rapid duplicate rejection cannot suppress the accepted invite success snapshot;
- stale account/Space/generation completion remains fenced;
- no raw error/private data, generic manager, local queue, in-flight semantic state or IPC change;
- cancellation and role families remain explicitly pending.
