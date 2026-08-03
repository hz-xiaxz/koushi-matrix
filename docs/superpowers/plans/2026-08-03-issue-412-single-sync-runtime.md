# Issue #412 Single Sliding Sync Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Require Simplified Sliding Sync for every production session, remove Legacy Sync and fallback behavior, and make room-list/timeline readiness depend on a committed Sliding Sync response.

**Architecture:** This is the second sequential PR for Issue #412 and starts only after the QA/parity PR is merged. Rust owns discovery, persisted positive capability evidence, blocked-session recovery, the sole `SyncService`, the sole unfiltered `all_rooms` list, and readiness. React receives only engine-neutral state. The provisional verification phase uses `EncryptionSyncService`, then stops before the normal account `SyncService` starts.

**Tech Stack:** Rust, matrix-rust-sdk, Tokio, serde, Tauri DTOs, TypeScript, Vitest/Playwright, headless-core-qa.

---

## Task 1: Introduce typed Simplified Sliding Sync discovery

**Files:**
- Create: `crates/koushi-sdk/src/sliding_sync_discovery.rs`
- Modify: `crates/koushi-sdk/src/lib.rs`
- Create: `crates/koushi-sdk/tests/sliding_sync_discovery.rs`
- Modify: `crates/koushi-sdk/tests/password_login.rs`

- [ ] Add wiremock tests for `/versions` responses mapping `org.matrix.simplified_msc3575: true` to `Supported`, explicit false/missing to `Unsupported`, transport failure to `Unreachable`, and malformed/non-success responses to `InvalidResponse`.
- [ ] Assert no test sends authenticated invite-only probes and no result uses server fingerprinting.
- [ ] Run `cargo test -p koushi-sdk --test sliding_sync_discovery`; confirm RED because the typed discovery API does not exist.
- [ ] Implement the four-state discovery type and one unauthenticated `/versions` query in the new module. Keep raw response bodies and URLs out of `Debug` and errors.
- [ ] Route password-login discovery through this API and delete the invitation-list behavior probe tests from `password_login.rs` only after replacements are green.
- [ ] Run `cargo test -p koushi-sdk --test sliding_sync_discovery` and `cargo test -p koushi-sdk --test password_login`; confirm exit code 0.
- [ ] Commit with message `feat: add typed sliding sync discovery`.

## Task 2: Model capability gating and positive-cache restoration in Rust state

**Files:**
- Create: `crates/koushi-state/src/state/sliding_sync.rs`
- Modify: `crates/koushi-state/src/state.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/reducer.rs`
- Modify: `crates/koushi-state/src/state/session.rs`
- Create: `crates/koushi-state/tests/sliding_sync_capability.rs`
- Modify: `docs/architecture/state-machine.md`

- [ ] Add reducer tests for Supported advancing login/restore, Unsupported entering a recoverable capability-blocked state, Unreachable/InvalidResponse entering distinct retryable states, and retry clearing only the capability attempt.
- [ ] Add tests that capability blocking never clears credentials, stores, account identity, or positive support evidence and never labels offline restore as unsupported.
- [ ] Add tests that an existing positive cache permits offline stale restore while scheduling revalidation, whereas an absent cache cannot manufacture support.
- [ ] Run `cargo test -p koushi-state --test sliding_sync_capability`; confirm RED.
- [ ] Add engine-neutral capability/discovery state and reducer actions. Do not add React-local recovery flags.
- [ ] Update the normative state-machine diagrams and guards for discovery, capability blocked, offline-positive restore, revalidation, and sign-out.
- [ ] Run `cargo test -p koushi-state --test sliding_sync_capability` and `cargo test -p koushi-state --test session_state`; confirm exit code 0.
- [ ] Commit with message `feat: model required sliding sync capability`.

## Task 3: Persist positive capability evidence and gate every session entry path

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-core/src/runtime.rs`
- Modify: `crates/koushi-core/tests/runtime_session.rs`

- [ ] Add tests that password login and OIDC completion use the same pre-session discovery gate and that Unsupported does not persist a new session.
- [ ] Add store-reopen tests proving positive support evidence round-trips with `PersistableMatrixSession`, old sessions deserialize without it, and no negative result is persisted.
- [ ] Add stored-session tests for positive offline restore, retryable revalidation failure, and Unsupported blocking while preserving persisted credentials/stores.
- [ ] Run the narrow account/session tests and confirm RED.
- [ ] Add backward-compatible positive evidence to persisted sessions and centralize the shared account gate before promotion to Ready.
- [ ] Emit typed capability state events; keep homeserver URL, user id, device id, access token, and raw SDK errors out of diagnostics.
- [ ] Run `cargo test -p koushi-sdk --test password_login`, `cargo test -p koushi-core --lib account`, and `cargo test -p koushi-core --test runtime_session`; confirm exit code 0.
- [ ] Commit with message `feat: require sliding sync before session promotion`.

## Task 4: Replace provisional classic sync with EncryptionSyncService

**Files:**
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/sync.rs`
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-core/tests/runtime_e2ee.rs`

- [ ] Add a lifecycle test proving provisional device verification starts `EncryptionSyncService`, never constructs a classic `/sync` request, and stops/joins before the normal session SyncActor starts.
- [ ] Add recovery tests covering pause/resume around own-user SAS without allowing two encryption-sync owners.
- [ ] Run `cargo test -p koushi-core --lib provisional_verification_uses_encryption_sync_service` and `cargo test -p koushi-core --test runtime_e2ee provisional_verification`; confirm they fail on the current `restricted_sync`/`SyncOnce` implementation.
- [ ] Replace the restricted classic loop with a narrowly owned provisional encryption sync service. Preserve the verification transition semantics and rename lifecycle probes away from `restricted_sync`.
- [ ] Remove production routing of `SyncCommand::SyncOnce`; retain low-level test helpers only behind explicit test/QA compilation when still required by fixtures.
- [ ] Run `cargo test -p koushi-core --lib account` and `cargo test -p koushi-core --test runtime_e2ee`; confirm exit code 0.
- [ ] Commit with message `refactor: use sliding sync for provisional encryption`.

## Task 5: Add a committed all-rooms response observable to the vendored SDK

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/all_rooms.rs`
- Modify: `docs/upstream/matrix-rust-sdk-feedback.md`
- Test: `vendor/matrix-rust-sdk/crates/matrix-sdk-ui/src/room_list_service/mod.rs`

- [ ] Add a failing SDK test proving the observable remains unchanged at `SyncService::State::Running`, advances only after a successful all-rooms response is processed, reports only a process-local monotonic sequence and `pos_present`, and does not expose room ids or the `pos` value.
- [ ] Add a failure/reconnect test proving failed requests do not advance the sequence and a later successful response does.
- [ ] Run `cargo test -p matrix-sdk-ui committed_all_rooms_response_observable`; confirm RED.
- [ ] Add the minimum read-only latest-value observable at the point where all-rooms response handling and event-cache commit have succeeded. Do not add a second sync loop or Koushi product state inside the SDK.
- [ ] Record the additive patch and upstream rationale in the ledger.
- [ ] Run the SDK tests and `node scripts/check-sdk-submodule.mjs`; confirm exit code 0.
- [ ] Commit with message `feat(sdk): expose committed room list response`.

## Task 6: Collapse SyncActor to one SyncService and one all_rooms list

**Files:**
- Modify: `crates/koushi-core/src/sync.rs`
- Modify: `crates/koushi-core/src/account.rs`
- Modify: `crates/koushi-core/src/room.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-state/src/state/sync.rs`
- Modify: `crates/koushi-state/src/state/navigation.rs`
- Modify: `crates/koushi-state/src/reducer.rs`
- Modify: `crates/koushi-core/tests/runtime_room_list_sync.rs`
- Modify: `crates/koushi-core/tests/runtime_session.rs`

- [ ] Add/replace lifecycle tests proving exactly one normal `SyncService`, one `room-list` connection, one unfiltered `all_rooms` list carrying joined and invited rooms, and one encryption connection.
- [ ] Add tests that `Running` alone is not connected, the first committed response plus reconciled all-rooms range is connected, reconnect retains the same engine, and cancellation joins all owners.
- [ ] Add a constructor/compile contract that RoomActor receives a non-optional `RoomListService` and never falls back to `Client::rooms()`/`invited_rooms()` as live truth.
- [ ] Run `cargo test -p koushi-core --lib sync_service_has_one_all_rooms_owner` and `cargo test -p koushi-core --test runtime_room_list_sync`; confirm RED.
- [ ] Remove backend probing, fallback selection, `KOUSHI_QA_FORCE_SYNC_BACKEND`, Legacy loop ownership, and backend transition fences. Start only the mandatory SyncService after capability success.
- [ ] Replace `SyncMode`, `SyncBackendKind`, and `RoomListSource::{SyncService,Legacy}` with engine-neutral lifecycle/readiness state. Do not leave one-variant backend enums.
- [ ] Reconcile cache projections as stale until the all-rooms committed sequence advances and the full loaded range is applied; treat omission from a reconciled live range as removal, but not absence from cache.
- [ ] Run `cargo test -p koushi-core --lib sync`, `cargo test -p koushi-core --test runtime_room_list_sync`, `cargo test -p koushi-core --test runtime_session`, and `cargo test -p koushi-state --test sliding_sync_capability`; confirm exit code 0.
- [ ] Commit with message `refactor: require one sliding sync runtime`.

## Task 7: Remove Legacy timeline provenance and fallback repair

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-sdk/tests/timeline_gap_adapter.rs`
- Modify: `crates/koushi-core/src/timeline.rs`
- Modify: `crates/koushi-core/tests/runtime_timeline.rs`

- [ ] Add tests that every production timeline is event-cache backed, retry identity is engine-neutral, and absent range entries do not synthesize room leave events.
- [ ] Add a source/request inventory test that production Rust cannot construct a classic Matrix `/v3/sync` request.
- [ ] Run `cargo test -p koushi-sdk --test timeline_gap_adapter` and `cargo test -p koushi-core --lib timeline`; confirm RED on backend-tagged checkpoints and Legacy branches.
- [ ] Remove `MatrixCommittedRoomTimelineBackend`, Legacy committed-response constructors, global Legacy fences, and fallback timeline repair. Preserve event-cache lifecycle and bounded gap repair.
- [ ] Make retry/checkpoint keys depend on generation/response sequence/range identity rather than backend.
- [ ] Run `cargo test -p koushi-sdk --test timeline_gap_adapter`, `cargo test -p koushi-core --lib timeline`, and `cargo test -p koushi-core --lib gap_repair`; confirm exit code 0.
- [ ] Commit with message `refactor: remove legacy timeline provenance`.

## Task 8: Remove impossible wire states across Tauri and TypeScript

**Files:**
- Modify: `apps/desktop/src-tauri/src/dto.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/golden/frontend_app_state.json`
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/domain/coreEvents.generated.json`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/test/harnessMain.tsx`
- Modify: `apps/desktop/src/domain/timelineStore.test.ts`

- [ ] Add/update serialization and TypeScript tests so Legacy backend/mode/source tokens are rejected and the four capability results plus engine-neutral readiness round-trip.
- [ ] Run the DTO contract test and TypeScript typecheck; confirm RED while old variants remain.
- [ ] Update Rust DTOs, generated CoreEvent artifact, maximally populated frontend golden, TypeScript mirrors, fake snapshots, app harness, and IPC mock together.
- [ ] Render the Rust-owned capability-blocked state with retry/sign-out/change-homeserver actions; React must not infer capability from network or room data.
- [ ] Regenerate only the frontend golden via `UPDATE_GOLDEN=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden`; edit `coreEvents.generated.json` to match the serialization contract.
- [ ] Run `cargo test -p koushi-desktop core_event_wire_format_matches_checked_in_contract_artifact`, `cargo test -p koushi-desktop frontend_app_state_golden`, `npm --prefix apps/desktop run typecheck`, and `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "sliding sync capability" --workers=1`; confirm exit code 0.
- [ ] Commit with message `refactor: remove legacy sync wire states`.

## Task 9: Update canon and prove the no-Legacy runtime

**Files:**
- Modify: `REPOSITORY_RULES.md`
- Modify: `AGENTS.md`
- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/policies/engineering-rules.md`
- Modify: `docs/architecture/i18n.md`
- Modify: `crates/koushi-core/src/bin/headless-core-qa.rs`

- [ ] Update active canon to require Simplified Sliding Sync, a single all-rooms RoomListService, committed-response readiness, positive-cache restore, and no Legacy fallback or invite-only probe.
- [ ] Keep historical documents only when clearly marked historical/superseded.
- [ ] Run an inventory command over production Rust/events/DTO/TypeScript and confirm no `LegacySync`, forced-backend environment variable, invite-list capability probe, backend mode enum, or production `/v3/sync` constructor remains.
- [ ] Re-run the Tuwunel and Synapse invitation lanes established by PR1. Both must report SyncService/all-rooms success after the Legacy code is gone.
- [ ] Run the negative Synapse fixture and confirm it enters typed Unsupported without sending an authenticated sync request or deleting session/store material.
- [ ] Run `cargo test --workspace`, `cargo test -p koushi-desktop`, `npm --prefix apps/desktop run lint`, `npm --prefix apps/desktop run typecheck`, `npm --prefix apps/desktop run test`, and `node --test scripts/build-structure-contract.test.mjs`, preserving each command's own exit status.
- [ ] Self-review `git diff origin/main...HEAD` and `git status --short`, including generated artifacts and submodule changes.
- [ ] Commit with message `docs: require simplified sliding sync runtime`.
- [ ] Push, open PR2 as ready for review, monitor checks, fix failures, enable auto-merge, and wait for merge before starting `2026-08-03-issue-412-diagnostics-cleanup.md`.
