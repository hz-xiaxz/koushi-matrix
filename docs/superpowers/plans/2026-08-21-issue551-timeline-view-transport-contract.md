# Issue #551 TimelineView transport-contract extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `9d664e0e10ecb5f69fc4a60bacf766f592338827` (merged PR #598).
- `apps/desktop/src/components/TimelineView.tsx`: 5,198 newline-delimited lines, 188,642 bytes.
- Focused immutable baseline: App, TimelinePane render isolation, TimelineView rendering/interactions/threads/media = 179/179:
  `npm --prefix apps/desktop test -- --run src/App.test.tsx src/components/TimelinePane.renderIsolation.test.tsx src/components/TimelineView.rendering.test.tsx src/components/TimelineView.interactions.test.tsx src/components/TimelineView.threads.test.tsx src/components/TimelineView.media.test.tsx`.
- Existing contract callers import `TimelineTransport` through `./TimelineView`; the row and media leaves create erased reverse type edges to that composition root. `TimelineMessageBody` retains a separate erased `TimelineRowActionHandlers` reverse edge.

## Ownership decision

Move exactly the `TimelineTransport` interface, unchanged, to the direct private leaf `apps/desktop/src/components/timeline/TimelineTransport.ts`.

The leaf imports only its six contract types from `domain/coreEvents` and `domain/types`. It owns no implementation, state, callback, subscription, timer, retry, cleanup, React hook, or product semantics.

`TimelineView.tsx` imports the type from the leaf and explicitly type-re-exports it, preserving every existing public import path. `TimelineItemRow.tsx` and `TimelineMedia.tsx` switch their erased `TimelineTransport` imports directly to the leaf. `TimelineMessageBody.tsx` switches its erased `TimelineRowActionHandlers` import directly to sibling owner `TimelineItemRow.tsx`. Together these remove every production source dependency from `components/timeline/` back to the composition root; both sibling reverse edges remain type-only and are erased at runtime.

`ReturnToLiveHandler` and `invokeReturnToLiveSafely` stay in `TimelineView.tsx`: they are viewport/navigation presentation contracts, not the core transport port, and belong to the later viewport seam.

## Exact inventory

Production declaration and attached ownership header moved once:

1. the full three-line `// ---------------------------------------------------------------------------` / `// Transport interface (Tauri IPC, browser fake, or test mock)` / `// ---------------------------------------------------------------------------` banner
2. `interface TimelineTransport`

Approved leaf export: `TimelineTransport` only. The only new parent flat re-export is `TimelineTransport`; every existing parent re-export remains unchanged.

Expected production changes: five files only:

- create `components/timeline/TimelineTransport.ts`;
- move the header/interface from `TimelineView.tsx`, then import/re-export the leaf type;
- redirect erased `TimelineTransport` imports in `TimelineItemRow.tsx` and `TimelineMedia.tsx`;
- redirect the erased `TimelineRowActionHandlers` import in `TimelineMessageBody.tsx` to sibling owner `TimelineItemRow.tsx`.

No other caller import changes are needed or permitted. No compatibility wrapper, barrel, alias, runtime module edge, second interface, dependency, test-only export, or formatting churn.

## Invariants

- Interface comments, method names, optionality, argument order/types, return types and declaration order are token-equivalent after path normalization.
- Existing `TimelineView` import compatibility remains intact.
- Tauri, browser fake, App-level transport, harness mocks, media save typing and event-listener cleanup semantics are unchanged.
- CoreCommand/CoreEvent/DTO/wire, Rust state, DOM, CSS, i18n and resource ownership are untouched.
- This prerequisite does not by itself complete the TimelineView Issue checkbox; projection subscription and viewport ownership seams remain.

## Verification

- Deterministic TypeScript AST check: interface 1/1 in leaf, parent 0, exact member/token sequence, leaf export 1, parent re-export 1.
- Dependency check: no production file under `components/timeline/` imports `TimelineView` after the move.
- Same focused six-suite check before/after (179/179), typecheck, lint and diff check.
- After full-diff approval: complete frontend/Rust/policy gate matrix and CI.

## Review gate

- Design round 1: `reviewer-flash` recorded `Changes-required` because `TimelineMessageBody` retained a reverse type edge and the dependency gate could not pass.
- Amendment: redirect that erased edge to its sibling row owner, pin the attached header and exact baseline command, and expand the production scope to five files.
- Design round 2: `reviewer-flash` verified the amendment and recorded `Correct-to-implement`; its two non-blocking precision notes (full banner and new-re-export wording) are folded above.
- Implementation: approved, not started.
- Full diff and delivery: pending.
