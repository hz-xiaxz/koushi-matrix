# Issue #552 Phase 4.3d — Space invite-cancellation failure epoch

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI. `reviewer-flash` design Round 4 verified resolved-Failed/catch separation, navigation-epoch success admission, failure cleanup and both duplicate settlement orders, then recorded `Correct-to-merge` before implementation. Both rapid-duplicate settlement orders were RED; focused GREEN is 47/47.

## Decision

Keep a renderer epoch only for local cancellation transport-failure presentation, but remove it from semantic success admission:

- rename `spaceMembersCancelRequestRef` to `spaceMembersCancelFailureEpochRef`;
- dispatch captures the existing `spaceNavigationRequestRef`; a resolved snapshot applies only when that navigation epoch plus the full account/Space/generation fence remain current. Rapid duplicate clicks and same-Space panel close/reopen do not change it, while same-Space room/Space/Home navigation does;
- resolved snapshots preserve the existing correlated `operation.kind === failed` mapping: matching failed settlement sets local failure, while non-failed settlement advances the failure epoch and clears it, retiring duplicate catches independent of React flush ordering;
- catch may set local failure only for the latest failure epoch while the exact target is still invited in the current full-fence snapshot;
- navigation invalidation clears `spaceMembersCancelFailure` and advances the navigation/failure epochs.

Rust's first-admitted cancellation request remains the only semantic settlement owner.

## Current defect

A rapid duplicate before Rust's `CancellingInvite` projection reaches React can dispatch A then B. Rust admits A and rejects B (`CancellationAlreadyInFlight`), but App increments its request ref for B and later ignores A's authoritative success because A is no longer the latest renderer request. This is the same inversion removed for invite execution in Phase 4.3c.

Unlike invite execution, cancellation also owns renderer-local `spaceMembersCancelFailure`, used to expose transport failure when no Rust-shaped snapshot was returned. Deleting the epoch entirely would allow an old rejection from attempt A to overwrite/restore failure after a newer retry B. Therefore the epoch remains, but only around failure presentation.

## Authority split

### Rust/Tauri

- Rust admission checks exact Space/generation, retryable Idle/matching Failed context and target invited membership;
- first accepted request reduces `CancellingInvite(request_id, Space, user, generation)` before SDK work;
- reducer settlement accepts only that request/target/generation and reconciles authoritative membership;
- Tauri waits for exact correlated cancel settlement/failure under one deadline;
- monotone appStore admits returned authoritative snapshots.

### Renderer

- full account/Space/generation fence rejects account/cross-Space staleness; the captured Space-navigation epoch additionally rejects same-Space room navigation without imposing latest-click semantics or dropping valid completion across panel close/reopen;
- `spaceMembersCancelFailureEpochRef` answers only whether a transport rejection may set the current panel's local failure flag;
- catch additionally requires the target user still exists in `space_invited`, so a duplicate rejection settling after accepted success cannot resurrect failure in the normal ordering;
- a resolved correlated Failed operation keeps the failure banner/retry contract; authoritative non-failed settlement clears it.

## Verify-first checks

1. behavioral RED: dispatch two same-target cancellations before Pending projection; reject B and resolve accepted A success at equal/current appStore generation; assert the transient duplicate failure is cleared, invited row is removed and no cancellation failure remains. Current latest-request success check makes this fail; the unchanged navigation epoch allows A through;
2. adversarial inverse: accepted A success settles before B rejection; B catch sees the target no longer invited and cannot set local failure;
3. existing resolved-Failed→retry test remains GREEN: exact failed operation sets the banner and later non-failed success clears it;
4. existing same-Space navigation/account stale completion/rejection tests remain GREEN through captured navigation epoch + full fence; add close/reopen proof that a valid same-Space completion still applies because panel open does not advance navigation;
5. source RED proves success admission never compares the failure epoch; navigation/dispatch advance it, catch compares it, and non-failed settlement may advance it only to retire stale failure presentation;
6. away-and-back navigation does not resurrect a stale failure banner because invalidation clears local failure.

## Scope

- `apps/desktop/src/App.tsx`: rename/narrow cancel failure epoch, capture Space-navigation epoch for semantic view admission, preserve resolved-Failed mapping, target-presence catch guard and navigation failure cleanup;
- `apps/desktop/src/App.spaceMembers.test.tsx`: rapid duplicate and source-contract proofs;
- ownership inventory/canon and Phase 4 plan/index.

No Rust/Tauri/API/DTO/IPC/BrowserFake change. Role update remains Phase 4.3e.

## Local verification evidence

- focused Space-members: 47/47;
- full Vitest: 1505/1505;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter/domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2537 passed/12 ignored), Tauri tests (177 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

The exact PR head will run both normal Tuwunel/Synapse invitation lanes. No local real-homeserver scenario is added because Rust/Tauri/SDK cancellation semantics are unchanged; App tests deterministically exercise both duplicate settlement orders, same-Space navigation and panel close/reopen.

## Acceptance

- Rust first-admitted cancellation determines membership settlement;
- duplicate rejection cannot suppress accepted success or resurrect failure after success in normal settlement order;
- current transport rejection or resolved correlated Failed operation still presents fixed/private failure; retry/non-failed settlement advances the failure epoch and clears it without React flush-order dependence;
- stale account/Space/generation and same-Space navigation completion remains fenced, while valid same-Space panel close/reopen completion still applies;
- a contrived old duplicate rejection arriving after the same user is re-invited may still set local failure because transport errors expose no Rust request id; this bounded presentation limitation is documented for later token/error typing work;
- no generic manager, local semantic queue/in-flight state, raw error or IPC change;
- role latest-click inversion remains explicitly pending.
