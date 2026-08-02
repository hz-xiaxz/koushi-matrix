# Space Header and Invite History Design

## Scope

This batch ships issue #405 and issue #407 in one branch and one pull request.
Issue #405 is a presentation-only correction to the Space header. Issue #407
extends the existing invite workflow and Room Info settings so that history
visibility is understandable, independently editable, and retained while the
user moves between the invite dialog and Room Info.

## #405: two-row Space header

`Sidebar` keeps the existing title text, button order, button labels, handlers,
and keyboard focus order. The header becomes two explicit layout groups:

- a title row containing only the Space name, with the existing truncation rules;
- an action row containing the complete existing action group.

The header uses a grid with `min-width: 0` and a non-wrapping action group. The
action group may shrink through its own gap and button sizing, but no individual
action is allowed to wrap onto a third row. The regression check asserts the DOM
grouping and the computed layout contract at normal and narrow sidebar widths;
it does not rely on a visual screenshot.

## #407: Rust-owned invite history policy

The existing `InviteWorkflowState` remains the source of truth for the live
invite draft. It gains:

- `selected_scope`, validated against the current `InviteScopePlan` options;
- `history_policy`, containing the current history visibility, encryption state,
  edit permission, and a coarse recovery-readiness state.

The state reducer rebuilds the policy when the workflow opens or its room
changes. It preserves a valid selected scope when the workflow is refreshed
after Room Info saves, otherwise it selects the plan default. The query and
selected targets are never cleared by the Room Info detour. The new scope
selection command updates Rust state before the next render, and the invite
submit path reads the scope from the Rust snapshot.

The policy is deliberately coarse and private-data-free. It exposes no room
IDs, user IDs, event bodies, keys, or raw SDK errors. A locked or unverified
session in an encrypted room produces a `recoveryRequired` readiness value;
inviting remains enabled and the UI offers a Recovery action.

## Room Info and invite navigation

Room Info gets an independent “access and history” section. Join rule and
history visibility each have their own form and save action, so changing one
cannot silently submit the other. The current history value remains visible to
users without edit permission, while the history select and save action are
disabled when `can_edit_settings` is false.

The section explains the four Matrix history policies in plain language,
separates private/invite-only join rules from history visibility, marks
`worldReadable` as an advanced non-member-visible state, and states that
history sharing is not retroactive and cannot revoke already shared events or
keys. Encrypted rooms with `shared` history include a concise past-key sharing
explanation.

The invite dialog presents the normal three history choices and the current
value from the Rust policy. It includes the same warnings and a link to Room
Info. Choosing that link hides the dialog without closing the Rust workflow and
opens Room Info for the same room. Returning to the invite dialog restores the
query, selected users, and selected scope. After an independent Room Info save,
returning to the invite flow refreshes the policy and shows the new value.

## SDK and operation boundaries

`desktop_client_builder_defaults` explicitly calls
`with_enable_share_history_on_invite(true)`. No vendored SDK code is changed.
Actual invitations continue through the existing `invite_user_by_id`/room
invite helper and the existing batch operation; no custom crypto or key
transport is introduced.

## Verification

The change is developed test-first:

1. Rust state tests fail for policy projection, scope persistence, and
   independent history state updates.
2. TypeScript component tests fail for the two-row header, independent Room Info
   saves, history explanations, warnings, and invite navigation.
3. Browser-headless coverage verifies the dialog-to-Room-Info-to-dialog flow
   and that the draft remains intact.
4. The SDK contract test verifies the explicit builder call and the existing
   invite helper remains the operation path.
5. DTOs, browser fakes, Tauri mocks, golden snapshots, generated CoreEvent
   artifacts, TypeScript types, IME inventory, lint, typecheck, and focused Rust
   tests are updated as required by the repository contract.

## Non-goals

- changing Matrix history semantics or making shared history retroactive;
- revoking events or encryption keys already shared with invitees;
- adding a new crypto implementation or modifying the vendored SDK;
- changing Space header action labels, order, or behavior;
- adding a global search or unrelated invite-management feature.
