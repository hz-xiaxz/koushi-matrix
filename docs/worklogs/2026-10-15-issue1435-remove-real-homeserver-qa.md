# Delete the retired real-homeserver QA tombstone (#1435)

Canon consulted: `REPOSITORY_RULES.md` ("QA Gates And Cleanup", "Root-Cause Fix
Discipline", "Documentation And Work Records"), `AGENTS.md` ("Keeping these
notes maintainable"), engineering rules "Build, Dependencies, QA Gates", and the
QA-lane contract in `docs/agents/qa-lanes.md`.

Decision: option 2 on #1435 — delete the retired external-homeserver QA surface
in one finished PR, together with the gates that still required it, so no
checker, test-support list, test, or workflow references a deleted path. The
retired entry points stay quarantined in `docs/agents/history.md`.

## Deleted

- `crates/koushi-qa/src/bin/real-homeserver-qa.rs` (the fail-closed `main()`
  plus its `#[path]` module declarations).
- `crates/koushi-qa/src/bin/real_homeserver_qa/` — `config.rs`,
  `credentials.rs`, `event_source.rs`, `waiters.rs`, `cleanup.rs`,
  `compat_flow.rs`, `startup_latency.rs`, `admission.rs`, `admission_tests.rs`,
  `tests.rs`.
- The `[[bin]] name = "real-homeserver-qa"` entry in
  `crates/koushi-qa/Cargo.toml`. `headless-core-qa` and its
  `required-features = ["qa-bin"]` remain.
- `scripts/desktop-real-homeserver-qa.mjs`, unreachable from every package
  script since #1413.

## Updated in the same change

- `crates/koushi-qa/Cargo.toml` — dropped `tempfile`, used only by the deleted
  module's tests. Every other dependency is still used by `headless-core-qa` or
  `headless_core_qa/`.
- `scripts/check-protocol-qa-boundaries.mjs` — `real-homeserver-qa` removed from
  the required-binary manifest check and from the two `koushi-core`
  legacy-path/declaration loops. The checker still requires `headless-core-qa`
  and `required-features = ["qa-bin"]`, so it still proves the QA binary lives
  in `koushi-qa` and nowhere else.
- `scripts/check-protocol-qa-boundaries.test.mjs` — its fixture manifest no
  longer declares the deleted binary.
- `scripts/check-agents-docs.mjs` — `coreScenarios()` now reads only
  `headless_core_qa/registry.rs`; `compat`, `space_compat` and
  `startup_latency` were added to the hard-coded retired-flag list, so they
  cannot reappear in a fenced command. No `docs/agents/*.md` file named them as
  runnable, so no documented scenario name was lost.
- `apps/desktop/src/scripts/releaseTestSupport.ts` — `readRealHomeserverProductionSource()`
  deleted; it had no importers.
- `apps/desktop/src/scripts/macGuiQa.test.ts` — the deleted binary and runner
  dropped from `activeSourceFiles`.
- `apps/desktop/src/scripts/linuxGuiQa.test.ts` — the shared-`CARGO_TARGET_DIR`
  test keeps its `scripts/lib/local-homeserver-qa.mjs` assertion and drops the
  retired side.
- `apps/desktop/src/scripts/headlessAndRealQa.test.ts` — the case that spawned
  the deleted runner is gone; the package.json absence assertions stay.
- `scripts/build-structure-contract.test.mjs` — the deleted runner dropped from
  the submodule-guard source list.
- `docs/agents/history.md` — the retired-vocabulary row now records the
  deletion and keeps every retired entry point named.
- `docs/architecture/overview.md` — the `koushi-qa` crate bullet names only
  `headless-core-qa`.

## Verification

- `node scripts/check-protocol-qa-boundaries.mjs`
- `node scripts/check-agents-docs.mjs`
- `node --test scripts/check-protocol-qa-boundaries.test.mjs`
- `node --test scripts/build-structure-contract.test.mjs`
- `cargo fmt --check`
- `cargo test -p koushi-qa --features qa-bin --bin headless-core-qa`
- `npm --prefix apps/desktop run lint`
