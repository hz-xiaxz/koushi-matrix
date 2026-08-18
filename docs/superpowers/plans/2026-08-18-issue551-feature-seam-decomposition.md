# Issue #551 — Feature-seam decomposition execution plan

## Objective

Complete Issue #551 through small, behavior-preserving PRs that reduce change collision and make ownership and focused verification boundaries explicit. File length is an observation, not an acceptance target.

## Fixed delivery contract

- One PR moves one independently verifiable seam.
- Production behavior, public imports, wire/serde names, Tauri command names and registration, QA scenario/stage/token contracts, secret filtering, cleanup, and state/lifecycle ownership do not change.
- Rust continues to own product state, Matrix semantics, asynchronous work, retry, shutdown, and subscription residency. React keeps DOM resources and rendering only.
- New modules are private or `pub(crate)` unless an existing public path requires the smallest re-export.
- Keep exhaustive enums, registries, dispatch points, and static catalogs central when splitting them would only create indirection.
- Do not add barrels, one-implementation traits, wrapper services, compatibility aliases, or duplicate helpers to make a move compile.
- A behavior bug discovered during a move stops that PR; fix it verify-first in a separate issue/PR.
- Implementer: `luna-implementer`, GPT-5.6 Luna, low thinking, write-capable. Tasks must be mechanical and narrowly scoped. Escalate rather than letting low-thinking implementation make architectural choices.
- Reviewer: `reviewer-flash` (DeepSeek V4 Flash, read-only), adopted as the recommended different-model default after the user instructed continued autonomous progress. It reviews both the pre-implementation design and post-implementation full diff of each independently mergeable change.

## Per-PR protocol

1. Refresh `origin/main`; create a dedicated branch and worktree if another active worktree owns `main`.
2. Record the exact source seam, inbound callers, outbound dependencies, public visibility, state owner, teardown owner, and focused tests using CodeGraph.
3. Write or update the PR design slice in this plan. Record allowed files and explicitly forbidden behavior/API changes.
4. Run the focused baseline before edits and retain the successful command/result.
5. Obtain the selected reviewer's `Correct-to-merge` design verdict; fix and re-review findings before implementation.
6. Delegate the mechanical move to Luna low. Permit writes only to the named files. No opportunistic cleanup or renaming.
7. Run the same focused command, layer gates, formatting, generated/wire checks, and `git diff --check`.
8. Self-review the full diff for copied logic, visibility expansion, altered ordering, changed string/wire names, and stale source left behind.
9. Obtain the selected reviewer's `Correct-to-merge` full-diff verdict; fix and re-review findings.
10. Run all required local gates, open the PR, wait for every CI check to pass, merge, delete the remote branch, and update the Issue #551 ledger.

A move is incomplete if the old implementation remains copied in the façade or if the new module cannot be tested through the same public behavior.

## Success measures

Record these in each PR and in the final Issue audit:

- the moved symbols and their single owner before/after;
- inbound files that now avoid the former composition root;
- focused checks that cover the seam;
- public symbols/re-exports added (target: zero; explain any non-zero result);
- lifecycle/cleanup owners before/after (must remain one owner);
- deleted lines from the old file versus added forwarding/boilerplate lines;
- merge conflicts involving the former composition root during the delivery period.

No universal line limit is introduced. A residual composition root is acceptable only when its remaining content is one cohesive exhaustive registry or dispatch loop and the final audit records that evidence.

## Ordered delivery ledger

Each checkbox below is one PR unless the pre-implementation design proves it is not independently verifiable. Combining checkboxes requires reviewer approval; splitting one further is allowed.

### Live Issue candidate coverage

| Issue candidate | Planned disposition |
| --- | --- |
| `crates/koushi-core/src/timeline.rs` | Wave 3, projection through actor loop |
| `crates/koushi-core/src/bin/headless-core-qa.rs` | Wave 2, infrastructure then scenario families |
| `crates/koushi-core/src/account.rs` | Wave 3, pure mappings through actor loop |
| `crates/koushi-sdk/src/lib.rs` | Wave 3, leaf feature modules with root re-exports |
| `apps/desktop/src/components/TimelineView.test.tsx` | Wave 1B, shared support then feature suites |
| `crates/koushi-core/src/room.rs` | Wave 3, normalization through actor loop |
| `apps/desktop/e2e/basic-operations.spec.ts` | Wave 1B, support then feature specs |
| `apps/desktop/src/components/TimelineView.tsx` | Wave 4, presentation then viewport/transport |
| `apps/desktop/src-tauri/src/commands/mod.rs` | Wave 1D, builders/waiters into existing modules |
| `scripts/desktop-linux-gui-qa.mjs` | Wave 2, runner/integration/evidence/scenarios |
| `apps/desktop/src/scripts/releaseScripts.test.ts` | Wave 2, scanner/QA/package contracts |
| `crates/koushi-core/src/command.rs` | Wave 1C, feature payloads; exhaustive enums central |
| `crates/koushi-core/src/event.rs` | Wave 1C, feature events; exhaustive routing central |
| `crates/koushi-core/src/store.rs` | Wave 1D, persisted domains and credential backend |
| `crates/koushi-state/tests/session_state.rs` | Wave 1B, feature child modules |
| `apps/desktop/src/components/UserSettingsPanel.tsx` | Wave 1A, section components |
| `crates/koushi-state/tests/navigation_state.rs` | Wave 1B, feature child modules |
| `crates/koushi-core/src/runtime.rs` | Wave 5 after actor seams |
| `apps/desktop/src/App.tsx` | Wave 4 after transport/domain seams |
| `apps/desktop/src/backend/browserFakeApi.ts` | Wave 4 after production DTO seams |
| `apps/desktop/src-tauri/src/lib.rs` | Wave 5 after command extraction |
| `crates/koushi-state/src/reducer/mod.rs` | Wave 5 after command/event seams |
| `crates/koushi-core/src/bin/real-homeserver-qa.rs` | Wave 5 after shared QA support |
| `apps/desktop/src/test/appHarnessMain.tsx` | Wave 4 after browser fake split |
| `apps/desktop/src/i18n/messages.ts` | Deliberately centralized; schema-generation design only |

### Wave 1A — Pilot and presentation seams

The pilot uses an already named component boundary with existing unit and browser coverage. It validates the move-only protocol without touching state ownership.

- [x] **Pilot: User Settings search history** — move `SearchHistorySection` and its crawler-only rows/labels/helpers from `UserSettingsPanel.tsx` to private `components/user-settings/SearchHistorySection.tsx`. Keep all settings patches and callbacks unchanged; do not create an index/barrel.
  - Baseline/post: `npm --prefix apps/desktop test -- --run src/components/UserSettingsPanel.test.tsx`
  - Browser: `(cd apps/desktop && npx playwright test e2e/search-crawler-settings.spec.ts --workers=1)`
  - Layer: typecheck, lint, frontend Vitest, build.
- [ ] **User Settings sessions** — move `SessionsSection`, `SessionRow`, and session-local forms together.
- [ ] **User Settings account management** — move `AccountManagementSection` and its UIA form together.
- [ ] **User Settings security and room-key management** — move `SecuritySection` and its private dialog/status helpers; secret-bearing values remain callback-local and never enter observable state.
- [ ] **User Settings trust** — move `TrustSection`, verification/reset controls, trust rows, and trust-only label/tone helpers.
- [ ] **User Settings appearance controls** — move theme/density/font/emoji buttons together; leave the top-level panel composition and section navigation in `UserSettingsPanel.tsx`.

Focused tests for every User Settings PR are the panel unit test plus the matching Playwright feature spec. The final PR also runs all settings/security/session browser specs.

#### Pilot design slice: Search History

- **Move exactly:** the contiguous 426-line Search History block currently at `UserSettingsPanel.tsx:1791-2216`, including the complete three-line `#77 Search History Crawler section` header/separator and these symbols: `SearchHistorySection`, `CrawlerSpeedButton`, `CrawlerToggle`, `CrawlerRoomRow`, `CrawlerRoomEntry`, `CrawlerLastActiveEntry`, `crawlerRoomEntries`, `crawlerRoomRank`, `summarizeCrawlerRooms`, `crawlerLastActiveEntry`, `crawlerActivityAgeLabel`, `crawlerSpeedLabel`, `crawlerRoomStatusLabel`, and `crawlerFailureKindLabel`. Move the header/separator with the block; leave no dangling or duplicate separator.
- **Create:** `apps/desktop/src/components/user-settings/SearchHistorySection.tsx`. It may import only `t` and the existing `RoomSummary`, `SearchCrawlerFailureKind`, `SearchCrawlerRoomState`, `SearchCrawlerSettings`, `SearchCrawlerSpeed`, `SearchCrawlerState`, and `SettingsPatch` types. Export only `SearchHistorySection` for the direct parent import; do not re-export it from an index.
- **Modify:** `UserSettingsPanel.tsx` only to add that direct import, remove the moved block, and remove exactly the now-unused `SearchCrawlerFailureKind`, `SearchCrawlerRoomState`, `SearchCrawlerSettings`, and `SearchCrawlerSpeed` imports. Keep `SearchCrawlerState`, `RoomSummary`, and `SettingsPatch`, which remain parent props/types. The existing JSX call and props remain byte-for-byte equivalent apart from formatting/import order.
- **Do not modify:** tests, CSS, i18n messages, domain types, callbacks, settings patches, room sorting/ranking, status text, accessibility attributes, confirmation behavior, or Rust state.
- **Ownership invariant:** this remains a pure presentation component. `onUpdateSettings`, rebuild, start, and stop intents continue to cross the existing typed callback boundary; no local product state is added.
- **Privacy invariant:** `CrawlerRoomRow` continues to render only Rust-projected `display_label` or the neutral localized placeholder, never raw `roomId`. Internal `roomId` fields remain allowed for React keys and typed start/stop command callbacks; do not remove or render them during this move.
- **Text invariant:** the existing coarse failure-kind tokens (`roomNotFound`, `sdk`, `decryption`, `indexUnavailable`) intentionally remain unchanged and are covered by tests; do not opportunistically localize or rename them in this move.
- **Expected visibility delta:** one direct module export required by `UserSettingsPanel.tsx`, with no package/barrel/public API export.
- **Expected source effect:** roughly 425 lines leave `UserSettingsPanel.tsx`; forwarding boilerplate is one import and the existing JSX call, with no duplicate implementation retained.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and the new `user-settings/SearchHistorySection.tsx` only. Plan/completion records may be updated by the parent after verification.

### Wave 1B — Focused test suites

- [ ] Create one private `TimelineView` test-support module containing only shared fixtures, DOM geometry mocks, and transport builders currently duplicated across feature groups.
- [ ] Move `TimelineView` viewport/anchoring tests to a feature test file.
- [ ] Move scrollback/virtualization tests to a feature test file.
- [ ] Move rendering/message-state tests to a feature test file.
- [ ] Move interactions/composer tests to a feature test file.
- [ ] Move media tests to a feature test file.
- [ ] Move threads/focused-context tests to a feature test file.
- [ ] Move live-signals/read-state/room-key tests to feature test files; delete the emptied original monolith or retain only genuinely cross-feature tests with evidence.
- [ ] Extract only genuinely shared `basic-operations.spec.ts` fixtures into `e2e/support/`; keep feature assertions out of support.
- [ ] Move room/space and invite/directory scenarios from `basic-operations.spec.ts` to feature specs.
- [ ] Move activity/files/threads/navigation scenarios to feature specs.
- [ ] Move composer/send-queue/upload scenarios to feature specs.
- [ ] Move message actions/media/live-signals scenarios to feature specs.
- [ ] Move profile/settings/session scenarios to feature specs.
- [ ] Move security/E2EE scenarios to feature specs; delete the emptied monolith or retain only evidenced cross-feature smoke scenarios.
- [ ] Split `session_state.rs` into authentication, verification gate, secure backup, device cleanup, and lifecycle child modules under one integration-test root and one minimal shared fixture module.
- [ ] Split `navigation_state.rs` into room list, selection, sidebar/sorting, and anchors child modules under one integration-test root and one minimal shared fixture module.

Run the original complete test target before and after every test move. Test names, fixtures, snapshots, and assertions must not change in a move-only PR.

### Wave 1C — Rust leaf contracts

Keep `CoreCommand`, `CoreEvent`, and their exhaustive top-level dispatch visible in their current façade files. Rust enum declarations cannot be usefully fragmented; extract cohesive payloads, feature enums, private formatting/diagnostic helpers, and tests instead.

- [ ] `command/app.rs`: app/composer/navigation payload types and private diagnostics; retain `AppCommand` declaration centrally if extraction would require wrappers.
- [ ] `command/account.rs`: account request payloads and secret-safe `Debug` helpers.
- [ ] `command/room.rs`: room creation/management payloads and media-independent helpers.
- [ ] `command/timeline.rs`: timeline/media payload types and pure validation/scaling helpers.
- [ ] `command/search.rs`: search command payloads/helpers.
- [ ] Final command façade audit: leave only exhaustive enums, direct impl dispatch, and necessary re-exports; record why any large central match remains cohesive.
- [ ] `event/account.rs`: account/E2EE feature events and privacy-safe formatters.
- [ ] `event/room.rs`: room events and outcome payloads.
- [ ] `event/timeline.rs`: timeline event DTOs and serde helpers.
- [ ] `event/search.rs`, `event/attention.rs`, and `event/live_signals.rs`: move each cohesive leaf family separately.
- [ ] Final event façade audit: preserve checked-in wire artifacts byte-for-byte and retain exhaustive `CoreEvent` routing centrally.

Focused gates: `cargo test -p koushi-core --lib`, command/event privacy-safe Debug tests, wire artifact checks, QA binary tests, and Tauri compile/tests.

### Wave 1D — Store and Tauri adapter seams

`StoreActor` remains the one façade and encrypted/atomic I/O retains one implementation. Child modules may contain direct `impl StoreActor` blocks; do not introduce storage traits.

- [ ] `store/composer_drafts.rs`
- [ ] `store/scheduled_sends.rs`
- [ ] `store/navigation.rs`
- [ ] `store/room_preferences.rs`
- [ ] `store/read_state.rs`
- [ ] `store/credential_backend.rs` for OS/file/in-memory credential implementations and vault migration; keep atomic replacement common and singular.
- [ ] Final store façade/atomic-I/O audit.

For `apps/desktop/src-tauri/src/commands/mod.rs`, move builders/waiters into the existing feature command modules rather than adding another routing layer:

- [ ] authentication/session helpers;
- [ ] room/space operation helpers;
- [ ] timeline/focused-context/upload helpers;
- [ ] search/activity/threads helpers;
- [ ] E2EE/security helpers;
- [ ] final common façade audit retaining only generic submission, snapshot conversion, transaction IDs, and truly cross-feature wait machinery.

Focused gates: core store tests, Tauri library tests, command registration check, frontend golden/contract tests, and affected browser specs.

### Wave 2 — QA seams

- [ ] Split `headless-core-qa.rs` infrastructure into `registry`, `event_wait`, `participants`, `cleanup`, and `diagnostics`, preserving one central scenario/stage/token registry.
- [ ] Move each headless QA scenario family into `scenarios/*`, one family per PR.
- [ ] Split Linux GUI QA into runner, WebDriver, local session, evidence, and scenario modules while preserving entry point and tokens.
- [ ] Split release script tests into diagnostic scanner, QA contract, and packaging contract suites.

Every QA move runs its focused unit/contract tests and the affected disposable-server scenario. The final headless QA move runs `--server=both --scenario=all --core`; Linux GUI remains Tuwunel-only. Artifact privacy and cleanup are inspected, not inferred from exit status.

### Wave 3 — Production leaf actors

Each actor PR records task/subscription owner and shutdown/abort/join path before and after.

- [ ] Split `koushi-sdk/src/lib.rs` leaf-first: client/session, auth, profile, sync, timeline, search, room operations, E2EE, then QA reports; root re-exports preserve the API.
- [ ] Split `room.rs`: normalization and mapping first, then directory/mentions/pins/management/space-members, list observer, operations, and actor loop last.
- [ ] Split `account.rs`: pure session/trust/device/E2EE mappings first, then scheduled-send/observer subsystems, session lifecycle, and actor loop last.
- [ ] Split `timeline.rs`: projection/message mapping, media, read state, gap repair, send queue, subscription residency, manager, and actor loop in that order.

Do not move `TimelineManager` session-resident send/subscription ownership into replaceable `TimelineActor` presentation lifecycle.

### Wave 4 — Frontend orchestration

- [ ] Split `TimelineView.tsx`: message row/body and status surfaces first, media surfaces, transport/projection hook, then viewport controller. React must not acquire Matrix semantics or repair Rust transitions.
- [ ] Split `App.tsx` only after transport/domain seams exist: core transport, composer draft lifecycle, desktop attention, QA diagnostics, and verification gate.
- [ ] Split `browserFakeApi.ts` after production DTO seams stabilize: session/state, rooms, timeline, composer, media, settings, search, and views; mirror Rust contracts exactly.
- [ ] Split `appHarnessMain.tsx` fixtures, commands, composer, media, and events after the browser fake split.

Each visual move runs component tests and the smallest matching Playwright spec. Run the complete browser-headless gate at the end of each source-file decomposition.

### Wave 5 — Composition roots

- [ ] Split `runtime.rs` only after actor seams: connection/app actor, command admission, projection, activity, composer drafts, navigation, and scheduled sends; keep exhaustive dispatch central.
- [ ] Split Tauri `lib.rs` after command extraction: menu, window state, event forwarding, deep links, and bootstrap; keep `generate_handler!` central.
- [ ] Move reducer feature helpers from `koushi-state/src/reducer/mod.rs` into existing reducer modules; keep exhaustive `reduce` dispatch central.
- [ ] Split `real-homeserver-qa.rs` after shared QA support stabilizes: scenario, credentials, cleanup, waits, and startup latency; keep one cleanup/redaction boundary.

## Deferred by design

- `apps/desktop/src/i18n/messages.ts`: do not split. Its static catalog and completeness surface are more coherent together. Revisit only with a separately approved schema-generation design.
- Any exhaustive enum/registry remaining after its leaf payloads move: do not manufacture wrapper enums or macro indirection solely to reduce line count.
- Any seam whose tests require broad behavior changes or whose ownership cannot be stated unambiguously: stop and obtain a new design verdict rather than guessing.

## Full local gate before every PR

Run focused/layer gates first, then the repository gates applicable to the touched layers. Before merge, the default complete set is:

```bash
node scripts/check-sdk-submodule.mjs
node scripts/check-agents-docs.mjs
cargo test --workspace --locked
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo deny check
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test -- --run
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:ui-headless
cargo fmt --all -- --check
git diff --check
```

Add disposable homeserver/GUI lanes and generated/wire artifact comparisons when the touched seam owns those contracts. A host resource failure is fixed or worked around transparently and the same gate is rerun; it is not counted as product success.

## Final completion audit

Before closing Issue #551:

1. Re-read every issue checkbox against merged files and PRs.
2. Confirm all `split now` candidates are decomposed through reviewed merge commits.
3. Confirm every `split later` candidate is decomposed or has evidence that the residual composition root is cohesive; no line-count-only waiver is accepted.
4. Compare public exports, wire artifacts, Tauri registration, QA token registry, and frontend golden outputs with the pre-wave baseline.
5. Confirm one owner remains for every state, subscription, shutdown, cleanup, encrypted format, atomic I/O path, and QA token registry.
6. Run the complete local gate and require all final CI checks green.
7. Update Issue #551 with the PR ledger, review verdicts, test evidence, success measures, and explicit residual central registries; then close it.

## Completion record

- Plan status: draft, awaiting independent design review.
- Implementer selection: Luna low, selected by the user.
- Reviewer selection: `reviewer-flash` (DeepSeek V4 Flash, read-only); the recommended default was adopted when the user instructed continued autonomous progress after the eligible options were presented.
- Non-gating advisory pre-audit: `reviewer-flash` found no Critical/Important issues and five documentation Minors; all five were incorporated.
- Formal Pilot design review: `reviewer-flash` returned `Correct-to-merge`. Its only new Minor identified the opening separator at line 1791 outside the stated range; the range was corrected to the complete 426-line block before implementation.
- Pilot implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved the approved seam. One bounded follow-up truncated the untracked destination file; the same implementer deterministically restored it from the reviewed `HEAD` range, and the parent proved the normalized 13,874-byte block byte-identical before review.
- Formal Pilot full-diff review: `reviewer-flash` reviewed `/tmp/issue551-pilot.diff` (`sha256:cdb9dae149c06ea16b973bd51893166f1d48000fdf490f679fc4bfa7d70cd737`) and returned `Correct-to-merge` with no Critical, Important, or new findings.
- Pilot pre-edit baseline: `UserSettingsPanel.test.tsx` 25/25 passed; focused `search-crawler-settings.spec.ts` 15/15 passed; frontend typecheck and lint passed. The browser run used `CHOKIDAR_USEPOLLING=true` because unrelated processes consume the host inotify quota.
- Pilot delivery PR: #560; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the implementation CI run passed all seven required checks before final ledger update.
- Merged PRs: #560 is the first delivery PR.
