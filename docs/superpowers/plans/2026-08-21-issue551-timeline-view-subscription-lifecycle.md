# Issue #551 TimelineView subscription-lifecycle extraction

Status: design review pending. Behavior-preserving lifecycle-owner refactor.

## Baseline

- Base: `bd128a2a77b99bf35873c273ce880715721e8e6e` (merged PR #605 including concurrent #591 delivery).
- `apps/desktop/src/components/TimelineView.tsx`: 4,288 newline-delimited lines.
- Focused immutable baseline: all seven `TimelineView.*.test.tsx` suites, 175/175.

## Ownership decision

Create direct private hook leaf `apps/desktop/src/components/timeline/useTimelineEventSubscription.ts` as the sole React owner of:

1. `TimelineTransport.listenCoreEvents` registration and returned unsubscribe cleanup;
2. the 120ms empty-initial-window `ensureSubscribed` fallback timer and `clearTimeout` cleanup.

The 330-line event projection callback remains in `TimelineView`, where its store application, viewport/backfill/projection refs and product-side effects belong. It becomes a `useCallback` with exactly the old listener effect dependency list, preserving when React tears down/re-registers the listener. The hook receives that callback and only the three current-value refs/facts needed by the fallback fence.

This is not a generic effect wrapper or callback registry: it is transport-specific and owns two existing coupled subscription resources. It avoids a 30-field controller object while making unsubscribe/timer teardown explicit in one module.

## New leaf contract

Private options type `TimelineEventSubscriptionOptions` contains exactly:

- `transport: TimelineTransport`
- `onEvent: (payload: CoreEventPayload) => void`
- `itemCount: number`
- `timelineKeyHash: string`
- `timelineKeyHashRef: RefObject<string>`
- `timelineKeyRef: RefObject<TimelineKey>`
- `initialItemsSeenForTimelineKeyRef: RefObject<string | null>`

The leaf imports exactly three statements:

- `useEffect` and type-only `RefObject` from `react`;
- type-only `CoreEventPayload`, `TimelineKey` from `../../domain/coreEvents`;
- type-only `TimelineTransport` from `./TimelineTransport`.

It contains private constant `TIMELINE_SUBSCRIBE_FALLBACK_DELAY_MS = 120` and exports only `useTimelineEventSubscription`.

## Parent transformation

- Add type-only `CoreEventPayload` to the existing coreEvents import.
- Remove `TIMELINE_SUBSCRIBE_FALLBACK_DELAY_MS`.
- Replace the listener `useEffect` with `const handleTimelineCoreEvent = useCallback((payload: CoreEventPayload) => { ...existing callback body verbatim... }, [...exact old dependency list...]);`.
- Replace the listener and fallback effects with one unconditional `useTimelineEventSubscription({...})` call at the same location, after the scroll-diagnostics effect and before the existing general cleanup effect.
- The hook listener effect is exactly `useEffect(() => transport.listenCoreEvents(onEvent), [onEvent, transport]);`.
- Move the fallback effect body verbatim, substituting only `items.length`→`itemCount` and the private delay constant's module location.

Only `TimelineView.tsx`, the new hook leaf and this plan/index may change.

## Lifecycle invariants

- Listener registration still occurs before `ensureSubscribed`; unsubscribe is returned directly and runs on dependency change/unmount.
- Callback identity changes on exactly the former effect dependencies: `currentUserId`, `cancelScrollFollowUpFrames`, `emitDiagnosticLog`, `isAppLevelStore`, `resetActiveMeasurementDeferral`, `scheduleBackfillEvaluation`, `setViewportIntentToLiveEdge`, `timelineKeyHash`, `transport`.
- The event callback body, branch order, comments, diagnostics, store updates, key filtering, backfill fences, anchor capture and all return points remain token-exact apart from its named callback wrapper.
- Fallback remains disabled without `ensureSubscribed` or with nonempty items; captures scheduled key hash; fences current key and observed InitialItems; calls current timeline key; swallows rejection; clears its timer on dependency change/unmount.
- Fallback dependencies remain semantically exact: `itemCount`, `timelineKeyHash`, `transport`; ref identities are stable and excluded as before.
- Hook call/effects remain unconditional and preserve effect registration order relative to neighboring effects.
- Parent retains all product state, Matrix semantics, projection/store mutations, refs and non-subscription resources.
- No API/DTO/wire, DOM/CSS/i18n/a11y, dependency, test/config, barrel, generic wrapper, callback registry, duplicate logic or TODO change.

## Verification

- Source exactness: old callback body appears once inside `handleTimelineCoreEvent`; old inline listener/fallback effects are absent; dependency list 9/9; delay 120 appears once in hook.
- Ownership: one `listenCoreEvents`, one direct cleanup return, one fallback `setTimeout`, one matching `clearTimeout`, parent zero listener registrations/fallback timers.
- Type/import/export audit: leaf imports3, exports1/private options+constant2; parent adds one type import and removes one constant.
- Same focused command before/after, 175/175:
  `npm --prefix apps/desktop test -- --run src/components/TimelineView.interactions.test.tsx src/components/TimelineView.live-state.test.tsx src/components/TimelineView.media.test.tsx src/components/TimelineView.rendering.test.tsx src/components/TimelineView.scrollback.test.tsx src/components/TimelineView.threads.test.tsx src/components/TimelineView.viewport.test.tsx`.
- Typecheck, lint and diff check; then full matrix after diff approval.

## Review gate

- Design: `reviewer-flash` verified ref types, effect/dependency order, shared fences, cleanup semantics, imports and existing coverage and recorded `Correct-to-implement`.
- The executed baseline above confirms 175/175; `timelineKeyHash` remains deliberately in the exact nine-dependency list even though the callback reads its ref, because it forces room-change re-registration.
- Implementation: integrated by `luna-implementer` at medium reasoning and parent-audited.
- Source/lifecycle exactness: callback body 1/1, dependencies 9/9, parent listener 0, hook listener 1, direct unsubscribe, fallback timer/cleanup 1/1, imports 3/3 and delay 120 once.
- `TimelineView.tsx` is 4,273 newline-delimited lines; the 49-line hook is now the sole listener/fallback resource owner.
- Focused post-move 175/175; typecheck, lint and diff checks green.
- Full diff: `reviewer-flash` independently verified callback identity/order, shared ref fences, direct unsubscribe, fallback timer cleanup and parent ownership and recorded `Correct-to-merge`; parent source verifier closes its read-only base-diff limitation.
- Delivery: final repository gates, PR CI and merge pending.
