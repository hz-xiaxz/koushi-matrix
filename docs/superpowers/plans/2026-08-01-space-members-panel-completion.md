# Space Members Panel Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the Space members panel with a close control, real cached avatars, visible administrator markers, and safe cancellation of pending Space invitations.

**Architecture:** Keep presentation in `SpaceMembersPanel`, reuse `EntityAvatar` and the global `profile.users` avatar cache, and request thumbnails only when a member row becomes visible. Add a dedicated invite-cancellation command from the TypeScript client through Tauri, Core, State, and SDK; the SDK must re-read membership and must not call `kick_user` unless the target is still invited.

**Tech Stack:** React 19, TypeScript, Vitest/Testing Library, Tauri 2, Rust, matrix-rust-sdk.

**Status:** Implemented and independently reviewed on 2026-08-01. Final required
checks passed; the only reproduced failure is the pre-existing unrelated State
reducer test documented in the SDD progress ledger.

## Global Constraints

- The Space member source remains the direct Space JOIN/INVITE sets plus the union of joined members in all child rooms.
- A pending-invite cancellation must never intentionally kick a joined member; re-check current membership immediately before issuing the Matrix kick.
- UI actions are fenced by active Space ID and Space-member generation, matching the existing invite flow.
- Avatar downloads must be visibility-driven to avoid bulk member-avatar request floods.
- Diagnostics must contain operation/count/status tokens only, never user IDs, room IDs, display names, MXC URIs, or media URLs.
- Use existing localized role labels: `room.roleCreator` and `room.roleAdministrator`.
- No dependency additions.

---

### Task 1: Complete Space-member presentation

**Files:**
- Modify: `apps/desktop/src/components/SpaceMembersPanel.tsx`
- Modify: `apps/desktop/src/components/SpaceMembersPanel.test.tsx`
- Modify: `apps/desktop/src/components/rightPanel.tsx`
- Modify: `apps/desktop/src/components/rightPanel.test.tsx`
- Modify: `apps/desktop/src/styles.css`

**Interfaces:**
- Consume: `snapshot.state.domain.profile.users`, `onClosePanel`, and `onRequestMemberAvatarThumbnail` from `ContextualRightPanel`.
- Produce: `SpaceMembersPanelProps.onClose`, `.profileUsers`, and `.onRequestAvatarThumbnail`.

- [ ] **Step 1: Write failing component/integration tests**

Add tests that render a Space scope and assert:

```tsx
fireEvent.click(screen.getByRole("button", { name: "Close Space members" }));
expect(onClosePanel).toHaveBeenCalledTimes(1);
expect(screen.getByText("Administrator")).toBeTruthy();
expect(screen.getByText("Creator")).toBeTruthy();
expect(screen.getByRole("img", { name: "" })).toBeTruthy();
```

Mock `IntersectionObserver`, mark an avatar-bearing row visible, and assert exactly one call with its MXC URI. A ready cached `UserProfile.avatar.thumbnail.source_url` must render through `EntityAvatar`; an absent/failed image retains deterministic initials.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
npm test -- --run src/components/SpaceMembersPanel.test.tsx src/components/rightPanel.test.tsx
```

Expected: failures for the missing close button, role marker, avatar image, and visibility-triggered thumbnail request.

- [ ] **Step 3: Implement the minimal presentation changes**

Use the established People-panel header pattern:

```tsx
<button
  className="icon-button space-members-close"
  type="button"
  aria-label={t("action.close", { title: t("spaceMembers.title") })}
  onClick={onClose}
>
  <X size={ICON_SIZE.control} />
</button>
```

Render `EntityAvatar` from `profileUsers[entry.user_id]?.avatar`, preserve initials as fallback, and request an unresolved avatar only after its row intersects the panel viewport. Render a localized badge only for `creator` and `administrator`.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run the Step 2 command. Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/components/SpaceMembersPanel.tsx apps/desktop/src/components/SpaceMembersPanel.test.tsx apps/desktop/src/components/rightPanel.tsx apps/desktop/src/components/rightPanel.test.tsx apps/desktop/src/styles.css
git commit -m "fix: complete Space member presentation"
```

---

### Task 2: Add safe Space-invite cancellation to Rust and desktop transport

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`
- Modify: `crates/koushi-state/src/state/space_members.rs`
- Modify: `crates/koushi-state/src/action.rs`
- Modify: `crates/koushi-state/src/reducer/mod.rs`
- Modify: `crates/koushi-state/src/reducer/space_members.rs`
- Modify: `crates/koushi-core/src/command.rs`
- Modify: `crates/koushi-core/src/event.rs`
- Modify: `crates/koushi-core/src/room.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/room.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/backend/browserFakeApi.ts`
- Modify: `apps/desktop/src/backend/browserFakeApi.test.ts`
- Modify: `apps/desktop/src/backend/client.ts`
- Modify: `apps/desktop/src/backend/client.test.ts`
- Modify: focused Rust tests beside each changed module.

**Interfaces:**
- Produce TypeScript API:

```ts
cancelSpaceInvite(spaceId: string, userId: string, generation: number): Promise<DesktopSnapshot>;
```

- Produce Tauri command: `cancel_space_invite` with camel-cased `spaceId`, `userId`, and `generation` arguments.
- Produce operation state kind `cancellingInvite` carrying `request_id`, `space_id`, `user_id`, and `generation`.

- [ ] **Step 1: Write failing state, SDK contract, Core routing, Tauri, and TypeScript transport tests**

Tests must prove:

```text
invited -> cancellingInvite -> removed from space_invited -> idle
joined/not-invited -> no kick call -> projection reconciled without removing a joined entry
transport rejection -> failed with the target user retained in space_invited
stale Space/generation completion -> ignored
```

The SDK source-contract test must require `RoomMemberships::INVITE` membership validation before `.kick_user(`.

- [ ] **Step 2: Run focused tests and verify RED**

Run the nearest existing Space-member test targets in `koushi-sdk`, `koushi-state`, `koushi-core`, `src-tauri`, `browserFakeApi.test.ts`, and `client.test.ts`. Expected: missing cancellation symbols/routes.

- [ ] **Step 3: Implement the cancellation vertical slice**

At SDK level return a typed outcome equivalent to:

```rust
pub enum MatrixSpaceInviteCancellationOutcome {
    Cancelled,
    NotInvited,
}
```

Resolve the Space and target IDs, load `RoomMemberships::INVITE`, return `NotInvited` without mutation when absent, and call `kick_user` only for an invited target. Core must reconcile with a fresh `matrix_space_members_projection`, and State must apply only exact request/Space/generation settlements. The browser fake must model the same state transition.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run every command from Step 2. Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/koushi-sdk crates/koushi-state crates/koushi-core apps/desktop/src-tauri apps/desktop/src/backend
git commit -m "feat: cancel pending Space invitations safely"
```

---

### Task 3: Wire invite cancellation into the Space-member UI

**Files:**
- Modify: `apps/desktop/src/domain/types.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/components/SpaceMembersPanel.tsx`
- Modify: `apps/desktop/src/components/SpaceMembersPanel.test.tsx`
- Modify: `apps/desktop/src/components/rightPanel.tsx`
- Modify: `apps/desktop/src/components/rightPanel.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.spaceMembers.test.tsx`
- Modify: `apps/desktop/src/styles.css`

**Interfaces:**
- Consume `DesktopApi.cancelSpaceInvite` from Task 2.
- Produce `SpaceMembersPanelProps.onCancelInvite`, `canCancelInvite`, and a cancellation availability reason based on exact Space settings `permissions.can_kick`.

- [ ] **Step 1: Write failing UI/App integration tests**

Tests must assert:

```tsx
fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
expect(cancelSpaceInvite).toHaveBeenCalledWith(spaceId, invitedUserId, generation);
```

The button appears only in `space_invited`, is disabled without `can_kick` or while any Space-member operation is pending, reports localized failure copy, and stale completions after Space navigation do not overwrite the current snapshot. Diagnostics record only `cancel trigger=inline availability_reason=<token>` and `cancel outcome=<token>`.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
npm test -- --run src/components/SpaceMembersPanel.test.tsx src/components/rightPanel.test.tsx src/App.spaceMembers.test.tsx src/i18n/messages.test.ts
```

Expected: missing cancel button/callback/copy/flow.

- [ ] **Step 3: Implement minimal UI and App fencing**

Add English/Japanese copy:

```text
Cancel invitation / 招待を取り消す
Cancelling… / 取消中…
Could not cancel the invitation. Try again. / 招待を取り消せませんでした。もう一度お試しください。
```

Use a request ref independent from invite/open requests, exact Space/generation fences, `can_kick`, and the existing operation-pending gate. Keep the invited entry visible until backend state settles.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run the Step 2 command. Expected: all pass.

- [ ] **Step 5: Run static checks**

Run:

```bash
npm run typecheck
npm run lint
```

Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src
git commit -m "feat: expose Space invitation cancellation"
```
