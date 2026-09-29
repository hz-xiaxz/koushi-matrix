# Agent Rule Docs Consolidation — Design

Date: 2026-09-30. Status: approved design; spec pending user review.
Inventory (line-level evidence, owner table, mechanical dependencies):
[2026-09-30-rule-docs-consolidation-inventory.md](2026-09-30-rule-docs-consolidation-inventory.md),
taken at `origin/main` `de06d0ff`. Section numbers `§N`, `D#`, and `C#` below refer to it.

## Goal

Reduce what an agent must read for a typical task, without changing what any
rule requires. Success is measured as:

- Always-read material (AGENTS.md + REPOSITORY_RULES.md) drops from ~840 to
  ~370 lines.
- In-scope total drops from 3,797 to ~2,100 lines.
- Every rule that exists today still exists exactly once, in its designated
  owner, with unchanged meaning (except the contradiction resolutions below).
- All mechanical checks listed under "Invariants" pass.

## Scope

In scope (edited): `AGENTS.md`, `REPOSITORY_RULES.md` (RR),
`docs/policies/engineering-rules.md` (ER), `docs/agents/{verification,
environment,qa-lanes,troubleshooting,history,plans}.md`, `docs/README.md`
(descriptions only), and the code comments / config strings that cite
moved or renamed rule text (inventory §0.3–§0.5).

Append-only: `docs/agents/state-ownership.md` (SO) receives exactly three
rules that have no other owner (see "SO additions"). No other SO edits.

Out of scope: `docs/architecture/*` except updating the one cite at
`state-machine.md:96` if ER Documentation is removed. Dated plans/specs
under `docs/superpowers/` and `docs/qa/` are historical and are not edited.

## Target structure

File names and the AGENTS.md topic set are unchanged. No new topic files.

| File | Role after consolidation | Target lines |
| --- | --- | --- |
| AGENTS.md | Router. States explicitly: read RR every task; read other docs only via the task table. | ~66 |
| RR | Always-read core. One bullet per durable prohibition or mandatory procedure, 1–2 sentences, no mechanisms, examples, or history. | ~300 |
| ER | Reference. Topic-sectioned detailed policy that is not a feature spec. Read by task. | ~500 |
| verification | How to prove a change (procedure + commands). | ~200 |
| environment | Machine setup, CI maintenance notes, worktree/target cleanup commands. | ~270 |
| qa-lanes | Every lane, command, token; single operational owner of real-account safety; QA-harness ownership rules moved from ER Async 16. | ~330 |
| troubleshooting | Symptom → fix only. | ~180 |
| history | Quarantine + compressed post-mortems + rationale moved out of RR/ER/verification. | ~100 |
| plans | Compact index: one row per umbrella/plan, superseded plans marked. | ~110 |

Ownership per topic follows inventory §7 "Proposed ownership".

## Transformation rules

1. **Single owner.** Each duplicate in inventory §2 (D1–D56) keeps its text
   only in the proposed owner. Other locations are deleted or reduced to a
   one-line link.
2. **Spec out of rules.** Blocks in inventory §5 whose content is already
   stated in `overview.md`/SO are reduced to the short rule text proposed
   in §5/§6 plus a link to the owner. Nothing is deleted from a rule doc
   unless the owner already states it, or it moves to SO (next section).
3. **Stale removal.** Items in inventory §4 are dropped or moved to history
   as marked there. Rules embedded in a narrative keep the rule sentence.
4. **Concision.** Rewrite verbose rules per §6; keep RFC-style strength
   words (MUST/never/only) as they are. Rewording must not widen or narrow
   what is required.
5. **English only** in the docs (fix the Japanese fragments in history:96
   and verification:11-13).

## SO additions (append-only)

Move from ER/troubleshooting into the matching SO section:

1. Settings `schema_version` migration rule (a default change that saved
   files must not inherit requires a schema bump and a load-time migration)
   → SO "Settings, composer, and scheduled send".
2. Trust-recheck coalescing contract (troubleshooting:209-226)
   → SO "E2EE trust". Troubleshooting keeps the focused test commands.
3. Tooltip / px-token GUI presentation rules from ER GUI 0, if not already
   present → SO "GUI presentation contracts".

## Contradiction resolutions

| # | Resolution |
| --- | --- |
| C1 | ER header describes AGENTS.md as a router (matches AGENTS/RR). |
| C2 | Dev process name is `koushi-desktop`; fix troubleshooting:148. |
| C3 | Mention candidates come from `AppState.mention_candidates`; fix qa-lanes:199. |
| C4 | Verify actual `try_send` uses in code; state one rule in ER Async 11 that matches them; RR keeps one pointer sentence. |
| C5 | qa-lanes.md owns QA scenario/token contracts; fix RR:44-45. |
| C6 | Drop "strongest available model" wording; approval/review follows the risk-based rule in RR Review And Audit. |
| C7 | Local merge gate is the same feature-unified `--workspace` test run CI uses (with CI's exclusions); ER Build 3 states this. |
| C8 | Verify the Linux secret store the code uses and name it in RR Key Management. |
| C9 | Drop the wdio spike clause; Playwright is the headless DOM gate. |
| C10 | Fix `apps/desktop/playwright.config.ts:22` to cite qa-lanes.md. |
| C11 | Delete the trailing `# Current sync contract (Issue #412)` block in ER. |
| C12 | Drop the "Last amended" bump requirement and the stale stamps. |
| C13 | verification `### Rust lint gate` contains only lint; CI maintenance moves to environment. |

## Invariants (must hold after every PR)

- ER keeps heading `## Build, Dependencies, QA Gates` and its item `1.`
  (SDK submodule policy) with the phrasing pinned by
  `scripts/build-structure-contract.test.mjs`, followed by item `2.`.
- ER Secrets rule numbers stay stable where cited (at least 2 and 11), or
  every citation in inventory §0.3 is updated in the same PR.
- environment.md Linux GUI container block unchanged (pinned by
  `apps/desktop/src/scripts/linuxGuiQa.test.ts`).
- qa-lanes.md keeps `redact_edit_convergence=ok` and
  `thread_summary_convergence=ok`, the GUI scenario table, and the
  `#output-must-be-private-data-free` anchor.
- Every anchor in inventory §0.6 survives, or its referrers are updated in
  the same PR.
- `scripts/check-agents-docs.mjs` passes (router budget, topic links,
  retired flags only in history.md, scenario names exist).
- Fix the two broken `REPOSITORY_RULES L124-128` citations
  (`timeline/actor.rs`, `timeline/media.rs`) to cite the heading.

## Delivery

Three PRs, each a no-semantic-change rewrite except the contradiction
resolutions, merged in order:

1. **RR + ER** (+ SO additions, code-comment cite fixes, `docs/README.md`,
   `state-machine.md:96` cite, `playwright.config.ts:22`).
2. **docs/agents topics**: verification, environment, qa-lanes,
   troubleshooting (+ AGENTS.md router wording).
3. **history + plans**.

Each PR runs: `node scripts/check-agents-docs.mjs`,
`node --test scripts/build-structure-contract.test.mjs`,
`npx vitest run src/scripts/linuxGuiQa.test.ts src/scripts/headlessAndRealQa.test.ts`
(in `apps/desktop`), `node scripts/user-help.mjs --check`, `git diff --check`,
plus `cargo check` if code comments change. Each PR body contains a
moved/deleted-rule ledger: for every removed block, where its rule now
lives (or why it was stale). An independent read-only review compares
old vs new text for semantic drift before merge.

## Non-goals

- No change to product behavior, gates, or CI.
- No rewrite of `overview.md`, `state-machine.md`, or existing SO text.
- No new enforcement tooling beyond keeping existing checks green.
