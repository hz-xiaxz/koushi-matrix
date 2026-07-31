# Activity, Send Lifecycle, and Edit Mentions Implementation Plan

> For agentic workers: use superpowers:executing-plans or superpowers:subagent-driven-development to implement this plan task-by-task.

Goal: Implement issues #393, #388, and #382 from origin/main, verify the complete behavior, and publish/merge one PR.

Architecture: Keep Activity state, send lifecycle semantics, and Matrix edit mention construction in Rust. React, Tauri, and browser fakes transport typed DTOs and render Rust projections. Add diagnostics through the existing private-data-free diagnostics abstraction and preserve the existing send guard lifetime.

Tech Stack: Rust workspace (koushi-state, koushi-core), Tauri v2, React/TypeScript, Vitest, Rust tests, Playwright browser-headless tests, GitHub CLI and connector.

---

## Task 0: Baseline and plan commit

Files:
- Create docs/superpowers/specs/2026-07-31-activity-send-mention-393-388-382-design.md
- Create docs/superpowers/plans/2026-07-31-activity-send-mention-393-388-382.md

- [ ] Verify the isolated worktree is based on origin/main, run git submodule update --init --recursive vendor/matrix-rust-sdk, and run node scripts/check-sdk-submodule.mjs. Expected: clean tracked status and guard exit 0.
- [ ] Commit the approved design and plan with git add docs/superpowers/specs/2026-07-31-activity-send-mention-393-388-382-design.md docs/superpowers/plans/2026-07-31-activity-send-mention-393-388-382.md && git commit -m 'docs: plan activity send and edit improvements'.

## Task 1: #393 remembered Activity tab

Files:
- Modify crates/koushi-state/src/state/activity.rs
- Modify crates/koushi-state/src/reducer/activity.rs
- Modify crates/koushi-state/tests/activity_state.rs
- Modify crates/koushi-core/src/runtime.rs
- Modify crates/koushi-core/tests/runtime_activity.rs
- Modify apps/desktop/src/domain/types.ts
- Modify apps/desktop/src/backend/browserFakeApi.ts
- Modify apps/desktop/src/backend/browserFakeApi.test.ts

- [ ] Add reducer tests for fresh open defaulting to Recent, selecting Unread then closing/reopening preserving Unread, duplicate open while Open preserving active state, duplicate open while Opening preserving the request, and stale SnapshotLoaded not overwriting a newer selection.
- [ ] Run cargo test -p koushi-state --test activity_state remembered_activity_tab -- --nocapture and confirm the new tests fail for the missing remembered-tab behavior.
- [ ] Add a Rust-owned remembered tab alongside the Activity lifecycle, default it to Recent, update it in handle_activity_tab_selected, and use it in handle_activity_opened. Keep request-ID validation and make duplicate OpenActivity idempotent.
- [ ] Mirror the state shape in TypeScript and make BrowserFakeApi.openActivity preserve the selection and avoid overwriting an existing Opening/Open activity.
- [ ] Run cargo test -p koushi-state --test activity_state remembered_activity_tab -- --nocapture and npm --prefix apps/desktop run test -- --run src/backend/browserFakeApi.test.ts; require exit 0.

## Task 2: #393 bounded Activity projection

Files:
- Modify crates/koushi-core/src/runtime.rs
- Modify crates/koushi-core/tests/runtime_activity.rs
- Modify crates/koushi-state/tests/activity_state.rs

- [ ] Add failing projection tests for 201 read events returning exactly the newest 200, older backfill not displacing the window, event re-observation replacing without growing cardinality, old unread retention outside Recent, post-read eviction, and fully-read marker comparison outside Recent.
- [ ] Run the focused runtime_activity test binary and confirm the new assertions fail before implementation.
- [ ] Define ACTIVITY_RECENT_MAX_ROWS as 200. Continue classifying all candidates for Unread, sort Recent deterministically by timestamp descending, room ID ascending, and event ID ascending, and return only the newest 200.
- [ ] Prune rows by retaining the Recent window plus event-backed unread rows, fully-read marker rows, and rows needed by active mark-read reconciliation. Remove other rows and obsolete cleared-event tombstones without mirroring timeline Remove/Truncate/Clear.
- [ ] Emit one coalesced count-only projection diagnostic with observed, stored-before/after, recent-returned, unread-returned, marker/reconciliation-retained, and evicted counts.
- [ ] Run cargo test -p koushi-core --test runtime_activity activity -- --nocapture and cargo test -p koushi-state --test activity_state -- --nocapture; require exit 0.

## Task 3: #388 send lifecycle trace

Files:
- Modify crates/koushi-core/src/timeline.rs
- Modify crates/koushi-core/src/account_work.rs only if a narrow diagnostics adapter is required
- Modify crates/koushi-core/tests/runtime_timeline.rs
- Modify crates/koushi-core/tests/send_queue_fast.rs if the coordinator fixtures belong there

- [ ] Add failing coordinator trace tests for accepted, preflight, guard-acquired, enqueue-started, enqueue-finished, terminal-observed, terminal-bound, and guard-released stages, including enqueue-before-terminal, terminal-before-bind, failure, and cancellation. Assert no Matrix identifiers or content are logged.
- [ ] Run the focused tests and confirm they fail because the per-send trace does not exist.
- [ ] Add an internal generated correlation token, send-kind enum, and count/duration-only event emitter using the existing koushi_diagnostics path.
- [ ] Instrument command acceptance/preflight, interactive guard acquisition, SDK enqueue start/finish, local echo, available worker/encryption/network/upload boundaries, terminal observation, coordinator bind, and guard release. Record immediate/retained/after-bind delivery mode and coarse error classes where available.
- [ ] Keep InteractiveWorkGuard stored in the pending registration until terminal settlement. Run the new trace tests and existing guard/race tests; do not weaken assertions.

## Task 4: #382 shared mention-aware edit transport

Files:
- Modify apps/desktop/src/components/TimelineView.tsx
- Modify the existing shared composer mention primitive in apps/desktop/src/components/composer.tsx or its extracted module
- Modify apps/desktop/src/backend/client.ts
- Modify apps/desktop/src/backend/browserFakeApi.ts
- Modify apps/desktop/src-tauri/src/commands/mod.rs
- Modify apps/desktop/src-tauri/src/dto.rs only if a new DTO mirror is required
- Modify crates/koushi-core/src/command.rs
- Modify crates/koushi-core/src/timeline.rs
- Modify relevant TypeScript and Rust edit tests

- [ ] Add failing UI tests proving @ in main/thread inline edit opens the room-scoped popup, candidate selection passes a structured user ID, and stale candidate results are ignored. Add Core tests for unchanged, added, and removed mentions.
- [ ] Run the focused tests and confirm they fail because edit transport accepts only room_id/event_id/body and autocomplete is disabled.
- [ ] Reuse the normal composer candidate popup, token matching, keyboard selection, IME-safe handling, and intent pruning. Fence edit results by stable room/event/surface identity.
- [ ] Add MentionIntent to React transport, browser fake, Tauri command, Core command, and timeline actor. Seed from effective source mentions and never parse visible @name strings in Core.
- [ ] Construct correct Matrix edit content: top-level revision-new mentions and complete m.new_content.m.mentions for text-like edits; attachment-preserving MediaCaption with the same two-level semantics for audio/file/image/video.
- [ ] Run focused TimelineView tests, npm --prefix apps/desktop run typecheck, node --test scripts/check-ime-text-inputs.test.mjs, and node scripts/check-ime-text-inputs.mjs; require exit 0.

## Task 5: Integrated verification and diff review

- [ ] Run focused Rust gates with explicit exit capture: cargo test -p koushi-state --test activity_state, cargo test -p koushi-core --test runtime_activity, cargo test -p koushi-core --test send_queue_fast, and cargo test -p koushi-core --test runtime_timeline. Read each log and require EXIT=0.
- [ ] Run npm --prefix apps/desktop run typecheck, npm --prefix apps/desktop run lint, and npm --prefix apps/desktop run test -- --run; require exit 0 for each.
- [ ] Run targeted browser-headless Activity, mention-edit, thread-edit, and IME scenarios with one worker, plus any required local Core scenario.
- [ ] Read git diff origin/main...HEAD including untracked files, inspect DTO mirrors, generated artifacts, privacy of diagnostics, and all issue acceptance criteria. Run git status --short.
- [ ] Commit focused implementation changes and any generated contract artifacts only after verification.

## Task 6: Publish, review, and merge

- [ ] Verify gh --version, gh auth status, git status -sb, and intended scope.
- [ ] Push with git push -u origin agent/activity-send-mention-393-388-382.
- [ ] Open one Draft PR against main describing the three issue fixes, root causes, ownership/privacy constraints, and exact checks; link #393, #388, and #382.
- [ ] Inspect PR checks and review comments, use gh for Actions logs where required, fix actionable failures, and rerun local gates.
- [ ] When required checks are green and no blocking review remains, mark the PR ready and merge using the repository-required merge method. Verify the merged PR and main state.
