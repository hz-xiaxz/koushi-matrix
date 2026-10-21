# Open-issue inventory and small fixes — 2026-10-21

## Scope and evidence

The maintainer requested an inventory of every open issue, consolidation of stale
or overlapping tickets, and one PR containing the inventory and small fixes.
Search returned 41 candidates; individual issue reads confirmed 39 were open
and #1028/#1344 were already closed (GitHub search indexing lag). No open PRs;
base `7b826744` (PR #1575 merged). All 41 bodies and all available comment
threads were fetched through GitHub.
Closed dependencies #694, #1150, #1333 and #1334 were also checked. Historical
evidence is labelled as such; this inventory does not certify every feature or
rerun old homeserver jobs. Age alone is never a reason to discard unfinished work.

Canon consulted: REPOSITORY_RULES.md (root-cause fixes, review, issue closure),
architecture overview (viewport pagination), state-machine (thread eligibility),
i18n, and agent verification/state-ownership (snapshot mirrors and gates).

## Disposition of every search candidate

Links use the issue number; dates below are creation dates, not last activity.
"Consolidate" means preserve outstanding requirements/evidence in the owner and
close the redundant tracker, without claiming its underlying defect is fixed.

| Issue | Created | Disposition and remaining work |
| --- | --- | --- |
| [#1574](https://github.com/shinaoka/koushi-matrix/issues/1574) | Oct 21 | Fix here: preserve image controls and encryption badge on extremely short images. |
| [#1573](https://github.com/shinaoka/koushi-matrix/issues/1573) | Oct 21 | Fix here: remove the automatic-history preference end to end, including persisted opt-outs, catalogs and fixtures. |
| [#1517](https://github.com/shinaoka/koushi-matrix/issues/1517) | Oct 17 | Consolidate into #1515; retain Synapse runs and its own acceptance result. |
| [#1516](https://github.com/shinaoka/koushi-matrix/issues/1516) | Oct 17 | Consolidate into #1514; retain Synapse history-window failures separately. |
| [#1515](https://github.com/shinaoka/koushi-matrix/issues/1515) | Oct 17 | Keep as notification-delivery QA owner for both servers. Latest reports fail at install-sink, whereas the original report failed at request: no root-cause identity is assumed. |
| [#1514](https://github.com/shinaoka/koushi-matrix/issues/1514) | Oct 17 | Keep as history-window QA owner for both servers. Do not equate this failure with #1513 without a reproducer. |
| [#1513](https://github.com/shinaoka/koushi-matrix/issues/1513) | Oct 17 | Keep: source confirms responder-rate entries use the 30-day lifetime. Needs deterministic boundary/cap tests and independent crypto-path review. |
| [#1511](https://github.com/shinaoka/koushi-matrix/issues/1511) | Oct 17 | Keep: invited mention candidates change an intentional membership contract; separate feature work. |
| [#1494](https://github.com/shinaoka/koushi-matrix/issues/1494) | Oct 16 | Keep: external MSC4153 rollout and recipient policy decision; do not opportunistically change encryption policy. |
| [#1484](https://github.com/shinaoka/koushi-matrix/issues/1484) | Oct 16 | Keep: gate-negative unknown failure needs the actual failing assertion/log, not a timeout adjustment. |
| [#1482](https://github.com/shinaoka/koushi-matrix/issues/1482) | Oct 16 | Keep as owner of remaining actor HTTP waits, publication attribution, ordering/teardown and integration evidence from #1481. |
| [#1481](https://github.com/shinaoka/koushi-matrix/issues/1481) | Oct 16 | Consolidate into #1482: typing separation already landed (`f1072480`); remaining audit, fairness and publication proof must stay explicit. |
| [#1358](https://github.com/shinaoka/koushi-matrix/issues/1358) | Oct 11 | Consolidate into #1357, retaining the independent Synapse aggregate lane. |
| [#1357](https://github.com/shinaoka/koushi-matrix/issues/1357) | Oct 11 | Keep as aggregate native-stage QA owner for both servers. A shared stage token alone does not establish a shared defect. |
| [#1344](https://github.com/shinaoka/koushi-matrix/issues/1344) | Oct 11 | Already closed at snapshot verification; leave closed. Historical observability shipped in #1349 and later agreed optimization directions remain in its comments. Coordinate new work with #859 and do not reopen #1150 wholesale. |
| [#1343](https://github.com/shinaoka/koushi-matrix/issues/1343) | Oct 11 | Keep: Rust-projected room-wide unread-thread total and accessible header badge are a distinct feature. |
| [#1332](https://github.com/shinaoka/koushi-matrix/issues/1332) | Oct 11 | Keep: Authenticode/provider/Windows validation remain. Correct stale dependency numbers and distinguish Tauri signatures. |
| [#1335](https://github.com/shinaoka/koushi-matrix/issues/1335) | Oct 11 | Keep: real protected signing-key provisioning/continuity is an external release gate; checked-in pipeline code is insufficient evidence. |
| [#1331](https://github.com/shinaoka/koushi-matrix/issues/1331) | Oct 11 | Completed implementation: #1336 supplies Windows notification checks and #1338 extends signed updates. Verify existing lifecycle rejection tests; do not leave this blocked on Authenticode. |
| [#1330](https://github.com/shinaoka/koushi-matrix/issues/1330) | Oct 11 | Keep as Windows release umbrella; absorb #1028 provider/credential/validation decisions, link actual child numbers and distinguish shipped pipeline from unprovisioned release credentials. |
| [#1323](https://github.com/shinaoka/koushi-matrix/issues/1323) | Oct 10 | Consolidate into #1279: same user-verification scenario/server and incoming-request symptom, preserving recurrence links. |
| [#1289](https://github.com/shinaoka/koushi-matrix/issues/1289) | Oct 10 | Keep: timeline-stress timeout. Latest comment records recurrence; neither #859 nor #1344 proves this resolved. |
| [#1279](https://github.com/shinaoka/koushi-matrix/issues/1279) | Oct 10 | Keep as incoming verification owner: diagnostics distinguish idle receiver from target mismatch. Four local passes do not establish a flake fix. |
| [#1278](https://github.com/shinaoka/koushi-matrix/issues/1278) | Oct 10 | Keep, low-frequency: history-visibility setting returned Forbidden once, then passed. Readiness-race explanation remains a hypothesis. |
| [#1269](https://github.com/shinaoka/koushi-matrix/issues/1269) | Oct 10 | Keep: explicit Main/Unthreaded receipt policy decision. Related to thread UI but not duplicated by #1343. |
| [#1264](https://github.com/shinaoka/koushi-matrix/issues/1264) | Oct 10 | Keep: three cold-attention lower-bound residuals and memory tradeoff. #1573 concerns selected viewport loading and does not settle these counters. |
| [#1048](https://github.com/shinaoka/koushi-matrix/issues/1048) | Sep 28 | Keep/narrow on next audit: #1151 already boxed CoreEvent; argument grouping and other enum expectations require a current census. Do not repeat obsolete 6.9 KB claims. |
| [#1047](https://github.com/shinaoka/koushi-matrix/issues/1047) | Sep 28 | Keep: current production build measures main 2,054.19 kB and emojiData 700.09 kB (gzip 548.24 / 119.01 kB); update the baseline before selecting splits. No warning-limit increase. |
| [#1028](https://github.com/shinaoka/koushi-matrix/issues/1028) | Sep 27 | Already closed at snapshot verification. Add a redirect into #1330 preserving Authenticode provider options, timestamp checks, updater continuity and approved validation environment. |
| [#888](https://github.com/shinaoka/koushi-matrix/issues/888) | Sep 12 | Close completed: stages 1–4 merged, automated evidence recorded and Sep 14 user validation complete. Deferred cosmetic placeholder follow-up is not a reason to retain this completed migration umbrella. |
| [#859](https://github.com/shinaoka/koushi-matrix/issues/859) | Sep 9 | Keep: library selection/qualification is still a proposal, not approval or proof. Preserve native wheel/trackpad qualification and single-owner requirement. |
| [#446](https://github.com/shinaoka/koushi-matrix/issues/446) | Aug 6 | Keep: #448 fixed corruption only; generation/sequence correlation, bounded Reset recovery, positive Space-removal evidence and upstream contract remain. |
| [#441](https://github.com/shinaoka/koushi-matrix/issues/441) | Aug 6 | Keep/narrow: #486/#487 delivered build/artifact handling, but build-windows.yml has no mandatory installed-package launch/close/uninstall smoke. That automatable acceptance gap prevents closure; preserve it separately from signing. |
| [#288](https://github.com/shinaoka/koushi-matrix/issues/288) | Jul 21 | Keep/narrow: core-only scenario skip already exists; typed manifests, full waiter inventory, measured stage budgets and deterministic failure injection remain. Recurring QA reports are symptoms, not duplicates of this tooling work. |
| [#119](https://github.com/shinaoka/koushi-matrix/issues/119) | Jun 23 | Keep pending complete acceptance audit. The last recorded blocker (restored-account re-sign-in prefill) was addressed by #1178 and has a browser regression. That scoped fix alone does not certify all concurrent-account ownership/migration requirements. |
| [#60](https://github.com/shinaoka/koushi-matrix/issues/60) | Jun 15 | Keep as low-priority feature backlog: saved messages/bookmarks, including storage policy decision. |
| [#52](https://github.com/shinaoka/koushi-matrix/issues/52) | Jun 15 | Keep as feature backlog: receiving image media does not implement sticker-pack CRUD, sending and custom emoji. |
| [#51](https://github.com/shinaoka/koushi-matrix/issues/51) | Jun 15 | Keep as feature backlog: static and live location are separate phases of one feature, not current UI bugs. |
| [#50](https://github.com/shinaoka/koushi-matrix/issues/50) | Jun 15 | Keep as feature backlog: poll creation/voting/ending and authoritative aggregation remain. |
| [#49](https://github.com/shinaoka/koushi-matrix/issues/49) | Jun 15 | Narrow: preferences, appearance, shortcuts, Help and diagnostics now exist. Retain explicit remaining palette/breadcrumb/labs/app-lock decisions instead of claiming the whole original shell is absent. |
| [#47](https://github.com/shinaoka/koushi-matrix/issues/47) | Jun 15 | Narrow: #694 delivered active-session Manage account & devices. Remaining question is whether to support native password/deactivation/3PID/identity-server flows beyond server delegation; do not reintroduce a second remote device manager. |

## Consolidation mechanics

Preserve each original issue and comment permalink. For the QA pairs, copy BOTH
server signature labels to the retained owner before retiring the duplicate.
The nightly reporter searches the signature label, not the title, so future
reports will reach the owner without recreating a closed ticket. Keep independent
per-server acceptance checkboxes; consolidation is not proof of a shared cause.
For #1323, move its signature to #1279 using the same mechanism.

No crypto policy, actor concurrency, virtual-scroll engine, release signing
provider, memory budget or receipt semantics change is authorized by this small
fix batch. Those tickets retain their concrete remaining work and decisions.

## Small fixes and verification

#1573 removes the preference from Rust serialization, TypeScript, UI catalogs,
room/thread/focused callers and harnesses. Legacy JSON keys deserialize as
unknown fields and disappear on serialization. Positive measured viewport size
still gates automatic demand; draft-thread and request/anchor/retry guards remain.
The underfilled-without-scroll component regression failed before the fix
(zero pagination calls), and legacy false is covered by settings round-trip.

#1574 reserves enough frame height for the existing overlay controls and badge,
keeping the image contained without distortion. The browser regression uses a
4000×50 synthetic image and measures each control against its clipping frame;
before the fix a control extended to y=191 beyond frame bottom y=172.75.

## Integrated evidence and actual dispositions

- Rechecked 39 initially open issues: consolidated five duplicates (#1517 →
  #1515, #1516 → #1514, #1358 → #1357, #1323 → #1279, #1481 → #1482),
  closed completed #888/#1331, and refreshed nine stale scope/baseline notes
  (#47, #49, #119, #288, #441, #1047, #1048, #1332, #1335).
- #1028 and #1344 were already closed before the individual reads; #1028 now
  redirects to #1330 and retains its original completed closure reason.
- Current verified open count: 32. #1573/#1574 remain open until this PR lands.
- Signature-label searches returned exactly the intended open QA owner for all
  four migrated signatures; future bot reports retain server-specific evidence.
- Frontend: all 181 Vitest files / 2,249 tests pass; focused regression matrix
  passes; typecheck, ESLint/ownership guards and production build pass.
- Browser: all 583 Chromium tests pass (5.8 minutes), including the 42-test
  media/scrollback/thread-placement matrix, settings opt-out absence and
  restored-account prefill regressions. All 15 media tests also pass on WebKit
  (9.6 seconds); this spec is now in the permanent WebKit allowlist.
- Rust: settings_state 30 passed; desktop library 278 passed / 1 ignored.
  The snapshot golden was regenerated using UPDATE_GOLDEN and then verified
  without it as part of the desktop library suite.
- State-crate all-targets clippy passes with warnings denied.
- rustfmt, SDK/submodule guard, command/snapshot guard, agent-doc and user-help
  checks, secret scan and diff whitespace checks pass.
- Current bundle warning is retained and measured under #1047; no threshold
  was raised. Main/emoji data remain over 500 kB.
- Additional inventory probe `node --test scripts/build-structure-contract.test.mjs
  scripts/desktop-updater-manifest.test.mjs`: 7 pass / 2 fail on untouched baseline
  contracts. The stale assertions expect dev line-table debug info while Cargo
  sets debug=0, and direct --scenario=invites_dm while CI dispatches --lane=pr.
  These source-test/canon inconsistencies are recorded under #288; this PR does
  not change the build profile, CI lanes or weaken their assertions.
- No new homeserver scenario or native Windows/macOS GUI run was performed.
  This change does not certify the remaining QA, release, memory or concurrency
  issues complete.

Self-review traced persisted settings → Rust serialization → DTO golden → TS
settings/callers → shared room/thread viewport evaluation. New-thread semantic,
resync, anchor, in-flight and bounded retry guards remain. The short-image fix
reserves frame space and preserves object-fit containment and keyboard actions.
No property gains a separate value/edit/save location. Historical specs remain
historical; the current architecture, thread contract and settings guide were
updated. No additional independent security/concurrency review is required for
this preference removal and CSS geometry change.
