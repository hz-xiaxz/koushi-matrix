# Issue #552 final acceptance audit

Status: final inventory/audit locally verified and approved by exact-final-diff review against `origin/main` `ca9dc74529655b0eeba8e7b1babcbfc333d0b8c3`; pending PR/CI, merge and issue closure.

## Scope and closure rule

This is Phase 7. It changes documentation only. #552 may close only after this audit maps every issue criterion to merged artifacts, the final inventory has no unresolved semantic owner, exact-head CI passes, and the audit PR merges. Optional GPUI Phase 8 is excluded.

## Merged phase evidence

| Phase | Ownership seam | Merged evidence |
| --- | --- | --- |
| 0 | Evidence inventory and phased plan | PR #707, merge `aea695f6` |
| 1 | Rust-owned thread-root lifecycle/display placement/backfill | #708 / PR #710, merge `f61a9eef` |
| 2A | Frontend-neutral `DesktopApi` dependency direction | PR #711, merge `732459ad` |
| 2B1–2B4 | Link/media, desktop attention, window/dialog and event-subscription ports | PRs #712–#715, merges `fd0e6b16`, `5cf5843c`, `5b375a2e`, `90d48bf` |
| 3 | Bounded pre-Core timeline acknowledgement delivery owner | PR #716, merge `0604334e` |
| 4.1 | Room/Space settings view-demand fences proven renderer-specific | PR #720, merge `63174380` |
| 4.2 | Diagnostics dialog intent proven renderer-specific | PR #721, merge `38b00cb1` |
| 4.3a | Bounded/account-scoped Space-member panel demand | PR #722, merge `088bc6b1` |
| 4.3b | Invite-workflow convergence and renderer surface lifetime | PR #723, merge `01e434ee` |
| 4.3c | Rust first-admitted Space invite settlement | PR #724, merge `137b01ff` |
| 4.3d | Rust cancellation settlement + renderer failure epoch | PR #726, merge `7255bfb6` |
| 4.3e | Rust role settlement + renderer failure epoch | PR #727, merge `c022ddc7` |
| 4.4 | Room/Space pre-submit navigation intent epochs | PR #728, merge `dec7e173` |
| 5A | Alias autosave sequencing proven bounded renderer transport/result ownership | PR #729, merge `51ed9fd4` |
| 5B | Main/thread caption sequencing proven mounted-editor/terminal ownership | PR #731, merge `2b560d3f` |
| CI stability | Await initial Space loads; renew composer generation after account switch | PRs #733/#735, merges `f1cbee8c` / `ca9dc745` |
| 6 | Public non-Tauri Core command/event/snapshot/start/shutdown proof | PR #734, merge `4a0f1ca0` |

Each task-level design and exact final diff received the selected read-only `reviewer-flash` `Correct-to-merge` verdict before merge. PR required checks covered Frontend, Playwright DOM, Rust workspace/Tauri/wasm/dependencies, macOS Tauri, Windows ACL, QA binary and Tuwunel/Synapse invitation lanes.

## Acceptance-criterion mapping

### 1. Publish an evidence-based inventory

Complete in `docs/architecture/frontend-ownership-inventory.md`, refreshed to `ca9dc745`. Each row records site, lifetime/disappearance, classification/authority, settlement/duplicate semantics and final decision.

### 2. Identify already-correct Rust/projection paths

The inventory explicitly keeps `appStore`/`timelineStore` as projection caches; Timeline/Room/Account actors, SDK subscriptions, send/composer state, room-key admission and backend tasks remain Rust-owned. DOM measurement, virtualization, focus/dialog/input and mounted feedback remain renderer-owned.

### 3. Identify duplicated transitions/lifecycles

Final reconciliation covers thread-root placement/lifetime, invite/mention query result authority, ACK retry/delivery, App request families, Space-member mutation latest-click inversions, mutation sequencing and room-key immediate feedback. Browser Fake/harness transitions are classified as bounded test mirrors rather than production owners.

### 4. Migrate high-value leaves incrementally

PRs #710–#734 each isolate one reviewable seam. Structural adapter isolation is recorded separately from semantic migrations and was never counted as Rust semantic ownership.

### 5. One documented semantic owner

Rust owns durable aliases/captions, Space membership/roles/cancellation, settings operations, invite workflow, timeline/thread lifecycle, ACK post-acceptance semantics and Core resources. Retained frontend owners are explicitly pre-submit/mounted/cross-surface intent, local failure/feedback, projection/cache, platform adapter or test mirror.

`TimelineView.pendingKeyRequests` is final-classified renderer feedback: it suppresses the transport-to-first-projection gap and owns toast/ARIA feedback only. Rust `DecryptRetryController` and `TimelineActor.key_request_states` own admission/coalescing/terminal state. Existing tests cover duplicate clicks, Rust pending/terminal clear, A→B→A rejection, keyboard activation and private fixed copy.

### 6. Async Rust cancellation and awaited settlement

#708 projection workers cancel and await on replacement/teardown. Existing actor/task owners retain ordered shutdown. Phase 3 adds no Rust task; its finite renderer delivery controller cancels timers synchronously on reset/dispose. Phase 6 proves consumer drop and awaited `CoreRuntime::shutdown`, including lag recovery composition.

### 7. Remove TypeScript semantic state after cutover

Removed artifacts include thread lifecycle/placement registries and derivation, TimelineView ACK retry refs/timers, invite latest-request authority and unbounded Space-member load Map/Set state. Cancel/role refs were renamed/narrowed to local failure presentation. Retained navigation/alias/caption/key-request state has renderer-specific evidence.

### 8. Frontend cleanup stays renderer-local

Final inventory contains no frontend Matrix/durable product transition owner. Kept state is DOM/layout/virtualization/focus/dialog/input, mounted view intent/feedback, transport cache/adapter or test mirror.

### 9. Tauri compatibility

Neutral ports and convergence waits preserve command/event names and serialized DTO contracts. `FrontendDesktopSnapshot` remains in `apps/desktop/src-tauri`; Phase 6 imports no Tauri type.

### 10. Focused tests

Deterministic evidence includes reducer/actor/runtime tests, deferred A/B/A and replacement tests, bounded delivery fake-clock tests, source contracts, full Vitest, Playwright DOM and real-server QA. No migration depends on sleeps, raw log assertions or manual GUI inspection.

### 11. Current Tauri + future native renderer

React/Tauri remains production. The public Core integration test starts Core, attaches consumers, submits a connection-scoped command, observes event/versioned-snapshot convergence and lag recovery, then awaits shutdown without Tauri/WebView types.

## Final owner audit

The inventory has no `Unresolved`, `Keep for now`, or semantic `investigate` decision. Remaining low-priority QA listener consolidation is explicitly deletion/cleanup, not product ownership. Optional GPUI work is not required.

## Fresh local verification

- frontend Vitest: 1516/1516;
- Playwright DOM: 263/263;
- Rust workspace: 2579 passed / 13 ignored;
- Tauri: 177 passed / 1 ignored;
- QA binary: 135/135;
- typecheck, lint/IME/docs, production build and secret scan: passed;
- rustfmt, wasm state/search, cargo-deny and cargo-machete: passed;
- SDK submodule, diagnostic isolation and adapter/domain boundary guards: passed.

The first parallel frontend attempt started typecheck/Playwright before `npm ci` completed and failed only because binaries were absent; after lockfile-local install, both exact commands passed as recorded above.

## Final verification and closure

Before merge:

- run full frontend Vitest, Playwright, typecheck, lint/docs/IME, build and secret scan;
- run Rust workspace, Tauri, QA binary, rustfmt, wasm, cargo-deny/machete, SDK and boundary guards;
- verify exact-diff approval and all eight PR checks on the submitted head.

After merge:

1. verify the audit merge commit is an ancestor of `origin/main`;
2. verify #552 is still open, then close it with links to this audit and the merged phase PRs;
3. verify the issue is closed and final main CI is green;
4. clean only #552 audit branches/worktrees.
