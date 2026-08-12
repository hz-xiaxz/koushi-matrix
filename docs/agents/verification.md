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

`.github/workflows/ci.yml` runs on every pull request:

| Job | Covers |
| --- | --- |
| `Frontend (typecheck / vitest / build / secret-scan)` | typecheck, vitest, build, secret scan, ESLint import boundaries, Tauri adapter boundary, domain-crate platform deps |
| `Browser headless (Playwright DOM tier)` | `npx playwright test` — a red spec is a blocked merge |
| `Rust (workspace / src-tauri / wasm)` | submodule guard, diagnostic-isolation guard, workspace tests, Tauri DTO + IPC contract tests, wasm build, `cargo-deny`, `cargo-machete` |
| `Core invitations (tuwunel)` / `Core invitations (synapse)` | real homeserver `--core --scenario=invites_dm` per server |
| `Core QA binary tests` | `cargo test -p koushi-core --features qa-bin --bin headless-core-qa` |
| `Windows overlay ACL IPC` | `cargo test -p koushi-windows-overlay-acl windows_overlay_ipc_is_authorized` |

`cargo test --workspace` does not compile the QA binaries: both bin targets set
`required-features = ["qa-bin"]`. Only the `Core QA binary tests` job compiles
them.

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
diff and judge it against the canon yourself, using the `preflight-review`
skill checklist (production-path trace, contract mirrors, async/ownership,
terminal semantics, verify-first integrity, local gate matrix).

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

Operationally, all agents—including `gpt-5.6-sol`, which tends to overcompensate
with defensive machinery—must add no defensive machinery absent a reproduced
failure or named invariant. Retry loops, persisted incident buffers, fallback
services, classification state machines, and speculative diagnostics require that
evidence; put the smallest guard at the authoritative boundary. This is agent
guidance, not a product contract, and never weakens required security, privacy,
trust-boundary validation, data-loss prevention, accessibility, or explicitly
approved requirements.

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
- Do not let two agents edit shared hot files concurrently. Treat
  `crates/koushi-state/src/{state.rs,action.rs,reducer.rs}`,
  `crates/koushi-core/src/{command.rs,event.rs,runtime.rs}`,
  `apps/desktop/src-tauri/src/{dto.rs,commands.rs}`,
  `apps/desktop/src/{App.tsx,components/TimelineView.tsx,i18n/messages.ts,styles.css}`,
  browser-headless specs, and Linux GUI QA scripts as main-agent integration
  points unless the task explicitly grants a narrow patch.
- Review prompts for cheap agents must ask for consistency with
  `REPOSITORY_RULES.md`, `docs/architecture/overview.md`,
  `docs/architecture/state-machine.md` when reducers or state machines change,
  `docs/policies/engineering-rules.md`, `AGENTS.md`, and the relevant dated
  implementation plan. A silent, timed-out, or budget-exceeded cheap-agent run
  is not review evidence.
