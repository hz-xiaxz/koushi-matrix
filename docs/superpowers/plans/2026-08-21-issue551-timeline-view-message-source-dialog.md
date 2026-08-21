# Issue #551 TimelineView message-source dialog extraction

Status: design review pending. Scope is move-only and behavior-preserving.

## Baseline

- Base: `3c7e0a31a92480ce0622748b0f316c3abe81aaa9` (merged subscription lifecycle PR #606 with blockers integrated).
- `TimelineView.tsx`: 4,273 newline-delimited lines.
- Focused baseline: `App.test.tsx` plus all seven `TimelineView.*.test.tsx` suites, 255/255.

## Ownership decision

Move the complete event-source dialog presentation to direct leaf `apps/desktop/src/components/timeline/MessageSourceDialog.tsx`:

1. `MessageSourceDialog`
2. `megolmSessionReasonLabel`
3. `messageSourceJson`

The leaf owns dialog rendering, source JSON fallback shaping, rotation-reason labels and its three clipboard callbacks. `TimelineView` retains message-source state, event handling and dialog open/close composition.

## Imports, visibility and compatibility

Leaf imports exactly:

- `Copy`, `XCircle` from `lucide-react`;
- `useCallback` from `react`;
- `t` from `../../i18n/messages`;
- type-only `TimelineMegolmSessionReason`, `TimelineMessageSource` from `../../domain/coreEvents`;
- `writeClipboardText` from `./TimelineMessageBody`.

Export only `MessageSourceDialog`; both helpers remain private.

Parent imports and explicitly re-exports `MessageSourceDialog`, preserving imports used by `App.test.tsx` and `TimelineView.interactions.test.tsx`. Remove exactly orphaned parent imports `Copy`, `XCircle`, and `TimelineMegolmSessionReason`; retain `TimelineMessageSource`, `useCallback`, `t`, and `writeClipboardText` for other parent owners.

Only `TimelineView.tsx`, the new leaf and this plan/index may change.

## Invariants

- All three bodies/comments/JSX/type tokens/order remain exact apart from export modifier and relative paths.
- Dialog role, labels, classes, button order, icons, clipboard payloads, JSON pretty-printing, encryption fingerprint and rotation-reason rendering remain exact.
- `original_json` precedence and fallback event JSON semantics remain exact.
- No source state/event/transport owner moves; no new hook/state/timer/listener/resource.
- No Matrix/DTO/wire, CSS/i18n/a11y, dependency, test/config, barrel, wrapper, duplicate logic or TODO change.

## Verification

- AST exactness: 3/3 leaf, parent 0, exports 1/private 2, imports 5 statements / 7 bindings, orphan imports 3/3.
- Same focused command before/after 255/255:
  `npm --prefix apps/desktop test -- --run src/App.test.tsx src/components/TimelineView.interactions.test.tsx src/components/TimelineView.live-state.test.tsx src/components/TimelineView.media.test.tsx src/components/TimelineView.rendering.test.tsx src/components/TimelineView.scrollback.test.tsx src/components/TimelineView.threads.test.tsx src/components/TimelineView.viewport.test.tsx`.
- Typecheck, lint, diff; then full matrix after diff approval.

## Review gate

- Design: `reviewer-flash` verified the complete JSX/data/import/caller ownership graph and recorded `Correct-to-implement`; its two minor reproducibility notes are folded into the import counts and literal command above.
- Implementation: integrated by `luna-implementer` and parent-audited.
- Exactness: 3/3 statements, parent 0, exports 1/private 2, imports 5/5 and orphan imports 3/3; `TimelineView.tsx` 4,273 → 4,104 newline-delimited lines and the leaf is 173.
- Focused post-move 255/255; typecheck, lint and diff checks green; parent source-state/event ownership remains in place.
- Full diff and delivery pending.
