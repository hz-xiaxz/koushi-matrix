# Release version gate covers Cargo.lock

## Scope

v0.11.1 bumped `apps/desktop/package.json`,
`apps/desktop/src-tauri/tauri.conf.json`, and
`apps/desktop/src-tauri/Cargo.toml`, but left the root `Cargo.lock`
`koushi-desktop` entry on `0.11.0`. Nothing enforced the fourth file, so the
first `cargo` command on the next branch rewrote it and the release bump landed
in the unrelated #955 PR.

This change makes the lockfile part of the release contract and enforces it.

Consulted `REPOSITORY_RULES.md` (Canon-First Change Protocol, "QA Gates And
Cleanup": a document that promises a check must have an enforcing script),
`docs/agents/verification.md`, and `docs/releases/desktop-release.md`.
Started from `origin/main` commit `e448a60b`.

## Changes

| Surface | Change |
| --- | --- |
| `scripts/desktop-release-version.mjs` | Reads the `koushi-desktop` version from the root `Cargo.lock` and includes it in the current-state consistency check; the mismatch error names the refresh command. |
| `scripts/desktop-release-version.test.mjs` | New. Drives the script against throwaway fixture roots: matching set passes, stale lock fails, manifest disagreement still fails, lockfile without the package is reported. |
| `.github/workflows/ci.yml` | Rust job runs the checker tests and the guard on every PR, not only release PRs. |
| `docs/releases/desktop-release.md` | Four synchronized version files; a lockfile-refresh step before the local gates; the expected release-PR diff. |
| `.{agents,claude,opencode}/skills/koushi-release/SKILL.md` | Prepare mode no longer says "three manifests". |

`readVersionsFromGit` deliberately still reads only the three manifests. That
path exists to obtain a previous version to compare against, and a historical
commit predating this rule must not hard-fail the release workflow.

## Verification

RED first: against the previous script, `node --test
scripts/desktop-release-version.test.mjs` fails 2 of 4 — the stale-lock case and
the missing-package case — and passes 4 of 4 after the change.

| Check | Result |
| --- | --- |
| `node --test scripts/desktop-release-version.test.mjs` | 4 passed |
| `node scripts/desktop-release-version.mjs` (repository root) | `version=0.11.1`, exit 0 |
| Same guard with the lock forced back to `0.11.0` | exit 1, names `lock=0.11.0` |
| `cargo metadata --format-version 1 >/dev/null` on that forced state | restores the entry, `git diff Cargo.lock` empty |
| `node scripts/check-agents-docs.mjs`, `node scripts/user-help.mjs --check`, `git diff --check` | pass |

Not run: the release workflow itself, which only executes on `main`.
