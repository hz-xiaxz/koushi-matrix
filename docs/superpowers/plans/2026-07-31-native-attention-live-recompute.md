# Native Attention Live Recompute Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete issue #353 by making active-room notification suppression depend on the real native-window focus state and by recording the Rust-owned recomputation decision without private data.

**Architecture:** Tauri converts main-window `Focused(bool)` events into a typed `AppCommand` with a callback-ordered observation generation. The AppActor reduces that command into process-local `NativeAttentionContext`, which is skipped by AppState serialization and rejects stale asynchronous delivery. The existing room-list reducer remains the single owner of badge/candidate policy and consumes the stored focus fact. Core observes the completed room-list reduction and records a fixed-field diagnostic.

**Tech Stack:** Rust, koushi-state reducer, koushi-core actor/diagnostics, Tauri 2 window events, Cargo tests, Node repository guards.

---

## Task 1: Reproduce the focus-policy gap in the reducer

**Files:**
- Modify: `crates/koushi-state/src/reducer/mod.rs`

- [x] Add a reducer test named `unfocused_active_room_live_unread_creates_native_attention_candidate`.

  Construct a ready state with the active room selected, apply the initial
  `RoomListUpdated`, apply `NativeWindowFocusChanged { focused: false }`, then
  apply a second room-list snapshot whose active-room unread/highlight count
  increases. Assert:

  ```rust
  assert_eq!(state.native_attention.summary.badge_count, 2);
  assert!(state.native_attention.summary.candidate.is_some());
  assert_eq!(
      state.native_attention.dispatch,
      NativeAttentionDispatchState::Idle
  );
  ```

- [x] Add `focus_change_does_not_replay_existing_native_attention_candidate`.

  Seed a live candidate from another room, reduce both focus transitions, and
  assert that the transition clears/suppresses transient delivery rather than
  producing another deliverable candidate. Badge and unread totals must remain
  unchanged.

- [x] Run the focused RED gate and record the non-zero exit:

  ```bash
  cargo test -p koushi-state --lib native_attention > /tmp/issue-353-state-red.log 2>&1
  echo "EXIT=$?"
  ```

  Expected: compilation fails because `NativeWindowFocusChanged` and the
  process-local context do not exist yet, or the unfocused test fails because
  the reducer still hard-codes `window_focused: true`.

- [x] Commit the RED tests:

  ```bash
  git add crates/koushi-state/src/reducer/mod.rs
  git commit -m "test(state): reproduce native attention focus gap"
  ```

## Task 2: Add Rust-owned focus context and make the reducer green

**Files:**
- Modify: `crates/koushi-state/src/state/native_attention.rs`
- Modify: `crates/koushi-state/src/state/mod.rs`
- Modify: `crates/koushi-state/src/lib.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/reducer/native_attention.rs`
- Modify: `crates/koushi-state/src/reducer/room.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`

- [x] Add a conservative process-local focus context:

  ```rust
  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  pub struct NativeAttentionContext {
      pub window_focused: bool,
      pub window_focus_observation_generation: u64,
  }

  impl Default for NativeAttentionContext {
      fn default() -> Self {
          Self {
              window_focused: true,
              window_focus_observation_generation: 0,
          }
      }
  }
  ```

  Export the type from `state` and `koushi_state`, then add this skipped field
  to `AppState`:

  ```rust
  #[serde(skip)]
  pub native_attention_context: NativeAttentionContext,
  ```

  Initialize it in `AppState::default`. Do not add it to Tauri or TypeScript
  DTOs.

- [x] Add the reducer input:

  ```rust
  NativeWindowFocusChanged {
      focused: bool,
      observation_generation: u64,
  },
  ```

  Route it from `reduce` to
  `native_attention::handle_native_window_focus_changed`.

- [x] Move the existing room-derived attention projection into a
  `pub(crate)` helper in `reducer/native_attention.rs`. It must accept the
  observation kind and use:

  ```rust
  window_focused: state.native_attention_context.window_focused,
  ```

  Preserve badge preference masking and the current room notification/ignored
  user inputs.

- [x] Implement focus handling so a focus transition is reducer state, not a
  transient Matrix observation:

  ```rust
  pub(crate) fn handle_native_window_focus_changed(
      state: &mut AppState,
      focused: bool,
      observation_generation: u64,
  ) -> Vec<AppEffect>
  ```

  Reject a generation that is not newer than the last accepted observation.
  If the accepted value is unchanged, return no effects. Otherwise update the
  context, recompute the current persistent totals, clear the transient
  candidate, and leave dispatch non-deliverable. Emit
  `NativeAttentionChanged` only when the serialized `NativeAttentionState`
  changes.

- [x] Keep the focused-active-room test green and add an assertion that the
  default context is focused. Verify serialization omits
  `native_attention_context`.

- [x] Run:

  ```bash
  cargo test -p koushi-state --lib native_attention > /tmp/issue-353-state-green.log 2>&1
  echo "EXIT=$?"
  cargo test -p koushi-state --lib room_list_update > /tmp/issue-353-room-list.log 2>&1
  echo "EXIT=$?"
  ```

  Expected: both exits are `0`.

- [x] Commit:

  ```bash
  git add crates/koushi-state
  git commit -m "fix(state): track native window focus for attention"
  ```

## Task 3: Route native focus through the typed core boundary

**Files:**
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/native_attention.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [x] Add RED command tests in `crates/koushi-core/src/command.rs` proving the
  new command preserves its request ID and has private-data-free `Debug`:

  ```rust
  AppCommand::ObserveNativeWindowFocus {
      request_id,
      focused: false,
      observation_generation: 7,
  }
  ```

  The debug form must contain the command name and boolean, with no Matrix
  identifier fields.

- [x] Add a Tauri unit test for a pure helper:

  ```rust
  fn observed_native_window_focus(event: &tauri::WindowEvent) -> Option<bool>
  ```

  It returns `Some(true/false)` only for `WindowEvent::Focused`, and `None` for
  resize, move, close, and destroy events.

- [x] Run the RED gates:

  ```bash
  cargo test -p koushi-core --lib observe_native_window_focus > /tmp/issue-353-core-command-red.log 2>&1
  echo "EXIT=$?"
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib observed_native_window_focus > /tmp/issue-353-tauri-focus-red.log 2>&1
  echo "EXIT=$?"
  ```

- [x] Add
  `ObserveNativeWindowFocus { request_id, focused, observation_generation }`
  to `AppCommand`, its `request_id()` match, and its manual redacted `Debug`.

- [x] In AppActor command handling, reduce it to:

  ```rust
  AppAction::NativeWindowFocusChanged {
      focused,
      observation_generation,
  }
  ```

  Then handle its effects through the existing request-correlated effect path.
  This command must be accepted before session readiness so the conservative
  default is replaced as soon as the main window reports focus.

- [x] Add `build_observe_native_window_focus_command` in the Tauri
  native-attention command module. In the main-window event callback,
  synchronously allocate a strictly increasing observation generation, clone
  the app handle, obtain the next request ID from `CoreRuntimeState`, and
  best-effort submit that typed command on Tauri's async runtime. Do not invoke
  sound, notification, permission, or badge APIs from the focus callback.

- [x] Add
  `native_window_focus_generation_rejects_stale_async_delivery` and
  `native_window_focus_generation_is_monotonic_and_exhaustion_safe`. The
  reducer must ignore an older delivery, and the Tauri counter must stop rather
  than wrap at `u64::MAX`.

- [x] Run:

  ```bash
  cargo test -p koushi-core --lib observe_native_window_focus > /tmp/issue-353-core-command-green.log 2>&1
  echo "EXIT=$?"
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib observed_native_window_focus > /tmp/issue-353-tauri-focus-green.log 2>&1
  echo "EXIT=$?"
  cargo test -p koushi-state --lib native_window_focus_generation_rejects_stale_async_delivery
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib native_window_focus
  ```

- [x] Commit:

  ```bash
  git add crates/koushi-core/src/command.rs crates/koushi-core/src/runtime.rs apps/desktop/src-tauri/src/commands/native_attention.rs apps/desktop/src-tauri/src/lib.rs
  git commit -m "fix(desktop): route native window focus to core"
  ```

## Task 4: Add the private-data-free core recomputation diagnostic

**Files:**
- Modify: `crates/koushi-state/src/effect.rs`
- Modify: `crates/koushi-state/src/state/native_attention.rs`
- Modify: `crates/koushi-state/src/reducer/native_attention.rs`
- Modify: `crates/koushi-state/src/reducer/room.rs`
- Modify: `crates/koushi-core/src/runtime.rs`

- [x] Add a RED runtime test that reduces an initial room snapshot and a live
  unread increase, inspects the diagnostic sink, and requires one event with:

  ```text
  source=native.attention
  stage=recomputed
  observation=live
  unread_count=...
  badge_count=...
  candidate=message|direct_message|mention|none
  suppression=window_focused|initial_sync|backfill|self_message|duplicate|capability_unavailable|none
  window_focused=true|false
  active_room_match=true|false
  ```

  Assert the serialized diagnostic contains none of the fixture's room ID,
  event ID, user ID, room label, or message body.

- [x] Run RED:

  ```bash
  cargo test -p koushi-core --lib native_attention_recomputed_diagnostic > /tmp/issue-353-diagnostic-red.log 2>&1
  echo "EXIT=$?"
  ```

- [x] Add a projection result alongside the existing
  `native_attention_state_from_rooms` wrapper. It returns the projected state
  plus `active_room_match: bool`, calculated from the selected candidate's
  internal room ID before that ID is discarded. It must not expose or retain
  the room ID.

- [x] Add an internal `AppEffect::RecordNativeAttentionRecomputed` carrying
  only the observation enum, counts, candidate kind, suppression reason,
  window-focus boolean, and active-room-match boolean. Emit it on every real
  `RoomListUpdated` recomputation, even when the WebView-visible attention
  projection did not change.

- [x] Handle that effect in the AppActor by recording `source=native.attention`
  and `stage=recomputed`. Map enums to fixed tokens; never log IDs or display
  content. This keeps the diagnostic coupled to the exact reducer decision
  instead of reconstructing policy from the post-reduction state.

- [x] Run GREEN:

  ```bash
  cargo test -p koushi-core --lib native_attention_recomputed_diagnostic > /tmp/issue-353-diagnostic-green.log 2>&1
  echo "EXIT=$?"
  cargo test -p koushi-core --lib unread > /tmp/issue-353-unread-regression.log 2>&1
  echo "EXIT=$?"
  ```

- [x] Commit:

  ```bash
  git add crates/koushi-state/src/effect.rs crates/koushi-state/src/state/native_attention.rs crates/koushi-state/src/reducer/native_attention.rs crates/koushi-state/src/reducer/room.rs crates/koushi-core/src/runtime.rs
  git commit -m "feat(core): trace native attention recomputation"
  ```

## Task 5: Lock the existing provenance invariants

**Files:**
- Modify: `crates/koushi-state/src/state/native_attention.rs`

- [x] Add table-driven projection tests proving:

  1. `InitialSync`, `Backfill`, and `SelfEvent` observations keep persistent
     unread/badge totals but produce no transient candidate;
  2. their suppression reasons are respectively `InitialSync`, `Backfill`, and
     `SelfMessage`;
  3. `Live` with the same room data produces a candidate when no focus or
     capability suppression applies.

  These tests lock the projection boundary. Timeline pagination continues to
  emit timeline actions only, while server room-list unread metrics remain the
  sole input to this policy; do not add guessed provenance to React or Tauri.

- [x] Run:

  ```bash
  cargo test -p koushi-state --lib native_attention_observation > /tmp/issue-353-observation.log 2>&1
  echo "EXIT=$?"
  ```

  Expected: `EXIT=0`.

- [x] Commit:

  ```bash
  git add crates/koushi-state/src/state/native_attention.rs
  git commit -m "test(state): lock native attention observation suppression"
  ```

## Task 6: Update canon, run integrated verification, and open the PR

**Files:**
- Modify: `docs/architecture/state-machine.md`
- Modify: `docs/superpowers/plans/2026-07-31-native-attention-live-recompute.md`

- [x] Update the state-machine document to show main-window focus as a native
  typed input to the AppActor and to state that focus context is process-local,
  Rust-owned, and excluded from the WebView DTO.

- [x] Mark every completed plan checkbox and run formatting:

  ```bash
  cargo fmt --all -- --check
  ```

  The desktop package has no `format:check` script. Its actual `lint`,
  `typecheck`, and focused test gates below were run instead.

- [x] Run focused and contract gates, reading each command's exit:

  ```bash
  cargo test -p koushi-state --lib native_attention
  cargo test -p koushi-core --lib native_attention
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib native_attention
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib observed_native_window_focus
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib core_event_wire_format_matches_checked_in_contract_artifact
  npm --prefix apps/desktop test -- src/domain/desktopAttention.test.ts
  npm --prefix apps/desktop run typecheck
  npm --prefix apps/desktop run lint
  node scripts/check-sdk-submodule.mjs
  ```

- [x] Run the issue's integrated local Conduit proof once:

  ```bash
  PATH=/tmp/koushi-desktop-local-qa-bin:$PATH npm --prefix apps/desktop run qa:headless-local -- --server=conduit --scenario=native_attention --core --core-backend=both --timeout-ms=240000
  ```

- [x] Review the exact finished scope:

  ```bash
  git diff origin/main...HEAD
  git status --short
  ```

  Verify repository-rule consistency, state ownership, private-data-free
  diagnostics, no WebView DTO drift, and no unrelated files.

- [x] Commit canon/plan completion:

  ```bash
  git add docs/architecture/state-machine.md docs/superpowers/plans/2026-07-31-native-attention-live-recompute.md
  git commit -m "docs: record native attention focus flow"
  ```

- [ ] Push and open a standalone PR whose body includes `Closes #353`, the
  RED/GREEN evidence, all final gates, and the deliberate provenance limit.
  Wait for required CI, fix only failures attributable to the branch, and merge
  with a normal merge commit after all checks are green.
