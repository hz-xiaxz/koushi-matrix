# Issue #551 remaining QA ownership decomposition

## Scope and immutable baseline

One ownership-area PR finishes the remaining QA files in Issue #551:

- `scripts/desktop-linux-gui-qa.mjs`
- `apps/desktop/src/scripts/releaseScripts.test.ts`
- `crates/koushi-core/src/bin/real-homeserver-qa.rs`

Immutable base: `1adf0d565695bd767bc609ada1b97dfd33aad9d0`.

| Source | `wc -l` | SHA-256 | Inventory |
| --- | ---: | --- | --- |
| Linux GUI runner | 5,285 | `6bfd36823ff3248ffccc4d594c7f7b4576fbb09ae2dcccb220ee31f22ff284f4` | 199 top-level declarations, 26 scenarios |
| Release contracts | 4,914 | `55218d8b085c25f9304b98304d249478e92839ecb793b406f52b59afaeca940b` | 70 top-level support declarations, 154 tests |
| Real-homeserver binary | 3,447 | `1480c8c95aa161460cc3bbdc42f8fc168828c133bfbda9ccc2949abe42543c7a` | 75 production items, 13 tests |

Baseline evidence:

- release contracts: 154/154
- real-homeserver binary with `qa-bin,test-hooks`: 13/13
- Linux runner syntax, `--list`, child-env keys, artifact root, WebDriver capabilities: green and output hashes recorded
- build-structure contracts: 7/7
- agents-doc guard: green

This is a move-only decomposition. If extraction reveals a behavior defect, stop and repair it as a separate verify-first change.

## Non-negotiable contracts

- Keep the Linux CLI path and package script unchanged. Preserve all 26 scenario names, order, dispatch, success/evidence tokens, child-environment filtering, private-data-free artifacts, FIFO credential transport, WebDriver deletion, process settlement, and ordered local-session teardown.
- Keep `real-homeserver-qa` as the Cargo binary name and preserve scenario parsing, stdout tokens, request correlation, absolute deadlines, credential-store guard, transcript secret/identifier scan, coarse cleanup warnings, startup-latency semantics, and final cleanup on every path.
- Preserve all 154 Vitest names/bodies exactly once. Tests continue to read authoritative source files directly; no compatibility aggregate suite or copied assertions.
- New modules are private implementation files. JavaScript/TypeScript exports are only direct sibling-module seams; Rust uses private `#[path] mod` plus `pub(super)` where necessary.
- No barrel, wildcard import, re-export façade, one-implementation interface, wrapper-only service, duplicate registry, duplicate cleanup/redaction owner, TODO, or dead compatibility path.

## Linux GUI runner ownership

Create `scripts/desktop-linux-gui-qa/`:

- `options.mjs`: the one leaf owner of `repoRoot`, `desktopDir`, `desktopPackageRequire`, `args`, `optionValue`, all immutable CLI-derived values (`guiScenario`, server/profile/login flags, artifact root, timeout), and their pure validation/path derivation. Every consumer imports these live immutable bindings directly; no module imports `main` for configuration.
- `main.mjs`: the existing top-level probe/run/usage composition. It executes on import; wrapping it in a new function is unnecessary. It imports immutable values from `options`, registry dispatch, plus the lower-level probe functions it already calls: child-environment projection from `redaction`, WebDriver capabilities from `webdriver`, QA-title/window projections from `evidence`, and tool checks from `runtime`.
- `registry.mjs`: the one exhaustive scenario/checklist registry and dispatch owner. Move `run`, `checks`, scenario validation, and the direct existing `../lib/sdk-submodule-status.mjs` import together here so dispatch does not bypass the registry; import `repoRoot` from `options`.
- `runtime.mjs`: tool/build/Xvfb/DBus/app/WebDriver process startup and generic final process settlement. The cohesive DBus/process group stays here: `startDbusMonitor`, `triggerNotificationSmoke`, both DBus waiters, `recordProcessOutput`, `terminateProcessGroup`, `settleChild`, and `sleep`.
- `webdriver.mjs`: WebDriver loading/capabilities/session deletion and all generic DOM/action/polling primitives. Neutral room selection/context helpers (`openRoomContextMenu`, room selection/active-room/timeline-mounted diagnostics, section lookup) also stay here because multiple feature scenarios use them.
- `local-session.mjs`: disposable homeserver/users/rooms, local session object, FIFO path creation/writes, deterministic timeline-navigation seed constants/body, `recordLocalGuiEvidence`, and the one ordered local-session teardown. It imports `writeSensitivePayloadToPath` from the existing shared `../lib/sensitive-fifo.mjs`; it must not copy or wrap that writer. Existing local-homeserver helpers remain direct imports from `../lib/local-homeserver-qa.mjs`. It depends directly on options, runtime process settlement, WebDriver session deletion, evidence, and redaction; neither lower owner imports local-session. Scenario code imports the seed helper downward instead of duplicating it.
- `evidence.mjs`: pure QA-title/window/DBus parsing, artifact paths, and private-data-free evidence projection only; it does not start or wait on processes.
- `redaction.mjs`: child environment filtering and captured-output sanitization.
- `scenarios/auth.mjs`, `rooms-timeline.mjs`, `media.mjs`, `settings-security.mjs`: feature-specific scenario bodies and feature-only helpers. Room and timeline scenarios remain one owner because their workspace selection/context helpers are shared; generic DOM pieces still move down to `webdriver`. No scenario imports a sibling.

The root keeps the shebang and imports `main.mjs` for side effects. It contains no second registry, token list, cleanup, or redaction rule.

Dependency direction is:

```text
options + redaction + webdriver + pure evidence projections
  -> runtime
  -> local-session
  -> scenario modules
  -> registry
  -> main
  -> root entrypoint

main -> options, redaction, webdriver, evidence, runtime  # probe-only direct imports
registry/runtime/webdriver/evidence/redaction -> options   # immutable configuration only
local-session -> options, runtime, webdriver, evidence, redaction
```

`options` imports no project module. `runtime`, `webdriver`, `evidence`, and `redaction` may import options but never local-session or scenarios. `local-session` may import options, runtime, webdriver, evidence, and redaction but never scenarios. Scenario modules never import siblings, registry, main, or root. `registry` imports each scenario owner directly and remains the exhaustive dispatch point. “Reverse import” means a lower ownership layer importing a higher layer; `main` directly importing lower-layer probe functions and local-session importing lower teardown primitives are intentional downward edges.

Source-characterization consumers in `releaseScripts` and `scripts/build-structure-contract.test.mjs` must read the owning module or a deterministic list of all production modules. The real-homeserver negative whole-source privacy assertions likewise read its deterministic production-module concatenation rather than only the root. Public CLI probes remain byte-for-byte equivalent.

Mechanical exactness explicitly permits only these source-contract edits: (1) retarget a source read to its owning module or deterministic production-module list, (2) adjust a relative import literal for the new module depth (for example `./lib/sensitive-fifo.mjs` to `../lib/sensitive-fifo.mjs`), and (3) apply a negative whole-source assertion to deterministic module concatenation. These are reviewed source-guard corrections, not product/test behavior changes; all other test bodies remain exact.

## Release contract ownership

Replace the monolithic test with direct Vitest-discovered siblings under `apps/desktop/src/scripts/`:

- `diagnosticSourceScanner.ts`: the single owner of the existing lexical/scope/data-flow diagnostic scanner.
- `diagnosticSourceScanner.test.ts`: 25 scanner/runtime-diagnostic tests.
- `releaseConfiguration.test.ts`: 13 branding, preflight, version, workflow, packaging, Tauri capability, CSP, icon, and storage contracts.
- `headlessAndRealQa.test.ts`: 28 headless and real-homeserver runner/token/privacy contracts.
- `linuxGuiQa.test.ts`: 39 Linux runner, local server, WebDriver, container, token, cleanup, and source-module contracts.
- `macGuiQa.test.ts`: 45 manual/mac environment, login, title, screenshot, evidence, and cleanup contracts.
- `qaTitleAndAppWiring.test.ts`: 4 QA-title and App event-wiring contracts.
- `releaseTestSupport.ts`: only the shared repository-root, subprocess, tracked-file, and source-reading helpers that have multiple consumers.

Each test file imports Vitest directly and is independently runnable. No aggregate `releaseScripts.test.ts` remains. Single-consumer fixtures stay beside their tests; shared setup and source assertions are not duplicated.

## Real-homeserver binary ownership

Create `crates/koushi-core/src/bin/real_homeserver_qa/`:

- `config.rs`: constants, scenario/config parsing, data paths, and deterministic room/message plans.
- `credentials.rs`: credential loading, compile/keychain guards, and transcript privacy validation.
- `event_source.rs`: event-source traits/futures and absolute-deadline primitive.
- `waiters.rs`: request-correlated events, snapshot barriers, timeline/search/pagination waits.
- `cleanup.rs`: cleanup state, catch-all cleanup, logout, and coarse warning tokens.
- `compat_flow.rs`: ordinary compatibility/space/timeline-stress flow.
- `startup_latency.rs`: restore/recovery/startup-latency flow and pagination terminal checks.
- owner-local `*_tests.rs`, with exhaustive cross-owner contracts retained only when genuinely cross-owner.

The root retains crate docs/import namespace, private path modules, `main`, `run`, and the final dispatch/cleanup composition only. Dependency direction is:

```text
config, event_source
  -> credentials
  -> waiters
  -> cleanup
  -> compat_flow, startup_latency
  -> root composition
```

Flow siblings never import each other. Lower layers never import a flow.

## Parallel implementation and integration

Use four isolated worktrees from the immutable base:

1. Linux runner modules and its root façade, using the revised four-scenario-owner graph above.
2. Diagnostic scanner plus its 25 tests.
3. Remaining 129 release/QA contract tests and shared support.
4. Real-homeserver Rust modules and tests.

Workers are mechanical Luna/low and write only their assigned destinations. Workers 2 and 3 stage destination files without independently rewriting the same parent; the integration owner removes the original test file once and resolves imports/source guards. No concurrent writer shares a worktree.

Before integration, each worker proves body hashes/names, symbol counts, duplicate absence, syntax/rustfmt, and `git diff --check`; workers do not run full workspace builds. Integration runs focused owner tests and deterministic inventories before formal review.

## Deterministic verification

- Linux: all 199 baseline declarations exactly once after normalizing direct `export`; the new `options` module introduces no duplicate owner; `timelineNavigationSeedBody` exists only in local-session; 26 scenarios/order/tokens unchanged; no cycle/reverse import; CLI probe output hashes unchanged.
- Release contracts: all 154 test names/bodies exactly once with owner counts `25 + 13 + 28 + 39 + 45 + 4`; scanner support declarations exactly once; no aggregate compatibility file.
- Real binary: all 75 production items exactly once after normalizing `pub(super)` and all 13 tests exactly once; Cargo target bytes unchanged; module graph acyclic.

Focused gates:

```bash
find scripts/desktop-linux-gui-qa -name '*.mjs' -print0 | xargs -0 -n1 node --check
# checkJs integration catches missing/incorrect exports and free identifiers
apps/desktop/node_modules/.bin/tsc --allowJs --checkJs --noEmit --module nodenext --moduleResolution nodenext --target es2022 --skipLibCheck scripts/desktop-linux-gui-qa.mjs scripts/desktop-linux-gui-qa/*.mjs scripts/desktop-linux-gui-qa/scenarios/*.mjs
node --check scripts/desktop-linux-gui-qa.mjs
node scripts/desktop-linux-gui-qa.mjs --list
node scripts/desktop-linux-gui-qa.mjs --check-tools
node scripts/desktop-linux-gui-qa.mjs --child-env
node scripts/desktop-linux-gui-qa.mjs --child-env-keys
node scripts/desktop-linux-gui-qa.mjs --print-artifact-root
node scripts/desktop-linux-gui-qa.mjs --print-real-login-transport
node scripts/desktop-linux-gui-qa.mjs --print-webdriver-capabilities --app-binary=/tmp/koushi
# releaseScripts concern tests also invoke every --qa-title-* and window-state probe
node --test scripts/build-structure-contract.test.mjs
npm --prefix apps/desktop test -- --run src/scripts/diagnosticSourceScanner.test.ts src/scripts/releaseConfiguration.test.ts src/scripts/headlessAndRealQa.test.ts src/scripts/linuxGuiQa.test.ts src/scripts/macGuiQa.test.ts src/scripts/qaTitleAndAppWiring.test.ts
cargo test -p koushi-core --bin real-homeserver-qa --features qa-bin,test-hooks
node scripts/check-agents-docs.mjs
```

After the formal full-diff review, run the full repository local gate once, the applicable Tuwunel Linux GUI lane in its documented container, then CI 7/7. Update all three Issue #551 checkboxes only after merge.
