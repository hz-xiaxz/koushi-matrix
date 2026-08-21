# Issue #551 viewport scheduler teardown hardening

Status: design review pending. This is an independently mergeable blocker repair for TimelineView subscription PR #606.

## Failure evidence

The unchanged `scheduleTimelineFrame` fallback has now failed after otherwise-green Vitest runs three times, including PR #606 CI:

- all 1,369 tests passed, then two unhandled `ReferenceError: window is not defined` errors;
- stack: `TimelineViewportVirtualization.ts:91`, fallback `() => run(window.performance.now())`;
- originating suite: `TimelineView.viewport.test.tsx`;
- PR #606 other six CI jobs passed.

The base `bd128a2a` function retains the PR #600 scheduler implementation. The race is pre-existing but now reproducible under CI teardown: a queued fallback closure dereferences the global `window` after jsdom has removed it.

## Ownership decision

Keep `scheduleTimelineFrame` as the sole owner of its RAF+timeout race. At schedule time, capture bound browser capabilities needed later:

- optional `requestAnimationFrame.bind(window)`;
- optional `cancelAnimationFrame.bind(window)`;
- `setTimeout.bind(window)`;
- `clearTimeout.bind(window)`;
- `performance.now.bind(window.performance)`.

`run()` and returned `cancel()` use only these captured functions, never the later global `window`. First-callback-wins, idempotent cancellation, callback timestamp-at-invocation and sibling-handle cancellation remain unchanged.

No component caller, ref, effect or cleanup contract changes.

## Verify-first regression

Add one focused test file `apps/desktop/src/components/timeline/TimelineViewportVirtualization.test.ts` with first line `// @vitest-environment jsdom`.

One test captures scheduled fallback handlers using spies, schedules a frame while `window` exists, saves its exact global property descriptor, removes it with `Reflect.deleteProperty(globalThis, "window")`, and proves:

1. invoking the captured fallback does not throw and invokes the callback once with captured `performance.now()`;
2. the fallback cancels the scheduled RAF and clears its timeout;
3. a second returned handle can be cancelled after global `window` removal without throwing, and remains idempotent.

Run the test against the pre-fix function first: invoking the captured fallback must throw `ReferenceError: window is not defined` at the existing global `window.performance.now` dereference. Restore the saved global descriptor and all mocks in `finally` so the regression itself cannot contaminate other suites.

## Change scope

Production change is limited to `scheduleTimelineFrame` in `TimelineViewportVirtualization.ts`; tests add the one focused file. Plan/index may change.

No API/export/type/timing constant, callback ordering, timeout duration, fallback-vs-RAF behavior, dependency, DOM/product/Matrix/DTO/wire/CSS/i18n change. No wrapper, new scheduler, compatibility shim or TODO.

## Verification

- Red proof: focused regression fails on base `bd128a2a` with `window is not defined`.
- Green proof: focused regression passes after capability capture.
- New focused regression: 1/1 green after recording the RED proof.
- Existing TimelineView seven suites remain 175/175; the new suite is additional.
- Full Vitest must complete with 1,370/1,370 (1,369 existing plus the new regression) and zero unhandled errors.
- Typecheck, lint, build, Playwright, workspace/policy gates and CI 7/7.

## Review gate

- Design round 1: `reviewer-flash` recorded `Changes-required` because the new test lacked explicit jsdom environment/removal mechanics and capability binding/counts were ambiguous.
- Amendment: pin jsdom, property-descriptor removal/restoration, exact RED error, bound capabilities and 1,370-test total.
- Design round 2: `reviewer-flash` independently verified the exact installed Vitest/jsdom deletion semantics, test counts and capability bindings and recorded `Correct-to-implement`.
- Implementation note: install all window spies before scheduling so the captured references are spies; schedule-time optional RAF/cancel-RAF capture is intentional and equivalent to the prior runtime guards.
- Implementation: integrated by `luna-implementer` at medium reasoning and parent-audited.
- RED: the focused jsdom regression failed on unchanged production with `ReferenceError: window is not defined` at the deferred fallback.
- GREEN: focused regression 1/1, existing TimelineView suites 175/175, full Vitest 1,370/1,370 with zero unhandled errors; typecheck, lint and diff checks green.
- Captured capabilities are the only production delta; fallback/RAF first-wins and cancel idempotence are asserted.
- Full diff and delivery pending.
