# Issue #657 Harness Resource Lifecycle

## Scope and root cause

`apps/desktop/src/test/appHarnessMain.tsx` is the composition root for the
browser-headless application harness. Its `currentSnapshot` is replaceable, but
three page-lifetime containers are not reconciled with that replacement:

- `preparedUploadBytes` retains target/staged/variant byte arrays until a send;
- `composerLeases` retains leases until explicit release or a renderer-generation
  replacement;
- `TauriIpcMock` invocation history survives every harness snapshot replacement
  unless each spec remembers to call `clearInvocations()`.

The defect is harness lifecycle ownership, not product state. Keep all ownership
in `appHarnessMain.tsx`; do not move these containers into React, Rust DTOs, or a
new registry/module.

## Authoritative lifecycle decisions

### Prepared upload bytes

After every successful `setCurrentSnapshot(next)`, first compare the previous
and next Ready-session owner `(homeserver, user, device)`. Clear all prepared
bytes when the next snapshot is not Ready or when that owner changes, even if a
synthetic fixture accidentally retains old staged-item projections. For an
unchanged Ready owner, retain bytes only for staged items still present in the
normalized next snapshot:

1. derive the active main `ComposerTarget` from `timeline.room_id` and its
   `staged_uploads`;
2. derive the active thread target, when open, and its `staged_uploads`;
3. for each ready item, admit only keys for variants still listed by that item;
4. delete every other `preparedUploadBytes` key.

The account gate covers actual harness `logout`, `change_homeserver`,
`reset_local_data`, and account/device replacement responses even when they
retain stale UI slices. Presence reconciliation covers clear staging, item
removal, room/thread target replacement, and same-account full snapshot
replacement. `send_prepared_uploads` may keep its existing eager target clear;
the reconciliation is the final invariant. Do not add byte caps or duplicate
bytes.

### Composer leases

After every successful `setCurrentSnapshot(next)`, retain a lease only when:

- the snapshot session is still Ready and its homeserver/user/device equals the
  lease account;
- the lease renderer generation equals the current harness generation; and
- its exact main/thread target remains active in the snapshot.

Delete every other lease synchronously before returning the snapshot. Beginning
a new renderer generation continues to clear all leases. A deleted lease must fail the existing
`release_composer_draft_lease` check as stale, and acquisition with a retired
renderer generation or retired account must fail through the existing
`acquire_composer_draft_lease` checks. Do not claim lease-keyed protection for
composer commands that do not accept a lease ID, manufacture a replacement
lease, or add lease caps.

### Invocation history

Invocation history belongs to one browser-page harness run. Reset it once at the
end of `boot()`, after the seed-row retry loop has observed the seed DOM row or a
spec-owned external event. The row can become visible during the loop's existing
25 ms yield, so the terminal check and clear alone do not fence an immediate
Playwright action.

Create one page-local boot-settlement Promise before `harnessControl` is exposed.
`harnessControl.invoke` awaits it before calling the recording mock. At the end
of `boot()`, clear startup invocation history and then resolve settlement in the
same JavaScript task. Thus an external command issued as soon as the seed row is
visible waits, is recorded only after the clear, and cannot be erased. App-owned
startup IPC continues through the `mockIPC` callback directly and is not gated,
so startup cannot deadlock. Do not gate internal IPC, `setSnapshot`, or event
push helpers.

Do **not** clear history in `setCurrentSnapshot` or
`window.__harness.setSnapshot`: existing specs legitimately replace snapshots
inside command responses and assert cumulative invocation indices across
mid-test replacements. Internal and external snapshot projection therefore both
retain command evidence. Preserve `clearInvocations()` as the explicit
within-test reset API.

Mock installation, command registration, snapshot ownership, and boot ordering
remain in the current composition root; the only boot change is the terminal
history clear after seed settlement.

## Verify first: deterministic RED

Add `apps/desktop/e2e/app-harness-resource-lifecycle.spec.ts` before changing the
harness. Use synthetic snapshots and `window.__harness.invoke`; no sleeps, logs,
private Matrix data, or direct module internals.

1. **Clear staging releases bytes**: stage one synthetic ready upload via
   `stage_upload_bytes`, prove `prepared_upload_preview` returns its bytes,
   invoke `clear_upload_staging`, then require the same preview to return `[]`.
   Baseline RED: staging state clears but bytes remain.
2. **Target replacement releases bytes**: stage bytes for the active main target,
   install a snapshot with another active room/no old staged item, and require
   the old preview to return `[]`.
3. **Logout/account replacement releases bytes**: stage bytes, drive the actual
   `logout` response whose fixture retains old timeline slices, and require the
   old preview to return `[]`. Separately install a different Ready account while
   deliberately retaining the old staged projection and require `[]`; this
   proves the account owner gate rather than a hand-built empty projection.
4. **Target replacement revokes leases**: begin a renderer generation, acquire a
   lease for the active target, replace the active room/thread target, and prove
   `release_composer_draft_lease` rejects the old lease as stale.
5. **Logout/account replacement revokes leases**: acquire a lease, drive logout
   and a different-account fixture, and prove release rejects. Also prove
   `acquire_composer_draft_lease` rejects the retired account or renderer
   generation.
6. **Explicit release remains valid**: an unchanged owner/target lease releases
   once successfully and a second release fails, preserving the existing
   contract.
7. **Invocation boot boundary**: navigate to the harness and wait for the seeded
   reply row. Without first polling empty history or yielding, immediately invoke
   `get_snapshot`; require it to be the sole invocation at index 0. Then perform
   mid-test `setSnapshot`, invoke `get_snapshot` again, and require cumulative
   indices `[0, 1]`. This proves startup cleanup, the immediate post-ready command
   fence, and preservation across intentional snapshot replacement.

Drive all transitions through the public `AppHarnessControl`; tests must not
export maps, counts, or cleanup helpers merely to inspect internals.

Focused RED/GREEN command:

```bash
npm --prefix apps/desktop exec playwright test e2e/app-harness-resource-lifecycle.spec.ts
```

Record the command's own non-zero RED exit before editing `appHarnessMain.tsx`,
then run the unchanged command GREEN.

## Minimal implementation

Change only:

- `apps/desktop/src/test/appHarnessMain.tsx` — add small local reconciliation
  helpers, call them from `setCurrentSnapshot`, and after the boot seed loop
  settles clear startup history then resolve the public-invoke settlement Promise
  in the same task;
- `apps/desktop/e2e/app-harness-resource-lifecycle.spec.ts` — behavioral RED/GREEN
  coverage;
- this plan and `docs/agents/plans.md` — review/worklog indexing.

Prefer one pass over each existing `Map`; no new class, module, registry, timer,
limit, command, DTO, or product-state field.

## Preservation and risks

- Reconciliation happens only after normalized snapshot construction, so it
  cannot invalidate resources if snapshot installation throws.
- Key derivation continues to use the existing `composerTargetKey` and
  `preparedUploadKey`; do not parse Matrix identifiers from key strings except
  for matching keys generated by those helpers.
- Internal command responses and mid-test external snapshot replacements retain
  IPC records. This protects existing cumulative-index Playwright assertions
  after Rust-shaped snapshot responses.
- Snapshot replacement intentionally retires resources even when synthetic room
  IDs repeat under another account; account identity is part of lease admission,
  while staged bytes survive only if the authoritative replacement still
  contains the exact staged variant.
- Lease-rejection scenarios terminate after proving stale `release`/`acquire`;
  the complete Playwright gate must additionally prove no later App navigation
  attempts to drain a lease already retired by direct fixture replacement.
- The post-seed clear is one fixed boot boundary. A page-local Promise gates only
  public harness invokes until `clearInvocations` then settlement resolution in
  one task. Resolve in `finally` as well so an exceptional boot fails visibly
  instead of leaving external commands hung. The focused assertion deliberately issues the first command
  immediately after the seed row, without an empty-history poll; the full
  Playwright suite remains the regression gate for early setup flows.
- The existing `query_mention_candidates` fixture assigns `currentSnapshot`
  directly but changes neither session nor composer target; it remains outside
  the reconciliation choke point in this change.
- No private values are logged or added to errors. Tests use synthetic IDs,
  filenames, and bytes.

## Non-goals

- No product behavior, Rust state, browser-fake implementation, Tauri command,
  snapshot schema, or boot decomposition.
- No speculative byte/lease/history cap.
- No automatic global `beforeEach`; one page boot owns one automatic startup
  history clear and `clearInvocations` remains available within a test.
- No clearing command history from internal snapshot-returning commands or
  mid-test `setSnapshot` calls.

## Gates and acceptance mapping

| Requirement | Evidence |
| --- | --- |
| clear staging and target/session replacement release prepared bytes | Public Playwright preview tests after each transition |
| composer leases retire on target/session/account replacement | Public command rejection tests using old lease IDs |
| invocation history has deterministic ownership | automatic post-seed boot reset plus internal-command and mid-test `setSnapshot` preservation tests |
| browser-fake contracts are mirrored without duplicate product state | Diff inspection against existing target/lease helpers and the recorded #634/#641/#650/#651 lifecycle behavior |
| mock install/registration/snapshot/boot remain composed | Exact-file review; no module extraction |
| GUI/headless behavior remains valid | focused Playwright, full Playwright, full Vitest, typecheck/lint/build |
| repository integration remains valid | SDK guard, Rust workspace/all-targets, Tauri, wasm, QA binary, boundaries/docs/security/dependency gates and CI 7/7 |

Implementation must not begin until `reviewer-flash-opencode-go` records
`Correct-to-merge`. After implementation, review the exact full diff and all
RED/GREEN evidence before opening the PR.

## Review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. Moving the
  history clear to `setSnapshot` broke cumulative invocation assertions, and
  presence-only byte reconciliation missed actual logout/account replacement.
- Round 2, `reviewer-flash-opencode-go`: `Correct-to-merge`. The boot-only
  history boundary, Ready-account byte gate, real release/acquire lease evidence,
  and corrected lifecycle references resolved every blocker.
- Post-implementation Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`.
  The seed row can become visible during the retry loop's 25 ms yield, allowing a
  first external command to be recorded before the final clear.
- Pre-rework design review, `reviewer-flash-opencode-go`: `Correct-to-merge`.
  Gating only public harness invokes on clear-then-resolve settlement is
  deadlock-free; internal startup IPC remains direct and exceptional boot resolves
  through `finally`.

## Implementation evidence

- RED: with the final nine-test spec present and only the harness production
  patch temporarily reversed, the focused command exited 1: eight lifecycle
  cases failed and the explicit-release preservation case passed.
- GREEN: after restoring the same production patch, the unchanged focused spec
  passed 9/9. The focused lifecycle/regression set passed 40 tests.
- Boot-race rework: the immediate first-command test and all lifecycle cases
  passed 9/9, then 90/90 with `--repeat-each=10`; TypeScript typecheck and `git
  diff --check` passed.
