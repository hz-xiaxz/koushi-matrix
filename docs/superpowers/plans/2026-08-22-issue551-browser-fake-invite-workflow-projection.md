# Issue #551 Browser Fake Invite Workflow Projection

## Scope

Move one pure browser-fake invite candidate/scope/history projection family into private leaf `apps/desktop/src/backend/browser-fake/inviteWorkflow.ts`. Mechanical ownership decomposition only: no state/lifecycle/API/DTO/fixture/test/behavior changes.

Immutable baseline: main `dd9215892ae4fc488995cd15aff52e94d03b4b7b`; parent 6,172 lines / 205,909 bytes / SHA-256 `d8c805966163c17ca0d32c3b01e92ffedf3cb5b24789e1ba3b1259ec72887d8e`.

## Exact declaration set

Move these ten complete top-level declaration source slices in original order, preserving bodies/comments/formatting:

1. `INVITE_ALREADY_IN_SPACE_MESSAGE`
2. `buildFakeInviteHistoryPolicy`
3. `inviteScopeKey`
4. `defaultInviteWorkflowState`
5. `buildFakeInviteScopePlan`
6. `buildFakeInviteTargetQuery`
7. `fakeInviteCandidate`
8. `fakeInviteTextMatches`
9. `fakeValidMatrixUserId`
10. `fakeRoomHasMember`

No adjacent live-signal/default/profile/room-management state owner moves.

## Leaf boundary

Type-only import exactly `DesktopSnapshot`, `InviteHistoryPolicy`, `InviteScopeSelection`, `InviteTargetCandidate`, `InviteWorkflowState`, and `RoomHistoryVisibility` from `../../domain/types`.

Export only seven parent-used declarations: the constant plus `buildFakeInviteHistoryPolicy`, `inviteScopeKey`, `defaultInviteWorkflowState`, `buildFakeInviteScopePlan`, `buildFakeInviteTargetQuery`, and `fakeRoomHasMember`. Keep `fakeInviteCandidate`, `fakeInviteTextMatches`, and `fakeValidMatrixUserId` private.

Parent adds one direct seven-name import. Remove only the parent type imports whose remaining uses are wholly moved: `InviteHistoryPolicy`, `InviteTargetCandidate`, `InviteWorkflowState`, `RoomHistoryVisibility`. Retain `InviteScopeSelection` because DesktopApi and class methods use it.

No barrel, wrapper, callback registry, class, fixture, state, cache, timer, or default export.

## Exactness and references

- AST source slices10/10 exact after ignoring only added `export` modifiers; original order; parent0; exports7/private3.
- Parent reference occurrences after adding the import: constant3 (import+2), history3 (import+2), scope-key7 (import+6), default11 (import+10), scope-plan3 (import+2), target-query4 (import+3), room-member2 (import+1).
- Leaf-only references: candidate4, text-match11, Matrix-ID validator1; `defaultInviteWorkflowState` also has one leaf-internal call.
- BrowserFakeApi public methods, request IDs, snapshot mutation ordering, invite operation state, room/profile fixtures, and all resource owners stay in the parent.

## Implementation evidence

- Exact AST declaration slices10/10 in original order, parent0, exports7/private3; all parent/leaf occurrence counts exact.
- One direct parent import, six leaf types, only four approved parent type removals; API/class/state/resource delta0.
- Parent 6,172→5,942 lines; private leaf243; combined6,185.
- Browser fake114 + client25, typecheck/lint/diff and deterministic verifier green.
- Post-implementation full-diff review: `reviewer-flash` `Correct-to-merge`.
- Final local matrix: exactness green; browser fake114, client25, Vitest1,400, Playwright248, workspace all-targets, Tauri149/1 ignored plus keyring5, Headless Core QA130, wasm state/search, typecheck/lint/build, Tauri/domain/IPC boundaries, secret/release/version, SDK/docs, rustfmt, `cargo deny`, `cargo machete`, and diff checks green. Initial workspace run hit the pre-existing `corrupt_load_attempts_once_per_session` 3-vs-2 race; exact×3, runtime-timeline file, and full workspace rerun were green before continuing.

## Verification

Use TypeScript AST statement ranges against immutable `dd921589`; verify body/token/order, parent0, export/private/import/type surfaces, occurrence counts, public API/class/resource inventory and clean extraction holes. Baseline/post focused browser fake114 + client25, especially invite candidate/scope/history/already-member tests. Then full frontend/Rust/Tauri/Headless/wasm/policy matrix, full-diff review, latest-main check, CI7/7, merge and #551 evidence.

## Review gate

Pre-implementation review: `reviewer-flash` `Correct-to-implement`. Focused runner baseline is 114 tests (92 ordinary plus 22 parameterized cases).
