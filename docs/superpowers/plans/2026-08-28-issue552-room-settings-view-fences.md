# Issue #552 Phase 4.1 — Room-settings request-fence decision

Status: implemented, locally verified and exact-final-diff approved. `reviewer-flash` design Round 4 returned `Correct-to-merge` before implementation; exact-diff Round 3 returned `Correct-to-merge` after Round 2's sole Minor finding (the stale Phase 3 task-plan merge status) was fixed. All six focus areas (RED tests, current request/target failure release, pending dedupe, Space navigation epoch, single panel invalidation and renderer ownership) passed with no findings. Before the production edit, the corrected behavioral RED proved rejection remained unhandled and close/reopen stayed at one call, while the source contract independently failed on missing catch/Space epoch/single-invalidation; unchanged pending-dedupe and Space-panel supersession checks stayed GREEN. Focused post-fix GREEN is 122/122.

## Decision

**Keep** `roomSettingsRequestRef`, `spaceSettingsRequestRef`, `roomSettingsLoadRef`, and `spaceSettingsLoadRef` as renderer-specific view-intent/demand owners. Do not move them to Rust and do not replace them with a generic request manager.

This phase is a Keep decision with one narrow renderer-demand cleanup fix, not a forced deletion. Rust already owns Matrix room-settings fetch/update semantics and request-correlated Core terminals. The retained refs do not decide room settings; they answer questions Rust cannot know: whether the same React panel/dialog intent is still current and whether a mount effect already dispatched a load while no load-state projection exists.

## Traced boundary

```text
Room/Space panel or same-room People/Profile intent
  -> App loadRoomSettings(room_id)
  -> Tauri allocates Core RequestId
  -> RoomCommand::LoadRoomSettings
  -> RoomActor SDK fetch
  -> Rust RoomSettingsSnapshotLoaded + RoomSettingsLoaded(request_id)
  -> Tauri waits for matching request terminal
  -> current versioned snapshot
  -> App exact target/view-intent admission
  -> monotone appStore snapshot admission
```

### Rust authority already present

- Every load receives a Core `RequestId`; Tauri waits only for the matching `RoomSettingsLoaded` or correlated failure under one deadline.
- Rust owns SDK fetch, privacy-safe settings/member projection, `room_management.selected_room_id/settings`, update operation request IDs, permissions and update/moderation/role terminals.
- `setSnapshot` enters the monotone appStore boundary; stale snapshot generations do not replace newer state.

### Why the refs are not duplicate Rust semantics

1. `RoomManagementState` deliberately has no public "panel demand" or "dialog still open" field. A successful settings load is valid Rust state even when the initiating React panel has disappeared.
2. `roomSettingsRequestRef` distinguishes competing **same-room** view intents. Navigation identity cannot distinguish two People/Profile opens in the same room. The existing adversarial test resolves two same-room Room Info→People loads in reverse order with the same state generation; only the renderer epoch prevents the older intent from replacing the newer member presentation.
3. `spaceSettingsRequestRef` invalidates an in-flight Space-info load when the renderer changes to People/Profile without changing active Space/account identity. Rust cannot infer that local panel replacement from the load command.
4. `roomSettingsLoadRef` / `spaceSettingsLoadRef` are mount-effect demand dedupe. Loads do not project a Rust `Pending` operation, so unrelated snapshot/effect re-renders would redispatch the same valid request without these one-shot markers.
5. Current completion admission also requires the current navigation request, current active room/Space in both live and returned snapshots, and `exactRoomSettingsForRoom`. Those checks preserve presentation scope; they do not override Rust settings authority.

Deleting these refs would either reintroduce duplicate loads or allow a valid-but-superseded same-target response to reopen/replace the wrong local panel. Adding Rust panel-demand state would invert the ownership boundary and expose renderer semantics in AppState for no lifecycle benefit.

## Scope

In this PR:

- add an ownership comment beside the four refs in `App.tsx`;
- add explicit rejection handling to the Room-info and Space-info load effects: only the still-current request/target clears its load marker, allowing a later panel reopen/dependency transition to retry without an unhandled rejection;
- make the Space-info effect capture/check `spaceNavigationRequestRef` (its actual navigation owner) instead of the coincidentally co-incremented room epoch;
- remove `openSpaceMembers`' explicit `spaceSettingsRequestRef` bump because `setRightPanelModeClosingFocusedContext("people")` already performs the same invalidation before its first await;
- add an `App.test.tsx` source contract tying request epochs to same-room view-intent admission and load refs to effect dedupe/failure release;
- retain and rerun the existing behavioral tests for late active-room/DM results and equal-generation same-room supersession;
- add deterministic tests proving:
  - Room-info load rejection is swallowed, releases only its current demand marker, and a close/reopen retries;
  - one pending Room-info effect is not duplicated by close/reopen before failure/settlement;
  - a pending Space-info settings result cannot apply after the same-Space panel moves to Members/People;
- update inventory/canon/Phase 4 status.

The production change is limited to renderer demand cleanup and the correct local navigation epoch. Rust state, Tauri command, API/DTO, BrowserFake behavior, dependency, and IPC are unchanged.

## Verification

Before production edits, add the Room-info rejection/reopen behavioral check and the effect/source ownership contract. They are RED because current effects have no catch/release and still use the wrong Space navigation epoch plus duplicate invalidation. Add the pending-dedupe and Space-info panel-replacement checks as adversarial unchanged GREEN proof for the refs retained by the Keep decision; do not fabricate a deletion.

Run:

- focused `App.spaceMembers.test.tsx` and App source tests;
- full Vitest and Playwright;
- typecheck, lint, build and normal boundary/privacy/docs gates;
- exact-final-diff reviewer approval and current-head CI.

### Local verification evidence

- focused App suites: 122/122;
- full Vitest: 1494/1494;
- Playwright DOM tier: 263/263 (the first symlinked-dependency run exposed a font asset network error; lockfile-local `npm ci` corrected the worktree environment and both focused and full reruns passed);
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter and domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2535 passed/12 ignored), Tauri tests (175 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

No real-homeserver lane is added for this renderer-only panel-demand change: Core/Tauri room-settings command, snapshot, DTO and IPC behavior are unchanged; the deterministic BrowserFake/App DOM tests exercise the changed completion boundary directly.

## Expected files

- `apps/desktop/src/App.tsx` (comments and bounded effect cleanup/epoch correction)
- `apps/desktop/src/App.test.tsx` (source ownership contract)
- `apps/desktop/src/App.spaceMembers.test.tsx` (behavioral failure/dedupe/panel-replacement proof)
- ownership inventory, state-ownership canon, remaining-phase plan/index docs and the stale Phase 3 task-plan merge status

## Acceptance

- every room/Space settings ref has a documented renderer-only lifetime and concrete behavioral proof, including failure release and same-Space panel replacement;
- no ref is misclassified as Matrix/settings authority;
- rejection is handled without raw error/log exposure, demand markers release only for the current request/target, and no duplicate request is dispatched while pending;
- Space completion uses `spaceNavigationRequestRef`, and panel invalidation bumps its settings epoch exactly once;
- no speculative Rust panel state or generic queue is introduced;
- Phase 4.1 is complete and later request families remain separately gated.
