# Issue #551 Timeline row transport-action extraction

Status: full diff approved; delivery pending. Scope is behavior-preserving adapter ownership extraction.

## Baseline

- Base: `1dcae07c1d2a21de9dd82ae3e95f25277f1d47c7` (timeline diagnostics PR #613 merged, including SDK pointer from #612).
- `TimelineView.tsx`: 4,048 newline-delimited lines / 154,667 bytes.
- Focused baseline: interactions + media suites, 53/53.

## Ownership decision

Create direct hook leaf `apps/desktop/src/components/timeline/useTimelineRowTransportActions.ts` owning exactly the stateless row action adapters:

1. `onSendReaction`
2. `onRetrySend`
3. `onCancelSend`
4. `onRedactReaction`
5. `onEdit`
6. `onRedact`
7. `onPin`
8. `onUnpin`
9. `onDownloadMedia`
10. `onLoadMessageSource`
11. `onForwardMessage`
12. `onLoadLinkPreviews`
13. `onHideLinkPreview`
14. `onCopyText`

The hook receives only `transport`, optional `onDiagnosticLogEntry?: (entry: DiagnosticLogEntry) => void`, and the already-derived `timelineDiagnosticKind`; it returns a `Pick<TimelineRowActionHandlers, ...>` object containing exactly those 14 stable callbacks. The 14 callback declarations/bodies/dependency arrays move exactly. The only new glue is the private return type, hook parameter contract, and exact return object.

Parent obtains `rowTransportActions`, destructures only its stable retry/cancel callbacks for the existing bulk actions, and passes the full object directly to `TimelineItemRow` with one prop spread. No forwarding wrapper or callback registry is added.

## Explicit exclusions

The following remain in `TimelineView` because they own or coordinate different state/resources:

- bulk retry/cancel loops: room ID + not-sent transaction selection;
- room-key request action: optimistic state, epoch/key fences and rejection cleanup;
- alias dialog actions: local dialog/draft state;
- media-viewer open/close: focus resource and timer;
- reply/thread/navigation callbacks supplied by parent;
- read signals and viewport observation;
- save-media capability and row presentation/profile/receipt data.

## Imports, visibility and compatibility

Leaf imports exactly six statements:

- `useCallback` from `react`;
- type `DiagnosticLogEntry` from `../../domain/diagnostics`;
- type `ComposerDocument` from `../../domain/types`;
- type `TimelineRowActionHandlers` from `./TimelineItemRow`;
- type `TimelineTransport` from `./TimelineTransport`;
- `writeClipboardText` from `./TimelineMessageBody`.

Export only `useTimelineRowTransportActions`; keep its `TimelineRowTransportActions` alias private. Parent removes exactly orphaned `ComposerDocument` and `writeClipboardText` imports. Existing `TimelineItemRow` props/interface and every public import path remain unchanged. No reverse edge or cycle is introduced.

Only `TimelineView.tsx`, the new hook and this plan/index may change.

## Invariants

- Callback parameter types, bodies, catch behavior, optional-call behavior and dependency arrays remain exact.
- Preview diagnostic timestamp/source/messages, request/failed ordering and failure handling remain exact.
- Clipboard rejection swallowing remains exact.
- Retry/cancel functions used by bulk actions retain stable callback identity.
- `TimelineItemRow` receives the same 14 prop keys/functions; no duplicate or override key exists around the spread.
- No state/ref/effect/timer/listener/observer/transport ownership beyond these stateless callback adapters moves.
- No Matrix/DTO/wire, CSS/i18n/a11y, dependency, test/config, barrel, compatibility shim, wrapper-only component, duplicate logic or TODO change.

## Verification

- Exactness: callback declarations 14/14 in hook and parent 0; hook output keys 14/14 once each; parent spread 1, explicit moved props 0, duplicate keys 0.
- Move the pure `timelineDiagnosticKind` derivation ahead of the unconditional top-level hook call; place the hook before retained bulk retry/cancel callbacks and destructure its stable retry/cancel functions there.
- Hook exports 1/private type 1/imports 6; optional diagnostic callback contract and excluded owners remain exact.
- Parent orphan imports `ComposerDocument` and `writeClipboardText` are exactly removed.
- Same focused command before/after 53/53:
  `npm --prefix apps/desktop test -- --run src/components/TimelineView.interactions.test.tsx src/components/TimelineView.media.test.tsx`.
- Typecheck, lint and diff; then full matrix after diff approval.

## Review gate

- Design: `reviewer-flash` traced the exact callbacks, prop keys, dependencies, type/runtime graph and focused tests and recorded `Correct-to-implement`; its three placement/optionality notes are folded into the contract and verification above.
- Implementation: integrated by `luna-implementer` and parent-audited.
- Exactness: callbacks 14/14, parent 0, output keys 14/14, imports 6/6, exports 1/private type 1, parent spread 1/explicit moved props 0/duplicates 0, excluded owners 4/4 and orphan imports 2/2; `TimelineView.tsx` 4,048 → 3,944 newline-delimited lines and the hook is 145.
- Focused post-move 53/53; typecheck, lint and diff checks green; bulk callback identity and all state/resource owners remain exact.
- Full diff: `reviewer-flash` independently traced every callback, prop key, dependency and retained owner and recorded `Correct-to-merge`; parent AST/git evidence closes its read-only raw-diff limitation.
- Delivery: final repository gates, PR CI and merge pending.
