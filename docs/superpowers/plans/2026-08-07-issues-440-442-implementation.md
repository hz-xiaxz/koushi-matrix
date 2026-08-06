# Issues #440 and #442 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reliably re-share the current Megolm outbound session after encrypted sends, and eliminate IME/diagnostic render churn, in one ready pull request for issues #440 and #442.

**Architecture:** Extend the vendored Matrix SDK's existing room-key sharing path with one forced-current-session mode that preserves all existing recipient eligibility checks while bypassing only the already-shared-device filter. Core observes successful sends at the existing terminal handoff, schedules three bounded actor-owned attempts through the account scheduler, and routes manual requests through the same typed result. The desktop editor suppresses intermediate composition publication, while diagnostic entries move from React state to a fixed-capacity ref-owned ring buffer and wrong-key events retain only the existing aggregate counter.

**Tech Stack:** Rust 2024, Tokio, vendored matrix-rust-sdk, Tauri 2, TypeScript, React 19, Vitest/Testing Library, Cargo tests.

## Global Constraints

- Start from `origin/main` in `.worktrees/issues-440-442`; do not modify the user's main checkout or `HANDOFF.md`.
- Deliver #440 and #442 in one branch and one ready PR; merge with a merge commit, never squash.
- Build the reproducing headless/unit check before each fix and observe RED before GREEN.
- Do not add dependencies, persisted retry state, plaintext room-key material, raw room/user/device identifiers in diagnostics, or a general profiler/telemetry framework.
- Forced re-share must reuse existing device trust/history-visibility eligibility, never create or rotate an outbound session, never duplicate a pending to-device share, and re-check the expected session at execution time.
- Automatic attempts are exactly own-other-devices near 3 seconds, peer devices near 5 seconds, and own-other-devices near 15 seconds after the first successful send observed for a session; repeated sends for that session do not add attempts.
- Automatic work is bounded to those three attempts, lives only for the current account actor generation, and is cancelled on manager/account shutdown.
- Manual re-share uses the same forced path with `AllEligible` and returns a typed `Sent`, `NoSession`, `NoRecipients`, or `StaleSession` result.
- During IME composition, do not parse/project the contenteditable DOM or call document, selection, typing, draft, or IPC callbacks; composition end publishes one document/history transition.
- Wrong-timeline-key events remain discarded and increment `keyMismatchDropped`, but emit no per-event diagnostic entry.
- Follow `REPOSITORY_RULES.md`, architecture/state-machine docs, `docs/policies/engineering-rules.md`, and the approved design at `docs/superpowers/specs/2026-08-07-megolm-reshare-ime-churn-design.md`.

---

### Task 1: Forced current-session room-key sharing in the vendored SDK

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/session_manager/group_sessions/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/olm/group_sessions/outbound.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-base/src/client.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/room/mod.rs`
- Test: inline test modules in the four files above, concentrating recipient-filter behavior in `group_sessions/mod.rs`

**Interfaces:**
- Produces: `matrix_sdk::room::RoomKeyReshareTarget::{OwnOtherDevices, PeerDevices, AllEligible}`.
- Produces: opaque `matrix_sdk::room::OutboundGroupSessionToken` with equality/clone but redacted `Debug`.
- Produces: `Room::current_outbound_group_session_token() -> Result<Option<OutboundGroupSessionToken>>`.
- Produces: `Room::force_reshare_room_key(expected: Option<&OutboundGroupSessionToken>, target: RoomKeyReshareTarget) -> Result<RoomKeyReshareResult>` where `RoomKeyReshareResult` is `Sent { requests: usize, recipients: usize } | NoSession | NoRecipients | StaleSession`.

- [ ] **Step 1: Add crypto RED tests for the actual recipient break**

Create a current outbound session, mark Alice's eligible device as already shared, and assert a forced `PeerDevices` call creates one to-device request for that device while normal `share_room_key` still creates none. Add literal assertions that `OwnOtherDevices` excludes Alice, pending shares are not repeated, a mismatched expected token returns `StaleSession`, and no current session returns `NoSession` without creating one.

```rust
assert!(machine.share_room_key(room_id, [&alice], settings.clone()).await?.is_empty());
let forced = machine
    .force_reshare_room_key(room_id, Some(&token), RoomKeyReshareTarget::PeerDevices, [&alice])
    .await?;
assert_eq!(forced.recipient_count(), 1);
assert_eq!(forced.requests().len(), 1);
assert_eq!(machine.current_outbound_group_session(room_id).await?.unwrap().session_id(), token.as_str());
```

- [ ] **Step 2: Run the focused crypto test and record RED**

Run: `cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p matrix-sdk-crypto --lib force_reshare`

Expected: FAIL because the forced-current-session API does not exist.

- [ ] **Step 3: Implement the minimum shared crypto branch**

Refactor the existing recipient filter into a mode parameter. In force mode, load only the current outbound session, compare the expected token before collecting devices, retain existing trust/history-visibility/Olm-session checks, exclude devices already present in the pending share map, and bypass only the `ShareState::Shared` rejection. Return coarse request and recipient counts with the pending requests; do not call session creation or rotation.

```rust
enum ShareMode<'a> {
    Normal,
    Force { expected_session_id: Option<&'a str>, target: RoomKeyReshareTarget },
}
```

- [ ] **Step 4: Add BaseClient and Room forwarding/sending with typed outcomes**

Reuse the current room membership and encryption-settings lookup in `BaseClient::share_room_key`. `Room::force_reshare_room_key` sends the returned `ToDeviceRequest`s through the same loop as `share_room_key`, marks only successfully sent requests, and returns the coarse result. Keep `Room::reshare_room_key` as a compatibility wrapper over `AllEligible`, not as a second implementation.

- [ ] **Step 5: Run SDK GREEN gates**

Run each command separately and read its exit status:

```bash
cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p matrix-sdk-crypto --lib force_reshare
cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p matrix-sdk-base --lib share_room_key
cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p matrix-sdk --lib reshare_room_key
```

Expected: all PASS.

- [ ] **Step 6: Commit the submodule change**

```bash
git -C vendor/matrix-rust-sdk switch -c koushi/issue-440-forced-reshare
git -C vendor/matrix-rust-sdk add crates/matrix-sdk-crypto crates/matrix-sdk-base crates/matrix-sdk
git -C vendor/matrix-rust-sdk commit -m "fix: force reshare current outbound room key"
```

### Task 2: Koushi adapter and typed manual re-share result

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-core/src/room.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/room.rs`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/domain/coreEvents.generated.json`
- Modify: `apps/desktop/src/components/RoomInfoPanel.tsx`
- Modify: `apps/desktop/src/components/RoomInfoPanel.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/i18n/messages.test.ts`

**Interfaces:**
- Consumes: SDK token, target, and result from Task 1.
- Produces: `koushi_sdk::OutboundGroupSessionToken`, `RoomKeyReshareTarget`, and `RoomKeyReshareOutcome` with custom redacted `Debug` for the token.
- Produces: `RoomEvent::RoomKeyReshared { request_id, room_id, outcome }`.
- Produces: Tauri/backend `reshareRoomKey(roomId) -> Promise<RoomKeyReshareOutcomeDto>`; it no longer returns a full snapshot because the command does not mutate product state.

- [ ] **Step 1: Add RED adapter/core contract tests**

Add literal serialization assertions for all four coarse outcomes and a room-handler test proving an SDK `NoRecipients` success emits `RoomKeyReshared` rather than `OperationFailed`. Assert the opaque token's `Debug` contains no token text.

```rust
assert_eq!(format!("{:?}", OutboundGroupSessionToken::from_test_value("secret-session")), "OutboundGroupSessionToken(<redacted>)");
assert_eq!(serde_json::to_value(RoomKeyReshareOutcome::NoRecipients)?, json!("no_recipients"));
```

- [ ] **Step 2: Run focused tests and record RED**

```bash
cargo test -p koushi-sdk --lib room_key_reshare
cargo test -p koushi-core --lib reshare_room_key
```

Expected: FAIL because the typed adapter/outcome does not exist.

- [ ] **Step 3: Implement the thin adapter and manual core path**

Map SDK types once in `koushi-sdk`; do not expose Matrix SDK internals above that crate. In `RoomActor::handle_reshare_room_key`, obtain a `UserRoomOperation` interactive guard only around the SDK enqueue, call `AllEligible` with no expected token, emit the typed success outcome, and preserve existing SDK errors as `RoomOperationFailed`.

- [ ] **Step 4: Add RED Tauri/React manual-result tests**

In the Tauri command test, feed each `RoomKeyReshared` outcome and assert the exact returned snake-case DTO. In `RoomInfoPanel.test.tsx`, click the button and resolve `no_session`, `no_recipients`, and `sent`; assert the distinct localized status and that rejection alone renders the error state.

- [ ] **Step 5: Run frontend/Tauri RED tests**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib reshare_room_key
npm --prefix apps/desktop test -- --run src/components/RoomInfoPanel.test.tsx src/i18n/messages.test.ts
```

Expected: FAIL because the API still returns `DesktopSnapshot` and the panel treats every resolution as success.

- [ ] **Step 6: Wire typed outcome through Tauri and React**

Have the Tauri command wait for the matching typed room event and return only `RoomKeyReshareOutcomeDto`. Update the browser client type and `App.reshareRoomKey` to return that value without `setSnapshot`. Replace the panel's boolean success with the outcome union, and add only the three required copy keys (`sent`, `noSession`, `noRecipients`; stale maps to the existing retryable error copy for manual calls).

- [ ] **Step 7: Run manual-flow GREEN gates and commit root changes**

```bash
cargo test -p koushi-sdk --lib room_key_reshare
cargo test -p koushi-core --lib reshare_room_key
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib reshare_room_key
npm --prefix apps/desktop test -- --run src/components/RoomInfoPanel.test.tsx src/i18n/messages.test.ts
node scripts/check-sdk-submodule.mjs
git add vendor/matrix-rust-sdk crates/koushi-sdk crates/koushi-core apps/desktop
git commit -m "fix: force manual Megolm room-key reshare"
```

Expected: all PASS and the root commit records the new SDK gitlink.

### Task 3: Bounded automatic post-send re-share attempts

**Files:**
- Modify: `crates/koushi-core/src/account_work.rs`
- Modify: `crates/koushi-core/src/timeline.rs`
- Test: inline `#[cfg(test)]` modules in the same files

**Interfaces:**
- Consumes: `koushi_sdk::current_outbound_group_session_token` and `force_reshare_room_key` from Task 2.
- Produces: `AccountWorkKind::RoomKeyReshare`, a background policy with one-request batches.
- Produces: manager-owned `RoomKeyReshareSchedule`, keyed by `(room_id, OutboundGroupSessionToken)`, containing exactly three abortable timer handles.
- Produces: `TimelineManagerMessage::RunRoomKeyReshare { key, actor_generation, token, target, attempt }` wakeups; timer tasks never call the SDK directly.

- [ ] **Step 1: Add scheduler RED tests with paused Tokio time**

Drive the real schedule with a test mailbox and literal deadlines. Assert zero wakeups before 3 seconds, own/peer/own targets at 3/5/15 seconds, one schedule for repeated observations of the same token, a fresh schedule for a new token, and no wakeup after cancellation/drop.

```rust
tokio::time::advance(Duration::from_secs(3)).await;
assert_eq!(rx.recv().await.unwrap().target, RoomKeyReshareTarget::OwnOtherDevices);
tokio::time::advance(Duration::from_secs(2)).await;
assert_eq!(rx.recv().await.unwrap().target, RoomKeyReshareTarget::PeerDevices);
```

- [ ] **Step 2: Run scheduler tests and record RED**

Run: `cargo test -p koushi-core --lib room_key_reshare_schedule`

Expected: FAIL because no post-send schedule exists.

- [ ] **Step 3: Implement the minimal actor-owned schedule**

At the existing successful `handle_send_terminal_handoff`, only for room timelines, query the current token and call `observe`; do not delay or alter `SendCompleted`. Store the schedule on `TimelineManager`, use `executor::spawn` plus `executor::sleep`, and send the three wakeups to the manager mailbox. Deduplicate by room/token, prune the prior room token when a new token is observed, and abort all handles during the manager's existing ordered shutdown.

- [ ] **Step 4: Execute wakeups through current-state fences**

On each wakeup, require the same manager session, matching actor generation, an existing room timeline, and the same current token. Acquire one `RoomKeyReshare` account-work permit, call forced share for the target, emit one private-data-free diagnostic containing only target/attempt/outcome/request/recipient counts, then drop the permit. `NoSession`, `NoRecipients`, `StaleSession`, or SDK error ends that attempt; no retry beyond the fixed timer list.

- [ ] **Step 5: Add behavior tests for stale/current execution**

Use the existing test session seam to assert stale actor generation and changed token cause no SDK call, while the current token calls the adapter exactly once. Add policy assertions that `RoomKeyReshare` is background, preemptible, concurrency one, and batch limit one.

- [ ] **Step 6: Run GREEN gates and commit**

```bash
cargo test -p koushi-core --lib room_key_reshare_schedule
cargo test -p koushi-core --lib room_key_reshare_execution
cargo test -p koushi-core --lib account_work
git add crates/koushi-core
git commit -m "fix: schedule bounded Megolm reshares after sends"
```

Expected: all PASS.

### Task 4: Publish contenteditable composition once

**Files:**
- Modify: `apps/desktop/src/components/ImeTextControl.tsx`
- Modify: `apps/desktop/src/components/ImeTextControl.test.tsx`

**Interfaces:**
- Preserves: `ImeInlineMentionEditor` public props and imperative handle.
- Changes: composing `input` events invoke only the caller's raw `onInput`; `compositionend` invokes `publishDom()` once and commits one undo entry.

- [ ] **Step 1: Change the existing IME test to RED on observable callbacks**

Extend `keeps mention identity while composition updates neighboring text` to assert zero `onDocumentChange` calls and zero `onSelectionChange` calls after multiple composing inputs, one final call after `compositionend`, preserved mention identity, and one undo restoring the pre-composition document. Keep the existing IME Enter fence assertion.

- [ ] **Step 2: Run the focused test and record RED**

Run: `npm --prefix apps/desktop test -- --run src/components/ImeTextControl.test.tsx`

Expected: FAIL because each composing input currently calls `publishDom(false)`.

- [ ] **Step 3: Apply the one-guard fix**

Change the editor `onInput` handler to skip `publishDom` whenever either composition flag is true; retain the existing single `publishDom()` in `onCompositionEnd`.

```tsx
if (!composingRef.current && !event.nativeEvent.isComposing) publishDom();
onInput?.(event);
```

- [ ] **Step 4: Run all focused IME gates and commit**

```bash
npm --prefix apps/desktop test -- --run src/components/ImeTextControl.test.tsx
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
git add apps/desktop/src/components/ImeTextControl.tsx apps/desktop/src/components/ImeTextControl.test.tsx
git commit -m "fix: publish IME mention edits once"
```

Expected: all PASS.

### Task 5: O(1) diagnostic buffering and aggregate-only key mismatch reporting

**Files:**
- Modify: `apps/desktop/src/domain/diagnostics.ts`
- Modify: `apps/desktop/src/domain/diagnostics.test.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/TimelineView.tsx`
- Modify: `apps/desktop/src/components/TimelineView.test.tsx`

**Interfaces:**
- Replaces: `appendDiagnosticLogEntry(entries, entry, limit)` with `createDiagnosticLogBuffer(limit)`.
- Produces: `DiagnosticLogBuffer.append(entry): void` and `DiagnosticLogBuffer.snapshot(): DiagnosticLogSnapshot`, where `snapshot` returns chronological entries and cumulative `droppedEntries`.
- Preserves: existing `recordTimelineKeyMismatch()` aggregate counter.

- [ ] **Step 1: Add RED ring-buffer behavior tests**

Append four literal entries to a capacity-three buffer. Assert chronological `[2, 3, 4]`, `droppedEntries === 1`, a second snapshot is unchanged, and capacity normalization keeps at least one item. The test must call the real buffer, not inspect its fields.

```ts
const buffer = createDiagnosticLogBuffer(3);
for (const timestampMs of [1, 2, 3, 4]) buffer.append({ timestampMs, source: "test", message: `${timestampMs}` });
expect(buffer.snapshot()).toEqual({ entries: [entry2, entry3, entry4], droppedEntries: 1 });
```

- [ ] **Step 2: Run diagnostic RED test**

Run: `npm --prefix apps/desktop test -- --run src/domain/diagnostics.test.ts`

Expected: FAIL because only the copying array helper exists.

- [ ] **Step 3: Implement a fixed-capacity circular buffer and ref ownership**

Use one preallocated TypeScript array, write index, size, and dropped counter. In `App`, replace `useState<DiagnosticLogEntry[]>` with one `useRef(createDiagnosticLogBuffer())`; append directly without a state setter. Route schema-mismatch and timeline/panel diagnostics through that same buffer. At export time, call `snapshot()` once and combine its entries/dropped count with the runtime snapshot.

- [ ] **Step 4: Change wrong-key burst test to RED**

Emit at least three wrong-key events, assert `keyMismatchDropped` increases by three through the real transport stats, assert no `timeline.key` diagnostic callback, and assert the wrong events do not alter visible timeline items.

- [ ] **Step 5: Remove only the per-event diagnostic path**

In `TimelineView`, retain `recordTimelineKeyMismatch(); return;` and remove the fingerprint construction plus `emitDiagnosticLog` call/imports that become unused. Do not add sampling, batching, or another log channel.

- [ ] **Step 6: Run GREEN gates and commit**

```bash
npm --prefix apps/desktop test -- --run src/domain/diagnostics.test.ts src/domain/timelineTransportStats.test.ts src/components/TimelineView.test.tsx
npm --prefix apps/desktop run typecheck
git add apps/desktop/src/domain apps/desktop/src/App.tsx apps/desktop/src/components/TimelineView.tsx apps/desktop/src/components/TimelineView.test.tsx
git commit -m "perf: remove IME and diagnostic render churn"
```

Expected: all PASS.

### Task 6: Canon consistency, integrated verification, and publication

**Files:**
- Modify if the implemented contract adds durable detail: `docs/policies/engineering-rules.md`
- Verify: `REPOSITORY_RULES.md`, `docs/architecture/overview.md`, `docs/architecture/state-machine.md`, `AGENTS.md`
- Verify: all files and submodule gitlink changed by Tasks 1-5

**Interfaces:**
- Produces: one self-reviewed root branch and one accessible SDK submodule commit, ready for PR review/CI.

- [ ] **Step 1: Check canon and generated contracts**

Confirm the change does not move Matrix semantics into React, does not add a reducer state transition, and keeps diagnostic data private-safe. Update only the durable policy document if the final code introduces a rule absent from the approved design; do not duplicate the dated spec into canon.

- [ ] **Step 2: Run formatting and focused aggregate gates**

Run each independently and record the actual exit status:

```bash
cargo fmt --all -- --check
cargo test -p koushi-sdk --lib room_key_reshare
cargo test -p koushi-core --lib account_work
cargo test -p koushi-core --lib room_key_reshare
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib reshare_room_key
npm --prefix apps/desktop test -- --run src/components/ImeTextControl.test.tsx src/domain/diagnostics.test.ts src/domain/timelineTransportStats.test.ts src/components/TimelineView.test.tsx src/components/RoomInfoPanel.test.tsx src/i18n/messages.test.ts
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
node scripts/check-sdk-submodule.mjs
```

Expected: every command exits 0.

- [ ] **Step 3: Review the complete diff including new files and submodule**

```bash
git diff origin/main...HEAD
git status --short
git -C vendor/matrix-rust-sdk show --stat --oneline HEAD
git -C vendor/matrix-rust-sdk diff HEAD^
```

Check exact issue coverage, session non-creation, recipient eligibility, timer bounds/cancellation, account scheduler use, typed manual outcomes, IME callback count, O(1) buffer behavior, privacy, and accidental unrelated changes.

- [ ] **Step 4: Push the accessible SDK commit and root branch**

```bash
git -C vendor/matrix-rust-sdk push -u origin koushi/issue-440-forced-reshare
git push -u origin agent/issues-440-442
```

- [ ] **Step 5: Open one ready PR and monitor CI**

Open a non-draft PR with `Fixes #440` and `Fixes #442`, a concise design summary, and exact verification commands. Monitor checks; on a failure, inspect the completed job log, reproduce the exact command locally, fix with a RED/GREEN check, self-review, and push. Do not explain an unusually long job as normal without comparing a recent green baseline.

- [ ] **Step 6: Merge with a merge commit and verify closure**

After all required checks pass:

```bash
gh pr merge --merge --delete-branch
git fetch origin main
```

Verify the PR state is `MERGED`, issues #440 and #442 are `CLOSED`, the merge commit is reachable from `origin/main`, and the recorded SDK gitlink resolves from its pushed remote.
