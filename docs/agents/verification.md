# Verification Discipline

How correctness is established in this repository. Read this before fixing
anything. The lane catalog is in [qa-lanes.md](qa-lanes.md); the contract
surfaces a change must mirror are in
[state-ownership.md](state-ownership.md#snapshot-and-wire-contract-mirrors).

## Verify first, no human eyes

Correctness is guaranteed by reproducible headless verification, never by manual
or visual GUI inspection. Build the verification (体制) BEFORE the fix and let
the same check turn green as the proof of the fix: 体制 → 修正, strictly, never
the reverse.

- For any bug / regression / perf / behavior change, FIRST add or extend a
  headless check that REPRODUCES the problem (RED): a `headless-core-qa`
  scenario against a local homeserver, a Rust/TypeScript unit test, or a
  Playwright spec — asserting on `CoreEvent` / `AppStateSnapshot` / tokens /
  DOM, never on logs or fixed sleeps. The fix is "done" only when that same
  check turns GREEN.
- Measure performance claims; never eyeball them. Gate on a number — e.g. the
  `cache_restore` scenario asserts a deep-history anchor is restored from cache
  in ≤ N backward-paginate cycles while the network is blocked.
- To prove cache-served / offline behavior, block the network in-harness (the
  `headless-core-qa` `QaTcpProxy.disable()` pattern) and assert success with no
  `network` origin (the #123 `EventsOrigin` observer).
- Source-text assertions (`source.contains("…")`) are structure guards, not
  behavioral proof. Prefer a test that drives the behavior and asserts on
  emitted events/state.
- Native / manual GUI inspection is the last and weakest layer: a confirmation
  only, never the primary correctness gate.

## Minimize human round trips

Human-in-the-loop debugging is a bottleneck. Always look for ways to minimize
the number of human reproduction and feedback round trips. Rich diagnostics are
one important means: before asking the human to retry, add enough sanitized
information to distinguish the leading hypotheses in one run, including the
relevant stage, outcome, elapsed time, error classification, and useful counts
or booleans. Prefer one deliberately rich diagnostic pass over adding one field
after each retry. Never log secrets, credentials, recovery material, keys,
tokens, or unnecessary raw identifiers.

Before running an expensive Linux/macOS/Windows GUI lane as a debugger, add a
cheap private-data-free diagnostic token or title state for the missing product
transition, then run focused Rust/Tauri/browser checks. Full native GUI lanes
are final evidence for an issue, not the first place to discover command
routing failures.

## Read the gate's own exit status

Read the gate's own exit status, never a pipeline's. `cargo test … | grep …`
reports grep's status, and appending anything (`; echo done`, `; true`) reports
that instead, so a failing suite looks green. Run `<gate> > /tmp/x.log 2>&1;
echo "EXIT=$?"` and report that number. A 2026-07-25 change claimed a green
`cargo test --workspace` this way and pushed a red DTO golden to CI.

A subagent's "gates passed" claim is not evidence — re-run the gate yourself.

## Verification by stage

- **Iteration:** start with the smallest reproducing check and expand to the
  affected integration/feature matrix once it passes. The first command need
  not be the entire CI suite.
- **Before review:** read the full diff, including untracked/new files, and run
  checks for the changed contracts. Use the checklist below and the relevant
  state-ownership section; installed generic review skills do not define this
  repository's artifact-update commands or gate matrix.
- **Before merge:** run the local gates in
  [engineering rules](../policies/engineering-rules.md#build-dependencies-qa-gates)
  and inspect required CI results. For documentation-only changes under that
  policy, run `node scripts/check-agents-docs.mjs` when touching this tree,
  check affected links and command references, and run `git diff --check`.
  Do not claim unrun product suites passed.

## Running focused tests

- When running focused Rust crate unit tests, add `--lib` unless integration
  tests are intentionally part of the gate. Example: `cargo test -p koushi-core
  --lib some_unit_test_name`. Without `--lib`, Cargo still launches every
  matching integration-test binary after the library test, which is slow even
  when those binaries run zero tests.
- When running a focused Rust integration test, target the integration-test
  binary with `--test <name>` instead of using only a package-wide name filter.
  Example: use `cargo test -p koushi-state --test search_state`, not `cargo test
  -p koushi-state search`, because the latter launches every integration-test
  binary and then filters inside each one.
- Do not run a long-duration end-to-end or homeserver scenario after every small
  implementation edit. First complete the coherent assertion-driven flow, using
  compile checks, focused unit/integration tests, and short fail-fast
  checkpoints while iterating. Remove superseded fixture paths and review the
  finished diff, then run the long scenario once as the integrated gate. Re-run
  it only when its own evidence identifies a necessary change or after the final
  reviewed fix; do not spend the full timeout to discover one incomplete phase
  at a time.
- When a broad Playwright or browser-headless run reveals multiple failures with
  the same shape, stop one-test-at-a-time spot fixes. First read the shared
  harness, component lifecycle contract, and related fixtures as a group;
  classify whether the problem is fixture drift, a missing DTO mirror, unstable
  Playwright actionability, or product behavior. Repair the shared
  helper/contract boundary before rerunning the broad gate.

## What CI actually gates

`.github/workflows/ci.yml` runs on every pull request. Every job in the table
below, including `Rust lint (rustfmt / clippy)`, is a required status check
for merging into `main`:

| Job | Covers |
| --- | --- |
| `Frontend (typecheck / vitest / build / secret-scan)` | typecheck, vitest, build, secret scan, ESLint import boundaries, Tauri adapter boundary, domain-crate platform deps |
| `Browser headless (Playwright DOM tier)` | `npx playwright test` — a red spec is a blocked merge |
| `Rust lint (rustfmt / clippy)` | `cargo fmt --check`, then workspace, QA-binary, and release-configuration clippy with `-D warnings` (see [Rust lint gate](#rust-lint-gate)) |
| `Rust (workspace / src-tauri / wasm)` | submodule guard, diagnostic-isolation guard, one feature-unified workspace suite (including the `koushi-core-testkit` integration targets and the `koushi-desktop` DTO/IPC contract tests), wasm build, `cargo-deny`, `cargo-machete`, the CI-cache report, and workspace cargo metrics |
| `macOS Tauri cargo check` | `cargo check --profile ci -p koushi-desktop` plus `cargo clippy ... -- -D warnings` on macOS, including `#[cfg(target_os = "macos")]` paths excluded by Linux CI |
| `Core invitations (tuwunel)` / `Core invitations (synapse)` | real homeserver `--core --scenario=invites_dm` per server |
| `Core QA binary tests` | `cargo test -p koushi-qa --features qa-bin --bin headless-core-qa` |
| `Windows overlay ACL IPC` | `cargo test -p koushi-windows-overlay-acl windows_overlay_ipc_is_authorized`, OIDC launch tests, and desktop clippy with `-D warnings` for Windows-only cfg paths |

### Rust lint gate

Run from the repository root with the pinned toolchain (`rust-toolchain.toml`
lists the `clippy` and `rustfmt` components):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p koushi-qa --features qa-bin --all-targets --locked -- -D warnings
cargo clippy --release --workspace --locked -- -D warnings
```

The release run matters because `cfg(debug_assertions)` QA paths disappear
there, so imports that only those paths use are unused only in release builds.

`cargo fmt` formats workspace members only; do not use `cargo fmt --all`,
which also rewrites path dependencies such as `vendor/matrix-rust-sdk`. The
vendored SDK is excluded from the workspace and is not linted.

Fix findings rather than silencing them. Crate- or module-wide `allow`
attributes are not accepted. A per-item `#[expect(lint, reason = "...")]` is
acceptable only when the lint is a false positive or the fix would harm
clarity (for example, a Tauri command whose parameters are named IPC
arguments). Use `#[cfg(target_os = ...)]`, or `cfg(any(<platform>, test))`
for pure helpers that are tested everywhere, for genuinely platform-specific
code. Shared contract variants produced only on one platform keep a narrowly
scoped `cfg_attr(not(<platform>), allow(dead_code))`.

`cargo test --profile ci --workspace` does not compile the QA binaries: both bin targets set
`required-features = ["qa-bin"]`. Only the `Core QA binary tests` job compiles
them.

The required Rust jobs use the repository's `[profile.ci]` profile. It inherits
the test profile, keeps debug assertions and overflow checks enabled, and sets
`debug = 0`, `incremental = false`, and symbol stripping for reproducible
hosted builds. Local `dev` and production `release` behavior is unchanged.

Every job that sets `CARGO_TARGET_DIR` gives the rust-cache action the matching
workspace mapping (`. -> target-ci`, `. -> target-macos-check`, or
`. -> target-windows-overlay`). The scheduled Issue #738 probe uses the same
explicit mapping. The primary Rust job's cache report fails closed when
rust-cache claims an exact hit but representative registry and git dependency
artifacts (`tokio`, `serde`, `ruma`) are absent from `target-ci/ci/deps`, or
when the SDK cache claims a hit without SDK fingerprints and artifacts. The
workspace metrics step records total and vendored-SDK `Compiling` lines and test
totals, so a restored archive that Cargo does not reuse is visible.

rust-cache prunes path dependencies under the repository root, so the vendored
Matrix SDK has its own exact-keyed artifact cache. Cargo judges path
dependencies by source mtime and checkout stamps every file with the current
time, so the job first normalizes the SDK's tracked sources to a fixed old
mtime. This is safe only because the key pins the SDK gitlink, the lockfile,
every workspace manifest, `ci.yml`, the profile, and the rustc release; a
different feature set still selects a different fingerprint hash. The CI key
reads that release from `rustc -vV` in the job, where rustup resolves it from
`rust-toolchain.toml`, so a toolchain bump rekeys it automatically. A
toolchain bump still edits, together with `rust-toolchain.toml`, every
`dtolnay/rust-toolchain@<version>` reference and step name in `ci.yml` and
`release-desktop.yml`, and the literal `rust-<version>` in the release
workflow's SDK cache keys. Keep every
key complete when changing any of those inputs, because an exact hit is never
re-saved.
rust-cache has the same property, and its key hashes only manifests, the
lockfile and the toolchain. When a change to the cargo command set changes the
resolved dependency feature graph, bump the Rust job's `shared-key` suffix, or
the newly required dependency builds recompile on every run.

The Rust job runs `cargo test --profile ci --workspace --exclude
sidebar-composition --exclude key-management` once. `koushi-core-testkit` and
`koushi-desktop` are workspace members, so this runs every test the former
standalone `-p` steps ran. Those steps added no test: each resolved its own
package's dependency graph and feature set (for example `koushi-core/test-hooks`
from their dev-dependencies), which only recompiled the vendored SDK and Koushi
stack (20 and 11 crates, about 2m10s each on warm main runs). No test is
gated on `not(feature = "test-hooks")`, and the `koushi-core` lib suite lists
the same tests with and without `test-hooks`. Production feature sets
(without dev-dependency features) are compiled by `macOS Tauri cargo check`
and the release workflow. QA binaries, wasm, macOS, Windows, and homeserver
jobs stay separate because they provide distinct feature, platform, target, or
runtime coverage.

Do not assume a green PR means a homeserver job passed — check the job
explicitly, and confirm whether it is a required check before treating it as a
merge gate.

Do not explain an unusually long CI step as normal repository variance without
comparing it to recent successful runs. Inspect the same job step's duration in
a recent green run; once the current step exceeds twice that baseline, stop
passive waiting and reproduce the exact workflow command locally (including
integration tests and exclusions), or inspect the completed job log if
available. A 2026-07-31 PR waited about 40 minutes on a Rust workspace step
whose recent green baseline was about 5 minutes; the exact local CI command
exposed seven integration-test expectation failures that an earlier
`--lib`-only gate had missed.

## Diff self-review

Before opening a PR or requesting a review, read the branch's own finished
diff and judge it against the applicable canon yourself. Trace changed
production paths, contract mirrors, async/ownership and terminal semantics;
confirm verify-first evidence and the applicable local gate matrix. The
`preflight-review` skill can supply general prompts, but use this repository's
[artifact instructions](state-ownership.md#snapshot-and-wire-contract-mirrors)
for exact update procedures.

```bash
git diff origin/main...HEAD
git status --short   # untracked files are absent from git diff entirely
```

Priorities, in order:

1. Repository-rule consistency — `REPOSITORY_RULES.md`,
   `docs/architecture/overview.md`, `docs/architecture/state-machine.md` when
   reducers change, `docs/policies/engineering-rules.md`, `AGENTS.md`, and the
   relevant dated plan.
2. Rust/Tauri best practices and consistency with the surrounding code.
3. Security and privacy — secret leakage, private data in Debug/logs/QA output.
4. Contract correctness — state machine, command/event, and DTO shapes.

User-guide consistency is part of this pre-PR check. Use the
[PR checklist](../../.github/pull_request_template.md) and compare affected
instructions with the implementation and tests: menu/category paths, UI labels,
prerequisites, outcomes, and limitations. Update the guide and
[settings location map](../help/settings.md) in the same PR when behavior or
placement changes; otherwise explain why no manual change is needed. Run
`node scripts/user-help.mjs --check` for navigation and generated llms.txt.
A passing link check alone does not establish that instructions are correct.

For UI changes, check the changed surfaces against
[Property Display And Editing](../../REPOSITORY_RULES.md#property-display-and-editing)
in both self-review and independent audit: no property with its display and
its editing in separate places, no duplicate display/edit locations, no
unrelated actions between them, and no setting-like entry without a matching
destination. Report findings with the property, both locations, and the
effect.

Scope notes that repeatedly matter:

- Read `Cargo.toml` and `src/lib.rs` alongside a change that adds feature gates,
  changes module visibility, or exposes test-only APIs. Judging the change
  without them invents problems that are not there.
- Include new files explicitly. `git diff` alone is empty for untracked paths,
  so a review that only reads it can miss an entire new module.
- When a finding is caused by a canon gap rather than this change, amend the
  canon too — see the rule-update requirement in `REPOSITORY_RULES.md`.

Self-review is load-bearing, not a formality: reading the finished #328 diff
surfaced a second real bug (an `identifier()` comparison that missed a sent
local echo) that the passing tests did not cover.

## Design simplicity

Follow the normative design-simplicity rules in
`docs/policies/engineering-rules.md`: do not add defensive machinery without a
reproduced failure or named invariant.

Put the smallest necessary guard at the authoritative boundary. This never
weakens security, privacy, trust-boundary validation, data-loss prevention,
accessibility, or explicitly approved requirements.

## Issue #738 flake measurement

The required CI workflow remains retry-free. The separate
`.github/workflows/issue-738-flake-probe.yml` is a scheduled/manual,
non-required measurement job; a failed probe is reported as a failed probe and
cannot turn a required check green. It checks out one SHA and runs these named
probes repeatedly: the Rust
`committed_room_cleanup_bypasses_a_saturated_account_mailbox` test in
single-thread and default mode, plus the named stale-live-edge and
first-unread-pill Vitest tests. Rust test binaries are compiled in an explicit
warm-up step outside measured attempts, so cold compilation is not mislabeled
as a test flake. Each individual attempt is bounded to 120 seconds. The closed
probe command list does not accept arbitrary shell commands and
child output is not recorded; failures use fixed signatures only.

Run locally with a bounded attempt count:

```bash
node scripts/flake-probe.mjs --attempts 10 --output-dir artifacts/issue-738-flake-probe
node scripts/summarize-flake-probe.mjs --sha <40-hex-sha> artifacts/issue-738-flake-probe/flake-probe-results.json
```

The probe artifact contains one JSON record per executed attempt, JUnit XML,
and a Markdown summary. `attempt` is an execution of one named test and its
number restarts in each workflow run; `recorded_at` keeps rows from separate
artifacts distinct. A GitHub workflow rerun is a separate run and must not
replace or hide failed attempt records. The summarizer accepts one or more JSON
result artifacts, reports
attempt total, failures, failure rate, and the observed date window, and can
validate one unchanged SHA with `--sha <40-hex-sha>` or
`--require-unchanged-sha`. Use `--max-failure-rate 0.01` when a nonzero exit at
or above the strict 1% threshold is wanted.

A seven-day result is eligible for interpretation only when the observed
attempt timestamps span at least seven days; a shorter window remains pending.
The `<1%` value is computed from all attempts, not workflow reruns. Issue #738
also requires ten consecutive full CI runs for one unchanged SHA with no rerun;
the flake probe does not measure that criterion. Acceptance remains pending
until both the ten-CI-run and seven-day attempt-level evidence actually exists.

## IME-safe text input checks

When changing any text field, textarea, password/recovery entry, upload caption,
search box, or form, use the primitives in
`apps/desktop/src/components/ImeTextControl.tsx`. Run the focused contract and
the production surface inventory from the repository root:

```bash
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx
```

The normal desktop lint command (`npm --prefix apps/desktop run lint`) includes
the inventory gate. If the gate finds a new surface, migrate it to the shared
primitive. Do not add a per-file exception or local composition workaround.

## Cost-controlled agent delegation

- Use cheaper implementation agents only for bounded, low-ambiguity work: source
  search, issue inventory, single-file tests, small module-local Rust patches,
  docs consistency checks, and narrow diff reviews. Prompts must name the issue,
  allowed files, forbidden shared files, expected verification command, and the
  exact output format.
- Main agents own cross-boundary design, state-machine boundary decisions,
  shared enums/DTOs, Tauri/TypeScript wire contracts, `App.tsx`,
  `TimelineView.tsx`, `styles.css`, canon docs, commits, issue comments, and
  close decisions. Cheap-agent output is a draft to verify, not accepted
  evidence by itself.
- Do not let two agents edit shared hot files concurrently. Use the canonical
  [shared-surface list](../../REPOSITORY_RULES.md#shared-hot-files) rather than
  duplicating it here; coordinate ownership before granting a narrow patch.
- Review prompts name the applicable canon sections and follow
  [Review And Audit](../../REPOSITORY_RULES.md#review-and-audit). A silent,
  timed-out, or budget-exceeded run is not review evidence. Higher-priority
  agent instructions may add review requirements; repository guidance does
  not cancel them.
