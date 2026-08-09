# Runtime Alerts and Automatic Live-Edge Return Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate runtime warnings and diagnostic copying into the homeserver status popover, and automatically return a focused timeline to live mode when it reaches the authoritative live edge.

**Architecture:** `App` remains the source of cross-domain runtime warning and diagnostic-report composition, while `TopBar` owns only popover presentation and transient copy feedback. `TimelineView` compares its latest readable focused event with an authoritative display-event ID supplied by `TimelinePane` and requests the existing focused-context close operation exactly once.

**Tech Stack:** React 18, TypeScript, Vitest, Testing Library, Rust-projected desktop DTOs, CSS.

## Global Constraints

- Remove the root Secure Backup runtime banner without weakening encrypted-composer blocking.
- Aggregate only existing failed, reconnecting, or degraded runtime states; do not persist alert history.
- Copy the same privacy-safe diagnostic report used by the existing diagnostics dialog.
- Auto-return only when anchored, at bottom, and both known display-event IDs match.
- Resolve edit and annotation events to their relation target before comparing IDs.
- Keep an explicit return control whenever automatic proof is absent.

---

### Task 1: Runtime alerts in the session-status popover

**Files:**
- Modify: `apps/desktop/src/components/Shell.tsx`
- Modify: `apps/desktop/src/components/Shell.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.diagnostics.test.tsx`
- Modify: `apps/desktop/src/styles.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

**Interfaces:**
- Produces: `RuntimeAlert` and `RuntimeAlertKind` presentation types exported from `Shell.tsx`.
- Produces: `TopBar` props `runtimeAlerts`, `onRetryRuntimeAlert`, and `onCopyDiagnostics`.
- Consumes: existing Secure Backup, sync, and current-session state in `App`.

- [ ] **Step 1: Write failing Shell tests**

Add tests that render `TopBar` with warning and error alerts, assert the highest-severity trigger indicator and accessible count, open the popover, assert every alert detail, invoke Secure Backup retry, and verify successful and failed copy-state feedback.

- [ ] **Step 2: Run Shell tests and verify RED**

Run: `npm --prefix apps/desktop test -- --run src/components/Shell.test.tsx`

Expected: FAIL because `TopBar` does not accept or render runtime alerts and has no direct diagnostic-copy action.

- [ ] **Step 3: Implement the minimal alert and copy UI**

Add the following stable presentation interface:

```ts
export type RuntimeAlertKind = "secureBackup" | "sync" | "session";

export interface RuntimeAlert {
  kind: RuntimeAlertKind;
  severity: "warning" | "error";
  title: string;
  detail: string;
  retryable: boolean;
}
```

Render one `AlertTriangle` indicator in the status trigger, add a runtime-warning list and actions to `SessionStatusPopover`, and keep copy feedback in component-local state. The copy callback returns a promise; rejection renders a localized retryable failure state.

- [ ] **Step 4: Write failing App integration tests**

Extend the diagnostics test to project Secure Backup degradation into `TopBar`, assert that `.secure-backup-runtime-banner` is absent, and assert that copying fetches a fresh diagnostic snapshot and writes a report containing `Koushi diagnostics`.

- [ ] **Step 5: Run App diagnostics tests and verify RED**

Run: `npm --prefix apps/desktop test -- --run src/App.diagnostics.test.tsx`

Expected: FAIL because `App` still renders the root banner and does not provide runtime-alert or copy callbacks.

- [ ] **Step 6: Implement App alert composition and shared diagnostic report construction**

Compose localized alerts from these exact cases:

```ts
secureBackupRuntimeDegraded
sync.lifecycle === "reconnecting"
sync.lifecycle === "failed"
currentSessionStatus.status === "failed"
```

Extract one closure that builds `diagnosticReport(...)` from a supplied runtime diagnostic snapshot. Reuse it from both `openDiagnostics` and the new copy callback. Preserve `encryptedComposerBlocked` unchanged. Remove the root banner JSX and obsolete banner CSS.

- [ ] **Step 7: Run Task 1 tests and verify GREEN**

Run: `npm --prefix apps/desktop test -- --run src/components/Shell.test.tsx src/App.diagnostics.test.tsx`

Expected: both files PASS with no unhandled promise rejection.

- [ ] **Step 8: Commit Task 1**

```bash
git add apps/desktop/src/components/Shell.tsx apps/desktop/src/components/Shell.test.tsx apps/desktop/src/App.tsx apps/desktop/src/App.diagnostics.test.tsx apps/desktop/src/styles.css apps/desktop/src/i18n/messages.ts
git commit -m "feat: integrate runtime warnings into session status"
```

### Task 2: Automatic return from focused timeline at the live edge

**Files:**
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/components/panes.tsx`
- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Modify: `apps/desktop/src/components/TimelineView.test.tsx`

**Interfaces:**
- Produces: optional `relation_type` and `relation_event_id` on `RoomLatestEventSummary`.
- Produces: exported pure helper `roomLatestDisplayEventId(summary): string | null`.
- Produces: optional `liveLatestEventId` prop on `TimelineView`.
- Consumes: existing `onReturnToLive`, `latestReadableEventId`, and tolerant `viewportAtBottom` state.

- [ ] **Step 1: Write failing display-event mapping tests**

Add table-driven tests proving that an ordinary event maps to its own ID, `m.replace` and `m.annotation` map to `relation_event_id`, and a relation without a target returns `null`.

- [ ] **Step 2: Write failing focused-timeline behavior tests**

Add tests showing that `onReturnToLive` fires exactly once when `isAnchored`, `viewportAtBottom`, `latestReadableEventId`, and `liveLatestEventId` agree. Add negative tests for mismatched IDs, unknown IDs, and an above-bottom viewport, and assert the explicit return button remains in all negative cases.

- [ ] **Step 3: Run focused TimelineView tests and verify RED**

Run: `npm --prefix apps/desktop test -- --run src/components/TimelineView.test.tsx`

Expected: FAIL because relation mapping and the `liveLatestEventId` prop do not exist and focused mode never auto-returns.

- [ ] **Step 4: Implement DTO mapping and guarded auto-return**

Add the serialized relation fields to the TypeScript DTO, derive the authoritative display event in `TimelinePane`, and pass it to `TimelineView`. Add an effect with a ref guard keyed by room, anchor, and live event. Invoke `onReturnToLive` only after all five proof conditions in the design are true.

- [ ] **Step 5: Run TimelineView tests and verify GREEN**

Run: `npm --prefix apps/desktop test -- --run src/components/TimelineView.test.tsx`

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

```bash
git add apps/desktop/src/domain/types.ts apps/desktop/src/components/panes.tsx apps/desktop/src/components/TimelineView.tsx apps/desktop/src/components/TimelineView.test.tsx
git commit -m "fix: return focused timelines at the live edge"
```

### Task 3: Integration verification and release readiness

**Files:**
- Modify only if verification exposes a regression in files already listed above.

**Interfaces:**
- Consumes: all Task 1 and Task 2 interfaces.
- Produces: a reviewable, dependency-clean branch.

- [ ] **Step 1: Run focused regression suites**

Run:

```bash
npm --prefix apps/desktop test -- --run \
  src/components/Shell.test.tsx \
  src/App.diagnostics.test.tsx \
  src/components/TimelineView.test.tsx \
  src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 2: Run desktop type checking**

Run: `npm --prefix apps/desktop run typecheck`

Expected: exit 0.

- [ ] **Step 3: Run repository formatting and diff checks**

Run: `git diff --check`

Expected: exit 0 with no output.

- [ ] **Step 4: Run dependency audit required by AGENTS.md**

Run the repository's package-lock and runtime dependency audit commands before any release build. Stop if a high-severity vulnerability is reported.

Expected: zero high-severity vulnerabilities.

- [ ] **Step 5: Commit any verification-only correction**

If verification required a correction, stage only the affected feature files and commit with a focused message. If no correction was needed, do not create an empty commit.

- [ ] **Step 6: Push, open a ready PR, and request review**

Push `codex/runtime-alerts-auto-live-edge`, open a non-draft PR describing both user-visible fixes and their evidence, then request an independent code review.

- [ ] **Step 7: Monitor CI and review through merge**

Resolve actionable feedback, keep the branch synchronized without destructive operations, wait for required checks, and merge only after all checks are green and the PR is review-ready.

- [ ] **Step 8: Rebuild DMG from merged `origin/main`**

After merge, fetch and fast-forward/reset the clean worktree to the merged `origin/main`, rerun the dependency audit, build the DMG, verify it with `hdiutil verify`, and record its SHA-256 digest.
