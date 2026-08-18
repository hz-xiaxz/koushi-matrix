# Issue #543 — Exclude muted rooms from native Dock attention

## Objective

Make the Rust-owned native badge and badge-driven sound exclude muted rooms, while preserving the intentional distinction that a muted room row may still show its raw unread count. Recompute native attention immediately when the effective room notification mode changes.

## Governing contract

- `REPOSITORY_RULES.md`: Rust owns notification semantics; fix the authoritative projection.
- `docs/architecture/state-machine.md`: native attention excludes muted rooms from the Dock badge and accepts notification-mode changes as recomputation inputs. The same change clarifies the previously ambiguous distinction between persistent raw-unread badge eligibility and transient candidate eligibility.
- `docs/agents/state-ownership.md`: React and Tauri only map `AppState.native_attention` to native surfaces.
- Issue #543: a muted room must not create an otherwise unlocatable Dock badge or badge-driven sound.

This is a bug fix to the existing contract, not a new state or wire transition. No Tauri, React, DTO, SDK, or state-machine shape changes are required.

## Root cause

`native_attention_projection_from_rooms` currently adds every unique room's raw `unread_count` to `badge_count` before checking the room's effective notification mode. `RoomNotificationMode::Mute` suppresses candidates and attention totals later, so the Dock retains a count that Home/Space aggregation excludes.

Room notification-mode updates also recompute the sidebar but not `AppState.native_attention`, leaving a stale Dock count until another room-list observation arrives.

## Minimal design

1. In `native_attention_projection_from_rooms`, resolve the effective room mode before badge accumulation.
2. After duplicate rejection, exclude a room from `badge_count` when either the explicit muted-room input contains it or its effective mode is `Mute`.
3. Count that room in `badge_excluded_room_count`; count only badge-eligible unique rooms in `badge_room_count`.
4. Preserve the existing #433 semantics for every non-muted room:
   - Dock badge uses raw unread messages, not push-rule notification count.
   - mention-only rooms retain raw unread badge contribution.
   - manual `marked_unread` without raw unread does not fabricate a Dock count.
   - low-priority rooms and ignored-user DMs remain candidate-suppressed but retain raw unread badge contribution, matching the current Home account aggregate. This is now stated explicitly in the Native Attention canon; they remain badge-eligible and are not included in `badge_excluded_room_count`.
5. When room preferences load or `RoomNotificationModeSet` changes the effective mode, recompute native attention through the existing reducer helper with `NativeAttentionObservationKind::Live`. Emit `NativeAttentionChanged` only when the projection changes and retain the existing private-data-free recomputation diagnostic effect.
6. Candidate and dispatch behavior remains unchanged. Sound already observes positive Dock-badge deltas, so correcting the Rust badge source also suppresses sound for muted-room-only activity.

## Verify-first implementation order

### RED

Modify focused Rust integration tests before production code:

- Rename the existing low-priority-plus-explicit-muted test to reflect the split policy and change its badge total from `9` to `5`: the low-priority room retains its raw unread contribution of `5`, while the muted room's `4` contributes zero. Also assert `badge_room_count == 1` and `badge_excluded_room_count == 1` on the full projection so diagnostic count semantics are pinned.
- Add an effective `RoomNotificationMode::Mute` case; the present implementation must fail because it adds raw unread before the mode check.
- Add a reducer test proving a mode transition from `All` to `Mute` immediately clears `native_attention.summary.badge_count`; the present implementation must fail because settings changes do not recompute native attention.

Run and record the non-zero exits:

```bash
cargo test -p koushi-state --test attention_surface
cargo test -p koushi-state --test notification_settings_state
```

### GREEN

Implement only the projection ordering and settings-triggered recomputation described above. Add table coverage for normal unread, mentions-only, muted, explicit-muted input, low-priority, ignored-user DM, manual marked-unread, duplicate room, mute/unmute transition, and clearing the last included unread.

## Files

- Modify: `docs/architecture/state-machine.md`
- Modify: `crates/koushi-state/src/state/native_attention.rs`
- Modify: `crates/koushi-state/src/reducer/settings.rs`
- Modify: `crates/koushi-state/tests/attention_surface.rs`
- Modify: `crates/koushi-state/tests/notification_settings_state.rs`
- Modify: `docs/agents/plans.md`
- Create: this plan

No other production file is expected.

## Verification

Focused:

```bash
cargo test -p koushi-state --test attention_surface
cargo test -p koushi-state --test notification_settings_state
cargo test -p koushi-state --test navigation_state
```

Repository gates before PR:

```bash
node scripts/check-sdk-submodule.mjs
cargo test --workspace --locked
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test -- --run
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:ui-headless
cargo deny check
cargo fmt --all -- --check
git diff --check
```

## Completion record

- Design reviewer: `reviewer-flash-opencode-go` (DeepSeek V4 Flash), selected by the user.
- Implementer: `luna-implementer` (GPT-5.6 Luna), write-capable.
- Design review verdict: `Correct-to-merge` after one findings round; the final review required `Live` observation for settings recomputation and explicit diagnostic count assertions, both incorporated before implementation.
- Diff review verdict: `Correct-to-merge`; the unnecessary test-only public re-export found in the first post-implementation review was removed before the final verdict.
- Verify-first evidence: the muted badge and notification-mode recomputation tests failed before production edits, then passed with the fix.
- Local focused verification: private projection unit test 1 passed; `attention_surface` 20 passed; `notification_settings_state` 16 passed; `navigation_state` 55 passed; rustfmt and diff checks passed.
- Full local verification: workspace, QA-bin, Tauri, cargo-deny, frontend typecheck/Vitest/lint/build, browser-headless (76 Vitest + 248 Playwright), SDK-submodule, agents-doc, rustfmt, and diff gates pass. Playwright required `CHOKIDAR_USEPOLLING=true` because unrelated CodeGraph processes had exhausted the host inotify watch quota; no product test failed.
- CI verification: all seven required checks passed after rebasing onto the current `origin/main`.
