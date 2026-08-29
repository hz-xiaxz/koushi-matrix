# Issue #753 Rust test-structure cleanup

Status: design approved for implementation by different-model review round 2.

## Outcome

Land one behavior-preserving test-structure PR that removes scattered Rust self-source assertions from crate test binaries, transfers every structural assertion into one explicit repository lint entry point, moves every large inline test module into a sibling test module, and enforces both policies. Production behavior, public protocol, behavioral assertions, and behavioral test identities remain unchanged; every removed structural test has an auditable one-to-one lint-rule replacement.

## Verified baseline

Inventory is against `origin/main` at `276d7d07` after #750.

- A baseline `git grep` reports 365 textual `include_str!` lines in `crates/` and `apps/desktop/src-tauri`, of which 361 appear to target Rust source and four embed legitimate non-Rust artifacts (`docs/architecture/state-machine.md`, the Windows capability JSON, and `coreEvents.generated.json` twice). Independent lexical counting differed by one call, so these are provisional orientation numbers only. The implementation checker must regenerate and record the authoritative invocation count with its own parser before migration; completion depends on zero Rust-source embeddings and the closed four-artifact allowlist, not either provisional total.
- Rust-source includes occur in approximately 232 test functions: 177 in `koushi-core`, 29 in src-tauri, 24 in `koushi-sdk`, and 2 in `koushi-state`. They assert structural ownership, call-path, privacy, cfg, and forbidden-vocabulary contracts; they are not fixture data.
- There are 149 inline module bodies whose directly attached cfg expression includes `test`, totaling about 65k physical lines from the first attached attribute through the matching closing brace. Seventy-seven are at least 200 lines; 33 are at least 500 lines. External declarations (`mod tests;` and `#[path] mod tests;`) are excluded. The largest are `timeline/outbound_send.rs` (~3131), `timeline/read_state.rs` (~3061), `timeline/navigation.rs` (~2796), `runtime.rs` (~2762), and `account/session_lifecycle.rs` (~2595). The implementation checker regenerates these counts using the same balanced lexical scanner used by the enforcement rule.
- Baseline collection is 2564 workspace tests plus 135 `headless-core-qa` binary tests. The exact baseline lists are captured outside the repository for before/after comparison.

## Canon amendment

Amend `REPOSITORY_RULES.md` before implementation and bump `Last amended`:

- define a large inline cfg-test module as at least 200 physical lines under the same scanner definition and require extraction; this is a hard ceiling, not permission for a 199-line integration-style test to stay inline;
- retain and reconcile the existing rule: small pure single-helper tests may remain inline, private tests beyond one screen move to siblings, and cross-module/public behavior belongs under crate `tests/` even below 200 lines;
- state that source-structure assertions belong to the single repository checker, while behavioral tests must drive callable behavior;
- correct the stale example referring to nonexistent `crates/koushi-core/src/tests.rs` and point to current crate integration tests instead.

The enforcing checker and canon amendment land together. This is the durable architecture/rule change required by Canon-First; no dated plan alone defines the threshold.

## Source-contract policy

### Classification

1. An assertion that drives a callable API/state machine and checks an outcome is behavioral and stays a normal Rust test.
2. An assertion that reads `.rs` files to require/forbid source structure is a structural source contract. It remains exact lint evidence, not behavioral proof.
3. Non-Rust `include_str!` fixture/artifact embedding is legitimate and out of this policy.
4. Obsolete or duplicated source contracts may be deleted only when the PR records the enforcing replacement and proves the old test would add no independent coverage.

### One repository lint contract

Add one repository checker, `scripts/check-rust-test-structure.mjs`, with focused Node tests. Wire both the checker tests and checker execution as explicit steps in the Rust CI job beside diagnostic-isolation, and expose the checker through one package script for local use. It becomes the sole owner of Rust source-structure assertions:

- Translate every existing `.rs`-targeting `include_str!` assertion into an explicit named checker rule, then remove the Rust-source include and its structural assertion from the crate/bin test. If a test mixes behavior and source inspection, retain the behavioral setup/outcome assertions in Rust and move only the structural assertion to the checker.
- Keep a checked-in mapping table in the checker comments or companion review document from each old fully-qualified Rust test identity to its replacement named rule(s). Record old assertion count and replacement assertion count; deletion without a mapped rule or stronger behavioral proof is forbidden.
- Use direct, subsystem-named JavaScript rule functions, not a generic source-contract DSL, snapshot/baseline manifest, parser dependency, generated regex inventory, or second test framework. Helper functions may strip comments/test-only items and find balanced Rust items, following the existing diagnostic scanner style. The lexical contract explicitly handles normal/byte/raw strings (including `r#"..."#` families), char literals versus lifetimes, line/block comments, and nested braces; focused fixtures prove each case.
- After migration, first-party Rust has zero `include_str!` calls targeting `.rs` source, including literal, multiline, and `concat!(env!("CARGO_MANIFEST_DIR"), ...)` forms in parent-`cfg(test)` modules, `qa-bin`/`smoke` bins, integration tests, and vendored-SDK source checks. The checker resolves these forms before classifying the target and needs no fragile test-only allowlist: any remaining Rust-source embedding is an error.
- The four non-Rust artifact/document embeddings remain allowed and are tested explicitly.
- Inline cfg-test modules larger than 200 physical lines are rejected. Physical lines include attributes, declaration, and closing brace; external `mod tests;`, `#[path = "..."] mod tests;`, feature-gated declarations, and modules below the threshold are accepted.
- The checker reports file, module/function/rule, target, and size without printing source contents or private data.

This is a real consolidation: structural facts exist only in the checker after migration. Behavioral Rust tests remain behavioral proof. Do not retain scattered self-source assertions under a new label.

## Mechanical test-module move

For every inline cfg-test module of at least 200 lines:

- Preserve all attributes, module name, body, nested modules, assertions, and test names.
- Replace only the inline body with the corresponding external declaration, e.g. `#[cfg(test)] mod tests;`.
- Move the body to Rust's natural sibling path:
  - `src/foo.rs` → `src/foo/tests.rs` (or `src/foo/<module_name>.rs` for non-`tests` names);
  - `src/foo/mod.rs` → `src/foo/tests.rs`;
  - crate root `src/lib.rs` → `src/tests.rs` when no collision exists;
  - preserve feature/test cfg attributes exactly.
- Dedent the moved body without changing token order. Source-contract migration lands first, so moved Rust test bodies contain no source-embedding paths to adjust.
- Tests requiring parent-private access remain sibling unit tests with `use super::*` semantics unchanged.
- The verified >=200-line inventory has 75 modules using `super` and two remaining modules (`account/account_management.rs`, `account/local_data_cleanup.rs`) using crate-private account/test-support APIs; all 77 therefore require sibling unit placement. There is no public-only large inline candidate in this PR. Existing public integration tests stay under crate `tests/`, and no production visibility is widened.
- Existing already-external/integration tests are not reorganized unless required to remove a source-contract assertion.
- Accept both existing `#[path = "x_tests.rs"] mod tests;` declarations and the natural sibling layout; do not rewrite existing external layouts merely for uniformity.

Migration may be scripted, but the script is a one-shot implementation aid, not a committed production dependency. It accepts only top-level Rust module items with directly attached cfg attributes; a module nested in a function/impl or an ambiguous parse is a hard error. It must emit a machine-checkable extraction ledger proving each removed inline body is byte-identical to the added sibling body after only one uniform dedent, with attributes/module name/declaration recorded. Review the generated diff per crate and reject overlapping/ambiguous brace extraction rather than guessing.

## Verification

### Verify-first structural evidence

Before edits:

- capture `cargo test --workspace --exclude sidebar-composition --exclude key-management -- --list --format terse` (2564 tests);
- capture `cargo test -p koushi-core --features qa-bin --bin headless-core-qa -- --list --format terse` (135 tests);
- capture the exact inline-module and Rust-source-include inventory.

After edits require:

- exact behavioral test identities unchanged. Source-only identities may disappear only through the checked mapping ledger; mixed tests retain their original identity. Compare the baseline list after subtracting mapped source-only tests and require exact equality, not only aggregate counts;
- every old structural assertion maps to an executing named checker rule or a documented stronger behavioral assertion, with no duplicate fact owner;
- the new checker and its parser fixtures pass, including nested braces/raw strings/comments, cfg attributes, non-`tests` names, `#[path]` external modules, multiline/literal/`concat!` includes, parent-gated files, and feature-gated bins;
- zero inline cfg-test modules over 200 physical lines;
- zero first-party Rust `include_str!` calls targeting `.rs` source; exactly the four reviewed non-Rust embeddings remain;
- `cargo fmt --all -- --check`, `git diff --check`, focused affected crate tests, Core QA binary tests, and src-tauri tests;
- full workspace, frontend typecheck/lint/tests/build, Playwright, wasm, secret scan, cargo-deny, cargo-machete, and exact hosted CI platform/homeserver gates before merge.

Compare test-list identities using normalized harness lines, not only process exit or aggregate count. A missing, filtered, duplicated, or renamed behavioral test is a blocker.

## Change organization

Keep one PR but separate reviewable commits:

1. canon amendment + exact inventory + checker parser/rule tests;
2. source-contract rule migration grouped by subsystem, deleting each old Rust assertion only with its mapped replacement;
3. move-only module extraction grouped by crate with extraction-equivalence ledger;
4. behavioral collection/assertion audit and final review record.

No production-state changes, compatibility shims, TODO placeholders, ignored tests, relaxed assertions, or generic test framework.

## Review gate

- Pre-implementation reviewer round 1: `deepseek-brainstormer` (different model family from Luna), `VERDICT: FINDINGS`. Blockers fixed in this revision: structural assertions now migrate into and are removed in favor of the single checker; no test-only allowlist remains; inventory corrected; canon amendment added; all 77 large modules proven private/internal; migration equivalence and Rust-CI wiring specified.
- Pre-implementation reviewer round 2: `deepseek-brainstormer`, `VERDICT: CORRECT-TO-IMPLEMENT`. Non-blocking reminders incorporated: inventory totals are provisional until checker parsing; canon reconciles the 200-line ceiling with stricter placement rules; lexer fixtures cover raw/byte strings and lifetimes; `concat!(env!(...))` is detected; extraction is top-level-only.
- Pre-implementation verdict: **approved**.
- Implementer: `luna-implementer` after approval, in bounded sequential slices.
- Final-diff reviewer: pending (`reviewer-flash`).
- Final integration/self-review: pending (`gpt-5.6-sol`).

## Acceptance

- Structural source contracts execute from one repository lint entry point; first-party Rust test binaries contain no `.rs` source embedding, and behavioral tests do not inspect source text.
- No first-party production module contains an inline cfg-test body over 200 lines.
- Private tests use sibling modules; public-only tests use crate integration-test placement without widening production visibility.
- All prior assertion facts execute either as unchanged behavioral tests or mapped checker rules; behavioral workspace/QA test identities are preserved exactly.
- Formatting or moving a test file cannot retarget a source contract.
