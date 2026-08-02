# Room-list readiness and cold-start projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prevent an unproven empty room snapshot from replacing a useful cached/provisional room list during cold start, while exposing explicit Rust-owned loading/ready/failed readiness and fencing stale backend generations.

**Architecture:** Add a serializable room-list readiness/source/generation projection to `koushi-state`. Room-list lifecycle actions carry the backend generation and authoritative/provisional distinction; the reducer preserves the last usable snapshot while loading, accepts an authoritative zero only for the current generation, rejects stale generations, and emits crawler availability only after readiness. `RoomActor` owns the observed generation and keeps the SyncService observer's initial empty Reset provisional until `SyncActor` proves connectivity; legacy first-response and current-generation failure paths settle the same state machine. Tauri, browser fakes, desktop modeling, and Shell rendering mirror the Rust projection without deriving readiness locally.

**Tech Stack:** Rust (`koushi-state`, `koushi-core`), serde DTOs, Tauri snapshot deltas, TypeScript/React, Vitest/Playwright, focused Cargo tests.

---

## 1. Amend the canonical state-machine contract

- [x] Extend `docs/architecture/state-machine.md` with the room-list lifecycle diagram and guards: `Uninitialized -> Loading -> Ready/Failed`, provisional cache retention, current-generation authoritative empty replacement, stale-generation rejection, and crawler admission only from `Ready`.
- [x] Update the relevant RoomActor/SyncActor bullets in `docs/architecture/overview.md` to document the one live observer, connectivity proof boundary, and preservation across SyncService-to-legacy fallback.
- [x] Self-review the canon edits against issue #409 and run `git diff --check` before code changes.

## 2. Add RED headless reducer/state coverage

- [x] Add focused `koushi-state` tests for default uninitialized readiness, loading with cached rooms preserving the existing projection, loading with an unproven empty snapshot not clearing rooms, current-generation authoritative empty becoming ready, failed readiness retaining the last usable rooms, and stale generation updates being ignored.
- [x] Add a reducer test proving `NotifySearchCrawlerRoomsAvailable` is absent before readiness and emitted once the current generation becomes ready.
- [x] Run the focused state test command and record the expected failure before implementing production behavior.

## 3. Implement the Rust-owned room-list contract

- [x] Add `RoomListReadiness`, `RoomListSource`, generation, and coarse failure tokens to `crates/koushi-state/src/state`; keep serialization and debug output private-data-free.
- [x] Add lifecycle actions in `crates/koushi-state/src/action.rs` and reducer handlers in `crates/koushi-state/src/reducer/room.rs`/`mod.rs` for bootstrap start, provisional snapshot, authoritative snapshot, and failure.
- [x] Keep existing room-list filters/sorts as projection concerns, but make recomputation preserve the readiness metadata and never turn an unproven empty snapshot into a ready empty list.
- [x] Ensure logout/session clear resets readiness and clears cached room projections through the existing reducer transitions.
- [x] Run `cargo test -p koushi-state --lib` with the focused filter and then the relevant integration test binary.

## 4. Fence SyncService, legacy fallback, and RoomActor observations

- [x] Add the observation generation/source to `RoomMessage::SyncStarted` and to the RoomActor's active observation state; increment/fence each backend handoff and ignore delayed projection sends from retired observers.
- [x] Have RoomActor enter loading on backend start, retain non-empty cache/provisional entries when available, and hold an initial unproven SyncService empty Reset until the readiness proof arrives.
- [x] Relay the SyncService room-list connectivity proof and legacy first successful response to the current RoomActor generation; reproject the current entries when proof arrives so an authoritative zero is accepted.
- [x] Preserve the last usable list across SyncService failure/fallback, settle current-generation failed readiness without clearing it, and prevent stale old-backend projections from replacing the new backend snapshot.
- [x] Add deterministic `koushi-core` tests for SyncService Running-before-proof, empty Reset, proof/authoritative zero, failure, legacy delayed first success, and stale generation rejection. Keep diagnostics to source/stage/tokens/counts/timing/generation only.
- [x] Gate `NotifySearchCrawlerRoomsAvailable` on the Rust readiness state and add/adjust the account/search tests for no crawler admission before bootstrap readiness.
- [x] Run the focused core tests with `--lib` and the SDK submodule guard before proceeding.

## 5. Mirror readiness through Tauri and browser contracts

- [x] Add the readiness field to `apps/desktop/src-tauri/src/dto.rs`, changed-slice projection, serialization contract tests, and the maximally-populated golden fixture.
- [x] Mirror the exact tagged DTO in `apps/desktop/src/domain/types.ts`, `apps/desktop/src/backend/browserFakeApi.ts`, app harness snapshots, and browser fake tests; update `roomListProjection.ts` only as a test fixture mirror, never as a product-semantic source of readiness.
- [x] Run the relevant DTO golden/core-event contract tests, `npm --prefix apps/desktop run typecheck`, and the browser fake tests.

## 6. Render loading without local state invention

- [x] Add localized loading text in `apps/desktop/src/i18n/messages.ts` for the room-list bootstrap state in every supported catalog.
- [x] Keep `apps/desktop/src/domain/desktopModel.ts` as the Rust-projection consumer and update `apps/desktop/src/components/Shell.tsx` so loading/failed readiness renders a stable loading/error status and does not present an authoritative `0` count; ready state renders the Rust-owned sidebar data unchanged.
- [x] Add a browser-headless Shell regression test covering the loading-to-ready transition; existing sidebar/model fixtures cover ready empty and populated states. Keep React state limited to presentation controls.
- [x] Run the IME inventory gates (the Shell change must not add text inputs), focused Shell tests, and the required TypeScript lint/typecheck.

## 7. Integrated verification, self-review, and publication

- [x] Initialize and verify the exact Matrix SDK gitlink with `git submodule update --init --recursive vendor/matrix-rust-sdk` and `node scripts/check-sdk-submodule.mjs`.
- [x] Run the final focused Rust, DTO, browser-headless, and type/lint gates with explicit exit-status capture; no dedicated local scenario currently exercises readiness, so the deterministic Rust/UI regressions are the integrated gate.
- [x] Read `git diff origin/main...HEAD` plus all untracked files, run `git status --short` and `git diff --check`, and resolve any canon/contract/privacy issue found in self-review.
- [ ] Commit the implementation with an issue reference, push `codex/issue-409`, open the PR against `main`, and enable squash auto-merge under the user's standing approval.
- [ ] Monitor PR checks and merge state; fix only failures caused by this change, rerun the affected gate, and report the final PR/merge result.
