# Issue #551 App residual composition-root audit

Status: final audit approved; delivery pending. This document decides whether the split-later `App.tsx` candidate is complete after PRs #626–#631.

## Measured result

- Audit base: `4ed8312fef75bcc0ed32dfef5728eb67df961812`.
- `App.tsx`: 6,183 newline-delimited lines / 215,821 bytes / SHA-256 `7d126224c1768529a81f95f688c64a09e5e657fc429a8b64920b07f6e8186649`.
- Candidate baseline before App seams: 7,245 lines / 258,131 bytes.
- Residual reduction: 1,062 lines (14.7%).
- Extracted direct leaves: Tauri timeline transport194, QA diagnostics98, SessionVerificationGate567, desktop-attention hook159, UI-latency hook61, backend App runtime10; destructive confirmation joined the existing dialogs owner.
- Residual top-level named declarations before `App`: approximately57 plus the Window augmentation.
- App body direct named declarations: approximately356.

## Delivered ownership seams

| PR | Owner | Result |
| --- | --- | --- |
| #626 | Tauri timeline transport | module-load transport, CoreEvent readiness fence and media save moved; App augmentation remains composition-owned |
| #627 | QA diagnostics projection | stateless DOM/security/timeline diagnostics moved; RAF/state/request fencing stays App-owned |
| #628 | destructive confirmation | pure presentation moved into the existing dialogs owner with App compatibility re-export |
| #629 | SessionVerificationGate | verification/backup/cleanup presentation controller moved; one backend API singleton and App admission remain |
| #630 | desktop attention effects | module resources and memo/effect lifecycle moved; Rust-owned summary/title policy inputs remain App-owned |
| #631 | UI-latency diagnostics | self-contained state/RAF/publication/cancel lifecycle moved with pinned-frame cleanup proof |

Every PR received design/full-diff review, deterministic exactness checks, full local gates, CI7/7 and merge evidence. Public App compatibility exports, Rust DTO/command/event semantics, Tauri registrations and i18n catalogs remained stable.

## Residual hook/resource graph

Inside `App`, direct executed hooks are approximately147:

- state47;
- refs50;
- effects31;
- callbacks13;
- memos3;
- custom/store hooks3.

The attention custom hook executes its internal memo+three effects at the former position, so underlying hook order is unchanged.

Residual resources:

- seven DOM listener registrations;
- four direct Tauri `listen` lifecycles;
- one timeline-transport subscription;
- two module-lifetime QA error listeners;
- five timeout ownership sites;

The sole UI-latency RAF loop now lives entirely in its extracted hook. Each residual listener/subscription/timer cleanup remains in the same React effect or module-lifetime policy owner. No product state is synthesized locally.

## Residual composition

App still owns the boundaries that make it a composition root:

- schema mismatch, boot, restoring/logout, verification/backup, capability-blocked and authentication early gates;
- Rust snapshot admission and StateDelta batching;
- Activity/Explore/Invites/Timeline primary-pane selection;
- ready-shell composition through TopBar, WorkspaceRail, Sidebar, TimelinePane and ContextualRightPanel;
- App-owned dialogs/overlays and caller state;
- Tauri menu/QA/state-refresh/event listener cleanup;
- timeline transport augmentation that applies snapshot/navigation presentation acknowledgments;
- navigation/search/room/space command correlation and stale-result fences;
- diagnostics report/dialog/clipboard request fencing;
- composer submission/draft/upload/typing presentation lifecycle.

## Focused source coverage

App-focused suites remain:

- `App.test.tsx`:78;
- App diagnostics14;
- App search5;
- App space members27;
- App composer lifecycle3;
- total127.

Extracted owners add gate29, QA diagnostics1, dialogs14, desktop-attention28 and UI-latency3 (domain2 + hook1). Source contracts read owner files separately; no new source concatenation was introduced.

## Rejected further seams

- **Composer draft/submission controller:** refs, lease overlays, revision fences, upload settlement, typing, QA send and navigation drain are interleaved from early initialization through command handlers. A hook would return a large callback/state bag or split timer/lease ownership.
- **QA listeners:** QA-send listeners depend on sendText, selection, snapshot refresh and QA refs. The module error listeners are a defensive fallback superseded by `bootErrorCapture` in normal production import order; extraction would be cosmetic, while consolidation/removal requires verify-first behavior/privacy work, not movement.
- **Timeline transport augmentation:** the small memo is exactly where backend transport methods become App snapshot/navigation presentation operations; extracting it creates an arbitrary setter adapter.
- **Avatar effects:** refs precede effects/callbacks and share dedupe/retry state; extraction changes hook order or splits one resource owner.
- **Search/space/room operations:** request fences and refs are shared with navigation, room settings, panels, diagnostics and dialogs; extraction requires controller-sized callbacks or duplicate stale guards.
- **Auth/ready shell:** remaining early gates and shell are the composition root's render responsibility; moving them transfers a giant prop bag.
- **Small pure helpers:** residual helpers are runtime-bound setup/classification used beside their only caller or are already in domain/app leaves; further files would be line-count-only fragmentation.

## Cohesion decision

The residual is one cohesive React composition root: Rust snapshot/event admission, one ordered hook/resource graph, command correlation, transport-to-presentation augmentation, early-gate/shell rendering and caller-owned dialogs. Residual review found the self-contained UI-latency lifecycle after #630; it shipped in #631.

No further move-only seam avoids hook-order changes, resource splitting, callback/prop bags, duplicate Rust semantics, stale-result guard duplication, reverse dependencies or public compatibility churn. The App split-later candidate should be marked complete after an unconditional formal reviewer verdict and merge of this evidence.

## Verification evidence

PRs #626–#631 each passed focused and full repository gates. The audit branch is documentation-only; run agents-doc lint, diff checks and the required repository matrix before delivery. Latest `origin/main` and PR base must match.

## Review gate

- Read-only post-#629 audit found the attention lifecycle seam; #630 delivered it with exactness and CI7/7.
- First residual review then found the self-contained UI-latency RAF/state seam; #631 delivered it with exactness, cleanup proof and CI7/7.
- `reviewer-flash` re-derived all measurements/rejections, verified #631 closed the sole prior finding, and recorded unconditional `Correct-to-record-and-complete-App-checkbox-after-latency-fix`.
- Final local evidence: Vitest1,372, Playwright248 with polling, workspace all-targets, desktop149/1 ignored and Headless Core QA130; typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/diff gates green.
- Delivery and Issue #551 checkbox update pending.
