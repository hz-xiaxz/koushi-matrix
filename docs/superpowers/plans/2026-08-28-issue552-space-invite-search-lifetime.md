# Issue #552 Phase 4.3b — Space invite-search lifetime and converged snapshots

Status: implemented, locally verified and exact-final-diff approved. `reviewer-flash` design Round 3 recorded `Correct-to-merge` before implementation. Exact-diff Round 1 passed all focus areas with three nonblocking Minors; account symmetry and wait-loop contract enumeration were fixed, and exact-diff Round 2 plus verdict follow-up recorded `Correct-to-merge`. RED proved room query rejection was unhandled and Tauri returned immediate snapshots; focused frontend GREEN is 173/173 and focused Tauri GREEN is 172 passed/1 ignored.

## Decision

Keep two distinct renderer presentation fences and repair the backend convergence seam they consume:

- `SpaceMembersPanel::inviteSearchRequestRef` remains mounted-component query/DOM ownership (debounce, spinner and candidate-list latest-wins).
- Replace App's Space-only ref with one `inviteWorkflowLifetimeEpochRef` shared by room-dialog open/query/close and Space-panel query/reset; every newer workflow intent invalidates older completions across surfaces.
- Make Tauri `open_invite_workflow`, `search_invite_targets` and `close_invite_workflow` return exact converged versioned snapshots instead of immediate potentially pre-reducer snapshots.
- Add the Phase 4.3a full-account Space fence and exact destination/query admission before App applies/returns search results.

Do not add query state to React beyond the existing mounted panel list, add a generic request manager, or change IPC names/arguments/DTO shapes.

## Current boundary and gap

```text
panel/dialog local query intent
  -> App shared workflow-lifetime epoch (+ Space account fence where applicable)
  -> DesktopApi open/search/close
  -> Tauri submits AppCommand(RequestId)
  -> [CURRENT GAP: immediately reads current_snapshot]
  -> Core serially reduces InviteWorkflowOpened/QueryChanged/Closed
  -> Rust InviteWorkflowState projection + StateDelta
  -> exact returned snapshot / appStore / local candidate array
```

Queue acceptance does not prove the reducer has installed, queried or cleared the workflow. BrowserFake mutates-and-returns synchronously and therefore hides this production-only race.

## Authority split

### Rust

- owns candidate derivation, exact-MXID parsing/status, scope plan, selected scope, history policy and shared invite-workflow state;
- serially applies open/search/reset commands and publishes versioned StateDelta snapshots;
- owns account/session invalidation of invite workflow.

### Tauri

- allocates connection-scoped RequestIds and returns only after an attached Core connection observes a snapshot satisfying the submitted open/search/reset;
- owns one dedicated short convergence deadline and lag-safe snapshot recheck, not query semantics.

### Renderer

- panel-local epoch owns only whether an async candidate array may update the currently mounted Space search UI;
- App lifetime epoch owns only whether an open/search/reset promise may apply/return after a newer room-dialog or Space-panel intent;
- full account/Space/generation fence rejects a prior-account Space result even when ids/generation collide.

These fences are not duplicate Rust query authority: Rust cannot know whether the component/dialog that requested an array still exists.

## Tauri convergence design

For each open/search/close command:

1. attach a fresh `CoreConnection` before submit;
2. allocate the RequestId from that connection and submit through it;
3. under `INVITE_WORKFLOW_CONVERGENCE_TIMEOUT = 2s` (independent of both the 60-second room-operation timeout and the panel's 250ms debounce), repeatedly recheck `versioned_snapshot` before and after received events; lag triggers another snapshot check;
4. open terminal: `invite_workflow.query.room_id` matches the submitted destination;
5. search terminal: `invite_workflow.query.room_id/query` exactly match submitted values;
6. close terminal is evaluated on Rust `AppState`: `invite_workflow == InviteWorkflowState::default()` (before frontend DTO conversion);
7. if the snapshot already satisfies an identical open/search or already-closed command, it is an equivalent terminal after accepted submit; no generation advance is required;
8. on deadline return one fixed `Err`, never an arbitrary current snapshot; convert only an exact terminal snapshot directly to the frontend DTO.

A superseded query can legitimately miss its transient exact state and hit the short deadline. Renderer lifetime/query epochs classify that fixed rejection as stale and settle UI without retry.

No sleep, log assertion or new Core event is introduced.

## Renderer settlement design

- Every room-dialog open/query/close and Space-panel query/reset increments or captures `inviteWorkflowLifetimeEpochRef` as appropriate.
- Room-dialog completion also requires the current dialog destination and exact returned destination/query.
- Space search completion requires current lifetime epoch, the full account/Space/generation fence in both live and returned snapshots, and exact returned destination/query.
- All `openInviteWorkflow`, `searchInviteTargets` and `closeInviteWorkflow` consumers catch fixed transport/convergence errors. Search returns `[]` so `SpaceMembersPanel`'s `.then` clears its spinner; close leaves the already-closed local UI closed; open/query keeps local drafts but applies no mismatched snapshot.
- Current, non-stale failures may append only a fixed private-data-free diagnostic token. Raw errors are never logged.

A search submitted before close can settle after the local UI closes. Its Rust StateDelta remains authoritative and may transiently reflect serialized workflow state; this PR does not filter StateDelta. The App epoch prevents that stale returned promise from directly calling `setSnapshot` or returning candidates, while the close/unmount path submits the closed-state intent. Scope/select/remove/invite completions remain explicitly deferred.

## Verify-first checks

Before production edits:

1. Tauri RED: accepted open/search/reset cannot return a baseline pre-reducer snapshot; deterministic helper tests prove exact open/query/closed terminals, already-equivalent terminals, lag recheck and dedicated deadline error;
2. App RED: pending old-account Space search resolves after same Space/generation account replacement and cannot call `setSnapshot` or return candidates;
3. App RED: pending Space search resolves after reset completes and cannot re-dirty workflow or return candidates;
4. room-dialog RED: overlapping query/open/close intents admit only the latest exact destination/query and stale completion cannot reopen/re-dirty workflow;
5. query-correlation RED: a returned snapshot whose query does not exactly match submitted destination/query is rejected;
6. failure RED: convergence timeout/rejection is caught at every App open/search/close/reset consumer, emits at most a fixed private-data-free diagnostic, returns `[]` to Space search so its spinner settles and never creates an unhandled rejection;
7. unchanged GREEN: panel debounce, newer-query stale candidate rejection, cancel/unmount reset, exact-MXID merge and privacy.

## Scope

- `apps/desktop/src-tauri/src/commands/room.rs` and focused command tests: converged open/search/close snapshot wait with a dedicated short deadline;
- `apps/desktop/src/App.tsx`: one cross-surface workflow lifetime epoch, full Space fence + exact destination/query admission and bounded fixed-token error handling for every open/search/close/reset consumer;
- App/SpaceMembersPanel tests and source contracts;
- ownership inventory/canon and Phase 4 plan/index.

Invite execution, scope selection, target selection/removal, cancellation and role mutation epochs remain later decisions.

## Local verification evidence

- focused invite/Space/source suites: 173/173;
- full Vitest: 1501/1501;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter/domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2537 passed/12 ignored), Tauri tests (177 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

The normal Tuwunel/Synapse invitation CI lanes remain applicable and will run on the exact PR head. Local deterministic Tauri wait-source tests cover exact/equivalent terminals, lag recheck and fixed deadline without sleeps.

## Acceptance

- production Tauri open/search/reset returns only an exact equivalent projection, never a mismatched pre-command snapshot;
- convergence timeout is short, fixed-error and handled by every renderer consumer without a stuck spinner or unhandled rejection;
- stale returned open/query/reset/account completion cannot directly call `setSnapshot`, mutate local dialog state or return panel candidates; Rust StateDelta remains authoritative and deferred scope/select/remove/invite completions are not covered by this claim;
- panel and App epochs have separate documented renderer lifetimes, while the App epoch is shared across room and Space invite-workflow surfaces;
- Rust remains the sole candidate/workflow semantic owner;
- no sleep, retry loop, generic manager, private log data, API/DTO/IPC shape or BrowserFake semantic change.
