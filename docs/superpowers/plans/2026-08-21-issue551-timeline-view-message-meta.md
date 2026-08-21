# Issue #551: TimelineView message metadata seam

## Status

- Design: `reviewer-flash` found the retained timestamp-helper caller, verified the amended two-export/one-re-export contract, and recorded `Correct-to-implement`.
- Implementation: integrated; 3/3 declaration exactness, 2/2 leaf exports, focused tests, typecheck, lint and diff checks are green.
- Full-diff review: `reviewer-flash` independently compared the three moved declarations and recorded `Correct-to-merge`; no finding remains.
- Delivery: final repository gates, PR CI and merge pending.

## Objective

Move the existing stateless message heading/status presentation from `TimelineView.tsx` into one direct leaf without changing sender labels, timestamps, presence labels, edited state, send-state labels, sent checkmark, DOM, i18n, accessibility, CSS, or Rust-owned DTO interpretation.

This is the second independently mergeable Wave 4 TimelineView slice after the message-body renderer. It moves no row state, action, media, receipt popup, transport, event projection, viewport, timer, listener, observer or Matrix semantics.

## Immutable baseline

- Commit: `5b35590c3ac6d754cb66125c2fdc706d15182815`
- Source: `apps/desktop/src/components/TimelineView.tsx`
- Size: 7,684 newline count / 7,685 editor positions including EOF; 276,129 bytes
- SHA-256: `ebdc57b6067ec366080a525ccd6b02792fd2fbab3fd57b50bc221f22ed989b1a`
- Exact declarations: `formatMessageTimestamp` (line 6748), `presenceLabel` (6757), exported `MessageMeta` (6852)
- Focused baseline: `TimelineView.rendering.test.tsx` + `App.test.tsx`, 99/99 green

Line numbers are navigation hints only; extraction uses complete TypeScript AST statements.

## Target layout

```text
apps/desktop/src/components/
├── TimelineView.tsx
└── timeline/
    ├── TimelineMessageBody.tsx
    └── MessageMeta.tsx
```

No index/barrel, wrapper, hook, context, registry, alias, default export, new type abstraction, dependency or duplicated helper.

## Ownership and exports

`MessageMeta.tsx` owns exactly three declarations in immutable order:

1. `formatMessageTimestamp`
2. `presenceLabel`
3. `MessageMeta`

The leaf exports `MessageMeta` and parent-only `formatMessageTimestamp`; `presenceLabel` remains private.

`TimelineView.tsx` directly imports `MessageMeta` for `TimelineItemRow` and `formatMessageTimestamp` for retained `formatThreadSummary`. It explicitly re-exports only `MessageMeta` to preserve the existing flat public path. The timestamp helper is not re-exported from `TimelineView.tsx` and does not become package API. No external caller currently imports it, but visibility is not narrowed during a move-only refactor.

The leaf imports only:

- React `ReactNode`;
- `Check` from `lucide-react`;
- `getActiveLocale` and `t`;
- `PresenceKind`.

The inline prop type path changes only from `import("../domain/types").PresenceKind` to the one-directory-deeper `import("../../domain/types").PresenceKind`. The helper uses a normal type-only `PresenceKind` import from the same module. There is no leaf→parent import or cycle.

The parent removes `Check` only after extraction proves no retained use. `getActiveLocale`, `t`, `ReactNode`, and `PresenceKind` remain because retained TimelineView/row helpers consume them.

## Behavior invariants

Preserve all three statements byte-equivalently except the approved `MessageMeta` export/path formatting:

1. null timestamp remains absent; non-null timestamps use active locale and short time style.
2. presence maps only online/away/offline to the same catalog keys.
3. send state maps sending/notSent/cancelled identically; sent renders the same `Check` mark.
4. edited marker remains hidden when redacted.
5. sender span, `dir="auto"`, `<time>` ISO value, classes, data attributes, ARIA labels, icon size and ordering remain unchanged.
6. all inputs remain Rust-projected facts; React adds no state inference, command, retry or local repair.
7. no hook, effect, timer, listener, observer, transport or resource lifecycle moves.

## Mechanical implementation

One Luna/low worker may edit only:

- `apps/desktop/src/components/TimelineView.tsx`
- new `apps/desktop/src/components/timeline/MessageMeta.tsx`

It moves complete AST statements, adds the minimum direct import/re-export, and removes only the newly unused parent `Check` import. Tests, CSS, i18n, domain, config, dependencies and all other components are forbidden.

Any body/DOM/text/prop/callback change, wrapper, helper generalization, extra export, circular import, test edit or uncertain dependency is a stop condition.

## Exactness

A temporary TypeScript verifier proves:

- 3/3 declarations exist once in the leaf and zero remain in the parent;
- statement text matches immutable source after only export/path normalization;
- leaf exports exactly `MessageMeta` and parent-only `formatMessageTimestamp`;
- parent locally imports both names and flat-re-exports only `MessageMeta` exactly once;
- no file outside the parent imports the leaf;
- only the two production files and this plan record change;
- no barrel/default export/hook/state/wrapper/alias/duplicate/TODO/dependency exists.

## Integrated implementation evidence

- `TimelineView.tsx`: 7,684 → 7,590 lines; metadata moved to a 102-line direct leaf.
- TypeScript AST exactness: 3/3 declarations moved exactly once; parent retains zero; leaf exports exactly `MessageMeta` and parent-only `formatMessageTimestamp`.
- Parent locally imports both names and flat-re-exports only the existing public `MessageMeta` path.
- Focused baseline/post 99/99; typecheck, lint and diff check green. No test/CSS/i18n/domain/transport/state/resource/behavior change.

## Verification

```bash
npm --prefix apps/desktop test -- --run \
  src/components/TimelineView.rendering.test.tsx src/App.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
git diff --check
```

After full-diff `Correct-to-merge`, run all Issue #551 repository boundary/security/wire/docs/SDK/Rust gates before PR/CI/merge. No local homeserver/native GUI lane is required unless verification exposes runtime-path ambiguity.
