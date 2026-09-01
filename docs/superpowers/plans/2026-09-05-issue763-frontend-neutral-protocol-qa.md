# Issue #763 — Frontend-Neutral Protocol And QA Isolation

Status: implemented and locally verified; pending exact-final-diff review and hosted CI.

Base: `origin/main` `941f7f40eaf53a55f6aaedb8829b073e9ea5c795`.

Selected independent reviewer: Fireworks `reviewer-flash` (read-only, different model family from the planned Luna implementation worker).

## Outcome

Complete #763 in one independently mergeable PR with separately reviewable commits:

1. Add a zero-SDK, zero-Tauri, zero-platform-dependency `koushi-protocol` crate that owns the public Rust command, event, identity, failure, command-admission, versioned-snapshot and state-delta DTOs.
2. Keep actor lifecycle, command admission/routing policy, event projection, state-delta construction, SDK handles and async runtime machinery in `koushi-core`.
3. Make Tauri and QA consume protocol DTOs directly rather than importing them through runtime implementation modules. `koushi-core` may re-export the stable top-level DTOs used in public runtime signatures, but no old `command`/`event`/`ids`/`failure` compatibility module remains.
4. Move both authoritative QA binary trees to a dedicated `koushi-qa` package while preserving binary names, CLI forms, scenario names, evidence tokens, cleanup, privacy rules and CI coverage.
5. Remove the Tauri `koushi-thumbnail://` scheme from Core/state/protocol. Core projects an opaque renderable-thumbnail reference; adapters choose a URI or consume bytes.
6. Remove filesystem paths and QA-only Tokio oneshots from the public command DTOs so the extracted protocol is genuinely transport-neutral.

This is an ownership/move change. It does not change Matrix behavior, user-visible workflow, scenario semantics, evidence tokens, retry policy or reducer transitions.

## Recon Evidence

At the pinned base:

- Public boundary source is split across `koushi-core/src/{command,event,failure,ids,state_delta}.rs` and command/event submodules (about 6,900 lines).
- Identity and failure DTOs use only serde plus `koushi-state::AuthFailureKind`.
- Event DTOs use only `koushi-state`, serde, protocol-local identities/failures and `serde_json::Value`; Core-only display-label/action/source helpers are mixed into the same files.
- `StateDelta` and `StateDeltaChangedSlices` are DTOs, while `build_state_delta`, slice auditing and sidebar composition are Core projection logic.
- Command DTOs originally depended on `koushi-key::SessionKeyId`, but a focused wasm check proved that depending on the complete key crate pulls `rand/getrandom` and fails `wasm32-unknown-unknown`. `SessionKeyId` therefore moves with the other protocol identities; credential account-name derivation remains a `koushi-key` extension trait. Four native-artifact request structs contain `PathBuf`. Two QA-only Account variants (`QaSetLocalDeviceBlacklisted`, `QaRefreshDeviceKeysAndAssertKnown`) contain Tokio oneshot senders; the separate test-gated `SyncOnce` variant carries only `RequestId`.
- `CoreCommand::{composer_draft_scope,requires_ready_session}` and `AccountCommand::requires_ready_session` are Core admission policy, not protocol shape.
- `CommandSubmitError`, `EventStreamLag`, request-outcome services, composer lease registries and actor messages are runtime types and must not move.
- Core currently stores a `koushi-thumbnail://localhost/<kind>/<hash>` string in `AvatarThumbnailState::Ready.source_url`; Tauri only looks bytes up after receiving that Core-minted URL.
- `headless-core-qa` and `real-homeserver-qa` comprise 30 files under `koushi-core/src/bin`, with 88 current headless binary tests. Both targets are feature-gated by `qa-bin`, and runners/CI select them with `-p koushi-core`.
- Baseline passed: SDK-submodule guard; `cargo metadata --no-deps`; command/event redaction and `runtime_core` integration tests (23); headless QA binary tests (88).

## Canon Reconciliation

Current canon still says `koushi-core` owns headless QA binaries and its layer diagram has no protocol/QA crate. Amend canon before implementation:

- `koushi-protocol` owns transport-neutral public Rust DTOs and has no SDK, Tauri, async-runtime, filesystem/platform or OS dependency.
- `koushi-core` remains the only production runtime owner and depends on the protocol; it owns policy and projection implementations, never adapter wire wrappers.
- `koushi-qa` owns authoritative product QA orchestration and depends on Core test hooks. It is not a production/default package and contains no product implementation.
- Tauri remains the transport/platform adapter. Tauri-specific URI construction and native-path registration stay there.
- Commands are transport-neutral typed Rust inputs; secret-bearing command aggregates are deliberately not wholesale serde payloads. Each adapter validates/deserializes its own IPC request and constructs the protocol command. Events, identities, failures and state-update DTOs preserve their existing safe serde contracts.
- A future native frontend may consume protocol DTOs plus Core thumbnail bytes without inventing a URI scheme.

Update `REPOSITORY_RULES.md`, `docs/architecture/overview.md`, `docs/policies/engineering-rules.md`, `docs/agents/{verification,qa-lanes,state-ownership}.md`, and the plan index. No reducer/state-machine transition changes are planned; `state-machine.md` changes only if implementation discovery finds a current crate-ownership claim there.

## Crate And Dependency Contract

### `koushi-protocol`

Allowed direct dependencies:

- `koushi-state`
- `serde`, `serde_json`, and `thiserror` where already required by moved DTOs

Move `SessionKeyId` from `koushi-key` into protocol identities with a custom redacted `Debug` and full-value synthetic leak test; homeserver, user and device identifiers must not become artifact-facing output. `koushi-key` depends on protocol and implements a `SessionKeyIdCredentialNames` extension trait for `account_name`, `local_unlock_account_name`, and `matrix_session_account_name`, preserving their exact URL-safe-base64 storage contract. No credential naming, random generation, key derivation, cryptography, `rand`, or `getrandom` moves into protocol, and no compatibility re-export remains in `koushi-key`. This dependency direction is acyclic and keeps protocol wasm-clean.

Move these DTO families:

- `ids`: `RuntimeConnectionId`, `RequestId`, `AccountKey`, `SessionKeyId`, `TimelineKey`, `TimelineKind`, `TimelineGeneration`, `TimelineBatchId`;
- `failure`: all public `CoreFailure` and typed failure-kind enums;
- `command`: `CoreCommand`, `AccountCommand`, `AppCommand`, `SyncCommand`, `RoomCommand`, `SearchCommand`, `ThreadsListCommand`, `TimelineCommand`, and their payload/value types;
- `event`: `CoreEvent` and every public nested account/activity/attention/live-signal/room/search/timeline event/value DTO;
- `state_update`: `CoreCommandAdmission`, `AppStateSnapshot`, `VersionedAppStateSnapshot`, `StateDelta`, and `StateDeltaChangedSlices`.

Preserve every serde attribute, enum representation, field name, custom redacted `Debug`, numeric representation and privacy property unless this plan explicitly changes the thumbnail reference. Do not incidentally normalize enum casing.

The protocol source contains data-shape helpers only (for example stable identity accessors and bounded image-dimension calculations). It contains no AppState-dependent projection, SDK call, routing/admission policy, actor handle, Tokio channel, filesystem path, Tauri URI or QA-only variant.

### Core implementation remains

- Move command routing/admission helpers to `koushi-core::command_policy` as free functions or a Core-local extension trait over protocol commands.
- Move display-label, permalink, message-action and message-source projection helpers to `koushi-core::event_projection`.
- Keep `build_state_delta` and slice auditing in Core, importing protocol delta DTOs.
- Keep `CommandSubmitError`, `EventStreamLag`, request outcome, connection handles, leases, media staging and runtime envelopes in Core.
- Update internal imports to `koushi_protocol`; delete the old DTO-defining Core modules instead of leaving module-path shims.
- Keep deliberate top-level `koushi_core::{CoreCommand,CoreEvent,...}` re-exports only where they are part of `CoreRuntime`'s public ergonomic API. Tauri and QA use `koushi_protocol` directly for DTO imports, proving adapters do not depend on runtime implementation modules.

## Native Artifact Command Boundary

The extracted command must not carry `PathBuf`. Add one narrow Core native-artifact path port; do not add a generic platform framework.

- Protocol request structs retain secret/policy data and carry only whether an optional artifact is requested. They carry no path or adapter object.
- The existing `RequestId` keys one exact native artifact registration plus a closed kind (`RoomKeyExportDestination`, `RoomKeyImportSource`, `RecoveryKeyDestination`).
- Before command submission, Tauri registers the user-selected `PathBuf` in its native-artifact port under the exact request/kind. The command contains only `RequestId`, the closed kind/required boolean and existing secret data.
- AccountActor resolves and consumes that registration immediately before the existing SDK file API. Missing, mismatched or already-consumed registrations fail closed through the existing typed operation-failure path and perform no SDK/file effect.
- Submission failure unregisters the path; successful take removes it; adapter/runtime drop clears all remaining registrations. Normal Debug exposes only kind/presence, never path or secret.
- `koushi-qa` injects a temp-path port through Core test hooks for scenarios that exercise these flows. Default Core uses a rejecting port.

This port preserves existing SDK Matrix key-export format and avoids reimplementing or exposing room-key bytes. It also repairs the existing canon contradiction that claimed public commands contained no filesystem types.

## QA-Only Runtime Controls

Public protocol enums have one production shape and no `qa-bin`/Tokio variants.

- Add an internal Core runtime command envelope that distinguishes protocol commands from a private `CoreQaCommand` available under `test-hooks`.
- Replace `QaSetLocalDeviceBlacklisted` and `QaRefreshDeviceKeysAndAssertKnown` with narrowly named `CoreConnection`/QA test-handle methods that send the private command and await their existing oneshot acknowledgements. Replace `SyncOnce` with a separate test-hook submission whose completion remains proven by its existing event/snapshot observation; do not invent an acknowledgement channel.
- `koushi-qa` enables `koushi-core/test-hooks`; ordinary Core/Tauri builds do not compile the QA control surface.
- Do not broaden private actor/test APIs merely to make the moved source compile. Add one narrow Core QA support module for genuinely cross-package hooks; keep single-consumer fixtures in `koushi-qa`.

## Renderable Thumbnail Boundary

Replace Core's platform URI with an opaque cache reference.

- Rename Rust/TypeScript `AvatarThumbnailState::Ready.source_url` to `source_ref` and bump `SNAPSHOT_SCHEMA_VERSION` from 5 to 6. The state-update envelope version remains 1 because envelope ordering/generation semantics are unchanged.
- Core cache keys remain bounded, session-scoped, non-secret hashes such as `avatar/<hash>` or `link-preview/<hash>`. Core stores/projects only that reference.
- `lookup_renderable_thumbnail` accepts the opaque reference directly and returns bytes/MIME. It never parses or emits a Tauri scheme.
- Extend the existing `LinkMediaPort` with `renderableThumbnailSourceUrl(sourceRef)`. The Tauri implementation validates the closed reference shape and mints `koushi-thumbnail://localhost/<ref>`; the browser fixture implementation accepts explicit data/blob fixture references without adding product semantics.
- Avatar/link-preview renderers call this adapter method. Ordinary downloaded-media `mediaSourceUrl` behavior is unchanged.
- The Tauri custom-protocol handler converts request paths back to the validated opaque ref and retrieves Core bytes. CSP keeps the Tauri scheme because the desktop adapter still uses it.
- Update Rust/TypeScript golden contracts and browser fixtures. No compatibility acceptance of the old field/scheme is retained: Rust and frontend ship together, schema-v6 mismatch fails through the existing recovery gate.

## `koushi-qa` Package

Add `crates/koushi-qa` as a workspace member but not a default member.

- Preserve binary names `headless-core-qa` and `real-homeserver-qa` and their `required-features = ["qa-bin"]` contract.
- Move both root files and complete module trees without renaming scenarios or tokens.
- `koushi-qa/qa-bin` enables optional QA dependencies and `koushi-core/test-hooks`.
- Remove Core's two bin declarations, source trees and `qa-bin` feature after every QA-only Core cfg has either become `test-hooks`, moved to `koushi-qa`, or been deleted with its obsolete command variant.
- Update `desktop-headless-local-qa.mjs`, `desktop-real-homeserver-qa.mjs`, CI, release support and structural/source-path tests from `-p koushi-core`/Core paths to `-p koushi-qa`/QA paths. In the CI Rust job, extend the wasm command from `-p koushi-state -p koushi-search` to `-p koushi-state -p koushi-search -p koushi-protocol`. Keep npm script names and external CLI forms unchanged.
- Keep the authoritative CI job label `Core QA binary tests`; only its package command changes.

## Verify-First Gates

Before production moves, add RED structural tests that fail on the current tree:

1. `scripts/check-protocol-qa-boundaries.mjs` requires both crates, the dependency allowlist, protocol DTO ownership, no protocol SDK/Tauri/Tokio/path/QA variant, no Core QA trees, and no Core `koushi-thumbnail://` literal.
2. A script self-test builds synthetic valid and invalid fixture trees and proves each forbidden edge/path/definition is detected.
3. Extend the domain/platform dependency checker to include `koushi-protocol` and Matrix SDK/Tauri/runtime dependencies.
4. Add a Tauri/Rust contract test for opaque thumbnail ref -> adapter URI -> byte lookup, initially failing while Core mints the URI.
5. Preserve the existing command/event redaction, runtime boundary and QA-binary tests as behavior baselines. The same tests move or change package path and remain green.

Source guards supplement, not replace, compile/behavior tests.

## Implementation Phases

### Phase A — Canon, plan and RED structure

- Land canon responsibility changes before code.
- Add protocol/QA boundary checker and self-tests.
- Record current metadata/dependency edges and exact baseline commands.

### Phase B — Protocol identities, failures, events and state updates

- Add `koushi-protocol` and move the low-coupling DTO families first.
- Split Core event projection and state-delta construction from moved shapes.
- Update direct adapter imports and exact wire/golden tests.

### Phase C — Commands and native artifacts

- Add the narrow native-artifact port and remove `PathBuf` from commands.
- Add private Core QA commands and remove QA-only variants from protocol.
- Move command DTOs; split Core admission/routing policy.
- Prove redacted Debug and unchanged command outcomes.

### Phase D — Thumbnail reference cutover

- Establish opaque refs, adapter URI minting, schema-v6 mirrors and focused Rust/Tauri/React/browser tests.
- Confirm the Core/protocol source contains no Tauri scheme or DOM-paint vocabulary.

### Phase E — QA package move

- Move both binary trees and package dependencies.
- Update all runners, path-sensitive tests, docs and CI commands.
- Run the exact moved QA test command and focused local homeserver smoke without changing tokens.

### Phase F — Full verification and merge

- Run local CI-equivalent gates, exact diff self-review, Fireworks final-diff review, hosted eight-job CI, merge, close #763 and update #749.

## Verification Matrix

Read each command's own exit status:

```bash
cargo metadata --no-deps --format-version 1
cargo fmt --all -- --check
cargo test -p koushi-protocol
cargo test -p koushi-core --test command_redaction --test event_redaction --test runtime_core
cargo test -p koushi-core --lib
cargo test -p koushi-qa --features qa-bin --bin headless-core-qa
cargo test -p koushi-qa --features qa-bin --bin real-homeserver-qa
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --target wasm32-unknown-unknown -p koushi-state -p koushi-search -p koushi-protocol
node --test scripts/check-protocol-qa-boundaries.test.mjs
node scripts/check-protocol-qa-boundaries.mjs
node scripts/check-domain-crate-platform-deps.mjs
node scripts/check-rust-test-structure.mjs
node scripts/check-command-snapshot-contract.mjs
node scripts/check-agents-docs.mjs
node scripts/check-sdk-submodule.mjs
npm --prefix apps/desktop audit --package-lock-only --audit-level=high
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test -- --run
npm --prefix apps/desktop run build
(cd apps/desktop && npx playwright test)
cargo deny check
cargo machete
```

Also run the exact generated Core-event/frontend-state golden checks, Tauri adapter boundary, diagnostic isolation, secret scan, build-structure contract and at least one focused local `headless-core-qa` Tuwunel scenario through the unchanged npm runner. Hosted CI must pass all eight jobs on the reviewed exact head, including both homeserver invitation jobs and platform checks.

## Review Gate

No implementation starts until Fireworks `reviewer-flash` returns `CORRECT-TO-IMPLEMENT` for this document and the canon amendment. Every finding is fixed and re-reviewed. After implementation the same different-family reviewer reads the complete exact diff and returns `CORRECT-TO-MERGE` before PR creation.

## Acceptance Mapping

- Adapters consume protocol without runtime internals: direct `koushi-protocol` imports plus the existing `CoreRuntime` handle only; checker and compile tests enforce it.
- Protocol has no SDK/Tauri/platform dependency: manifest/source checker, cargo tree/metadata audit and wasm build.
- Production Core does not compile QA trees: source trees and bin targets are absent from Core; QA package is non-default and feature-gated.
- Existing QA commands/tokens/scripts/CI continue: same binary names/npm commands/scenario registry, moved binary tests, focused local server smoke and hosted jobs.
- Core does not mint a Tauri custom URI: opaque-ref behavior test and source checker; only Tauri adapter/CSP contains the scheme.
- Existing behavior/privacy remains: redaction tests, exact wire goldens, runtime tests, full workspace/frontend/browser/CI gates.

## Implementation Discovery Amendment

The first command-extraction wasm check failed before landing because the initially approved `koushi-protocol -> koushi-key` edge transitively compiled `rand/getrandom`, whose default backend rejects `wasm32-unknown-unknown`. Do not enable a JavaScript RNG feature merely to make a DTO crate compile. The dependency correction above moves only `SessionKeyId` into protocol and inverts the edge; all credential naming and crypto remain in `koushi-key`. Implementation resumes only after the same independent reviewer approves this amendment.

## Non-Goals

- No generic actor, transport, platform or serialization framework.
- No browser/WebWorker runtime implementation.
- No Matrix behavior, reducer transition, QA scenario/token or user-visible workflow change.
- No incidental serde casing cleanup or backward-compatibility shim for schema v5.
- No move of StoreActor/search/media actor ownership scheduled for #765 beyond the exact native-artifact and thumbnail seams required here.
