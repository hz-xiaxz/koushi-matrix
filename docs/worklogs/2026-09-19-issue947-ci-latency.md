# Issue #947 CI latency implementation

Issue #947 identified three avoidable sources of Rust CI latency: the cache
action restored `target` while Cargo wrote to job-specific `CARGO_TARGET_DIR`
paths, the Rust job reran package suites already covered by the workspace
suite, and hosted builds used the default debuginfo-heavy test profile.

This implementation keeps the distinct QA gates while making the ordinary
Rust path explicit:

- Rust cache mappings now match `target-ci`, `target-macos-check`,
  `target-windows-overlay`, and the non-required Issue #738 probe target.
- The workspace job excludes `koushi-core-testkit` and `koushi-desktop`, then
  runs each package once through an explicit `cargo test -p ... --profile ci`
  step. This satisfies the leaf-crate contract and preserves the Tauri DTO/IPC
  gate without compiling either package twice.
- `[profile.ci]` inherits `test`, keeps debug assertions and overflow checks,
  and disables debuginfo, incremental compilation, and symbols for hosted
  correctness CI. Local development and release profiles are unchanged.
- The primary Rust job caches representative vendored Matrix SDK artifacts
  with keys including runner, Rust toolchain, profile, enabled SDK feature set,
  workspace dependency inputs, and the checked-out SDK revision.
  `ci-rust-cache-report.mjs` records cache-hit state, target size, fingerprints,
  and SDK artifacts, and fails if a claimed SDK hit is empty.
- The headless QA helper accepts `--cargo-profile=ci`, and CI homeserver,
  macOS, Windows, and Rust commands select the profile explicitly.
- `.github/workflows/issue-947-ci-benchmark.yml` provides a manual cold/warm
  comparison for one immutable source and SDK revision. It captures Cargo
  compilation/checking/finish counts, test totals, cache-hit state, target and
  log sizes, and elapsed workspace-suite time as downloadable JSON and step
  summaries.

The issue's baseline was a median 16m15s Rust-job duration across 14 recent
successful runs. That baseline and the arithmetic estimate in Issue #947 are
kept as pre-change measurements; this change does not claim a post-change
speedup until a cold cache population run and a compatible warm run are
available. The CI cache report provides the measurements needed to compare
target paths, restored artifacts, target size, and later compilation behavior.

## Follow-up (2026-09-28): measured post-#946 behavior

Warm main runs 36308871002 and 36309650449 (the Rust job took 10m22s and
10m13s, against the 16m15s baseline) showed:

- The `target-ci` mapping works: an exact rust-cache hit, and no registry or git
  dependency appears in any `Compiling` list.
- The three cargo test invocations still compiled 20, 20 and 11 crates, which
  is the whole vendored SDK and Koushi stack each time (2m35s, 2m10s and 2m16s
  of compilation). Within one job, this only happens when feature sets differ,
  so the standalone `-p koushi-core-testkit` and `-p koushi-desktop` steps
  moved the recompilation rather than removing it. They ran no test the
  workspace suite could not run: no test is gated on
  `not(feature = "test-hooks")`, and `cargo test --profile ci -p koushi-core
  --lib -- --list` lists 1265 tests both with and without `--features
  test-hooks`.
- The vendored SDK cache missed, and it could not have helped on a hit.
  Locally, restoring older SDK artifacts after touching the sources to "now"
  (what checkout does) recompiled `matrix-sdk-common` and the rest of the SDK.
  With the sources normalized to a fixed old mtime, the same artifacts were
  fresh. The cache also omitted `build/matrix-sdk-*` build-script outputs, and
  its key did not cover the manifests and command that select the feature set.

The follow-up change runs one unified workspace invocation, normalizes vendored
SDK source mtimes before the exact-keyed SDK restore, adds build-script outputs
and a complete key, checks representative rust-cache dependency artifacts, and
records `Compiling` counts (total and vendored SDK) for every Rust job run.

Measured on PR #1041. The rust-cache `shared-key` was bumped because the
unified feature graph otherwise recompiled 36 registry and git crates on every
exact, never re-saved hit:

| Run | Rust job | Compiling (vendored SDK) | Cargo build | Tests |
| --- | ---: | ---: | ---: | ---: |
| Cold: new rust-cache and SDK keys (36387243973) | 8m36s | 620 (all) | 6m01s | 3089 |
| Warm: both exact hits (36389004671) | 4m53s | 12 (0) | 2m10s | 3089 |

Only the 12 Koushi workspace crates recompile on a warm run. The restored
archives are about 729 MB (rust-cache) and 78 MB (SDK artifacts). 3089 equals
the 2657 + 230 + 202 tests of the former three steps. The manual benchmark
(36386470391, full `target-ci` restore) agreed: 620 crates compiled cold and 12
warm.
