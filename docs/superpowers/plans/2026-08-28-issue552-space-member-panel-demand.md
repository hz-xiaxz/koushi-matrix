# Issue #552 Phase 4.3a — Space-member panel demand ownership

Status: implemented, locally verified and exact-final-diff approved. `reviewer-flash` design Round 3 verified all six focus areas with no findings and recorded `Correct-to-merge` before implementation. Exact-diff Round 1 passed all eleven focus areas with no blocking findings, and the verdict-only follow-up recorded `Correct-to-merge`. RED proved same-valued account replacement stayed at one load and the source guard found the unbounded Map/Set; focused GREEN is 41/41.

## Decision

Keep the Space-members panel-open epoch and pre-projection load dedupe as renderer-specific demand owners, but repair and bound their exact lifetime:

1. add the full ready account owner (`homeserver + user_id + device_id`, encoded with `composerDraftAccountOwnerKey`) to `SpaceMemberFence` so same Space/generation values cannot collide across account or device replacement;
2. rename `spaceMembersOpenRequestRef` to `spaceMembersPanelOpenIntentEpochRef`;
3. replace page-lifetime `Map` + `Set` load bookkeeping with one bounded current-demand record keyed by account + Space + generation;
4. rerun the automatic load effect when ready account identity changes.

Do not move panel-open intent into Rust, add a generic request manager or change Rust/Tauri/API/DTO/IPC contracts.

## Traced boundary

```text
ready account + active Space + Rust Space-members generation
  -> renderer SpaceMemberFence(account, space, generation)
  -> automatic or explicit panel demand
  -> one current renderer load-demand record
  -> DesktopApi.loadSpaceMembers(space, generation)
  -> Tauri allocates Core RequestId and waits matching SpaceMembersLoaded/failure
  -> Rust LoadRequested installs Loading(request_id, space, generation)
  -> RoomActor SDK projection + persistent SpaceMemberDemand
  -> reducer accepts only active request/space/generation
  -> versioned DesktopSnapshot/appStore
  -> renderer exact account/space/generation admission
```

## Authority analysis

### Rust authority

- Rust owns selected Space, demand generation, SDK membership projection, child-room completeness, profile/role projection and the persistent sync-driven `SpaceMemberDemand`.
- Tauri assigns a distinct Core `RequestId` and waits for its exact load terminal.
- State's `Loading { request_id, space_id, generation }` admits only the latest load projection; stale request results cannot mutate Rust state.

### Renderer authority that remains necessary

- Rust cannot know whether the React People panel open intent remains current while `closeFocusedContextIfHiddenBy` and settings/member loads await. Same-target repeated opens require a local epoch.
- Before the first Rust Loading projection returns, an effect rerender and an explicit panel open can dispatch the same valid load. Rust currently admits concurrent same-Space/generation loads (it rejects invite/cancel/role overlap, not another Loading), so one renderer demand record prevents duplicate SDK work.
- After a successful empty projection, selected Space/generation plus Idle is not enough to distinguish "loaded empty" from "not loaded". The renderer must retain one loaded demand marker to stop the automatic effect loop.

### Current defects

- `spaceMembersLoadedRef` is an unbounded page-lifetime `Set`; every Space generation/account-like collision can remain forever.
- `spaceMembersLoadInFlightRef` is a `Map` that can retain arbitrarily many never-settling navigation requests.
- Their keys omit account identity. A later account with the same Space id/generation can skip its required load, and an old account response can pass `spaceMembersSnapshotMatches`.
- The automatic effect depends only on Space/generation/selection, so a same-valued account replacement does not reevaluate demand.

## Bounded design

Use one ref:

```text
null
or { key: account\0space\0generation, promise: Promise | null }
```

- matching key + promise: return the in-flight promise;
- matching key + null promise: return current snapshot (already loaded);
- different key: replace with the new current demand;
- construct every demand from `spaceMembersFenceForSnapshot`; do not rebuild an account-less fence literal inside the loader;
- before `setSnapshot(nextSnapshot)`, require exact demand-record identity plus account/Space/generation matches in both the live and returned snapshots; an old completion returns null without applying anything;
- current successful non-null completion: retain `{ key, promise: null }`;
- current failed/stale completion: clear only if record identity still matches;
- old completion after A→B→A/new-account replacement cannot apply, clear or mark the newer record;
- explicit role-failure reload clears only a matching current loaded record.

This bounds renderer bookkeeping to one record while preserving retry and pre-projection dedupe. A superseded Tauri request may still reach its existing timeout because Rust State admits only the latest Loading request; this is pre-existing transport behavior. Its rejected promise is ignored by the account-aware stale fence and must not emit `load outcome=failed` for the new demand.

## Verify-first checks

Before production edits add:

1. behavioral RED: after an initial Space load succeeds, replace homeserver/user/device in the ready account through `setAppStoreSnapshot` while preserving the same active Space/selected Space/generation; assert a second load is issued for the new account, then settle the old/new promises adversarially and prove the old account snapshot is never applied or logged. Current effect dependencies and account-less fence/key make this fail;
2. source RED: reject `spaceMembersLoadInFlightRef`, `spaceMembersLoadedRef`, `Map` and `Set` demand bookkeeping; require one bounded demand ref and account identity in effect dependencies;
3. unchanged GREEN: pending duplicate suppression, failed automatic-load retry, A→B navigation fencing, sidebar/Space-info common open path and exact generation.

## Scope

- `apps/desktop/src/App.tsx`: shared fence account identity, one bounded demand record, open-epoch rename/comment, effect account dependencies, exact reload invalidation;
- `apps/desktop/src/App.spaceMembers.test.tsx`: account-replacement RED/GREEN plus source/boundedness contract;
- ownership inventory/canon and Phase 4 plan/index.

Invite search and invite/cancel/role request refs remain separate Phase 4.3b/4.3c decisions. This PR strengthens their shared account fence but does not decide or remove their family-specific epochs.

## Local verification evidence

- focused Space-members: 41/41;
- full Vitest: 1496/1496;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter and domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2535 passed/12 ignored), Tauri tests (175 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

No real-homeserver lane is added for this renderer demand-resource fix: Rust/Tauri load semantics, SDK projection, API/DTO/IPC and BrowserFake behavior are unchanged; deterministic App tests exercise the exact account replacement and adversarial completion boundary.

## Acceptance

- an old account result cannot match or mutate a new account's renderer panel demand;
- same-valued account replacement triggers a fresh Rust-owned load;
- renderer load bookkeeping is statically bounded to one record;
- same current demand still coalesces, successful empty projection does not loop, failure retries, and stale completion cannot clear newer demand;
- Rust remains the sole membership/projection/operation owner;
- no generic manager, new timer/retry, compatibility shim, API/DTO/IPC or BrowserFake semantic change.
