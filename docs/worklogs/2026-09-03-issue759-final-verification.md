# Issue #759 final local verification

## Final implementation audit

The final implementation contains one ordered versioned state-update lane, state-only command reconciliation, and two renderer-independent Core settlement points (`FocusedProjectionCommitted` and `GapProjectionRelayed`). Initial, settlement-resync, and gap-resync reads remain explicitly separate. Both timeline renderer acknowledgement commands and the frontend retry owner are deleted.

Phase E extended `scripts/check-command-snapshot-contract.mjs` to reject production occurrences of the removed Core/Tauri/TypeScript acknowledgement names and the deleted delivery-controller files. The exhaustive DesktopApi contract now treats the plan's “removed renderer ACK” row as deleted API, and pins both names as absent.

A full Core run exposed that unit AppActor fixtures supplied a closed focused-projection receiver. Production had a live account-tree sender, but a closed optional lane incorrectly terminated AppActor. The receiver is now fail-closed locally: closure disables only that select branch and leaves commands/actions running. The six exposed runtime tests were rerun individually and then passed in the full Core suite.

## Fresh local evidence

### Frontend

- TypeScript typecheck: passed.
- ESLint + IME + agent-doc guards: passed.
- Full Vitest: 103 files / 1,537 tests passed.
- Production Vite build: passed; only existing chunk/dynamic-import warnings.
- Secret scan: passed.
- Tauri boundary + command snapshot/ACK absence contract: passed.
- Relevant Phase D Vitest before full run: 4 files / 160 tests passed.

### Rust and contracts

- `koushi-core --lib`: 935 passed, 8 ignored.
- `koushi-sdk --lib`: 151 passed.
- `koushi-desktop`: 113 library tests + 5 keyring integration tests passed.
- Headless QA binary tests (`qa-bin`): 88 passed.
- State/search wasm check: passed.
- State, search, key, diagnostics, media, and Windows overlay packages: tests and doctests passed.
- Checked-in CoreEvent wire artifact test: passed.
- Rust structure checker tests: 19/19 passed.
- `cargo deny check`: advisories/bans/licenses/sources passed.
- `cargo machete`: no unused dependencies.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- SDK submodule, adapter/command ownership, Rust structure, diagnostic-isolation, domain-dependency, and agent-doc checks: passed.

The exact CI workspace command was attempted twice with a 60-second hard limit and stopped during cold feature-unification compilation before tests. An orphan `rustc` from the first timeout was found and terminated. The same workspace membership was then covered by bounded package runs above; no test failure was hidden by the workspace timeout.

### Playwright

All 31 current specs were exercised in bounded single-spec/small-group lanes: 263 tests passed. One existing variable-height scrollback assertion failed:

- `timeline-scrollback.spec.ts:413` — expected scroll drift ≤10, observed 1462.

This is the same variable-height scrollback failure class already reproduced on pre-B2 commit `078e6f5`; it is not introduced by the ordered-state/settlement changes. The current run's other 24 scrollback tests and every other spec passed. Hard-timeout lanes were split rather than immediately rerun; surviving Vite processes were located by port and terminated before the next lane.

## Review ledger

- Design: Fireworks DeepSeek V4 Flash 0731 — `CORRECT-TO-MERGE` before implementation.
- Phase A2: Fireworks — `CORRECT-TO-CONTINUE`.
- Phase B1: Fireworks — `CORRECT-TO-CONTINUE`.
- Phase B2 amendments and diff: Fireworks — `CORRECT-TO-MERGE` / `CORRECT-TO-CONTINUE` after findings fixed.
- Phase C diff: Fireworks — `CORRECT-TO-CONTINUE`; stale ledger comment fixed. The reviewer classified the missing AppActor-level integration proof as minor/non-blocking; focused decision tests, dropped-consumer delivery proof, and the final full Core suite cover the implemented seams.
- Phase D diff: Fireworks — `CORRECT-TO-CONTINUE`; local signal-shape comment added for the review's minor clarity finding.

## Final exact-HEAD review

Fireworks DeepSeek V4 Flash 0731 reviewed the complete `origin/main...HEAD` integration and returned `CORRECT-TO-PR`. It found no Critical or Important issue. All three new Minor findings were fixed before PR creation:

- async login discovery/OIDC/recovery UI handlers now enter the existing terminal-catch wrapper;
- the command contract scans every async helper rather than exempting `session.rs` at file scope;
- the snapshot DTO comment now documents only initial, settlement-resync, and gap-resync reads.

Focused App/contract tests (88), typecheck, command ownership guard, and diff check passed after these fixes. Re-review returned `CORRECT-TO-PR`; its two non-blocking guard-robustness notes were also fixed by recursively scanning command submodules and ignoring bodyless trait async declarations without swallowing the next function.
