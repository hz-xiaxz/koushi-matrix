# Space Header and Invite History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship #405 and #407 together: make the Space header predictably two rows, and make invite-time room history visibility discoverable, independently editable, state-preserving across Room Info navigation, and explicitly enabled for MSC4268 history sharing.

**Architecture:** Keep Matrix semantics and invite drafts Rust-owned. Extend `InviteWorkflowState` with a validated selected scope and a coarse `InviteHistoryPolicy`; route the existing invite batch through that state. Split Room Info access/history presentation and saves while retaining the existing `RoomSettingChange` command boundary. Add only the explicit Matrix SDK builder flag; do not modify `vendor/matrix-rust-sdk`.

**Tech Stack:** Rust (`koushi-state`, `koushi-core`, `koushi-sdk`), Tauri DTO/commands, React/TypeScript, Vitest/jsdom, Playwright browser-headless QA, serde JSON contract fixtures.

---

## 1. Establish the failing contracts

- [ ] Add Rust state tests in `crates/koushi-state/tests/invite_workflow_state.rs` for:
  - history policy projection for encrypted and unencrypted rooms;
  - `recoveryRequired` for a locked/unverified encrypted session;
  - selected scope staying selected when a workflow is reopened with the same plan;
  - invalid scope falling back to the plan default;
  - query and selected targets surviving the workflow refresh.
- [ ] Add component test cases in `apps/desktop/src/components/Shell.test.tsx`, `apps/desktop/src/components/RoomInfoPanel.test.tsx`, and `apps/desktop/src/components/dialogs.test.tsx` for the new DOM/grouping, independent save callbacks, history copy/warnings, and invite navigation callbacks.
- [ ] Add a browser-headless regression in `apps/desktop/e2e/basic-operations.spec.ts` for opening invite, navigating to Room Info, returning, and observing the same query/selected targets/scope.
- [ ] Add a focused SDK contract test in `crates/koushi-sdk/src/lib.rs` that is red until the desktop builder source explicitly contains `.with_enable_share_history_on_invite(true)` and the existing invite helper remains present.
- [ ] Run the focused RED commands and record the failing assertions before changing production code:
  - `cargo test -p koushi-state --test invite_workflow_state`
  - `npm --prefix apps/desktop run test -- --run src/components/Shell.test.tsx src/components/RoomInfoPanel.test.tsx src/components/dialogs.test.tsx`
  - `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "invite|history|Space header" --workers=1`
  - `cargo test -p koushi-sdk --lib desktop_client_builder_defaults`

## 2. Implement #405 Space header layout

- [ ] In `apps/desktop/src/components/Shell.tsx`, wrap the existing Space name in a title-row element and all existing action buttons in one action-row element without changing button order, labels, handlers, or focus order.
- [ ] In `apps/desktop/src/styles.css`, make `.workspace-header` a two-row grid with `min-width: 0`, a truncating title row, and a non-wrapping action row that remains a single group at supported narrow widths.
- [ ] Make the new Shell component test green, including the narrow-width DOM/layout contract.
- [ ] Commit the isolated fix as `fix: keep space header actions on one row`.

## 3. Add Rust-owned invite policy and scope state

- [ ] In `crates/koushi-state/src/state/invite_workflow.rs`, add serializable `InviteHistoryPolicy` and coarse `InviteHistoryReadiness` types, `selected_scope`, and `history_policy` to `InviteWorkflowState`, with serde defaults for backward-compatible snapshots.
- [ ] Add a pure policy builder using the Rust room summary, loaded room settings, room permissions, encryption flag, and coarse session readiness. Keep IDs and raw SDK data out of diagnostics and public failure values.
- [ ] Add `InviteScopeSelected` to `crates/koushi-state/src/action.rs`, route it in `crates/koushi-state/src/reducer/mod.rs`, and implement validation/preservation in `crates/koushi-state/src/reducer/invite_workflow.rs`.
- [ ] Add `SetInviteScope` to `crates/koushi-core/src/command.rs` and its runtime handling in `crates/koushi-core/src/runtime.rs`.
- [ ] Add Tauri builder/command wiring in `apps/desktop/src-tauri/src/commands/mod.rs`, `apps/desktop/src-tauri/src/commands/room.rs`, and `apps/desktop/src-tauri/src/lib.rs`, plus `setInviteScope` to `apps/desktop/src/backend/client.ts` and `apps/desktop/src/backend/browserFakeApi.ts`.
- [ ] Update all Rust/TypeScript DTO mirrors and fixture builders required by the new state shape: `apps/desktop/src-tauri/src/dto.rs`, `apps/desktop/src/domain/types.ts`, browser fakes, app harness/Tauri IPC mocks, `apps/desktop/src-tauri/tests/golden/frontend_app_state.json`, and `apps/desktop/src/domain/coreEvents.generated.json` if the checked contract changes.
- [ ] Run the focused Rust state and contract tests until green:
  - `cargo test -p koushi-state --test invite_workflow_state`
  - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact`
  - `UPDATE_GOLDEN=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden`
  - `npm --prefix apps/desktop run typecheck`

## 4. Split Room Info access/history controls

- [ ] In `apps/desktop/src/components/RoomInfoPanel.tsx`, add an independent access/history section that renders the current value without edit permission, explains `shared`/`invited`/`joined`, marks `worldReadable` as advanced and non-member-visible, and explains that join rule and history visibility are independent.
- [ ] Give join rule and history visibility separate forms and save buttons. Each form dispatches only its own `RoomSettingChange`; controls and save buttons are disabled when `can_edit_settings` is false.
- [ ] Add encrypted/shared past-key sharing and non-retroactivity copy, plus a Recovery link when the Rust policy says readiness is deficient.
- [ ] Add the exact English/Japanese catalog entries in `apps/desktop/src/i18n/messages.ts` and any required style rules in `apps/desktop/src/styles.css`.
- [ ] Extend `RoomInfoPanel.test.tsx` with independent callback assertions, disabled read-only assertions, all four visibility descriptions, encrypted/shared warning, and Recovery navigation.
- [ ] Run `npm --prefix apps/desktop run test -- --run src/components/RoomInfoPanel.test.tsx` and the IME gates because Room Info still contains text controls:
  - `node --test scripts/check-ime-text-inputs.test.mjs`
  - `node scripts/check-ime-text-inputs.mjs`
  - `npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx`

## 5. Make invite history discoverable and navigation-safe

- [ ] In `apps/desktop/src/components/dialogs.tsx`, render the three normal history choices from the Rust policy, current selection, plain-language descriptions, world-readable warning, encrypted/shared note, non-retroactivity note, readiness warning, and a Room Info navigation action. Keep the existing search input and target order intact.
- [ ] In `apps/desktop/src/App.tsx`, load the room settings before opening invite, read/write `workflow.selected_scope` through the new API, and split “hide invite dialog for Room Info” from “close invite workflow.” Preserve query, selected targets, and scope until the user explicitly cancels or completes the invite.
- [ ] In `apps/desktop/src/components/rightPanel.tsx`, pass Room Info callbacks for returning to the active invite workflow and opening Recovery without changing unrelated panel modes.
- [ ] Update `apps/desktop/src/backend/browserFakeApi.ts`, related mocks, and component/browser tests so the fake state follows the production Rust-shaped workflow.
- [ ] Make the dialog and browser-headless navigation tests green:
  - `npm --prefix apps/desktop run test -- --run src/components/dialogs.test.tsx src/components/RoomInfoPanel.test.tsx`
  - `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "invite|history" --workers=1`

## 6. Explicitly enable MSC4268 history sharing

- [ ] In `crates/koushi-sdk/src/lib.rs`, add `.with_enable_share_history_on_invite(true)` to `desktop_client_builder_defaults` without changing the vendored SDK.
- [ ] Keep `invite_user_by_id`/the existing room invite helper as the only invite operation path and add a source/behavior contract test covering that boundary.
- [ ] Run `cargo test -p koushi-sdk --lib desktop_client_builder_defaults` and the focused SDK tests.

## 7. Integrated verification and self-review

- [ ] Run the complete focused gates with real exit-status capture:
  - `node scripts/check-sdk-submodule.mjs`
  - `npm --prefix apps/desktop run lint`
  - `npm --prefix apps/desktop run typecheck`
  - `cargo test -p koushi-state --test invite_workflow_state`
  - `cargo test -p koushi-core --lib`
  - `cargo test -p koushi-sdk --lib`
  - `npm --prefix apps/desktop run test -- --run src/components/Shell.test.tsx src/components/RoomInfoPanel.test.tsx src/components/dialogs.test.tsx`
  - `npm --prefix apps/desktop exec -- playwright test e2e/basic-operations.spec.ts -g "invite|history|Space header" --workers=1`
- [ ] Review `git diff origin/main...HEAD` and `git status --short`, including every new file, against `REPOSITORY_RULES.md`, architecture docs, the design spec, privacy constraints, and DTO mirror requirements.
- [ ] Commit any final contract/fixture corrections, push `codex/issues-405-407-batch`, and open one PR that references both issues with `Closes #405` and `Closes #407` only after both acceptance sets are complete.
