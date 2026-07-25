# Thread Live-Update Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add privacy-safe diagnostics that locate a missing open-thread update at the Core projection, renderer store, or React commit boundary.

**Architecture:** Core records a thread-only projection record when it commits an SDK batch. The TypeScript timeline store exposes a pure pre-application classifier, `App` records the classifier result while applying the shared store update, and `TimelineView` records a deduplicated post-commit tuple. Existing timeline behavior remains unchanged.

**Tech Stack:** Rust, `koushi-diagnostics`, TypeScript, React, Vitest.

## Global Constraints

- This is diagnostic only; do not change timeline subscription, projection, rendering, or scrolling behavior.
- Record no message body, event ID, room ID, user ID, or transaction ID.
- Use the existing bounded diagnostics report and do not add unconditional stderr output.
- Run only targeted unit tests during implementation.

---

### Task 1: Classify renderer store application

**Files:**
- Modify: `apps/desktop/src/domain/timelineStore.ts`
- Test: `apps/desktop/src/domain/timelineStore.test.ts`

**Interfaces:**
- Consumes: `TimelineKeyState` and an `ItemsUpdated` timeline payload.
- Produces: `classifyTimelineItemsUpdatedApplication(store, payload): TimelineItemsUpdatedApplication`.

- [ ] **Step 1: Write failing classifier tests**

Add tests covering all five outcomes:

```typescript
expect(classifyTimelineItemsUpdatedApplication(createTimelineStore(), update))
  .toBe("missing_initial");
expect(classifyTimelineItemsUpdatedApplication(initializedStore, acceptedUpdate))
  .toBe("applied");
expect(classifyTimelineItemsUpdatedApplication(initializedStore, wrongGeneration))
  .toBe("generation_mismatch");
expect(classifyTimelineItemsUpdatedApplication(initializedStore, repeatedBatch))
  .toBe("duplicate_batch");
expect(classifyTimelineItemsUpdatedApplication(awaitingResyncStore, acceptedUpdate))
  .toBe("awaiting_resync");
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```bash
npm --prefix apps/desktop test -- --run src/domain/timelineStore.test.ts
```

Expected: FAIL because `classifyTimelineItemsUpdatedApplication` is not exported.

- [ ] **Step 3: Implement the pure classifier**

Add:

```typescript
export type TimelineItemsUpdatedApplication =
  | "applied"
  | "missing_initial"
  | "generation_mismatch"
  | "duplicate_batch"
  | "awaiting_resync";

export function classifyTimelineItemsUpdatedApplication(
  store: TimelineStoreState,
  payload: Extract<TimelineEvent, { ItemsUpdated: unknown }>["ItemsUpdated"]
): TimelineItemsUpdatedApplication {
  const existing = store.keys.get(keyStr(payload.key));
  if (!existing) return "missing_initial";
  if (existing.generation !== payload.generation) return "generation_mismatch";
  if (existing.lastAppliedBatchId !== null && payload.batch_id <= existing.lastAppliedBatchId) {
    return "duplicate_batch";
  }
  if (existing.awaitingResync) return "awaiting_resync";
  return "applied";
}
```

Keep `applyItemsUpdated` behavior unchanged.

- [ ] **Step 4: Run the tests and verify GREEN**

Run:

```bash
npm --prefix apps/desktop test -- --run src/domain/timelineStore.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/domain/timelineStore.ts apps/desktop/src/domain/timelineStore.test.ts
git commit -m "test(timeline): classify thread store updates"
```

### Task 2: Record renderer store and React commit boundaries

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Test: `apps/desktop/src/components/TimelineView.test.tsx`

**Interfaces:**
- Consumes: `classifyTimelineItemsUpdatedApplication`, `getKeyState`, and the existing `onDiagnosticLogEntry`.
- Produces: `thread.timeline stage=store ...` and `thread.timeline stage=committed ...` diagnostic messages.

- [ ] **Step 1: Write a failing thread commit diagnostic test**

Render a thread timeline with a controlled app-level store. Apply initial state
and one `ItemsUpdated` batch, then assert:

```typescript
expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
  expect.objectContaining({
    source: "thread.timeline",
    message: "stage=committed generation=3 batch=7 items=2"
  })
);
```

Rerender with the same store and assert the same message was emitted only once.

- [ ] **Step 2: Run the component test and verify RED**

Run:

```bash
npm --prefix apps/desktop test -- --run src/components/TimelineView.test.tsx \
  -t "records a deduplicated committed thread projection"
```

Expected: FAIL because no committed diagnostic is emitted.

- [ ] **Step 3: Add the post-commit diagnostic**

In `TimelineView`, retain the last emitted tuple in a ref and add a
`useLayoutEffect` restricted to `TimelineKind::Thread`:

```typescript
const lastThreadCommitDiagnosticRef = useRef<string | null>(null);

useLayoutEffect(() => {
  if (!("Thread" in timelineKey.kind) || !timelineKeyState) return;
  const signature = [
    timelineKeyState.generation,
    timelineKeyState.lastAppliedBatchId ?? "none",
    items.length
  ].join(":");
  if (lastThreadCommitDiagnosticRef.current === signature) return;
  lastThreadCommitDiagnosticRef.current = signature;
  emitDiagnosticLog(
    "thread.timeline",
    `stage=committed generation=${timelineKeyState.generation} ` +
      `batch=${timelineKeyState.lastAppliedBatchId ?? "none"} items=${items.length}`
  );
}, [emitDiagnosticLog, items.length, timelineKey, timelineKeyState]);
```

- [ ] **Step 4: Add the shared-store diagnostic in `App`**

For each thread `ItemsUpdated` payload, capture state before application,
classify it, apply the existing reducer, and append:

```typescript
const before = getKeyState(next, payload.event.ItemsUpdated.key);
const outcome = classifyTimelineItemsUpdatedApplication(
  next,
  payload.event.ItemsUpdated
);
const applied = applyTimelineEventWithProjectionResultAndRetention(
  next,
  payload.event,
  retainedTimelineKeyIdsRef.current
);
const after = getKeyState(applied.store, payload.event.ItemsUpdated.key);
appendDiagnosticLog({
  timestampMs: Date.now(),
  source: "thread.timeline",
  message:
    `stage=store outcome=${outcome} generation=${payload.event.ItemsUpdated.generation} ` +
    `batch=${payload.event.ItemsUpdated.batch_id} diffs=${payload.event.ItemsUpdated.diffs.length} ` +
    `before=${before?.items.length ?? 0} after=${after?.items.length ?? 0}`
});
```

Only run this branch when `"Thread" in payload.event.ItemsUpdated.key.kind`.

- [ ] **Step 5: Run targeted renderer tests**

Run:

```bash
npm --prefix apps/desktop test -- --run src/components/TimelineView.test.tsx \
  -t "thread"
npm --prefix apps/desktop run typecheck
```

Expected: thread tests PASS and TypeScript reports no errors.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/App.tsx \
  apps/desktop/src/components/TimelineView.tsx \
  apps/desktop/src/components/TimelineView.test.tsx
git commit -m "feat(desktop): trace thread update boundaries"
```

### Task 3: Record the Core projection boundary

**Files:**
- Modify: `crates/koushi-core/src/timeline.rs`
- Test: inline `#[cfg(test)]` tests in `crates/koushi-core/src/timeline.rs`

**Interfaces:**
- Consumes: `TimelineKey`, actor generation, timeline generation, batch ID, projected batch, and input diff count.
- Produces: one `core.thread_timeline` debug record with stage `projected`.

- [ ] **Step 1: Write a failing diagnostic unit test**

Capture the current diagnostics length, call the new recorder once with a
thread key and once with a room key, then inspect only the records appended
after the captured length and assert that exactly one exists:

```rust
assert_eq!(record.event.source, "core.thread_timeline");
assert_eq!(record.event.stage, "projected");
assert!(record.event.fields.iter().any(|field| field.key == "actor_generation"));
assert!(record.event.fields.iter().any(|field| field.key == "timeline_generation"));
assert!(record.event.fields.iter().any(|field| field.key == "batch_id"));
```

- [ ] **Step 2: Run the Core test and verify RED**

Run:

```bash
cargo test -p koushi-core thread_projection_diagnostic_records_only_thread_batches
```

Expected: FAIL because `record_thread_projection` does not exist.

- [ ] **Step 3: Implement the recorder and call it at commit**

Add a non-stderr recorder:

```rust
fn record_thread_projection(
    key: &TimelineKey,
    actor_generation: u64,
    timeline_generation: TimelineGeneration,
    batch_id: TimelineBatchId,
    input_diff_count: usize,
    projected_diff_count: usize,
    projected_item_count: usize,
) {
    if !matches!(key.kind, TimelineKind::Thread { .. }) {
        return;
    }
    koushi_diagnostics::record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.thread_timeline",
            "projected",
        )
        .field(DiagnosticField::count("actor_generation", actor_generation))
        .field(DiagnosticField::count("timeline_generation", timeline_generation.0))
        .field(DiagnosticField::count("batch_id", batch_id.0))
        .field(DiagnosticField::count("input_diffs", input_diff_count as u64))
        .field(DiagnosticField::count("projected_diffs", projected_diff_count as u64))
        .field(DiagnosticField::count("items", projected_item_count as u64)),
    );
}
```

Invoke it inside the existing generation lease immediately before emitting
`ItemsUpdated`, using `projected_batch.display_diffs.len()` and
`display_projection.display_items().len()`.

- [ ] **Step 4: Run the Core test and verify GREEN**

Run:

```bash
cargo test -p koushi-core thread_projection_diagnostic_records_only_thread_batches
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/koushi-core/src/timeline.rs
git commit -m "feat(core): trace thread projection batches"
```

### Task 4: Focused verification

**Files:**
- Verify only; no production files are expected to change.

**Interfaces:**
- Consumes: all diagnostics added by Tasks 1–3.
- Produces: evidence that targeted Rust and renderer tests pass without running long homeserver suites.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt --check
npm --prefix apps/desktop run lint -- --quiet
```

Expected: both commands exit 0.

- [ ] **Step 2: Run targeted tests together**

```bash
cargo test -p koushi-core thread_projection_diagnostic_records_only_thread_batches
npm --prefix apps/desktop test -- --run src/domain/timelineStore.test.ts \
  src/components/TimelineView.test.tsx -t "thread|classif"
npm --prefix apps/desktop run typecheck
```

Expected: all commands exit 0.

- [ ] **Step 3: Inspect the diff**

```bash
git diff origin/main...HEAD --check
git status --short
```

Expected: no whitespace errors; only the intended branch commits plus the
pre-existing untracked files are present.
