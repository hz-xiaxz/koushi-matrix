# Frontend Semantic Ownership Inventory (#552)

Status: refreshed evidence inventory as of `origin/main` `28a3dfb927d950e8a6724a933cb92e0c51111a01` (includes merged #683 leaf and releases through v0.3.1). This document classifies current owners; it does not itself migrate state or close #552.

Pinned epic contract: [`evidence/issue-552-contract.json`](evidence/issue-552-contract.json), SHA-256 `0371538cb18ab90b399fbd8114ec0678603ef3d24797e3f70d182898910c268f`, from <https://github.com/shinaoka/koushi-matrix/issues/552>.

## Classification key

- **Rust product/domain** — durable Matrix/product semantics or backend resource lifetime. React renders a projection and sends typed intent.
- **Renderer presentation** — DOM geometry, focus, transient overlays, immediate feedback, or view-local scheduling. Keep in React.
- **Transport/projection cache** — ordered copy of Rust snapshot/events, including gap/stale detection. Keep as a cache; it is not another semantic owner.
- **Test-backend mirror** — Browser Fake/harness behavior used to test the Rust contract. It is not production authority, but must stay equivalent and bounded.

Decisions: **keep**, **derive/delete**, **migrate leaf**, or **investigate**. A long-lived ref is not a migration target by size alone.

## Inventory

| Site / current owner | Lifetime and disappearance | Classification / authority | Settlement and duplicate semantics | Decision |
| --- | --- | --- | --- | --- |
| `domain/appStore.ts::applyAppStoreDeltas` | Page lifetime; reset on account/session snapshot clear | **Transport/projection cache**. Rust `StateDelta.generation` and snapshots are authoritative. | Drops duplicate/stale generations, requests full refresh on a forward gap; never invents domain transitions. | **Keep.** Already-correct #111 projection cache. |
| `domain/timelineStore.ts::TimelineStoreState`, `applyInitialItems`, `applyItemsUpdated` | Page lifetime, bounded inactive keys; keys clear on resync/account reset | **Transport/projection cache** for Rust `CoreEvent::Timeline`. Derived ID/timestamp indices are renderer acceleration only. | Actor/generation/batch fences reject stale diffs; pagination/gap fields copy Rust states. | **Keep.** Do not move maps/indices into Rust or reimplement the transport. |
| `backend/desktopApi.ts::DesktopApi`, link/media, attention, window/dialog and event ports, concrete browser/Tauri adapters, composition modules, and direct `@tauri-apps/*` imports | Page/runtime adapter lifetime; browser implementations exist only where behavior already existed outside native Tauri | **Renderer transport/platform adapters plus test-backend mirror.** Neutral interfaces have no implementation dependency; composition modules select or expose concrete adapters. | IPC names and Rust DTOs remain authoritative. Composition tests prove construction/selection; ports own stateless platform operations, not Matrix/product state. | **Phase 2 complete (2A and 2B1–2B4) as structural isolation; not semantic migration.** Every production Tauri import is adapter-owned and statically allowlisted. |
| Core `ThreadRootProjectionService` plus Room `TimelineActor::DisplayProjectionState` | Account/Room actor lifetime; dormant records survive bounded-window replacement and clear only from authoritative aggregate/redaction or room/session teardown | **Rust product/display lifecycle.** Core owns hydration, revisions, retained root snapshots, root/latest placement, stable display identity and explicit clear. Rust State mirrors typed transitions. | Display InitialItems/diffs are actor-generation-fenced and share one index domain; the 120-root admission bound includes dormant records; teardown cancels and awaits projection workers. | **Migrated in Phase 1 #708.** TypeScript has no projection map, bounded-window pruning, reply suppression, placeholder synthesis or placement decision; it maps Rust rows and retains renderer-only dividers/DOM behavior. |
| `TimelineView.tsx` mounted viewport controller (`pendingMeasuredHeightsRef`, anchors, range epochs, observers, frames) | Mounted timeline key; ordered teardown on key change/unmount | **Renderer presentation**. DOM measurement, virtualization, scroll anchors and visible-range facts have no backend owner. | ResizeObserver/frames/timers cancel at one key reset; Rust receives typed viewport facts. | **Keep.** #551 residual audit proves this cohesive DOM owner. |
| `TimelineView` Room DOM evidence, App Focused/Thread store evidence, and `backend/timelineAcknowledgementDelivery.ts` | Controller lives for the App renderer generation; resets on account/session owner change and disposes on App teardown. View unmount does not cancel accepted evidence delivery. | **Split renderer/transport/Rust boundary.** Renderer owns consumer evidence; the App adapter owns only bounded delivery to Core queue acceptance; Rust actor/request/generation/fence state owns all post-acceptance semantics. | Four closed channels (Room/Thread/Focused/repair), identical-intent coalescing, same-kind supersession, actor-aware identity, seven total attempts with finite backoff, fixed errors, explicit reset/dispose. | **Migrated in Phase 3.** React retry refs/timers are deleted; this renderer-specific transport owner exists because Core cannot retry evidence it never received. |
| `TimelineView.tsx::pendingKeyRequests`, `keyRequestEpochRef`, `keyRequestToast` | Mounted key/account; reset on timeline-key change and Rust terminal DTO | **Renderer presentation/investigate**, not product admission. Rust owns `DecryptRetryController::admit`, `begin_decrypt_retry`, `handle_request_room_key`, and `TimelineActor.key_request_states`. | Frontend Set suppresses pre-projection duplicate dispatch and handles delayed rejection/toast; Rust already coalesces same event/generation and owns terminal state. | **Keep for now / investigate.** No proven Rust semantic gap; do not migrate merely because it is a Set. |
| `TimelineView.tsx` avatar relevance/request/retry refs, App `requestedMemberAvatarMxcsRef`/`memberAvatarRetryCountsRef`, and `domain/avatarThumbnails.ts` | Mounted virtual/member window/key; clears with key/reset | **Renderer presentation** around a Rust-owned download command/cache. Relevance is DOM-window-specific. | Two-attempt request fence, retry release on typed event/failure, one shared teardown per surface. | **Keep.** #551 audit found no non-overlapping owner API. |
| `TimelineView.tsx` backfill epochs/evaluation/ref fences | Mounted key; cancels with projection/layout reset | **Renderer presentation** for when geometry warrants asking. Rust owns pagination operation/end state and SDK task. | Prevents repeated DOM-triggered requests until layout/projection settles; no Matrix history semantics synthesized. | **Keep.** Revisit only with a whole viewport-controller redesign. |
| `App.tsx::latestTextMutationQueueRef` / `applyLatestTextMutationSnapshot`, using `domain/latestAsyncResult.ts::createLatestMutationOperationQueue` | Page lifetime, keyed text mutations | **Partial migration.** Alias and main/thread caption mutations still require renderer serialization; invite and mention queries no longer use this queue. | Alias/caption A/B/A and invalidation retain latest-wins mutation admission; invite/mention dispatch every typed query and admit only Rust/appStore snapshots by their existing destination/request/generation fences. | **Migrate-leaf candidate 1 (Wave C, partial shipped).** Keep only the mutation queue; the invite/mention query semantic owner is now Rust request/generation state plus the monotone appStore fence. |
| `App.tsx::{room,space}Settings{Load,Request}Ref` | Page lifetime; demand marker resets on target navigation and same-target view replacement; request epoch advances for each local panel/profile intent | **Renderer presentation / transport fence.** Rust owns settings fetch/update semantics, correlated terminals and snapshot state; these refs own only whether a React panel intent remains current and whether its mount effect already dispatched. | Same-room equal-generation People results settle by latest renderer intent; load markers suppress duplicate effect dispatch, and only a still-current rejected request/target releases its marker for later retry. | **Keep; Phase 4.1 proven renderer-specific.** Rust cannot know panel/dialog lifetime, and appStore generation admission cannot order equal-generation same-room view intents. |
| `App.tsx::diagnosticsOpenIntentEpochRef` | Page/renderer lifetime; intentionally survives account replacement | **Renderer presentation.** Rust/Tauri own privacy-safe global/runtime diagnostic content; this epoch owns only which overlapping click may open the one dialog. Diagnostic snapshots are outside AppState/appStore and carry no request or state generation. | Both success and fixed-token failure settle only the latest open intent; stale completion cannot replace, annotate or reopen a later dialog. | **Keep; Phase 4.2 proven renderer-specific.** Moving dialog intent into Rust or serializing clicks would invert ownership/change behavior without a stronger backend fence. |
| `App.tsx::spaceMembersPanelOpenIntentEpochRef` and bounded `spaceMembersLoadDemandRef` | One current ready-account/Space/generation demand; record replaced on account/device/Space/generation change | **Renderer panel demand/resource.** Rust owns selection, generation, Loading request ID, SDK projection and persistent sync demand; React owns whether one panel open remains current and pre-projection duplicate suppression. | Full homeserver/user/device + Space/generation fence; one identity-safe in-flight/loaded record; old completion cannot apply, clear or mark a newer demand. | **Keep, bounded in Phase 4.3a.** Replaces unbounded account-less Map/Set; same-valued account replacement reloads. |
| `App.tsx` remaining room/Space navigation and Space-member invite/search/cancel/role request refs | Page/dialog/operation lifetime; manually incremented on navigation/close | **Renderer presentation / transport fence** around async command responses. Rust request IDs, demand generations and StateDelta ordering are authoritative. | Delayed promise result is ignored when local request ref/selection no longer matches. | **Investigate derive/delete by family.** Shared Space-member fence is account-aware after Phase 4.3a; family-specific epochs remain undecided. |
| `App.tsx` search debounce timer and query drafts | Dialog/view lifetime | **Renderer presentation** (typing draft and debounce of user intent). Rust owns search request/result correlation and crawler. | Timer clears on query/view changes; no durable retry or result semantics. | **Keep.** |
| `App.tsx` `pendingRoomLeave`, leave/confirm/dialog state, widths, pointer listeners, focus timers | Overlay/gesture lifetime | **Renderer presentation**. Matrix membership and operation state stay Rust-owned. | Explicit cancel/unmount cleanup; in-flight guard prevents accidental repeated UI intent. | **Keep.** Accessibility basics remain frontend-owned. |
| `App.tsx` composer overlays + debounce handles + `typingSignalRef` | Renderer/key lifetime, released on account/target/revision transitions | **Renderer presentation** over Rust `ComposerDocument`, revision, store, lease and typed-intent authority. | IME-safe local draft overlay settles only against accepted Rust revision; typing ref dedupes renderer intent; timer/overlay teardown is renderer-local. | **Keep.** Do not move DOM/input buffering. |
| `App.tsx::composerDraftLifecycleRegistryRef` | Page renderer generation; leases acquired/released through typed backend | Shared resource boundary: frontend owns renderer handle, Rust owns lease validity/account/target persistence. | Awaited acquire/release, generation replacement, #657 harness mirror cleanup. | **Keep.** One owner exists on each side of the typed lease boundary. |
| `App.tsx::submissionRegistryRef` and send overlays | Page/account/target; clears/settles from Rust submission IDs | Immediate presentation controller; Rust global submission registry/terminal state is authoritative. | Prevents local double UI settlement and preserves IME draft; terminal comes from Rust. | **Keep / audit only if duplicate transition is demonstrated.** |
| `App.tsx` State event/Core event/Tauri menu listeners + `stateRefreshTimerRef` | Page/runtime transport lifetime | **Transport resource owner** in the renderer. | Each effect/module listener has cleanup; refresh timer coalesces event gaps into authoritative snapshot fetch. | **Keep.** Backend task lifetime remains Rust-owned. |
| `App.tsx` QA send refs, diagnostics request generations, module error listeners | QA/page lifetime only | QA presentation/observability, not product state. | Reset by QA flow/page; privacy-safe diagnostics. Module error listeners overlap boot capture defensively. | **Keep; low-priority deletion audit** for duplicate error listeners, not a Rust migration. |
| `App.tsx` secure-backup retry in-flight ref and other button guards | View lifetime | **Renderer presentation** while Rust operation state is authoritative. | Avoids repeated click before snapshot; terminal/failure comes from Rust. | **Keep unless a reproducible duplicate command escapes Rust admission.** |
| `backend/browserFakeApi.ts` composer leases/draft maps/prepared bytes/submission ledger | Browser Fake instance/page | **Test-backend mirror** of Rust contracts. Not production state. | Bounded/reconciled by fake session/target generation; fixtures emulate terminal results. | **Keep as mirror; never cite as migration target.** Drift is test debt fixed against Rust. |
| `backend/browserFakeApi.ts` Activity/Space-member/search/settings local transitions | Browser Fake instance | **Test-backend mirror**, some duplicated state-machine logic intentionally required for browser tests. | Must install Rust-shaped snapshots and reproduce request/generation/failure guards. Merged #570/#582 behavior is part of the current Rust-shaped mirror contract. | **Keep and reduce duplication only in each reviewed contract migration.** |
| `apps/desktop/src/test/appHarnessMain.tsx::preparedUploadBytes`, `composerLeases`, invocation history | One Playwright harness page | **Test harness resource mirror**. #657 added snapshot/account/target reconciliation and boot history boundary. | Bytes and leases retire on authoritative replacement; invocation history has one boot boundary. | **Keep.** Already-correct reviewed lifecycle, not product state. |
| Pure dialogs, hover/focus/animation, alias drafts, media-viewer focus | Component mount/overlay | **Renderer presentation** | React cleanup and accessibility lifecycle only. | **Keep.** |

## Current migration-boundary evidence

- **Direct Tauri imports:** six production modules import `@tauri-apps/*`, all backend adapters: `backend/client.ts`, `backend/tauriTimelineTransport.ts`, and `backend/tauri/{desktopAttentionPort,desktopEventPort,linkMediaPort,windowDialogPort}.ts`. App, domain, hooks, composition roots, and neutral contracts are Tauri-free; ESLint and the App source contract enforce this exact allowlist.
- **Neutral API/port dependency:** `DesktopApi` lives in `backend/desktopApi.ts`; `TauriDesktopApi` and `BrowserFakeApi` implement it; `appRuntime` selects it. Link/media, attention, window/dialog and event operations use neutral contracts plus composition modules. `DesktopEventPort` unwraps only Tauri envelopes/channel names; App retains listener disposal, ordering, debounce, QA and menu semantics, while `tauriTimelineTransport` retains its readiness barrier and command adapter. All runtime checks import the leaf `backend/runtimeEnvironment.ts`. Phase 2 is structurally complete.
- **Remaining mutation queue:** `App.tsx::latestTextMutationQueueRef` has only local-alias and main/thread staged-caption `run`/`invalidate` users. Invite and mention query keys were removed by merged PR #683.
- **Acknowledgement ownership:** TimelineView captures Room DOM evidence; App's canonical store captures Focused/Thread application evidence. One App-lifetime `timelineAcknowledgementDelivery` controller owns at most four finite pre-Core delivery jobs and survives view unmount. Queue acceptance is its terminal. Tauri command shapes are unchanged; Core routes accepted intent to `TimelineActor`, whose actor/request/generation and repair fences own acceptance, stale rejection, continuation and repair timeout.
- **App request-ref families:** Phase 4.1 proves room/Space settings panel demand is renderer-specific; Phase 4.2 proves diagnostics open intent is renderer-specific; Phase 4.3a proves Space-member panel open/pre-projection load demand is renderer-specific and bounds it to one full-account record. Room/Space navigation and Space-member invite/search/cancel/role remain separate investigation families. Button/in-flight refs and local dialog lifetimes remain renderer presentation unless a duplicated semantic transition is proven.

## Already-correct Rust-owned paths

The following are not migration work:

- Application/session/settings/invite/Activity/Space-member state in `koushi-state::AppState`; React consumes snapshots/deltas (`docs/agents/state-ownership.md`, “The boundary”).
- Composer persistence, revision, lease admission and send/submission terminals in Core/state. Frontend overlays are IME/render-local.
- Timeline SDK actors, pagination, repair, thread attention, read-state outbox, media tasks and room-key recovery in Core. `timelineStore` is a projection reducer, not the SDK owner.
- Room-key request admission/coalescing (`DecryptRetryController::admit`, `begin_decrypt_retry`, `handle_request_room_key`) and per-event projected request state (`TimelineActor.key_request_states`).
- Search crawler, directory, room/Space operations and current-session status. App request refs only fence stale renderer promises.
- Harness cleanup from #657, invite admission from #658, composer-load evidence from #645, KaTeX admission from #668, transient projection/trust-loss reset contracts from #660, and the later Rust-owned read-state, Activity/redaction, Space-role, authentication-invalidation, room-list session-fence, verification, active-session management, and login/store lifecycle changes merged through #702.

## Duplicated semantics requiring evidence before change

1. **Resolved in Phase 1 #708: thread-root projection lifecycle and placement** — Core service/Room actor now own lifetime and placement; State mirrors explicit transitions and TypeScript only caches/renders Rust display items.
2. **Remaining alias/caption mutation ordering** — the TS mutation queue still serializes local-alias and staged-caption writes because current Rust admission does not yet prove latest-intent settlement for those mutation families. Invite/mention query ordering is already migrated.
3. **Projection ACK retry/backoff** — frontend owns reliable-delivery policy while Rust owns actor terminal waiting. DOM evidence must remain frontend; transport retry may move.
4. **Remaining App promise request refs** — Phases 4.1–4.3a resolve settings, diagnostics and Space-member panel/load demand as necessary renderer fences/resources. Navigation and Space-member invite/search/cancel/role paths may still duplicate Rust request/demand generations, but each family needs an equivalence test before deletion.
5. **Browser Fake transitions** — intentional test mirror, not production duplication. Change only alongside the corresponding Rust contract.

Room-key `pendingKeyRequests` is excluded from this list until a semantic gap is proven: Rust already owns operation admission/coalescing; the Set is optimistic presentation and dispatch suppression.

## Ranked disjoint leaf candidates

### 1. Current Phase 1 result: Rust-owned thread-root projection lifecycle (#708)

- **Value:** fixes observed thread disappearance/reappearance by removing bounded-window deletion and placement authority from State/TypeScript.
- **Owner:** `ThreadRootProjectionService` plus current Room `TimelineActor::DisplayProjectionState`; TypeScript remains cache/render only.
- **Proof:** deterministic RED captures State/frontend bounded-window deletion; GREEN covers dormant retention, authoritative zero/redaction/unsubscribe clear, Rust-owned root/latest ordering, existing-thread initial pagination terminal policy, bounded awaited teardown, wire removal, renderer-only input, and replay/repair/event-order convergence.
- **Scope:** this reviewed #708 PR precedes adapter isolation and remains disjoint from later ACK/request-ref/mutation work.

### 2. Shipped: retire invite/mention query admission from the latest-text queue

- **Value:** removes the second “which async result wins” semantic owner from App for invite and mention queries while preserving the queue where it serializes unversioned mutations.
- **Proof required:** delayed invite and main/thread mention A/B/A dispatch, adversarial settlement, explicit monotone nonzero snapshot generations, rendered final projection, and account/room/dialog replacement fences. Alias/caption A/B/A serialization and invalidation stay green.
- **Scope:** `App.tsx`, `domain/latestAsyncResult.ts`, focused App/latestAsync/appStore tests, and this inventory/plan; no new dependency, Rust/Tauri API, fake semantic, or Rust abstraction.
- **Current result:** merged PR #683 migrated invite target search and main/thread mention query admission to existing Rust request/generation state plus `appStore`; alias and main/thread caption mutation serialization remain renderer-owned pending separate reviewed contracts.
- **Disjointness:** merged #659 changed room-list reducer admission; #608 authentication invalidation diagnostics/copy; #559 read-state local/server boundaries; and #570 Activity/redaction/thread convergence. None share this query leaf.

### 3. Move projection/repair ACK retry policy to a reliable transport owner

- **Value:** a mounted view currently owns backoff/attempt terminal policy for a Rust actor resource.
- **Boundary:** React still computes committed DOM evidence and sends one typed observation. Tauri/Core owns reliable retry, cancellation and actor-generation settlement.
- **Risk:** cross-file actor/transport design; larger than candidate 1.

### 4. Retire redundant App request refs per command family

- **Value:** remove local stale-result fences already represented by Rust request IDs/generation and appStore admission.
- **Method:** one family per PR, exact delayed-result test, no generic request manager.
- **Phase 4.1 result:** room/Space settings refs are retained with explicit ownership proof; same-room equal-generation supersession, panel replacement, pending dedupe and rejection/retry checks show that they protect local view demand rather than duplicate Rust settings authority.
- **Phase 4.2 result:** diagnostics open intent is retained and named explicitly; overlapping success/failure, close/no-reopen and source-boundary checks prove that Rust owns diagnostic content while React owns one dialog's latest open intent.
- **Phase 4.3a result:** Space-member panel/open load demand remains renderer-owned but is reduced from unbounded account-less Map/Set bookkeeping to one account/Space/generation record; Rust retains projection and operation authority.
- **Risk:** some refs protect purely local selection/dialog lifetime and should remain.

Low priority: consolidate duplicate QA error listeners after a behavioral boot-error proof. This is deletion, not Rust migration.

## Shipped changes since the prior inventory base

- #659 / PR #675: room-list session-fence acceptance is merged.
- #570 / PRs #676, #680 and #682: SDK aggregates plus Activity/thread/room-latest redaction/edit convergence are merged; #570 is closed.
- #582 / PR #677: direct Space-member role management is merged; #582 is closed.
- #608 / PR #681: UnknownToken authentication invalidation is separated from E2EE trust; #608 is closed.
- #552 / PR #683: invite/mention query ordering is migrated to existing Rust authority.
- #559 / PR #684: local-viewed and server-confirmed read-state convergence is merged; #559 is closed.
- #694/#699 work through PRs #695, #697 and #702 strengthens Rust-owned verification, active-session management, and login/store lifecycle without creating a new frontend semantic owner.

## Disjoint issue contracts

The following contracts are now shipped and remain disjoint evidence, not pending #552 work:

- #659: fail closed before late room-list readiness/invites/rooms/spaces mutation.
- #608: classify UnknownToken authentication invalidation separately from E2EE trust and update locked UI copy.
- #559: split local viewed boundary from server-confirmed read state and bound persistent retry/outbox behavior.
- #570: redacted/edit convergence in Activity/unread/thread/conversation projections.

The shipped invite/mention query leaf touches none of those owners; alias/caption mutation serialization remains the separate retained owner in App.

## #552 acceptance status

| Epic criterion | Phase 0 status and evidence |
| --- | --- |
| Publish evidence-based inventory/classification | **Complete:** refreshed inventory above is pinned to current main and source/merge evidence. |
| Identify already-correct Rust-owned/projection-only paths | **Complete:** listed above, including `appStore`, `timelineStore`, Core actors, and renderer-only DOM owners. |
| Identify duplicated Rust/TS semantics | **Complete as inventory; partial as migration:** #708 thread-root duplication and Phase 3 view-owned acknowledgement retry are removed; Phases 4.1–4.3a classify settings, diagnostics and Space-member panel/load demand as renderer-specific, while remaining request families and alias/caption sequencing retain task-level proof gates. |
| Migrate selected high-value leaf owners incrementally | **Partial overall:** PR #683 migrated invite/mention query admission, Phase 1 migrated thread-root lifecycle/placement, Phase 3 migrated acknowledgement delivery, Phases 4.1–4.2 resolve settings/diagnostics, and Phase 4.3a bounds Space-member panel demand; other request-ref and mutation leaves remain. |
| One documented semantic owner per migrated subsystem | **Partial overall; complete through Phase 4.3a:** Rust owns invite/mention, thread-root, room-settings, diagnostic content and Space-member projection/operations; the renderer owns settings/Space-member panel demand, diagnostics dialog intent and ACK evidence; bounded adapters own only pre-Core delivery/dedupe. |
| Async Rust owners have cancellation/awaited settlement where required | **Partial overall:** #708 projection workers cancel and await on room/session teardown; Phase 3 introduces no Rust task and its renderer adapter synchronously cancels bounded timers on reset/dispose. Later owner phases remain. |
| Remove corresponding TS semantic state after cutover | **Partial overall:** invite/mention queue keys, #708 projection maps/pruning/placement, TimelineView ACK retry refs/timers, and unbounded Space-member load Map/Set are removed; retained settings/diagnostics/Space-member panel refs are documented renderer presentation, while mutation and other request fences remain pending proof. |
| Frontend cleanup primarily renderer-local | **Complete for current/shipped paths:** kept frontend owners are classified as presentation, transport cache, adapter resource, or test mirror. |
| Preserve Tauri command/event compatibility unless separately reviewed | **Complete to date:** inventory and PR #683 changed no Rust/Tauri command, event, DTO, or IPC name; later contract changes remain separately gated. |
| Focused transition/teardown/projection-equivalence tests | **Partial overall; complete through Phase 4.3a:** deterministic deferred query/settings/diagnostics/Space-member tests cover monotone snapshots, equal-generation settlement, panel replacement, pending dedupe, failure/reopen, account replacement, bounded demand and stale no-apply; future leaves require their own RED/GREEN tests. |
| Compatible with Tauri UI and future native Rust renderer | **Complete as an architectural invariant:** Core remains transport-neutral and React/Tauri remains supported; Phases 1, 2 and 6 strengthen evidence without making GPUI part of #552 closure. |

#552 stays open. The refreshed inventory and one shipped semantic leaf do not satisfy the remaining migration criteria.
