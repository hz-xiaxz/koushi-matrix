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
- [x] **User Settings shared UIA prerequisite** — move the existing `AccountManagementUiaForm` used by both Sessions and Account Management to one private module before either section; this avoids assigning the shared secret-handling boundary to the wrong feature.
- [x] **User Settings sessions** — after the shared UIA prerequisite, move `SessionsSection` and `SessionRow` together.
- [x] **User Settings account management** — after the shared UIA prerequisite, move `AccountManagementSection` and its local forms together.
- [x] **User Settings shared status-primitives prerequisite** — move the existing `TrustStatusRow`, `TrustActionButton`, `TrustTone`, and `DetailRow` used across the panel, Security, and Trust into one direct private shared module before either feature moves.
- [x] **User Settings shared failure-label prerequisite** — move the existing `failureKindLabel` shared by Security and Trust into the same concrete status module before either feature moves; do not expose a composition-root shim.
- [x] **User Settings security and room-key management** — after the shared status prerequisite, move `SecuritySection` and its private dialog/status helpers; secret-bearing values remain callback-local and never enter observable state.
- [x] **User Settings trust** — after the shared status prerequisite, move `TrustSection`, verification/reset controls, and trust-only label/tone helpers.
- [x] **User Settings appearance controls** — move theme/density/font/emoji buttons together; leave the top-level panel composition and section navigation in `UserSettingsPanel.tsx`.

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

#### Shared UIA prerequisite design slice

- **Reason for ordering:** `SessionsSection` and `AccountManagementSection` both call `AccountManagementUiaForm`. Moving it with Sessions would make Account Management depend on a Sessions-owned secret form; moving it with Account Management first would create the reverse feature dependency. Extract the already shared form once before either section.
- **Move exactly:** current `UserSettingsPanel.tsx:1536-1575`, containing only `AccountManagementUiaForm` and its unchanged DOM-ref password lifecycle. After removal, collapse the two adjacent blank separators between `SessionRow` and `KeyManagementStatus` to one; do not move either neighboring symbol.
- **Create:** `apps/desktop/src/components/user-settings/AccountManagementUiaForm.tsx`. Export only `AccountManagementUiaForm`; no index/barrel. Import only React `FormEvent`/`useRef`/`useState`, `Check`, `t`, `ImeSafeForm`, and `SecureImeTextField` through their existing paths.
- **Modify:** `UserSettingsPanel.tsx` only to add the direct import and remove the moved block. No existing parent import becomes unused because all moved dependencies remain used by other sections/rows.
- **Do not modify:** call sites, props, tests, CSS, i18n, domain types, account/session state, callback names, form classes, autocomplete, disabled logic, or clearing order. The similar identity-reset password form inside `TrustSection` is deliberately outside this seam and remains untouched for the later Trust PR.
- **Secret invariant:** the password remains only in the secure DOM ref, is passed directly to the typed callback, is cleared immediately after submission, and never enters React state, logs, diagnostics, or observable product state. `passwordFilled` remains boolean-only presentation state.
- **Expected visibility delta:** one direct module export required by two parent call sites, with no package/barrel/public API export.
- **Expected source effect:** 40 lines leave `UserSettingsPanel.tsx`; forwarding boilerplate is one import, with no duplicate implementation retained.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and the new `user-settings/AccountManagementUiaForm.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `npm --prefix apps/desktop test -- --run src/components/UserSettingsPanel.test.tsx`; `(cd apps/desktop && npx playwright test e2e/basic-operations.spec.ts -g "device session manager renames" --workers=1)`; typecheck and lint.

#### Sessions design slice

- **Move exactly:** the complete `SessionsSection` block at current `origin/main` `UserSettingsPanel.tsx:1105-1214` and the complete `SessionRow` block at `:1455-1535` (191 source lines total). Preserve their internal order in the destination as `SessionsSection` followed by private `SessionRow`; do not move the intervening `AccountManagementSection` or neighboring `RoomKeyPassphraseRequest`/`KeyManagementStatus` declarations.
- **Create:** `apps/desktop/src/components/user-settings/SessionsSection.tsx`. Export only `SessionsSection` for one direct parent import; keep `SessionRow` private and add no index/barrel. Import only React `FormEvent`/`useState`, the existing `Check`/`Edit3`/`RefreshCcw`/`X` icons, `t`, `ImeSafeForm`, `ImeTextField`, `AccountManagementUiaForm`, and the existing `AccountManagementState`, `DeviceSessionListState`, and `DeviceSessionSummary` types through their direct paths.
- **Modify:** `UserSettingsPanel.tsx` only to add the direct `SessionsSection` import, remove the two moved blocks, remove the now-unused `DeviceSessionSummary` type import, and collapse the two blank separators left around the removed `SessionRow` to one. Keep `AccountManagementUiaForm` in the parent because `AccountManagementSection` remains there. Keep the existing `SessionsSection` JSX call and every prop unchanged.
- **Do not modify:** tests, CSS, i18n, domain/Rust types, callbacks, account-management state, UIA operation filters, loading/failure presentation, device ordering, current/other partition, ordinal collection, rename trimming/fallback, IME `syncKey`, badges, button labels/classes, disabled behavior, or accessibility attributes.
- **Ownership invariant:** Rust remains owner of the device-session list and account-management/UIA product state. The extracted React component owns only the currently-renamed ordinal and one row's text draft. It dispatches the same typed query/rename/delete/UIA intents and does not repair or synthesize command results.
- **Lifecycle invariant:** preserve existing React-key and local-draft behavior exactly. Do not add effects, retries, timers, subscriptions, draft synchronization, or teardown machinery during this move.
- **Privacy invariant:** render only the existing Rust-projected device display name, verification/inactivity booleans, and localized neutral placeholder. Device ordinals remain internal React keys and typed callback arguments; do not render identifiers or add logging/diagnostics.
- **Expected visibility delta:** one direct module export and one parent import, with `SessionRow` private and no package/barrel/public API growth.
- **Expected source effect:** 191 source lines leave `UserSettingsPanel.tsx`; forwarding boilerplate is one import and the existing JSX call, with no duplicate implementation retained.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and the new `user-settings/SessionsSection.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25; focused `basic-operations.spec.ts` device-session-manager test 1/1 using `CHOKIDAR_USEPOLLING=true`; frontend typecheck; frontend lint. All four pre-edit baseline checks passed on merge base `db234d4c`.

#### Account Management design slice

- **Move exactly:** the complete contiguous `AccountManagementSection` block at current `origin/main` `UserSettingsPanel.tsx:1105-1342` (238 source lines). Do not move the neighboring `RoomKeyPassphraseRequest`, `KeyManagementStatus`, shared `AccountManagementUiaForm`, or any Trust/Security symbol.
- **Create:** `apps/desktop/src/components/user-settings/AccountManagementSection.tsx`. Export only `AccountManagementSection`; add no index/barrel. Import only React `FormEvent`/`useEffect`/`useRef`/`useState`, the existing `ExternalLink`/`KeyRound`/`ShieldAlert` icons, `t`, `ImeSafeForm`, `SecureImeTextField`, `AccountManagementUiaForm`, and the existing `AccountManagementCapabilities`, `AccountManagementState`, and `SavedSessionInfo` types through their direct paths.
- **Modify:** `UserSettingsPanel.tsx` only to add the direct `AccountManagementSection` import, remove the moved block, remove the now-unused parent `AccountManagementUiaForm` import and `ExternalLink` icon import, and collapse the two blank separators left by removal to one. Keep `KeyRound`, `ShieldAlert`, all three domain type imports, and the existing `AccountManagementSection` JSX call/props because they remain parent dependencies.
- **Do not modify:** props, tests, CSS, i18n, domain/Rust types, account-management capability loading or effect dependencies, active-operation filtering, change-password/deactivation/UIA callbacks, password comparison, trimming behavior (none exists), erase-data semantics, status rendering, autocomplete, test IDs, classes, disabled logic, clearing order, or accessibility attributes.
- **Ownership invariant:** Rust remains owner of capabilities, account operation/UIA state, and command outcomes. The extracted React component retains only presentation booleans, the erase-data checkbox, mismatch/completeness flags, and DOM refs; it dispatches the same typed load/change/deactivate/manage/UIA intents and does not repair command results.
- **Secret invariant:** both new-password values remain only in secure DOM refs and are passed directly through the existing typed callback after exact equality comparison. Reset continues to clear both refs and all boolean form state in the existing order. The shared UIA form continues to clear its own DOM-ref password immediately. No secret may enter React state, props beyond the immediate typed callback, logs, diagnostics, or observable product state. Do not opportunistically add post-submit clearing in this move.
- **Lifecycle invariant:** preserve the existing capability-loading effect and dependency list byte-for-byte. Add no retry, timer, subscription, cleanup, synchronization, or additional effect.
- **Expected visibility delta:** one direct module export/import; the shared UIA form becomes a direct child-module dependency instead of a parent dependency; no package/barrel/public API growth.
- **Expected source effect:** 238 source lines leave `UserSettingsPanel.tsx`; forwarding boilerplate is one import and the existing JSX call, with no duplicate implementation retained.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and the new `user-settings/AccountManagementSection.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25, including direct capability/button/success/UIA/secure-DOM-ref coverage; `session-status.spec.ts` 3/3 covers the shared delegated-URL/opener route and panel rendering, while the panel's own manage-account button remains unit-gated; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `359d3246`. No existing Playwright scenario directly manipulates the local password/deactivation forms, so the unit suite is their focused behavior gate and the complete browser-headless suite remains mandatory before merge; this move-only PR must not invent a new test or behavior.

#### Shared settings status-primitives prerequisite design slice

- **Reason for ordering:** `SecuritySection` and `TrustSection` both render `TrustStatusRow`/`TrustActionButton`, while the top-level session summary and Security both render `DetailRow`; `TrustTone` is consumed by the shared row and trust-only status helpers. Assigning these existing shared presentation primitives to either feature would create the wrong ownership direction. Extract them once before Security or Trust moves.
- **Move exactly:** the contiguous `TrustStatusRow` + separator + `TrustActionButton` block at current `origin/main` `UserSettingsPanel.tsx:1463-1514` (52 lines), the `TrustTone` declaration at `:1648` (1 line), and `DetailRow` at `:2169-2176` (8 lines), for 61 source lines total. Preserve that source order in the destination; do not move neighboring recovery/identity-reset, trust status-label/tone, toggle, or session helper symbols.
- **Create:** `apps/desktop/src/components/user-settings/SettingsStatusPrimitives.tsx`. Import only React `ReactNode`. Directly export exactly the four already shared declarations: `TrustStatusRow`, `TrustActionButton`, `TrustTone`, and `DetailRow`; add no index/barrel, wrapper, generalized prop abstraction, or new helper.
- **Modify:** `UserSettingsPanel.tsx` only to add one direct import for the four moved declarations (using inline `type TrustTone`), remove the three moved ranges, and collapse each resulting adjacent blank separator to one. Keep the parent `ReactNode` import because other parent helpers still use it. Keep every existing call site byte-for-byte unchanged.
- **Do not modify:** component/type names, props, defaults, tone union values, markup, classes, ARIA, button type/disabled/click semantics, tests, CSS, i18n, domain/Rust types, Security/Trust logic, or session rendering.
- **Ownership invariant:** this module owns only stateless DOM presentation primitives and one closed presentation-tone union. Rust continues to own all product state and outcomes; no hook, effect, state, retry, timer, subscription, callback adaptation, or cleanup is introduced.
- **Expected visibility delta:** four direct module exports consumed by the current panel composition file; no package/barrel/public API export. Later approved Security/Trust modules may import the same concrete primitives directly instead of through the composition root.
- **Expected source effect:** 61 source lines leave `UserSettingsPanel.tsx`; forwarding boilerplate is one direct import, with no duplicate implementation retained.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and the new `user-settings/SettingsStatusPrimitives.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25; focused `basic-operations.spec.ts` Security/local-encryption, E2EE trust-controls, and room-key/secure-backup scenarios 3/3 using `CHOKIDAR_USEPOLLING=true`; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `c0121719`.

#### Shared failure-label prerequisite design slice

- **Reason for ordering:** Security's room-key/backup labels and Trust's cross-signing/key-backup/identity-reset/verification labels all call the same closed `failureKindLabel`. Leaving it in `UserSettingsPanel.tsx` would require a composition-root export shim; assigning it to Security or Trust would create the wrong feature dependency. Move the existing mapper into the already merged concrete status module before either feature moves.
- **Move exactly:** the complete `failureKindLabel` block at current `origin/main` `UserSettingsPanel.tsx:1752-1769` (18 lines / parent-measured 535 bytes, to be re-proved after the move). Do not move neighboring `deviceTrustIcon`, tone helpers, any failure enum, or i18n catalog entry.
- **Modify:** `apps/desktop/src/components/user-settings/SettingsStatusPrimitives.tsx` to import `t` from `../../i18n/messages` and the existing `TrustOperationFailureKind` type from `../../domain/types`, then append the exact mapper body with only the required `export` keyword. The module then directly exports five concrete shared presentation declarations; it remains a leaf implementation module, not a barrel.
- **Modify parent:** add `failureKindLabel` to the existing direct status-primitives import, remove the moved block and now-unused parent `TrustOperationFailureKind` type import, and collapse the resulting adjacent blank separators to one. Keep all eight existing call sites byte-for-byte unchanged.
- **Do not modify:** failure tokens, switch exhaustiveness, labels, call sites, tests, CSS, i18n, domain/Rust types, Security/Trust state, callbacks, or lifecycle.
- **Ownership invariant:** this is a pure closed presentation-label mapping shared by two features. It adds no state, hook, effect, retry, timer, subscription, callback, logging, identifier rendering, or cleanup.
- **Expected visibility/source effect:** one additional direct export/import; 18 source lines leave `UserSettingsPanel.tsx`; no wrapper, duplicate, compatibility shim, barrel, or package/public API.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and existing `user-settings/SettingsStatusPrimitives.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25; focused Security/Trust/room-key Playwright scenarios 3/3 with polling; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `9e8d5d40`.

#### Trust and verification/reset controls design slice

- **Move exactly:** the contiguous `TrustSection` + `VerificationDialog` + `IdentityResetAuthControls` + `DeviceTrustList` range at current `origin/main` `UserSettingsPanel.tsx:784-1108` (325 lines / parent-measured 10,345 bytes), followed in the destination by the contiguous Trust-only status/label/tone/action-helper range `trustOverallStatus` through `keyBackupActionAvailable` at `:1110-1316` (207 lines / parent-measured 5,767 bytes). Re-prove both byte counts after the move. Preserve each range byte-for-byte and place one separator between them; do not move the parent composition, existing direct `TrustSection` call, neighboring Appearance helpers, shared status/failure primitives, or Security module.
- **Create:** `apps/desktop/src/components/user-settings/TrustSection.tsx`. Export only `TrustSection`; keep all dialogs, controls, device rendering, and status/label/tone/action helpers private; add no index/barrel. Import only React `FormEvent`/`ReactNode`/`useRef`/`useState`, icons `Check`/`KeyRound`/`RotateCcw`/`ShieldAlert`/`ShieldCheck`/`ShieldQuestion`/`ShieldX`/`X`, `t`, `ImeSafeForm`/`SecureImeTextField`, existing shared `TrustActionButton`/`TrustStatusRow`/`TrustTone`/`failureKindLabel`, `TrustHelpButton` through `../TrustHelp`, and existing `CrossSigningStatus`, `DeviceTrustLevel`, `E2eeTrustState`, `IdentityResetState`, `KeyBackupStatus`, and `VerificationFlowState` types through direct paths.
- **Modify parent:** add one direct `TrustSection` import; remove the two moved ranges; remove now-unused `KeyRound`/`RotateCcw`/`ShieldAlert`/`ShieldQuestion`/`ShieldX`/`X`, `SecureImeTextField`, all four shared status/failure imports, `TrustHelpButton`, and `CrossSigningStatus`/`DeviceTrustLevel`/`IdentityResetState`/`KeyBackupStatus`/`VerificationFlowState` type imports. Keep React `FormEvent`/`ReactNode`/`useRef`/`useState`, `Check`, `ShieldCheck`, `ImeSafeForm`/`ImeTextField`, and `E2eeTrustState` because the parent still consumes them. Collapse each removal site to one blank separator. Keep the existing `TrustSection` JSX call and all ten props byte-for-byte unchanged.
- **Do not modify:** callback names or flow IDs, verification state rendering, SAS emoji ordering/keys/ARIA, reset authentication branches, status switches/failure labels/tones/action availability, device keys/ordinals/labels/icons, button variants/disabled logic, test IDs/classes, tests, CSS, i18n, domain/Rust types, or Security behavior.
- **Ownership invariant:** Rust remains owner of E2EE trust, verification, cross-signing, backup, identity-reset, and device product state/outcomes. React retains only the existing boolean `passwordFilled` presentation state and secure password DOM ref, dispatches the same typed intents, and never synthesizes or repairs command results.
- **Secret/lifecycle invariant:** identity-reset password remains only in `SecureImeTextField`'s DOM ref and is passed directly to the typed callback, then the ref and boolean presentation flag are cleared in the existing order. Preserve OAuth/unknown/UIAA branching and cancel behavior exactly. Add no effect, retry, timer, subscription, async fence, callback adaptation, logging, or cleanup machinery.
- **Privacy invariant:** raw verification target user/device IDs and device IDs remain internal Rust DTO fields or React keys and are never rendered; the UI continues to render only neutral device ordinals, coarse trust labels, and SAS symbols. No secret or identifier logging is added.
- **Expected visibility/source effect:** one direct module export/import; 532 source lines leave `UserSettingsPanel.tsx`; no duplicate, wrapper, shim, barrel, package export, or public API.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and new `user-settings/TrustSection.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25 and focused `basic-operations.spec.ts` E2EE trust-controls scenario 1/1 with polling; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `1b36ce3b`. The focused browser scenario covers verification acceptance, cross-signing/key-backup actions, identity-reset cancel and DOM-ref password submission, Rust snapshot transitions, and non-rendering of the raw trust target; it does not directly exercise OAuth, unknown-auth, or empty-password disabled branches, so formal full-diff review must verify those branches and disabled logic byte-for-byte. Complete browser-headless remains mandatory before merge.

#### Appearance controls design slice

- **Keep the ownership seam in the parent:** retain the existing `settings-appearance` section element, heading, saving indicator, section-navigation button, and the parent-derived `selectedTheme`/`selectedFont`/`selectedEmoji` values in `UserSettingsPanel.tsx`. Replace only the controls body with one direct `AppearanceControls` call. This keeps top-level section composition/navigation in the panel while grouping the four mutually coupled control families.
- **Move exactly:** the contiguous controls JSX currently at `UserSettingsPanel.tsx:475-558` (84 lines / parent-measured 3,284 bytes) into the return value of `AppearanceControls`, and the contiguous `ThemeButton` through `EmojiButton` helper range at `:768-878` (111 lines / parent-measured 2,092 bytes). Re-prove both moved bodies byte-for-byte after accounting only for the new component wrapper. Do not move the section/heading/saving indicator, neighboring Notification helper, other settings controls, or parent-derived values.
- **Create:** `apps/desktop/src/components/user-settings/AppearanceControls.tsx`. Export only `AppearanceControls`; keep `ThemeButton`, `DensityButton`, `FontButton`, and `EmojiButton` private; add no index/barrel. Import only `t`, `DisplayDensity` through `../../app/localPresentation`, and existing `EmojiPreference`, `FontPreference`, `SettingsPatch`, and `ThemePreference` types through `../../domain/types`. No React import is required.
- **New component contract:** exactly six props: `displayDensity`, `selectedEmoji`, `selectedFont`, `selectedTheme`, `onDisplayDensityChange`, and `onUpdateSettings`, with the same existing types. The destination requires the already resolved `displayDensity` value after the parent applies its existing `"comfortable"` default. The parent direct call forwards those six existing values/callbacks without adaptation. Do not pass the complete settings object or saving state.
- **Modify parent:** add one direct `AppearanceControls` import; replace the 84-line controls body with the six-prop direct call; remove the 111-line helper range; remove now-unused `EmojiPreference`, `FontPreference`, and `ThemePreference` type imports. Keep `DisplayDensity` because it remains in parent props, and keep `SettingsPatch` because other parent controls and props consume it. Collapse the excess separator left before `NotificationSettingToggle` to one.
- **Do not modify:** settings patch shapes, selected guards, `aria-pressed`, button types/classes/labels, option order, density callback semantics, typography pairing (`font` always carries current emoji and vice versa), parent saving indicator, tests, CSS, i18n, domain/Rust types, or other settings behavior.
- **Ownership invariant:** Rust remains owner of persisted appearance/typography settings and command outcomes; the existing local display-density preference remains owned by the existing parent/App presentation boundary. The extracted module is stateless DOM presentation and dispatches only the same typed callbacks. Add no state, effect, retry, timer, subscription, fence, callback adaptation, logging, or cleanup.
- **Expected visibility/source effect:** one direct module export/import; 195 source lines leave `UserSettingsPanel.tsx` and one six-prop call replaces the controls body; all four button helpers stay private. No duplicate, wrapper-only forwarding layer, shim, barrel, package export, or public API.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and new `user-settings/AppearanceControls.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25 and focused `basic-operations.spec.ts` typography-settings scenario 1/1 with polling; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `887e5b25`. The focused browser scenario exercises font/emoji patch pairing and Rust snapshot application; theme/density selected guards and callbacks lack a direct existing interaction test, so formal full-diff review must prove those helpers byte-for-byte. Complete browser-headless, including `theme.spec.ts`, remains mandatory before merge.

#### Security and room-key management design slice

- **Move exactly:** the contiguous `SecuritySection` + `RoomKeyPassphraseRequest` + `KeyManagementStatus` range at current `origin/main` `UserSettingsPanel.tsx:790-1125` (336 lines / parent-measured 12,194 bytes), followed in the destination by the contiguous Security-only status/label-helper range `credentialStoreLabel` through `recoveryKeyDeliveryLabel` at `:1321-1467` (147 lines / parent-measured 4,227 bytes). Re-prove both byte counts after the move. Preserve each range byte-for-byte and place one separator between them; do not move neighboring top-level panel, `TrustSection`, verification dialog, identity-reset, Trust tone/status, shared-primitives, or session helpers.
- **Create:** `apps/desktop/src/components/user-settings/SecuritySection.tsx`. Export only `SecuritySection`; keep the request type and all status/label helpers private; add no index/barrel. Import only React `FormEvent`/`ReactNode`/`useRef`/`useState`, icons `Download`/`KeyRound`/`RefreshCcw`/`RotateCcw`/`ShieldAlert`/`ShieldCheck`/`ShieldQuestion`/`ShieldX`/`Upload`, `t`, `ImeSafeForm`, `SecureImeTextField`, the existing shared `DetailRow`/`TrustActionButton`/`TrustStatusRow`/`TrustTone`/`failureKindLabel`, and existing `DisplayPlatform`, `E2eeTrustState`, `LocalEncryptionState`, `RecoveryKeyDeliveryState`, `RoomKeyExportState`, `RoomKeyImportState`, `SecureBackupPassphraseChangeState`, and `SecureBackupSetupState` types through direct paths.
- **Modify parent:** add one direct `SecuritySection` import, remove the two moved ranges, remove now-unused `Download`/`Upload` icon imports and the five now-unused `RecoveryKeyDeliveryState`/`RoomKeyExportState`/`RoomKeyImportState`/`SecureBackupPassphraseChangeState`/`SecureBackupSetupState` type imports, and collapse each resulting adjacent blank separator to one. Keep `DisplayPlatform`, `E2eeTrustState`, `LocalEncryptionState`, all shared status-primitives imports, and all other React/icon/IME imports because parent/Trust still use them. Keep the existing `SecuritySection` JSX call and all thirteen props byte-for-byte unchanged.
- **Do not modify:** callbacks, file-chooser sequencing/cancellation, key-management state, request-path state, status switches, labels/failure mapping, form classes, test IDs, autocomplete, disabled logic, dialog ARIA, clearing order, tests, CSS, i18n, domain/Rust types, or Trust behavior.
- **Ownership invariant:** Rust remains owner of local-encryption/key-management product state and outcomes. React retains only the currently selected non-secret file path/request kind and secure DOM refs, then dispatches the same typed probe/reset/export/import/bootstrap/passphrase-change/open-recovery intents; it does not synthesize or repair command results.
- **Secret/lifecycle invariant:** room-key and secure-backup secrets remain only in `SecureImeTextField` DOM refs, never React state/logs/diagnostics. Preserve exactly when each ref is retained on chooser cancellation, cleared after typed callback dispatch, or cleared on dialog cancel. Preserve the current awaited chooser order and add no effect, retry, timer, subscription, async fence, callback adaptation, or cleanup machinery.
- **Privacy invariant:** destination/source paths remain local request state or typed callback arguments and are never rendered or logged. Existing coarse failure labels remain the only failure detail surfaced.
- **Expected visibility/source effect:** one direct module export/import; 483 source lines leave `UserSettingsPanel.tsx`; no duplicate, wrapper, shim, barrel, package export, or public API.
- **Allowed implementation files:** `UserSettingsPanel.tsx` and new `user-settings/SecuritySection.tsx` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** `UserSettingsPanel.test.tsx` 25/25; focused Security/local-encryption, E2EE trust-controls, and room-key/secure-backup Playwright scenarios 3/3 with polling; frontend typecheck; frontend lint. All four pre-edit checks passed on merge base `7cd8c255`.

### Wave 1B — Focused test suites

- [x] Create one private `TimelineView` test-support module containing only shared fixtures, DOM geometry mocks, and transport builders currently duplicated across feature groups.
- [x] Move `TimelineView` viewport/anchoring tests to a feature test file.
- [x] Move scrollback/virtualization tests to a feature test file.
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

#### TimelineView shared test-support design slice

- **Scope rule:** extract only helpers already used across at least two of the planned TimelineView feature groups. This prerequisite moves no test, assertion, snapshot, describe block, cleanup hook, production export, or product code.
- **Move exactly:** four non-contiguous ranges from current `origin/main` `apps/desktop/src/components/TimelineView.test.tsx`: `KEY` at `:91` (1 line / parent-measured 80 bytes); `message` at `:109-133` (25 / 590); the contiguous `imageMessage` + separator + `fileMessage` range at `:143-181` (39 / 882); and `navigationSnapshot` + `baseTransport` + `mockTimelineRects` at `:189-275` (87 / 2,763). Preserve source order and one separator between ranges; re-prove each normalized body byte-for-byte after adding only required `export` keywords.
- **Why these are shared:** `KEY`, `message`, and `baseTransport` are used throughout all planned feature groups; `imageMessage` spans edit/interactions and media; `fileMessage` spans reply-capability rendering and media; `navigationSnapshot` spans viewport/virtualization and thread navigation; `mockTimelineRects` spans anchoring/read-state/scrollback and thread rendering.
- **Deliberately retain feature-local helpers:** keep `latestEventSummary` with its one table-driven rendering test; `messages` with virtualization/scrollback; the documented `mockPresentationOrderRects` with latest-reply/thread projection; `installResizeObserverMock` with live-edge viewport tests; `changeInlineEditorText` with edit/composer interactions; and `expectLocalizedTooltip` with reply-action rendering. Do not move the whole top helper block or strand the presentation-order comment.
- **Create:** private direct leaf `apps/desktop/src/components/timelineViewTestSupport.ts`. Import only `vi` from `vitest`, `roomTimelineKey` and `TimelineItem` through `../domain/coreEvents`, and `TimelineTransport` through `./TimelineView`. Directly export exactly `KEY`, `message`, `imageMessage`, `fileMessage`, `navigationSnapshot`, `baseTransport`, and `mockTimelineRects`; add no index/barrel, generalized fixture factory, reset wrapper, default export, production re-export, or new dependency.
- **Modify parent:** add one direct import for the seven moved declarations; remove the four exact ranges; remove only the now-unused `TimelineTransport` type from the existing `TimelineView` import. Retain `roomTimelineKey` and `TimelineItem`, which remain directly used by feature-local tests. Collapse every exposed double separator to one, including the `KEY` removal, `message` removal, media-fixture removal, and retained `messages` → presentation-order-comment boundaries; keep the documentation comment attached directly to `mockPresentationOrderRects`.
- **Behavior/ownership invariant:** all fixture values, event/user/room identifiers, timestamps, transport no-op semantics, override precedence, DOM rect lookup order, scroll-offset handling, defaults, and returned DOMRect fields remain byte-identical. Test support owns no product state and adds no global setup, cleanup, timer, observer, subscription, listener, log, snapshot, or lifecycle behavior.
- **Expected visibility/source effect:** seven direct exports inside a test-only private module and one direct test import; 152 helper source lines leave the 12,059-line monolith. No package/public API, production façade, compatibility shim, duplicate, TODO, or dead helper.
- **Allowed implementation files:** `TimelineView.test.tsx` and new `timelineViewTestSupport.ts` only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** run the complete original target `npm --prefix apps/desktop test -- --run src/components/TimelineView.test.tsx`, plus frontend typecheck and lint. Baseline on merge base `8ca09f94`: 173/173 tests, typecheck, and lint passed. Full frontend/browser/Rust/QA/Tauri/static gates remain mandatory before merge even though this PR changes test support only.

#### TimelineView viewport/anchoring test design slice

- **Scope rule:** move only tests whose primary contract is automatic live return, room scroll-anchor persistence/restoration, or sent-message live-edge locking. Do not absorb tests merely because their setup observes scroll position.
- **Move exactly:** from `apps/desktop/src/components/TimelineView.test.tsx` on merge base `fc6e9bb7`, move the viewport-only `installResizeObserverMock` helper at `:192-233` (42 lines / parent-measured 997 bytes) and these twenty complete top-level test blocks in source order: `:262-330`, `:332-403`, `:405-498`, `:500-564`, `:566-601`, `:603-692`, `:5159-5227`, `:5229-5328`, `:5330-5448`, `:5582-5731`, `:5733-5801`, `:5803-5905`, `:5907-5973`, `:5975-6051`, `:6053-6119`, `:6121-6198`, `:6200-6304`, `:6306-6402`, `:6404-6511`, and `:6513-6582` (1,705 lines / 49,573 bytes). The blocks contain 22 cases because `:603-692` is a three-row `it.each`. Re-prove every moved body byte-for-byte and preserve source order, test names, assertions, fixtures, and internal mount/rerender order.
- **Selected behavior families:** the first six blocks prove anchored/focused automatic live return, missing-proof retention, transient proof loss, and rejected-close retry. The remaining fourteen prove visible-anchor capture, sent-message anchor persistence, in-session restoration/fallback/one-shot application, free-scroll stability, first-entry/resync behavior, sent-local-echo pinning, content-growth locking, and immediate user-input lock release.
- **Deliberate exclusions:** keep `:5450-5580` (`auto-backfills...`) with scrollback/virtualization because pagination is its asserted outcome. Keep focused-target centering/projection tests with threads/focused-context; diagnostics/virtual-height/pagination/repair tests with scrollback/virtualization; read-marker/receipt/jump tests with read-state; and media visibility tests with media. `installResizeObserverMock` is used only by two selected live-edge tests, so move it privately with this suite rather than exporting it from shared support.
- **Create:** private direct test file `apps/desktop/src/components/TimelineView.viewport.test.tsx`, retaining `// @vitest-environment jsdom`. Import `act`, `cleanup`, `fireEvent`, `render`, `screen`, and `waitFor` from Testing Library; `afterEach`, `describe`, `expect`, `it`, and `vi` from Vitest; `focusedTimelineKey`, `CoreEventPayload`, and `TimelineGapId` from `../domain/coreEvents`; `setActiveLocaleProfile` from `../i18n/messages`; `KEY`, `baseTransport`, `message`, and `mockTimelineRects` directly from `./timelineViewTestSupport`; and `TimelineView` plus `clearTimelineViewportSessionMemoryForTests` from `./TimelineView`. Add no React import, support wrapper, barrel, generalized viewport fixture, or production export.
- **Suite/cleanup invariant:** wrap moved blocks in the same `describe("TimelineView", ...)` so full test names remain unchanged. Copy the existing six-statement `afterEach` (`cleanup`, viewport-session-memory reset, English locale reset, real timers, mock/global restoration) into the destination and retain it unchanged in the parent. This intentionally gives each independently runnable file the same isolation; no test may depend on another test file or execution order.
- **Modify parent:** remove only the approved helper and test ranges. No parent import becomes unused; retain Testing Library APIs, Vitest APIs, `focusedTimelineKey`, `TimelineGapId`, `CoreEventPayload`, locale reset, viewport-memory reset, and shared-support imports because retained groups still use them. Collapse exposed separators to one after `mockPresentationOrderRects`, between the retained room-summary and edit tests, on both sides of retained `auto-backfills...`, and before the retained receipt-rendering test. Keep neighboring comments attached to their retained tests.
- **Behavior/ownership invariant:** move no production code, test-support implementation, snapshot, hook from the parent, fixture value, assertion, timer mode, observer callback behavior, scroll geometry, request ordering, mock restoration, or viewport session ownership. React test cleanup remains per-file DOM lifecycle; product viewport/session ownership is unchanged.
- **Expected effect:** 22/173 cases and 1,705 test-body lines leave the monolith for a cohesive feature suite; the parent retains 151 cases. The sole duplicated code is the existing six-statement file-isolation hook required for independently runnable Vitest files. No duplicate helper, compatibility shim, TODO, dead test, `.only`, `.skip`, or public API.
- **Allowed implementation files:** `TimelineView.test.tsx` and new `TimelineView.viewport.test.tsx` only. `timelineViewTestSupport.ts` and production files are read-only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** merge-base `fc6e9bb7` complete original target 173/173, frontend typecheck, and lint passed. Post-move require one combined invocation over both files to remain exactly 173/173, plus independent destination 22/22 and parent 151/151 runs, typecheck, lint, and diff check. Full frontend/browser/Rust/QA/Tauri/static gates remain mandatory before merge.

#### TimelineView scrollback/virtualization test design slice

- **Scope rule:** move only tests whose primary contract is stable-prepend/prefetch classification, virtual-height/scroll diagnostics, pagination admission, backfill repair fencing, rendered-projection acknowledgement, underfill suppression, or anchor-triggered pagination. Incidental viewport windowing does not make media/read/thread tests part of this seam.
- **Move exactly:** from `apps/desktop/src/components/TimelineView.test.tsx` on merge base `873f90d2`, move private helper `messages` at `:123-127` (5 lines / parent-measured 182 bytes); the two complete helper-contract tests at `:659-663` and `:665-669` (10 test-body lines / 598 bytes); and the complete contiguous 27-block range `:2560-4814` (2,255 lines / 66,120 bytes including its 26 internal one-line separators). Within the contiguous range the exact block starts are `2560`, `2607`, `2700`, `2801`, `2916`, `2995`, `3107`, `3200`, `3305`, `3418`, `3447`, `3520`, `3570`, `3685`, `3792`, `3883`, `3977`, `4053`, `4109`, `4171`, `4224`, `4289`, `4395`, `4487`, `4559`, `4632`, and `4684`; block ends are respectively `2605`, `2698`, `2799`, `2914`, `2993`, `3105`, `3198`, `3303`, `3416`, `3445`, `3518`, `3568`, `3683`, `3790`, `3881`, `3975`, `4051`, `4107`, `4169`, `4222`, `4287`, `4393`, `4485`, `4557`, `4630`, `4682`, and `4814`. Preserve all 29 blocks byte-for-byte and source-ordered.
- **Case count:** the two selected `it.each` blocks beginning `:3570` and `:3685` each have two rows; the other 27 blocks are single cases. Destination therefore owns 31/151 cases, parent retains 120, and parent + viewport + scrollback remains 120 + 22 + 31 = 173.
- **Selected behavior families:** helper contracts; privacy-safe scroll diagnostics; deferred/changed-row/programmatic measurement handling; stale frame/follow-up cancellation; explicit versus programmatic top demand; projection/terminal/repair fences; transport rejection and accepted empty pages; underfill and auto-load reevaluation; initial/repair/replay acknowledgement timing and retries; large-window transient underfill suppression; and the pagination-owned `auto-backfills after an in-session room anchor settles...` case deliberately retained by the viewport split.
- **Deliberate exclusions:** keep focused-target centering and thread gap/backfill/projection compensation with threads/focused-context; receipt/avatar/unread-jump tests with live-signals/read-state; media size, off-window image/avatar, and link-preview windowing with media/rendering; and gap-placement/continuity chrome with rendering/message-state. These may call virtualization or backfill machinery, but that is setup rather than their asserted ownership boundary.
- **Create:** private direct test file `apps/desktop/src/components/TimelineView.scrollback.test.tsx` with the jsdom header. Import `act`, `cleanup`, `fireEvent`, `render`, `screen`, `waitFor`; React `useState`; Vitest `afterEach`, `describe`, `expect`, `it`, `vi`; value `threadTimelineKey` and types `CoreEventPayload`, `TimelineDiff`, `TimelineItem` from `../domain/coreEvents`; type `TimelineContinuityState` from `../domain/types`; locale reset; shared `KEY`, `baseTransport`, `message`, `mockTimelineRects`, `navigationSnapshot`; and `TimelineView`, viewport-memory reset, `timelineBackfillThresholdForTests`, and `timelineRowsArePurePrependForTests`. Move `messages` privately; add no export, support change, wrapper factory, barrel, or production API.
- **Suite/cleanup invariant:** retain the same `describe("TimelineView", ...)` and copy the exact six-statement `afterEach` into the independently runnable destination while retaining it in the parent. Preserve fake timers, RAF/cancelRAF mocks, mutable geometry, rejected promises, transport callbacks, try/finally restoration, and test-local request state exactly; no cross-file or execution-order dependency is allowed.
- **Modify parent:** remove only the approved helper/test ranges, `TimelineDiff`, `timelineBackfillThresholdForTests`, and `timelineRowsArePurePrependForTests`. Retain `TimelineItem`, `TimelineContinuityState`, `useState`, all Testing Library/Vitest APIs, viewport-memory/locale cleanup, and every shared-support import because retained groups still use them. Leave one separator before the presentation-order helper comment, between the adjacent retained interaction tests, and between the retained committed-thread-projection and read-receipt tests; keep neighboring comments attached.
- **Behavior/ownership invariant:** change no test name, parameter row, assertion, fixture, diagnostic payload, timer/frame flush order, transport outcome, geometry, projection/repair identity, cleanup, snapshot, or product code. React test files own only their DOM/mock cleanup; Rust/product lifecycle and the existing production test contracts remain unchanged.
- **Expected effect:** 31 cases and 2,239 test-body lines plus the 5-line private generator leave the 10,143-line parent. No duplicated helper, compatibility shim, `.only`, `.skip`, `.todo`, TODO, public export, or new abstraction; only the six-statement file-isolation hook is duplicated as required.
- **Allowed implementation files:** `TimelineView.test.tsx` and new `TimelineView.scrollback.test.tsx` only. Existing viewport/support/production files are read-only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** merge-base `873f90d2` parent target 151/151, frontend typecheck, and lint passed. Post-move require parent + scrollback 151/151, independent scrollback 31/31 and parent 120/120, and all three TimelineView files 173/173, plus typecheck/lint/diff. Full frontend/browser/Rust/QA/Tauri/static gates remain mandatory before merge.

#### TimelineView rendering/message-state test design slice

- **Scope rule:** move only tests whose primary contract is latest-summary identity, listener/store-backed rendering, generic message DOM/status labels, localized structured notices, Rust-owned continuity chrome, or formatted message/reply content. Action dispatch, media loading/viewing, thread/focused projection, receipts/read state, live signals, and room-key recovery remain separate.
- **Move exactly:** from `apps/desktop/src/components/TimelineView.test.tsx` on merge base `e2f1f28e`, move private `latestEventSummary` at `:96-110` (15 lines / parent-measured 362 bytes) and these fifteen complete blocks in source order: `:184-208`, `:1078-1128`, `:1516-1555`, `:1557-1597`, `:1599-1667`, `:2708-2755`, `:2757-2827`, `:2829-2884`, `:4345-4400`, `:4781-4839`, `:4841-4883`, `:6906-6956`, `:6958-7022`, `:7024-7070`, and `:7072-7121` (772 test-body lines / 23,492 bytes). The natural adjacent grouped ranges are `:184-208`, `:1078-1128`, `:1516-1667`, `:2708-2884`, `:4345-4400`, `:4781-4883`, and `:6906-7121` (780 lines / 23,500 bytes including eight internal separators). Preserve helper and every body byte-for-byte and source-ordered.
- **Case count:** `:184-208` is a five-row `it.each`; the other fourteen blocks are single cases. Destination owns 19/120 cases, parent retains 101, and parent + rendering + viewport 22 + scrollback 31 remains 173.
- **Selected families:** ordinary versus relation room-summary display identity; formatted Markdown reply quote; listener-before-subscription/fallback suppression/prepopulated-store rendering; timestamp-flow layout without action dispatch; Rust-projected/fallback reaction sender labels; localized structured notices; hidden-row gap placement and authoritative conversation-start continuity; projected-link/formatted-list/whitespace/explicit-break rendering.
- **Boundary decisions:** include formatted reply quote because it asserts room-generic content with no thread projection/action. Include hover-action layout because it performs no action and asserts generic row flow. Exclude failed-gap Retry, source-dialog controls, URL/Matrix navigation, and link-preview cards because dispatch is a primary outcome; exclude media pipeline/windowing, thread projection/compensation, receipt/unread/live-signal, encryption, and room-key cases even when they assert rendered text.
- **Create:** private direct `apps/desktop/src/components/TimelineView.rendering.test.tsx` with jsdom header. Import Testing Library `act`, `cleanup`, `render`, `screen`, `waitFor`, `within`; Vitest `afterEach`, `describe`, `expect`, `it`, `vi`; types `CoreEventPayload`, `TimelineItem`; timeline-store `applyTimelineEvent`, `createTimelineStore`, `TimelineStoreState`; `RoomLatestEventSummary`; locale reset; `TimelineView`, viewport-memory reset, `roomLatestDisplayEventId`; `TimelineStoreContext`; and shared `KEY`, `baseTransport`, `message`. Move `latestEventSummary` privately. Add no React import, global module mock, export, wrapper, barrel, or support/production change.
- **Suite/cleanup invariant:** retain `describe("TimelineView", ...)`; copy the exact six-statement `afterEach` while retaining it in the parent. Preserve fake-timer transitions, listener registration/event order, store provider values, locale mutation/reset, DOM queries, and all test-local transport state exactly; no file-order dependency.
- **Modify parent:** remove only approved helper/test ranges, `roomLatestDisplayEventId`, and `RoomLatestEventSummary`. Retain every other Testing Library/React/Vitest/domain/shared-support/store/TimelineView/live-signal/room-key/IME/external-link import and the external-link module mock because excluded groups still use them. Normalize only exposed separators; retain `expectLocalizedTooltip`, `changeInlineEditorText`, and the documented `mockPresentationOrderRects` with their future owner groups and keep all neighboring comments attached.
- **Behavior/ownership invariant:** change no test name, table row, assertion, fixture, message/store payload, subscription order, timer state, locale, continuity position, formatted document, DOM structure, cleanup, snapshot, or product code. React owns per-file DOM/mock cleanup only; Rust-owned projections and product lifecycle remain unchanged.
- **Expected effect:** 19 cases and 772 test-body lines plus a 15-line private helper leave the 7,866-line parent. Only the six-statement isolation hook is duplicated; no duplicate helper, compatibility shim, `.only`, `.skip`, `.todo`, TODO, public API, or abstraction.
- **Allowed implementation files:** `TimelineView.test.tsx` and new `TimelineView.rendering.test.tsx` only. Existing viewport/scrollback/support/production files are read-only. Plan/completion records may be updated by the parent after verification.
- **Baseline/post:** merge-base `e2f1f28e` parent + viewport + scrollback 173/173, frontend typecheck, and lint passed. Post require rendering 19/19, parent 101/101, parent + rendering 120/120, parent + viewport + scrollback 154/154, and all four 173/173, plus typecheck/lint/diff. Full frontend/browser/Rust/QA/Tauri/static gates remain mandatory.

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
- TimelineView rendering/message-state formal design review: `reviewer-flash` verified all 15 ranges, helper/byte arithmetic, 19/101/173 counts, formatted-reply and hover-layout ownership decisions, destination imports, parent retentions, six-statement cleanup, exclusions, separators, and two-file/no-production boundary, found no findings, and returned `Correct-to-merge`.
- TimelineView rendering/message-state pre-edit baseline on merge base `e2f1f28e`: parent + viewport + scrollback 173/173, frontend typecheck, and frontend lint passed.
- TimelineView scrollback/virtualization delivery PR: #573 merged as `e2f1f28e72fdeae568e6cbf9f1a9b2606393b25a`; required CI passed 7/7 on the first run.
- TimelineView scrollback/virtualization implementation: Luna low performed the approved immutable-base move; the parent normalized the three approved separator sites only. The independently generated expected parent matched exactly (`sha256:50117a708d2b82c3d68964e1e09ca0983c2ac44e7334e5e51eda157adcaa275c`), and all 29 test bodies plus `messages` matched base bytes and source order.
- TimelineView scrollback/virtualization formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-timeline-scrollback.diff` (`sha256:cef1aec2be3eccf25f83c592e0b529889657cb816c411fb6e51b4d5e19a76494`), verified exact bodies/imports/cleanup, 31 + 120 = 151 and all-three 173 arithmetic, pagination-owned hybrid inclusion, retained neighbors, and the two-file/no-production boundary, and returned `Correct-to-merge` with no findings.
- TimelineView scrollback/virtualization complete gates: all three TimelineView files 173/173, parent + scrollback 151/151, independent scrollback 31/31 and parent 120/120; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace passed fresh full rerun (2,392 passed / 13 ignored) after one unrelated `runtime_room_list_sync` deadline failure passed both focused and full reruns; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, IME, rustfmt, diff, and cargo-deny passed.
- TimelineView scrollback/virtualization formal design review round 2: after the import-contract fixes, the same `reviewer-flash` rechecked every moved use and retained parent dependency, all ranges/counts/separators, helper ownership, cleanup isolation, exclusions, and the two-file boundary, found no new findings, and returned `Correct-to-merge`.
- TimelineView scrollback/virtualization formal design review round 1: `reviewer-flash` verified all ranges, counts, ownership exclusions, helper/import removals, cleanup, separators, and baseline, but returned `Not correct-to-merge` because the destination import contract omitted moved `threadTimelineKey` calls and did not pin `TimelineContinuityState` to `../domain/types`. Implementation did not begin; both findings were corrected for round 2. The parent-measured `messages` count remains exactly 5 lines / 182 bytes by UTF-8 script and is subject to post-move hash proof.
- TimelineView scrollback/virtualization pre-edit baseline on merge base `873f90d2`: parent 151/151, frontend typecheck, and frontend lint passed.
- TimelineView viewport/anchoring delivery PR: #572 merged as `873f90d28920848bf2539fe22ac7875bdcd6f83e`; final required CI 7/7 passed after dependency-install runner hangs were cancelled and only failed/cancelled jobs reran fresh.
- TimelineView viewport/anchoring implementation: the first Luna low mechanical attempt applied moving line ranges after earlier deletions shifted them; parent inspection caught the corrupt unstaged output before review, and both allowed files were fully restored from `HEAD`. The continuation used immutable `fc6e9bb7` source plus one-pass deletion and produced the approved two-file tree. Parent SHA-256 exactly matched the independently generated expected deletion result; all 20 test bodies and the helper matched base bytes and source order.
- TimelineView viewport/anchoring formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-timeline-viewport.diff` (`sha256:77485ac2295528dc8b9f22b90c0404858cd68d8a70d8b1c9ce1a8e06e771a871`), confirmed the exact 20-block/helper move, 22 + 151 = 173 counts, imports, six-statement cleanup isolation, excluded `auto-backfills...`, and two-file/no-production boundary, and returned `Correct-to-merge` with no blocking findings. Its two Minors were resolved by the parent's exact base-byte proof and correcting the remaining five/six-statement wording.
- TimelineView viewport/anchoring complete gates: combined 173/173, destination 22/22, parent 151/151; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, IME, rustfmt, diff, and cargo-deny passed.
- TimelineView viewport/anchoring formal design review: `reviewer-flash` checked every helper/test range, 20-block/22-case classification, destination imports, retained parent dependencies, six-statement cleanup isolation, explicit scrollback/focused/read/media exclusions, two-file allowlist, and 22 + 151 = 173 arithmetic, then returned `Correct-to-merge`. Before implementation, its two documentation Minors were fixed: the complete helper range is `:192-233` (42 lines / 997 bytes), and the copied cleanup has six statements.
- TimelineView viewport/anchoring pre-edit baseline on merge base `fc6e9bb7`: complete original target 173/173, frontend typecheck, and frontend lint passed.
- TimelineView test-support delivery PR: #571; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- TimelineView test-support implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the four approved ranges into one private test leaf. Parent normalized hashes proved all 1/80, 25/590, 39/882, and 87/2,763 line/byte ranges exact after removing only required `export` keywords.
- TimelineView test-support formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-timeline-test-support.diff` (`sha256:7224c2b7484635a4a45396d375b608ba2f0c84f845f22036be8c5289d201dc6b`) and returned `Correct-to-merge` with no blocking findings.
- TimelineView test-support complete local gates: original target 173/173; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- TimelineView test-support formal design review: `reviewer-flash` verified the four exact ranges, seven-symbol dependency/export/import boundary, call-site spread across planned feature groups, feature-local retained helpers, sole parent type removal, comments/separators, two-file allowlist, and absence of tests/hooks/product lifecycle changes, then returned `Correct-to-merge`. Its separator Minor was incorporated before implementation; parent-measured byte counts remain subject to the already-required normalized post-move hash proof.
- TimelineView test-support pre-edit baseline on merge base `8ca09f94`: complete original `TimelineView.test.tsx` 173/173, frontend typecheck, and frontend lint all passed.
- Appearance delivery PR: #568; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Appearance implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) completed the approved two-file extraction before its tool-call budget ended. The parent changed only Fragment-wrapper indentation and one excess separator; source hashes proved the 84-line/3,284-byte controls body and 111-line/2,092-byte helper range byte-identical.
- Appearance formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-appearance.diff` (`sha256:1eb8a46b6b648018f90fe8f3d906c822457f70701c55979f4df991dbe642a391`), including the ungated theme/density paths, and returned `Correct-to-merge` with no blocking findings.
- Appearance complete local gates: focused unit 25/25 and Playwright 1/1; frontend 1,366 Vitest and browser-headless 76 + 248 (including `theme.spec.ts`); Rust workspace 2,393 passed / 13 ignored after an unrelated composer-store timing assertion passed focused and complete reruns; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Appearance formal design review: round 1 timed out and returned `Not correct-to-merge` because evidence gathering was incomplete; implementation did not begin. In round 2, the same `reviewer-flash` completed every missing check, verified both exact ranges, complete imports/removals/retentions, sole export, six-prop contract, parent composition boundary, separator, two-file allowlist, patch/guard/ARIA/order/pairing semantics, App-local density versus Rust settings ownership, and honest coverage, then returned `Correct-to-merge`. Its only actionable documentation Minor clarified that the destination receives the parent's resolved density value.
- Appearance pre-edit baseline on merge base `887e5b25`: `UserSettingsPanel.test.tsx` 25/25, focused typography-settings Playwright scenario 1/1, frontend typecheck, and frontend lint all passed.
- Trust delivery PR: #567; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Trust implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved the approved two ranges, then stopped when its import trim over-removed parent-owned `DetailRow`. The parent restored only that required direct import; typecheck proved the correction, and normalized source hashes proved both moved ranges byte-identical: 325 lines/10,345 bytes and 207 lines/5,767 bytes.
- Trust formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-trust.diff` (`sha256:d9706dfea4f854adcd57cad086483c09093ca635e49c1e23b5c8492551e6467f`) including the restored `DetailRow`, all auth branches, disabled logic, flow/SAS actions, and identifier privacy, then returned `Correct-to-merge` with no findings.
- Trust complete local gates: focused unit 25/25 and Playwright 1/1; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Trust formal design review: `reviewer-flash` verified both exact ranges, all 4 components + 12 helpers, every destination import and parent removal/retention, sole direct export, unchanged 10-prop call, separators, two-file allowlist, Rust ownership, verification flow/SAS behavior, identity-reset DOM-ref lifecycle and OAuth/unknown/UIAA branches, identifier privacy, and focused coverage, then returned `Correct-to-merge`. Its new documentation Minors (pin `../TrustHelp`, explicitly retain `ImeTextField`, and require byte review of three ungated auth/disabled branches) were incorporated before implementation.
- Trust pre-edit baseline on merge base `1b36ce3b`: `UserSettingsPanel.test.tsx` 25/25, focused E2EE trust-controls Playwright scenario 1/1, frontend typecheck, and frontend lint all passed.
- Security delivery PR: #566; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Security implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the two approved ranges into one private direct module. Parent verification proved the 336-line/12,194-byte component/type/status range and 147-line/4,227-byte status/label range exact after the required export; the parent removed one extra generated separator before review.
- Security formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-security.diff` (`sha256:24cceaf9ac32fba49d70f1844168e5d696615e3cbc18d38a53e5de95f2e00cc5`) and returned `Correct-to-merge` with no findings.
- Security complete local gates: focused unit 25/25 and Playwright 3/3; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Security formal design review: `reviewer-flash` verified both exact ranges, every symbol/import/removal, the 13-prop call, private/shared helper boundary, Rust ownership, file-path privacy, each DOM-ref secret/chooser/clearing sequence, two-file allowlist, and focused coverage, then returned `Correct-to-merge`. Its two new documentation Minors (mark byte counts parent-measured/re-proved post-move and call the second range status/label helpers) were incorporated before implementation.
- Security pre-edit baseline on merge base `7cd8c255`: `UserSettingsPanel.test.tsx` 25/25, focused Security/Trust/room-key Playwright scenarios 3/3, frontend typecheck, and frontend lint all passed.
- Shared failure-label delivery PR: #565; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Shared failure-label implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the approved mapper into the existing private status module. Parent verification proved all 18 lines / 535 bytes exact after the required export.
- Shared failure-label formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-failure-label.diff` (`sha256:7056911a1316eabb96450d0d817ee64f40190fa9d358a2633d903a05f188b11a`) and returned `Correct-to-merge` with no findings.
- Shared failure-label complete local gates: focused unit 25/25 and Playwright 3/3; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Shared failure-label formal design review: `reviewer-flash` verified the exact 18-line range, 4 Security + 4 Trust call sites, sole parent type use, five-export leaf-module boundary, i18n/exhaustive switch, separator cleanup, and two-file allowlist, then returned `Correct-to-merge`. Its three new documentation Minors (name verification as the fourth Trust consumer, pin both relative import paths, and identify the 535-byte count as parent-measured/re-proved post-move) were incorporated before implementation.
- Shared failure-label pre-edit baseline on merge base `9e8d5d40`: `UserSettingsPanel.test.tsx` 25/25, focused Security/Trust/room-key Playwright scenarios 3/3, frontend typecheck, and frontend lint all passed.
- Shared status-primitives delivery PR: #564; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Shared status-primitives implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the three approved ranges into one private direct module. Parent verification proved the 52-line/976-byte rows block, 1-line/73-byte tone, and 8-line/200-byte detail row exact after required exports.
- Shared status-primitives formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-status-primitives.diff` (`sha256:127463fa510043cf353f9efbafe142c2079e73e897a9b0c763472d8e495964bd`) and returned `Correct-to-merge` with no findings.
- Shared status-primitives complete local gates: focused unit 25/25 and Playwright 3/3; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Shared status-primitives formal design review: `reviewer-flash` verified exact 52 + 1 + 8 line ranges, all three separator sites, actual cross-use, `ReactNode`-only destination dependency, type-only parent import, two-file allowlist, and stateless ownership, then returned `Correct-to-merge`. Its two new cosmetic Minors (align checklist order and state inline `type TrustTone`) were incorporated before implementation.
- Shared status-primitives pre-edit baseline on merge base `c0121719`: `UserSettingsPanel.test.tsx` 25/25, focused Security/Trust/room-key Playwright scenarios 3/3, frontend typecheck, and frontend lint all passed.
- Account Management delivery PR: #563; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Account Management implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the approved contiguous block into one private direct module. Parent verification proved all 238 lines / 8,450 bytes exact after accounting only for the required `export` keyword.
- Account Management formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-account-management.diff` (`sha256:2518d78636f77c2503b10d89cb8678e619bf38ac83b0f745984108c115b50fd2`) and returned `Correct-to-merge` with no code findings.
- Account Management complete local gates: focused unit 25/25 and `session-status.spec.ts` 3/3; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Account Management formal design review: `reviewer-flash` verified the exact 238-line range, complete import/call-site accounting, two-file allowlist, Rust/capability-effect ownership, DOM-ref secret lifecycle, and honest unit/browser coverage, then returned `Correct-to-merge`. Its only new Minor clarified that `session-status.spec.ts` covers the shared delegated-URL/opener route and panel render rather than clicking the panel button; the wording was corrected before implementation.
- Account Management pre-edit baseline on merge base `359d3246`: `UserSettingsPanel.test.tsx` 25/25, `session-status.spec.ts` 3/3, frontend typecheck, and frontend lint all passed.
- Sessions delivery PR: #562; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the first implementation CI run passed all seven required checks before final ledger update.
- Sessions implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved only the two approved blocks into one private direct module. Parent verification proved the 4,197-byte `SessionsSection` and 2,637-byte `SessionRow` bodies exact after accounting only for the required `export` keyword and inter-symbol separator.
- Sessions formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-sessions.diff` (`sha256:c170d2e5a508c83eddbdfc0a196cbb38d624b82cf0755adb91ad7c7c309e2d48`) and returned `Correct-to-merge` with no code findings.
- Sessions complete local gates: focused unit 25/25 and Playwright 1/1; frontend 1,366 Vitest and browser-headless 76 + 248; Rust workspace 2,393 passed / 13 ignored; QA binary 129; Tauri 154 passed / 1 ignored; npm audit/typecheck/lint/build, SDK, agents-doc, rustfmt, diff, and cargo-deny passed.
- Sessions formal design review: `reviewer-flash` verified both exact ranges, all imports/call sites, the two-file allowlist, coverage, and behavior/privacy/lifecycle/Rust-state ownership, then returned `Correct-to-merge`. Its only new Minor (collapse the blank separators left by the non-contiguous `SessionRow` removal) was incorporated before implementation.
- Sessions pre-edit baseline on merge base `db234d4c`: `UserSettingsPanel.test.tsx` 25/25, focused device-session-manager Playwright 1/1, frontend typecheck, and frontend lint all passed.
- Shared UIA prerequisite implementation: `luna-implementer` (GPT-5.6 Luna, low, write-capable) moved the exact form and preserved its secret lifecycle.
- Shared UIA prerequisite formal full-diff review: `reviewer-flash` reviewed `/tmp/issue551-shared-uia.diff` (`sha256:0c9bf9d3cb791149acdb9765aab010e85359b9b4264f3346412a8ffc6909bde7`) and returned `Correct-to-merge` with no findings.
- Shared UIA prerequisite formal design review: `reviewer-flash` returned `Correct-to-merge`; its only new Minors (collapse the post-removal double blank and leave the similar Trust form untouched) were incorporated before implementation.
- Shared UIA prerequisite pre-edit baseline: `UserSettingsPanel.test.tsx` 25/25 passed; focused device-session-manager Playwright 1/1 passed; frontend typecheck and lint passed.
- Shared UIA delivery PR: #561; focused and complete local gates passed after one unrelated runtime deadline retry, the first Rust CI attempt hit an unrelated existing diagnostic-counter timing failure, and the failed job rerun passed. Final required checks were 7/7 green before ledger update.
- Pilot delivery PR: #560; focused and complete local gates passed, formal design/diff verdicts are `Correct-to-merge`, and the implementation CI run passed all seven required checks before final ledger update.
- Merged PRs: #560 (Search History pilot), #561 (shared UIA prerequisite), #562 (Sessions), #563 (Account Management), #564 (shared status primitives), #565 (shared failure label), #566 (Security), #567 (Trust), and #568 (Appearance).
