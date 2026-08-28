# Issue #552 Phase 3 — Timeline acknowledgement delivery ownership

Status: implemented and fully verified on the approved branch; pending exact-final-diff review and merge.

## Decision

Choose Phase 3 option 2: **adapter-owned retry**.

The renderer remains the sole owner of consumer evidence: Room projection/repair evidence comes from TimelineView's committed DOM/layout boundary, while existing Focused/Thread projection evidence comes from the App-level canonical-store application boundary. Each path emits one typed acknowledgement intent. A bounded App-lifetime transport controller owns delivery only until the existing Tauri/Core command queue accepts that intent. Once `submit_core_command` returns success, Rust owns routing, actor-generation/request/generation admission, repair continuation or timeout, and stale acknowledgement rejection. No Rust retry task is introduced.

This is a renderer-specific exception to the Rust-owner invariant: Core cannot retry evidence it has never received, while `TimelineView` is too short-lived to own pre-Core transport delivery. The App transport adapter outlives room/thread/focused view mounts and owns no Matrix semantics.

## Traced current boundary

```text
TimelineView stable Room layout frame OR App-level Focused/Thread store application
  -> timelineProjectionEvidence / exact repair fence
  -> App acknowledgement controller / App-composed TimelineTransport
  -> TauriDesktopApi invoke
  -> navigation.rs submit_core_command
  -> bounded Core command queue acceptance
  -> AppCommand
  -> AccountActor -> TimelineManager -> TimelineActor
  -> projection request/generation fence OR repair render fence
  -> actor continuation / stale ignore / Rust repair timeout
```

### Before Core acceptance

`TauriDesktopApi.acknowledgeTimelineProjection` and `acknowledgeTimelineBatchRendered` resolve only when the Tauri command's `submit_core_command` succeeds. That helper clones the command handle and awaits a bounded queue send under `CORE_COMMAND_SUBMIT_TIMEOUT`; it rejects on timeout or closed command transport. The current TimelineView retry timers address only this pre-Core failure for Room projections/repair. App's existing Focused/Thread post-store `void api.acknowledgeTimelineProjection(...)` has no retry or rejection handling and also bypasses those timers.

### After Core acceptance

- Projection acknowledgement is routed through a response channel. The active actor accepts only the exact projection request ID and timeline generation under its actor-generation lease; duplicates/stale actor or generation values are ignored. Focused navigation and initial gap inspection are Rust-owned consequences.
- Repair acknowledgement carries actor generation, timeline generation, repair generation and minimum batch ID. The actor accepts only the matching/new-enough render fence; duplicates/stale values are ignored. Rust already owns a bounded render-settlement timeout that clears/requeues the fence.
- The Tauri promise does not await actor acceptance. Retrying after the command queue accepted would duplicate Core ingress and does not improve actor admission. Therefore the delivery terminal is **Core queue acceptance**, not an actor event.
- Residual, pre-existing Rust behavior is explicit: an enqueue-accepted projection acknowledgement that the actor rejects as stale leaves `pending_focused_navigation` armed. The existing `focused_anchor_action_is_impossible_before_actor_acceptance` test proves rejection retains the pending intent and a later exact actor-accepted acknowledgement settles it; the next navigation command can also replace/clear it. There is no Core timeout for this pending focused intent. This PR neither worsens nor hides that post-acceptance Rust policy.

## Current defect

`TimelineView` owns two retry records and browser timers. They are cancelled on key reset and unmount. Thus evidence captured and submitted just before the view disappears can be lost when the first pre-Core submission fails. The retry loop is also unbounded: attempts cap at six only for delay calculation, while timer reconstitution continues indefinitely while mounted.

## Bounded delivery controller

Add `apps/desktop/src/backend/timelineAcknowledgementDelivery.ts`, a family-specific controller (not a generic request manager).

### Inputs

The constructor receives exactly two submission functions with the current IPC argument shapes:

- projection: request ID, timeline key, **delivery-only actor generation**, timeline generation, item count, target-present;
- repair: timeline key, actor generation, timeline generation, repair generation, batch ID.

It also accepts an injectable scheduler for deterministic tests. Production uses browser timers.

### Ownership and bounds

The controller owns at most four jobs: latest Room projection delivery, latest Thread projection delivery, latest Focused projection delivery, and latest repair delivery. Projection delivery selects one closed channel from `TimelineKey.kind`; this permits the simultaneously mounted Room main pane and Thread/Focused consumer to settle independently while bounding old-key work. A newer identity supersedes only the older job in the same kind.

For each closed channel it retains:

- full typed payload and an opaque in-memory identity from all fence fields plus `timelineStoreKeyId(key)`. The identity is never logged or serialized. Projection identity includes the actor generation already present in the TimelineView render signature, even though Core's projection command does not serialize it;
- one promise shared by duplicate callers;
- attempt count;
- one scheduled retry handle;
- a monotone in-memory job token fencing late promise settlement.

Policy:

1. First attempt is immediate.
2. On pre-Core rejection, retry after 50, 100, 200, 400, 800 and 1,600 ms.
3. Seven total attempts are the hard maximum. Final failure rejects, removes the job, and latches that exhausted identity; identical later calls reject without submitting or recreating a timer. A different fence identity or account reset clears the latch. With six delays the backoff horizon is 3.15 seconds, plus up to the existing per-attempt Core-submit timeout. An outage beyond that finite horizon abandons the exact delivery until Rust emits a new identity or the account owner resets.
4. An identical active intent coalesces to the same promise. When the requested identity differs from active work, supersede that work before consulting the accepted cache; returning to an already accepted A must cancel pending B but resolves A without re-submission.
5. A newer identity in the same projection-kind/repair channel cancels/rejects the pending older job and starts immediately. Late completion from the old job is ignored by token/current-job identity. This covers key/actor/generation A→B→A replacement without stale settlement.
6. `reset()` synchronously cancels/rejects both jobs and clears accepted identities for account/session replacement; the controller remains reusable.
7. `dispose()` synchronously cancels/rejects both jobs and permanently rejects later delivery. App invokes it on renderer teardown.

At most four timers/four payloads exist; retries, memory and backoff are bounded. Errors are fixed transport tokens and never include Matrix identifiers or raw transport text.

## App owner and teardown

In `App.tsx`:

- add one `timelineAcknowledgementDeliveryRef`;
- add a lazy getter that creates the controller with the existing `api.acknowledgeTimelineProjection` / `api.acknowledgeTimelineBatchRendered` calls;
- make the two acknowledgement methods in `appTimelineTransport` delegate to that getter; all other transport overrides stay unchanged;
- route the existing Focused/Thread acknowledgement issued after `applyTimelineEventWithProjectionResultAndRetention` through the same getter, add a terminal `.catch(() => undefined)`, and keep it after successful store application;
- in the existing account-owner-change effect keyed by session homeserver/user/device/kind (`App.tsx`'s `retireComposerRendererGeneration` path), call `reset()` if the controller exists;
- on App effect teardown call `dispose()` and clear the ref.

Lazy creation avoids a dead browser-fake resource. Clearing the ref after cleanup is StrictMode-safe: a later renderer setup can create a fresh controller. TimelineView unmount does not reset/dispose the App owner.

## TimelineView cutover

Keep all existing stable-layout guards, request/actor/timeline/repair fence derivation, evidence calculation, scheduled animation frame, last-success signature and in-flight dedupe refs.

Delete only:

- `projectionAcknowledgementRetryRef`;
- `repairAcknowledgementRetryRef`;
- their key-reset/unmount timer cleanup;
- both catch branches' attempt/backoff/timer mutation and their `setProjectionSettlementRevision` retry bumps;
- `projectionSettlementRevision` only from the acknowledgement layout effect dependency array.

Retain `projectionSettlementRevision` state, its non-retry writers in `scheduleBackfillEvaluation`/anchor follow-up frames, and the later backfill-evaluation layout effect dependency. That revision is the explicit trigger that consumes `pendingBackfillEvaluationRef`; removing it would break same-length layout/live-edge/pagination/prepend settlement evaluation.

On transport rejection, each catch branch only clears its matching in-flight signature. It does not retry, mutate product state, emit a log, or mark success. A resolved controller promise still records the existing last-success signature. The controller, not the component, may continue delivery after view unmount.

`TimelineProjectionApplication` retains the InitialItems `actor_generation` in its applied result. The internal `TimelineTransport.acknowledgeProjection` method also adds `actorGeneration` as delivery metadata. TimelineView passes its current actor generation; App passes it to the controller identity but deliberately does not forward it to `DesktopApi`. This prevents an accepted identity from an old actor from suppressing the replayed projection acknowledgement when actor generation changes while request/timeline generation values are reused. DesktopApi, Tauri command names/args, Rust command/event/state shapes, browser fake no-op acknowledgements, DOM evidence, and IPC compatibility remain unchanged.

## Deterministic verify-first evidence

Before production edits:

1. Add controller tests with a manual scheduler/deferred promises proving:
   - first submission rejects, one scheduled retry recovers;
   - duplicate identity coalesces and accepted identity never re-submits;
   - an accepted projection for actor N does not suppress a replay with identical request/timeline generation at actor N+1; the new actor forces one new submission;
   - Room and Thread/Focused projection channels deliver independently;
   - pending A is superseded by B in the same kind, late A completion is ignored, and a later A with a new fence identity is admitted;
   - seven failures reject with no remaining timer/job;
   - `reset` and `dispose` cancel timers and fence late completion;
   - resolved Core queue acceptance schedules no retry.
2. Rewrite both existing component-owned retry tests in `TimelineView.scrollback.test.tsx`: replace "retries a rejected rendered-batch acknowledgement" with an externally owned controller transport that proves one 50 ms controller retry recovers, and replace "cancels superseded acknowledgement retry timers on unmount" with RED evidence that the same externally owned job survives TimelineView unmount and resolves. On the pre-fix component-owned implementation, the first test depends on React's timer and the second cancels the only retry, so the new expectations fail.
3. Keep stable-frame, exact projection evidence, exact repair fence and once-per-signature tests green.
4. Keep existing Rust tests green for exact projection request/generation admission, matching repair fences, stale rejection/idempotence, Rust repair timeout recovery, and `focused_anchor_action_is_impossible_before_actor_acceptance` (actor rejection retains pending navigation; later accepted acknowledgement settles it). Add no Rust production path or retry task.

No fixed sleeps or log assertions.

## Expected files

- `apps/desktop/src/backend/timelineAcknowledgementDelivery.ts` (new)
- `apps/desktop/src/backend/timelineAcknowledgementDelivery.test.ts` (new)
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/App.test.tsx`
- `apps/desktop/src/components/TimelineView.tsx`
- `apps/desktop/src/components/TimelineView.scrollback.test.tsx`
- `apps/desktop/src/components/timeline/TimelineTransport.ts`
- `apps/desktop/src/domain/timelineStore.ts`
- `apps/desktop/src/domain/timelineStore.test.ts`
- ownership inventory, state-ownership canon and remaining-phase plan/index docs

No Rust production, State, SDK, Tauri command, IPC/DTO, generated artifact, BrowserFakeApi, harness, CSS or dependency change is expected.

## Verification matrix

- focused delivery-controller, App source, TimelineView projection/repair tests;
- focused Core projection/gap-repair actor tests proving post-acceptance ownership;
- full frontend Vitest/Playwright/typecheck/lint/build;
- Rust workspace/core/state/SDK/Tauri tests, rustfmt, wasm, QA/source/wire/golden/generated guards where applicable;
- SDK submodule, docs/agents, adapter/domain boundaries, secret/privacy scan and `git diff --check`;
- exact-final-diff `reviewer-flash` approval and current-head CI before merge.

## Implementation evidence

- RED: controller test failed because the delivery module did not exist; the App source contract failed on both TimelineView retry refs/backoff; legacy unmount behavior cancelled the only retry.
- Focused controller/store/App/TimelineView tests: 5 files / 201 tests passed.
- Full Vitest: 99 files / 1489 tests passed.
- Playwright: 263 tests passed.
- Core post-acceptance focused projection/gap-repair fence tests passed; full Core 1076 passed / 8 ignored, State 40, SDK 169, Tauri 170 passed / 1 ignored.
- Typecheck, lint/IME/docs, build, rustfmt, adapter/domain guards, SDK-submodule check, secret scan, and `git diff --check` passed.

## Design review record

- Round 1 timed out before reading the design and returned an unverified `Not correct-to-merge`; it established no design defect.
- Round 2, `reviewer-flash`: core option-2/queue-terminal decision was sound, but the verdict was `Not correct-to-merge` due to two Important completeness gaps and two Minor precision gaps. Both legacy retry tests, post-acceptance focused-navigation residual/finite outage horizon, and exact existing account-owner reset hook were incorporated.
- Round 3 identified one new Important correction: `projectionSettlementRevision` is also the backfill scheduler's explicit trigger. The amended cutover now removes only retry bumps and the acknowledgement-effect dependency while retaining the state, non-retry writers and backfill-effect dependency. Round 4 confirmed that correction.
- During verify-first implementation, the controller identity audit found that projection actor generation is renderer-known but absent from the serialized command. The internal TimelineTransport/controller signature now carries it only for delivery supersession/dedupe while preserving IPC. Round 5 approved that identity boundary.
- Continued caller audit found App's existing Focused/Thread post-store acknowledgement bypassed TimelineView retry and can overlap Room delivery. The final controller has three closed projection-kind channels plus repair, and `TimelineProjectionApplication` carries actor generation into that path. A focused Round 6 approves the complete caller/bound boundary before implementation continues.

## Acceptance

- DOM evidence remains renderer-owned and one-shot;
- one App-lifetime controller owns only bounded pre-Core delivery;
- accepted command submission never enters another renderer retry loop;
- view unmount cannot cancel an already-created delivery job;
- key/actor/generation replacement, duplicate, exhaustion, reset and teardown are deterministically fenced;
- no React acknowledgement retry ref/timer remains;
- Rust remains the sole post-acceptance semantic owner; #552 stays open for later phases.
