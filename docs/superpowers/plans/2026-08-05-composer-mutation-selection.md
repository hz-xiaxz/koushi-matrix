# Composer Mutation And Selection Commit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Commit structured composer documents and selections through one editor-owned path.

**Architecture:** Expose the existing `ImeInlineMentionEditor.publish(DocumentMutation)` through its imperative ref and route parent-originated mutations through it. Preserve epoch fencing, IME native handling, and document history.

**Tech Stack:** React, TypeScript, contentEditable Selection API, Vitest/Testing Library.

## Global Constraints

- The DOM owns active IME composition and unacknowledged selection.
- Candidate-confirmation Enter remains native.
- Stale async key actions remain discarded.
- Use the existing `DocumentMutation`; add no parallel mutation type.

---

### Task 1: Add The Selection Regression First

**Files:**
- Modify: `apps/desktop/src/components/TimelineView.test.tsx`
- Modify: `apps/desktop/src/components/ImeTextControl.test.tsx`

**Interfaces:**
- Consumes: `inlineMentionEditorSelection`.
- Produces: assertions for text, `{start: 6, end: 6}`, and saved document.

- [ ] **Step 1: Extend the existing edit Shift+Enter test**

Place selection at offset 5 in `helloworld`, press Shift+Enter, then assert
`hello\nworld`, selection 6, and the submitted structured document.

- [ ] **Step 2: Add editor-level mutation cases**

Cover start/end insertion, selected-range replacement, atomic mention before
and after the caret, and repeated newline mutations.

- [ ] **Step 3: Verify RED**

Run: `npm --prefix apps/desktop test -- src/components/TimelineView.test.tsx src/components/ImeTextControl.test.tsx`

Expected: the edit caret assertion fails at offset 0.

- [ ] **Step 4: Commit the RED tests**

```bash
git add apps/desktop/src/components/TimelineView.test.tsx apps/desktop/src/components/ImeTextControl.test.tsx
git commit -m "test: reproduce composer mutation caret loss"
```

### Task 2: Expose One Editor Commit API

**Files:**
- Modify: `apps/desktop/src/components/ImeTextControl.tsx`
- Modify: `apps/desktop/src/components/composer.tsx`

**Interfaces:**
- Produces: `ImeInlineMentionEditorHandle` with `element`, `commit(mutation: DocumentMutation)`, `focus()`, and `selection()`.
- Consumes: existing internal `publish` and `DocumentMutation`.

- [ ] **Step 1: Add the imperative handle**

Use `useImperativeHandle` to expose `commit` by delegating directly to the
existing `publish`; keep DOM focus/selection access on the same handle.

- [ ] **Step 2: Replace parent document writes with one helper**

Change the composer ref to `ImeInlineMentionEditorHandle`. Add a helper that
increments `documentEpochRef` and calls `editorRef.current?.commit(mutation)`;
fall back to local state only before mount. Route Shift+Enter, range replacement,
mention acceptance, emoji/markdown insertion, and macOS yank/kill mutations
through it.

- [ ] **Step 3: Remove superseded selection repair**

Delete `requestAnimationFrame(setInlineMentionEditorSelection(...))` only from
paths now covered by `commit`. Retain focus restoration where toolbar focus
behavior requires it.

- [ ] **Step 4: Verify GREEN**

Run the two focused test files. Expected: all pass with selection 6.

### Task 3: Verify Parity And IME Fences

**Files:**
- Modify: `apps/desktop/src/components/TimelineView.test.tsx`
- Modify: `apps/desktop/src/components/panes.test.tsx`
- Modify: `apps/desktop/src/components/rightPanel.test.tsx`

**Interfaces:**
- Produces: main/thread/edit parity plus stale-result and IME confirmation proof.

- [ ] **Step 1: Add only missing parity assertions**

Drive Shift+Enter through main, thread, and edit composers. Assert IME
confirmation inserts nothing and a deferred newline is ignored after newer DOM
input advances the epoch.

- [ ] **Step 2: Run required IME gates**

```bash
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx src/components/TimelineView.test.tsx src/components/panes.test.tsx src/components/rightPanel.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
```

- [ ] **Step 3: Review and commit**

Read every composer/IME diff, confirm no raw text surface or duplicate mutation
path was added, run `git diff --check`, and commit the issue files.
