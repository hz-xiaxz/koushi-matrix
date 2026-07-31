# Issue #304 New Thread Draft Implementation Plan

> **For Codex:** Follow this plan task by task. Each behavior change starts
> with the named failing check. Do not create the PR until the complete diff
> and all final gates have been reviewed.

**Goal:** Make first-reply thread panes immediately usable without automatic
history loading, while preserving bounded history loading for known existing
threads and preventing scheduler queue time from appearing as active
pagination.

**Architecture:** A Rust-owned `ThreadOpenIntent` crosses the command, reducer,
snapshot, Tauri, and TypeScript boundaries. The reducer retains and promotes
the intent; React consumes it only as a backfill/presentation guard. Core
acquires the existing account-wide `ExplicitPagination` permit before
publishing `Paginating`.

**Tech stack:** Rust (`koushi-state`, `koushi-core`, Tauri v2), TypeScript,
React, Vitest, Playwright.

**Binding design:**
`docs/superpowers/specs/2026-07-31-new-thread-draft-design.md`

---

## Task 1: Amend the normative thread state machine

**Files:**

- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/architecture/overview.md` only if ownership text needs a
  durable clarification

1. Add `ExistingThread` and `NewThreadDraft` to the thread-pane diagram and
   guards.
2. Document the open-intent authority for room rows and Threads-list entries.
3. Document monotonic promotion on accepted local send or matching incoming
   activity.
4. Document that draft subscription is live but automatic backward history is
   ineligible.
5. Self-review the canon diff against the binding design and repository rules.
6. Commit:

```bash
git add docs/architecture/state-machine.md docs/architecture/overview.md
git commit -m "docs: define new thread draft lifecycle"
```

## Task 2: Add reducer RED tests for typed intent and promotion

**Files:**

- Modify: `crates/koushi-state/tests/timeline_thread_state.rs`

1. Add a test that opens a new draft and expects both `Opening` and `Open` to
   retain `NewThreadDraft`.
2. Add a test that expects matching `ThreadSubmissionAccepted` to promote the
   pane to `ExistingThread`.
3. Add a test that expects a matching incoming thread-activity action to
   promote it.
4. Add stale-room/root and already-existing no-op assertions.
5. Run the focused test and record the real RED exit:

```bash
cargo test -p koushi-state --test timeline_thread_state \
  > /tmp/issue-304-state-red.log 2>&1
echo "EXIT=$?"
```

## Task 3: Implement Rust state, actions, and reducer transitions

**Files:**

- Modify: `crates/koushi-state/src/state/thread.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`
- Modify: `crates/koushi-state/src/lib.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`
- Modify: `crates/koushi-state/src/reducer/thread.rs`
- Modify tests/fixtures only where the compiler proves they mirror the public
  state

1. Add serializable `ThreadOpenIntent`.
2. Add intent to `OpenThread`, `Opening`, and `Open`.
3. Retain intent across subscription success.
4. Promote on matching accepted submission.
5. Add the narrow matching incoming-activity action and reducer transition;
   route only event-backed thread activity to it in the later core task.
6. Preserve stale-signal and session guards.
7. Run:

```bash
cargo test -p koushi-state --test timeline_thread_state \
  > /tmp/issue-304-state-green.log 2>&1
echo "EXIT=$?"
cargo test -p koushi-state --lib thread \
  > /tmp/issue-304-state-lib.log 2>&1
echo "EXIT=$?"
```

8. Commit:

```bash
git add crates/koushi-state
git commit -m "feat: model new thread draft intent"
```

## Task 4: Add command/wire RED tests

**Files:**

- Modify: `crates/koushi-core/tests/runtime_timeline.rs`
- Modify: `apps/desktop/src-tauri` focused command/DTO tests
- Modify: `apps/desktop/src/domain` API/contract tests as required

1. Add a core command test that expects `OpenThread` intent to reach the
   reducer effect/state.
2. Add Tauri serialization tests for both intent tokens.
3. Add a TypeScript client test expecting `open_thread` to send the intent.
4. Run the narrow tests and record nonzero RED exits.

## Task 5: Carry intent through Core, Tauri, and TypeScript

**Files:**

- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/views.rs`
- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: desktop API implementation and mocks found by compiler/search
- Modify: maximally populated golden/contract artifacts

1. Add intent to the typed open-thread command at every boundary.
2. Preserve privacy-safe custom `Debug`.
3. Map the intent into frontend state DTOs without defaults.
4. Update maximally populated fixtures with a real intent.
5. Run:

```bash
cargo test -p koushi-core --test runtime_timeline \
  > /tmp/issue-304-core-wire.log 2>&1
echo "EXIT=$?"
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml \
  core_event_wire_format_matches_checked_in_contract_artifact \
  > /tmp/issue-304-tauri-wire.log 2>&1
echo "EXIT=$?"
npm --prefix apps/desktop run typecheck \
  > /tmp/issue-304-typecheck.log 2>&1
echo "EXIT=$?"
```

6. Commit:

```bash
git add crates/koushi-core apps/desktop/src-tauri apps/desktop/src
git commit -m "feat: transport thread open intent"
```

## Task 6: Add backfill-policy RED tests

**Files:**

- Modify: `apps/desktop/src/domain/timelineBackfillPolicy.test.ts`
- Modify: `apps/desktop/src/components/TimelineView.test.tsx`

1. Extend the pure policy fixture with semantic eligibility.
2. Parameterize all settlement triggers and expect a new draft to return a
   semantic blocker without calling pagination.
3. Prove an existing empty-cache thread requests through the generic policy.
4. Prove automatic loading disabled blocks that same empty thread.
5. Add a component assertion that replay/Idle/gap-release cannot produce a
   draft pagination command or spinner.
6. Run focused tests and record the RED exits:

```bash
npm --prefix apps/desktop test -- \
  src/domain/timelineBackfillPolicy.test.ts \
  src/components/TimelineView.test.tsx \
  > /tmp/issue-304-frontend-red.log 2>&1
echo "EXIT=$?"
```

## Task 7: Centralize frontend admission and render draft opening

**Files:**

- Modify: `apps/desktop/src/domain/timelineBackfillPolicy.ts`
- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Modify: `apps/desktop/src/components/rightPanel.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: API/mocks/browser fixtures surfaced by typecheck

1. Add a semantic eligibility input and blocker to the pure policy.
2. Pass `false` only for `NewThreadDraft`.
3. Delete `emptyThreadBackfillRequestedRef` and the separate empty-thread
   effect.
4. Derive entry intent from Rust-projected row facts: positive summary count is
   existing; absent/zero summary is draft; Threads-list entry is existing.
5. Render matching draft `Opening` as an empty composer-capable thread pane,
   while subscription continues asynchronously.
6. Run focused policy/component tests and typecheck.
7. Commit:

```bash
git add apps/desktop/src
git commit -m "fix: keep new thread drafts out of backfill"
```

## Task 8: Add scheduler-order RED tests

**Files:**

- Modify: `crates/koushi-core/src/timeline.rs` test module

1. Hold the account history slot with background work.
2. Start an explicit pagination request.
3. Assert no `PaginationState::Paginating` event is published while the
   request waits.
4. Release the permit and assert `Paginating` precedes SDK pagination terminal.
5. Drop/replace the actor while queued and assert no stale pagination state is
   published.
6. Run the focused test and record the RED exit:

```bash
cargo test -p koushi-core --lib \
  pagination_waits_for_permit_before_publishing_paginating \
  > /tmp/issue-304-pagination-red.log 2>&1
echo "EXIT=$?"
```

## Task 9: Publish pagination state only after admission

**Files:**

- Modify: `crates/koushi-core/src/timeline.rs`

1. Move permit acquisition before `Paginating`.
2. Recheck actor generation after admission and before publication.
3. Keep the permit across the one bounded SDK call.
4. Preserve terminal-state and oldest-edge behavior.
5. Use existing private-data-free account/timeline diagnostics.
6. Run the focused scheduler test plus:

```bash
cargo test -p koushi-core --lib account_work \
  > /tmp/issue-304-account-work.log 2>&1
echo "EXIT=$?"
cargo test -p koushi-core --lib timeline \
  > /tmp/issue-304-core-timeline.log 2>&1
echo "EXIT=$?"
```

7. Commit:

```bash
git add crates/koushi-core/src/timeline.rs
git commit -m "fix: publish pagination after scheduler admission"
```

## Task 10: Prove promotion from real core activity

**Files:**

- Modify: `crates/koushi-core/src/timeline.rs` or the narrow relay owner
- Modify: `crates/koushi-core/tests/runtime_timeline.rs`
- Modify: state/core tests needed for local accepted send

1. Add a failing core test for matching event-backed incoming thread activity.
2. Route that observation to the reducer promotion action without using vector
   shape alone as semantic evidence.
3. Confirm local submission acceptance already promotes through the reducer
   path.
4. Run the focused core/state tests and commit:

```bash
git add crates/koushi-core crates/koushi-state
git commit -m "fix: promote draft threads on activity"
```

## Task 11: Browser-headless acceptance

**Files:**

- Modify: the narrow existing desktop thread Playwright spec/fixture

1. Add a test that opens first-reply intent and immediately finds the composer.
2. Assert no Loading spinner and no `paginate_backwards` command across
   initial-empty, Idle, replay, and gap-release events.
3. Send the first reply, apply the Rust-shaped promoted snapshot, and assert
   normal thread lifecycle.
4. Add an incoming-activity promotion case without pane reopen.
5. Add existing-empty-cache and auto-load-disabled cases.
6. Run the exact focused Playwright test with one worker and record its exit.
7. Commit:

```bash
git add apps/desktop/e2e apps/desktop/src
git commit -m "test: cover new thread draft lifecycle"
```

## Task 12: Integrated verification and self-review

1. Run formatting/lint:

```bash
cargo fmt --all -- --check
npm --prefix apps/desktop run lint
```

2. Run focused Rust, Tauri, TypeScript, Vitest, and Playwright gates from the
   prior tasks. Read every command's own exit status.
3. Run the exact repository CI Rust command, not a `--lib` substitute.
4. Run the relevant local headless-core thread scenario if an existing
   scenario covers thread create/send; do not invent a long scenario merely to
   replace deterministic focused evidence.
5. Read:

```bash
git diff origin/main...HEAD
git status --short
git submodule status vendor/matrix-rust-sdk
node scripts/check-sdk-submodule.mjs
```

6. Check untracked files explicitly, privacy-safe fixtures, wire artifacts,
   canon consistency, stale guards, duplicate callers, and test strength.
7. Push the branch and open one standalone PR for #304 with `Fixes #304`.
8. Monitor all CI. Compare each long step with recent successful runs; after
   twice the normal duration, reproduce the exact CI command locally instead
   of passively waiting.
9. Merge with a normal merge commit only after all checks pass, then verify
   issue #304 is closed.
