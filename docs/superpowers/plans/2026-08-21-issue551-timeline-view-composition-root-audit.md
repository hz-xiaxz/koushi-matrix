# Issue #551 TimelineView residual composition-root audit

Status: architecture approved; delivery pending. This document decides whether the split-now `TimelineView.tsx` candidate is complete after its leaf/controller seams.

## Audited baseline

- Base: `d2e46a226d55de20aa791f4f6e806f5092691194` (row transport actions PR #614 merged).
- `TimelineView.tsx`: 3,944 newline-delimited lines / 150,904 bytes, reduced from the immutable first-seam baseline of 8,788 newline-delimited / 8,789 content lines.
- TypeScript-AST call inventory: 16 `useState`, 86 `useRef`, 17 `useEffect`, 14 `useLayoutEffect`, 43 `useCallback`, 10 `useMemo`.
- Seven ownership-focused TimelineView suites remain the controller gate; latest full frontend evidence is Vitest 1,370 and Playwright 248.

## Delivered ownership seams

The decomposition is represented by independently reviewed and merged PRs:

- message body #594; metadata/status #595; receipts #596; media #597;
- item row #598; transport contract #599; virtualization model/scheduler #600;
- event projection #601; viewport anchors/session #602; projection commit boundary #603;
- viewport observation #605; subscription lifecycle #606, with scheduler teardown #607 and SAS isolation #610;
- message-source dialog #611; diagnostics projection #613; row transport actions #614.

Direct leaves now own row/body/media/status/receipt presentation, source dialog, pure event projection, transport types, projection boundary, viewport math/scheduling, anchors/session memory, observation helpers, diagnostics projection, event subscription resources and stateless row transport adapters. Existing public imports are preserved through minimal explicit parent re-exports where required.

## Residual ownership graph

### Mounted DOM viewport controller

The dominant residual owns one coordinated resource graph:

- container/list DOM refs and visible/mounted row IDs;
- measured-height maps, virtual range and pending height commits;
- ResizeObserver, scheduled frames, idle/max-defer timers and scroll follow-up frames;
- free-scroll/live-edge intent, programmatic-write signatures and jump ownership;
- prepend, room-session and focused-target anchors;
- projection layout transactions, acknowledgement in-flight fences and retry timers;
- backfill request epochs/retry fences tied to layout/projection settlement;
- read/viewport observation dedupe tied to committed DOM geometry.

These resources share the timeline-key reset/unmount boundary and ordering across measurement, layout compensation, anchor restoration, projection acknowledgement and pagination. Splitting measurement, backfill or acknowledgement would require approximately 12–50 values/refs/imperative handlers, duplicate or divide cleanup, or change effect/layout-effect order. They therefore remain one mounted-DOM controller rather than wrapper hooks.

### Exhaustive event dispatch and reset boundary

`handleTimelineCoreEvent` remains the central composition loop because it coordinates:

- store reducer application and resync;
- timeline-key filtering and privacy-safe diagnostics;
- pagination epochs and projection settlement;
- anchor/measurement reset;
- source/navigation overlays;
- room-key and avatar side effects.

The shared timeline-key reset clears viewport, projection, retry, request, read, diagnostic and media/avatar resources in one ordered boundary. Moving individual event fragments would require event-forwarding registries/getters or multiple teardown owners.

### Avatar request/event lifecycle

Avatar lifecycle is conceptually cohesive but cannot become one move-only hook without violating the extraction constraints:

1. `relevantAvatarMxcsRef` is updated synchronously inside global resync, room-key reducer, display-policy reducer, InitialItems and ordinary timeline reducer paths, then reconciled by the `[items, profileUsers]` effect for externally owned stores.
2. The account-event branch accepts a thumbnail only when currently relevant or requested, releases retryable request fences below the bounded attempt cap, emits ordered diagnostics and commits thumbnail state.
3. The request window is calculated inside the shared virtual-range commit; the download effect owns requested-MXC dedupe, bounded retry counts, command-failure release and diagnostic ordering.
4. Timeline-key reset jointly clears range, relevance, requested MXCs and retry counts.

An early hook can see the event dispatcher but not the later projected viewport window. A late hook can see the window but is unavailable to the earlier subscription callback. One hook would therefore require relocating hook/effect order; two hooks would split ownership; imperative `setRelevantItems`/getter callbacks through every reducer path would be a callback registry; wrapping reducer application would co-own the store. The strong media tests pin behavior but do not create a clean API boundary. Avatar lifecycle remains in the controller unless a future first-class controller redesign changes these constraints.

### Other residual presentation state

Room-key optimistic state, alias dialog state and media-viewer focus are small cohesive mounted presentation lifecycles. Each is approximately 40–100 lines and crosses the central event/render boundary. Extracting them would add props/handlers and forwarding glue without reducing collision or creating an independent verification boundary. Image-preview requests share the virtual side-effect window. They remain local.

## Rejected residual splits

- **Viewport/backfill hook:** shared geometry, anchor, projection and scroll resources; oversized imperative API and overlapping cleanup.
- **Projection acknowledgement hook:** shared frames, retry timers, in-flight signatures and settled-render evidence; changed layout order risk.
- **Avatar hook:** hook-order paradox and synchronous reducer relevance described above.
- **Room-key hook:** event forwarding plus optimistic epoch/state API for a small presentation lifecycle.
- **Alias/media-viewer hooks:** line-count-only wrappers around cohesive ephemeral state.
- **Static helper scattering:** `formatTypingUsers` and `mediaUploadProgressForItem` remain local because standalone leaves would add APIs without ownership value.

## Final decision

The split-now `TimelineView.tsx` candidate is complete. The residual is one cohesive mounted-DOM timeline controller and composition root with:

1. one exhaustive event dispatch loop;
2. one ordered timeline-key reset/teardown boundary;
3. one virtualized measurement/anchor/backfill/projection controller;
4. one final row/chrome/dialog composition tree.

No remaining independently mergeable move-only seam reduces ownership collision without introducing shared state, duplicated cleanup, callback registries, wrapper-only APIs or effect-order changes. Future work may replace the whole mounted controller with a first-class controller architecture, but that is a behavior/design change rather than Issue #551 decomposition.

## Review and evidence gate

- Read-only residual reconnaissance completed on `d2e46a22` with concrete state/ref/effect/resource and candidate-API analysis.
- Formal `reviewer-flash` review independently traced the delivered leaves, every residual resource cluster, teardown ordering and candidate API, challenged the avatar hook-order conclusion, found no missed independently mergeable seam and recorded `Correct-to-record-and-complete-TimelineView-checkbox`.
- The reviewer-requested metric corrections are applied above and re-proved with a TypeScript AST walk plus `wc -l -c`.
- Fresh delivery gates: Vitest 1,370, Playwright 248 with polling, workspace all-targets, desktop 149/1 ignored and Headless Core QA 129; typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/format/diff gates green.
- After merge: mark the TimelineView Issue checkbox complete, record the line reduction and ownership/teardown/API/focused-test evidence, then proceed to the split-later candidates.
