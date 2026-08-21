# Issue #551: TimelineView message-body rendering seam

## Status

- Design: `reviewer-flash` required the missing type dependencies/import-reexport shape, verified the amended 35-item inventory against the immutable source, and recorded `Correct-to-implement`.
- Implementation: integrated; 35/35 declaration exactness, 5/5 leaf exports, focused tests, typecheck, lint and diff checks are green.
- Full-diff review: `reviewer-flash` independently compared all 35 declarations against the immutable source and recorded `Correct-to-merge`; no code finding remains.
- Delivery: final repository gates, PR CI and merge pending.

## Objective

Move the existing pure message-body rendering pipeline from `apps/desktop/src/components/TimelineView.tsx` into one direct private leaf module without changing rendering, DOM, links, mentions, spoilers, formatted HTML, code blocks, math, query highlights, clipboard behavior, i18n, accessibility, CSS classes, React keys, or existing import paths.

This is the first independently mergeable TimelineView seam required by the Issue #551 Wave 4 order: message row/body/status first, before media, transport/projection and viewport ownership. It moves no stateful row, transport, media, viewport, observer, timer, listener, Matrix semantics or Rust-owned product state.

## Immutable baseline

- Commit: `63e9bc29063521682b1db359eaa4ed12da31f573`
- Source: `apps/desktop/src/components/TimelineView.tsx`
- Source size: 8,789 content lines / 8,788 newline count; 307,949 bytes
- SHA-256: `bab5f210ffe6c3171d414ee49337ee908149ce82ffc3f54dad0013ed03702216`
- Exact move: 35 top-level declarations: `TimelineMentionToken` at immutable lines 934–938, then the contiguous 34 declarations from `OpenMatrixTargetHandler` through `writeClipboardText` at lines 1181–2283. Line numbers are navigation hints only and extraction uses complete TypeScript AST statements
- Focused baseline: `TimelineView.rendering.test.tsx` plus `App.test.tsx`, 99/99 green

## Target layout

```text
apps/desktop/src/components/
├── TimelineView.tsx                    # existing composition; direct leaf import + flat compatibility re-export
└── timeline/
    └── TimelineMessageBody.tsx         # pure message body/HTML/link/spoiler/math renderer
```

No `index.ts`, barrel, default export, wrapper component, hook, context, registry, class, state store, new dependency, compatibility alias, duplicated helper, or generalized utility.

## Exact declaration ownership (35/35)

Move in immutable source order:

1. `TimelineMentionToken`
2. `OpenMatrixTargetHandler`
3. `activateTimelineLink`
4. `renderTimelineMessageText`
5. `renderTimelineMessageTextWithSpoilers`
6. `renderPlainTextBody`
7. `renderPlainTextSegment`
8. `normalizeSpoilerSpans`
9. `renderTimelineMessageLine`
10. `renderQueryHighlight`
11. `FormattedNode`
12. `FORMATTED_TAGS`
13. `VOID_FORMATTED_TAGS`
14. `renderFormattedBody`
15. `parseFormattedHtml`
16. `linkifyFormattedNodes`
17. `linkifyFormattedNodeList`
18. `linkifyFormattedNode`
19. `linkifyFormattedTextNode`
20. `parseFormattedStartTag`
21. `renderFormattedNodes`
22. `renderFormattedNode`
23. `SpoilerRevealState`
24. `renderSpoiler`
25. `renderMathFormula`
26. `FormattedTagRenderer`
27. `formattedTagRenderers`
28. `decodeHtmlEntities`
29. `isValidHtmlCodePoint`
30. `findNextMentionToken`
31. `timelineMentionTokens`
32. `hasMentionTokenBoundary`
33. `isMentionStartBoundary`
34. `isMentionEndBoundary`
35. `writeClipboardText`

The numbered list is normative and sums to 35 complete AST declarations. If the parser reports another count or statement, stop and amend this design rather than line-slicing.

## Exports and compatibility

The leaf exports exactly the declarations required outside itself:

- existing public surface: `OpenMatrixTargetHandler`, `renderTimelineMessageText`;
- direct parent-only implementation imports: `renderPlainTextBody`, `renderFormattedBody`, `writeClipboardText`.

`TimelineView.tsx` uses both a local direct import and an explicit compatibility re-export. The local import binds `OpenMatrixTargetHandler`, `renderTimelineMessageText`, `renderPlainTextBody`, `renderFormattedBody`, and `writeClipboardText` for retained parent code. A separate `export { renderTimelineMessageText }` and `export type { OpenMatrixTargetHandler }` preserves the two existing public names so callers remain unchanged:

- `mediaLists.tsx` keeps importing `renderTimelineMessageText` from `./TimelineView`;
- existing tests and consumers keep their current `TimelineView` paths;
- `TimelineRowActionHandlers` continues referring to `OpenMatrixTargetHandler` through the parent import;
- no caller imports the private leaf directly except `TimelineView.tsx`.

The three parent-only named exports are not re-exported from `TimelineView.tsx` and do not become package API.

## Direct leaf imports

The leaf imports only existing dependencies used by the moved bodies:

- React `Fragment` and `ReactNode`;
- `Copy` from `lucide-react`;
- `katex`;
- `t`;
- `findQueryHighlightRange`;
- `openExternalHttpUrl` and `toExternalHttpUrl`;
- `parseMatrixPermalink` and `MatrixPermalinkTarget`;
- `TimelineItem`, `TimelineLinkRange`, and `UserProfile`;
- type-only `TimelineRowActionHandlers` from `../TimelineView`.

`TimelineMentionToken` moves into the leaf and remains private, avoiding a new parent export. The leaf→parent dependency is exactly one `import type` for the already-exported `TimelineRowActionHandlers`; the parent→leaf value imports form no runtime cycle because the reverse edge is erased by TypeScript. No runtime circular import is permitted.

Because the leaf is one directory deeper, path rewrites are fixed: parent domain/i18n imports become `../../domain/...` and `../../i18n/messages`, sibling component helpers such as `searchHighlight` use `../searchHighlight`, and the parent uses `./timeline/TimelineMessageBody`.

The parent removes only imports made unused by extraction. Parent imports still used by link previews, action menus, row rendering and other surfaces remain.

## Behavior and security invariants

Preserve byte-equivalent bodies, literals and order for all moved declarations, apart from required `export` keywords and import qualification:

1. `renderTimelineMessageText` keeps exact newline splitting, mention-token boundaries, query highlighting and React key construction.
2. Plain-body rendering keeps Rust-projected link-range order, safe external URL conversion, Matrix permalink parsing and callback/default external-open behavior.
3. Formatted HTML parser keeps the same closed tags/void tags, attribute parsing, entity decoding, linkification and sanitized-Rust-DTO trust boundary. It does not become an HTML sanitizer or accept server HTML beyond the existing Rust projection.
4. Spoiler reveal remains caller-owned presentation state; no hook/state moves into the leaf.
5. KaTeX modes/options, fallback rendering, code-block wrapping/copy controls, language labels, ARIA/title text and CSS classes remain unchanged.
6. Clipboard fallback keeps navigator clipboard → hidden textarea → `execCommand` ordering and cleanup.
7. No Matrix semantics, command shape, transport call, product retry, timer owner, event subscription or DOM resource lifecycle changes.
8. No i18n key, visible text, test id, class name, accessibility attribute, URL policy, profile/mention fallback or debug output changes.

## Mechanical implementation

One Luna/low write-capable worker may edit only:

- `apps/desktop/src/components/TimelineView.tsx`
- new `apps/desktop/src/components/timeline/TimelineMessageBody.tsx`

The worker extracts complete TypeScript AST statements from the immutable source in original order, adds the minimum imports/exports, updates the parent direct import/re-export, and removes only newly unused parent imports. It must not edit tests, CSS, i18n, domain types, transport, package/config files, or another component.

Any required prop change, wrapper, callback adaptation, body rewrite, duplicated helper, test edit, behavior fix, or uncertain dependency is a stop condition and returns to design review.

## Exactness evidence

A temporary non-repository TypeScript verifier must prove:

1. all 35 normative named declarations exist exactly once across parent and leaf;
2. each moved declaration body/initializer/type text matches the immutable source after normalization limited to `export` and formatting;
3. the parent no longer defines any moved declaration;
4. parent flat exports still include `OpenMatrixTargetHandler` and `renderTimelineMessageText` exactly once;
5. the leaf exports only the five approved names and keeps `TimelineMentionToken` private;
6. the leaf has exactly one parent type-only import (`TimelineRowActionHandlers`), the parent has the five-name local import plus two-name compatibility re-export, and no runtime cycle exists;
7. no caller outside `TimelineView.tsx` imports the private leaf;
8. no glob/barrel/default export, duplicate, wrapper, hook, state, TODO, new dependency or test change exists.

## Integrated implementation evidence

- `TimelineView.tsx`: 8,789 baseline content lines → 7,684 newline-terminated lines; message rendering implementation moved to a 1,118-line direct leaf.
- TypeScript AST exactness: 35/35 declarations moved exactly once; parent retains zero; leaf exports exactly the approved five names and keeps `TimelineMentionToken` private.
- Existing parent paths remain explicit five-name local import plus two-name compatibility re-export. The only reverse edge is the approved erased type-only `TimelineRowActionHandlers` import.
- Focused baseline/post: `TimelineView.rendering.test.tsx` plus `App.test.tsx`, 99/99 green. Typecheck, lint and `git diff --check` are green.
- No test, CSS, i18n, domain, transport, dependency, state, hook, timer, listener, DOM, Matrix or Rust-owned behavior changed.

## Verification

Baseline and identical post-move focused gate:

```bash
npm --prefix apps/desktop test -- --run \
  src/components/TimelineView.rendering.test.tsx src/App.test.tsx
```

Then:

```bash
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
git diff --check
```

After formal full-diff `Correct-to-merge`, run the repository boundary/security/wire/doc/SDK/Rust gates required by Issue #551 before PR/CI/merge. No local homeserver/native GUI lane is required unless compile/tests/review expose runtime-path ambiguity; this slice changes no runtime behavior.

## Stop conditions

Stop and amend/re-review if:

- the 35-item AST count/list differs;
- a moved declaration needs row/media/transport/viewport state;
- a caller path cannot be preserved by the explicit two-name compatibility re-export;
- extraction changes DOM, text, CSS, key, link, mention, spoiler, math, clipboard or accessibility behavior;
- a wrapper, hook, context, registry, barrel, alias, duplicate helper or public child surface is required;
- any focused assertion requires modification.
