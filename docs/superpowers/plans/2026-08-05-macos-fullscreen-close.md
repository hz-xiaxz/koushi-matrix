# macOS Fullscreen Close Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the macOS red close button hide the app without an automatic fullscreen `Reopen` immediately showing the window again.

**Architecture:** Keep the existing close-to-background policy and background runtime alive. At the macOS `CloseRequested` boundary, query the window fullscreen state, leave fullscreen before hiding when necessary, and emit a private-data-free action token. A pure decision helper plus a handler-wiring guard will cover the platform-specific branch without requiring a macOS GUI runner in unit tests.

**Tech Stack:** Rust, Tauri window events, `koushi-diagnostics`, Cargo unit tests.

## Global Constraints

- The branch must start at the latest `origin/main`.
- `CloseRequested` must continue to call `prevent_close()` and `hide()` so background tasks are not stopped.
- Fullscreen close must leave fullscreen before hiding; ordinary close must remain hide-only.
- Diagnostics must contain only lifecycle tokens/booleans, never raw native errors or private identifiers.
- The same regression check must be RED before the production change and GREEN after it.

---

### Task 1: Pin the fullscreen close decision

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/lib.rs` unit-test module

**Interfaces:**
- Produces `MacosCloseRequestedAction` and `macos_close_requested_action(Option<bool>)` for the macOS event boundary.

- [x] **Step 1: Write the failing test**

Add a unit test asserting that `Some(true)` selects `ExitFullscreenAndHide`, while `Some(false)` and an unavailable fullscreen query select `Hide`. Keep the existing source guard that verifies the handler still prevents close and hides the window, and extend it to require the fullscreen query and exit call.

- [x] **Step 2: Run the focused test to verify it fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib macos_close_requested
```

Expected: the new decision test fails because the action enum/helper and fullscreen branch do not exist yet.

- [x] **Step 3: Implement the minimal decision and handler branch**

Add the pure action decision, then in the existing macOS `CloseRequested` handler call `window.is_fullscreen().ok()`, call `window.set_fullscreen(false)` only for `ExitFullscreenAndHide`, and preserve `prevent_close()` plus `hide()`. Record `was_fullscreen` and the action token in `desktop.lifecycle / close_requested`.

- [x] **Step 4: Run the focused test to verify it passes**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib macos_close_requested
```

Expected: all matching tests pass, including the ordinary hide contract and fullscreen branch.

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/lib.rs docs/superpowers/plans/2026-08-05-macos-fullscreen-close.md
git commit -m "fix: exit macOS fullscreen before hiding on close"
```

### Task 2: Run the repository gates and publish

**Files:**
- No additional source files.

**Interfaces:**
- The branch contains one implementation commit based on `origin/main` and closes Issue #430 through the PR body.

- [x] **Step 1: Run focused and contract checks**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib macos_close_requested
cargo fmt --all -- --check
git diff --check
node scripts/check-sdk-submodule.mjs
```

- [x] **Step 2: Read the finished diff**

```bash
git diff origin/main...HEAD
git status --short
```

Confirm that only the Issue #430 handler, its regression test, and this implementation plan are included; leave the pre-existing untracked `HANDOFF.md` untouched.

- [ ] **Step 3: Push and open one ready PR**

```bash
git push -u origin agent/issue-430-macos-fullscreen-close
gh pr create --base main --head agent/issue-430-macos-fullscreen-close \
  --title "fix: close macOS fullscreen window to background" \
  --body "Fixes #430"
```

- [ ] **Step 4: Wait for required checks and merge without squashing**

```bash
gh pr checks <PR_NUMBER> --watch
gh pr merge <PR_NUMBER> --merge --delete-branch
```

Verify the PR state is `MERGED` and its merge commit is an ancestor of `origin/main`.
