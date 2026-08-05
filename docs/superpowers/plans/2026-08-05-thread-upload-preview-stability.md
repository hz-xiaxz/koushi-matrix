# Thread Upload Preview Stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent caption-only thread snapshots from reloading or replacing an unchanged prepared-upload preview.

**Architecture:** Reuse the existing stable-event pattern at the thread callback boundary. Keep `PreparedUploadPreview` keyed only by resource identity; focused assertions preserve its current replacement and cleanup lifecycle.

**Tech Stack:** React, TypeScript, Vitest/Testing Library, browser Blob URL APIs.

## Global Constraints

- Caption acknowledgements remain immediate and Rust-owned.
- Do not debounce, cache preview bytes, or duplicate staging state.
- Revoke every created Blob URL, but never one still rendered.

---

### Task 1: Reproduce The Thread Flicker Contract

**Files:**
- Modify: `apps/desktop/src/components/rightPanel.test.tsx`

**Interfaces:**
- Consumes: `RightPanel`, Rust-shaped thread staged-upload snapshots.
- Produces: regression proof that unchanged `(staged_id, variant_id, mime_type, target)` loads once.

- [ ] **Step 1: Add the failing integration test**

Stub `URL.createObjectURL`/`revokeObjectURL`, resolve non-empty preview bytes,
rerender several caption acknowledgements, and assert one loader call, the same
`img` node/src, and no revocation. Include one IME caption composition update.

- [ ] **Step 2: Verify RED**

Run: `npm --prefix apps/desktop test -- src/components/rightPanel.test.tsx`

Expected: FAIL because `loadPreview` is called again after rerender.

- [ ] **Step 3: Commit the RED test**

```bash
git add apps/desktop/src/components/rightPanel.test.tsx
git commit -m "test: reproduce thread upload preview reload"
```

### Task 2: Share The Existing Stable Event Hook

**Files:**
- Create: `apps/desktop/src/components/useStableEvent.ts`
- Modify: `apps/desktop/src/components/panes.tsx`
- Modify: `apps/desktop/src/components/rightPanel.tsx`

**Interfaces:**
- Produces: `useStableEvent<T extends (...args: any[]) => unknown>(handler: T): T`.
- Consumes: the latest callback and current thread room/root arguments.

- [ ] **Step 1: Move, do not rewrite, the hook**

Move the current ref/layout-effect/callback implementation from `panes.tsx` to
the new module and import it back into `panes.tsx`.

- [ ] **Step 2: Stabilize thread staging callbacks**

Create stable versions of the thread staging handlers in `RightPanel`, then
pass callbacks built from those stable functions and current target IDs to
`UploadStagingDialog`. Apply the same rule to clear, caption, send, select,
retry, original, and preview callbacks.

- [ ] **Step 3: Verify the focused contract**

Run:

```bash
npm --prefix apps/desktop test -- src/components/rightPanel.test.tsx src/components/panes.test.tsx src/components/dialogs.test.tsx
npm --prefix apps/desktop run typecheck
```

Expected: all tests pass and preview loading occurs once.

### Task 3: Prove Legitimate Replacement And Cleanup

**Files:**
- Inspect: `apps/desktop/src/components/dialogs.tsx`
- Modify: `apps/desktop/src/components/dialogs.test.tsx`

**Interfaces:**
- Consumes: a changed selected prepared variant.
- Produces: one new URL per variant and final cleanup revocation.

- [ ] **Step 1: Add replacement/close assertions**

Assert a new variant loads once, the previous URL is revoked after replacement,
and unmount revokes the final URL.

- [ ] **Step 2: Keep existing code if tests pass**

The current implementation swaps the state URL before revoking its predecessor.
Keep it unchanged when the replacement/cleanup assertions pass. A failure is a
new reproduced bug outside #427; stop and amend this plan before changing the
URL lifecycle rather than mixing an unplanned fix into the callback patch.

- [ ] **Step 3: Final verification and commit**

Run the focused tests, `npm --prefix apps/desktop run lint`, `git diff --check`,
read the complete diff, and commit only the files used by this issue.
