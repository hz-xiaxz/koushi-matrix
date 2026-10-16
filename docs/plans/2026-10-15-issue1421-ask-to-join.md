# #1421 — "Ask to join" (knock) in create + a knock-request review surface

Status: design, pre-implementation. Refs [#1421](https://github.com/shinaoka/koushi-matrix/issues/1421).

## Upstream parity (recorded from the issue)

Element Web (labs) and Element X (behind a flag) let the creator pick **Ask to
join** (`join_rule: knock`, or `knock_restricted` inside a Space) and review
pending requests from Room Info. The vendored SDK already exposes the matching
primitives: create-room join-rule content, `Client::knock`, and
`Room::subscribe_to_knock_requests()` plus
`KnockRequest::{accept, decline, decline_and_ban}`.

## Current state in this repo

- `RoomJoinRule` already has `Knock` and `KnockRestricted`; the Room Info editor
  already offers `knock` (`sdk_join_rule_for_update`), and the create-draft
  preview already normalizes `KnockRestricted` (restricted conditions, target
  clearing, `room_version_pinned`).
- `sdk_join_rule_for_access_policy` (create) rejects `KnockRestricted`, and
  `RoomAccessPolicy::is_submittable` returns false for it, so a knock_restricted
  create cannot be submitted.
- `RoomMemberMembership` is only `Joined | Invited | Unknown`, and the room members
  query uses `RoomMemberships::ACTIVE` (= `JOIN | INVITE`), so knock members are
  never fetched or shown.
- The create dialog (`CreateEntityDialog`) offers Space-members / Invite / Public
  only; the Rust `RoomAccessDraft` already accepts any `RoomJoinRule` for a create
  scope.

## Phase A — headless contract (Rust)

1. **SDK create.** `sdk_join_rule_for_access_policy` gains
   `KnockRestricted` → `JoinRule::KnockRestricted(Restricted::new(rules))`,
   requiring at least one membership target exactly like `Restricted`.
   `create_room_request` treats it as restricted for the empty-target rejection
   and pins a version that supports knock_restricted (V10) instead of V9.
2. **State.** `RoomAccessPolicy::is_submittable` accepts `KnockRestricted` with
   targets; `preview_create_room_access` rejects `KnockRestricted` with an empty
   target set (`EmptyAccessTargets`), matching the SDK.
3. **Rust-owned availability.** `CreateRoomAccessPreview` gains
   `knock_available` and `knock_restricted_available`, derived from the room
   version the create would use (knock needs v7+, knock_restricted v10+). React
   only renders the choice.
4. **Headless QA.** A new `headless_core_qa` scenario creates a room with
   `access_policy = Knock` on a disposable local homeserver, a second synthetic
   account knocks (`Client::knock`), and the scenario asserts the knocking
   account's membership is `knock`.

## Phase B — GUI

5. **Create dialog.** `CreateEntityDialog` adds "Ask to join" to "Who can join"
   when the Rust availability flag is true: `knock` at Home, `knockRestricted`
   inside a Space (with the attachment Space as the allow target).
6. **Knock review.** `RoomMemberMembership::Knocked`; the SDK member projection
   maps `MembershipState::Knock`; the room members query includes `KNOCK`. Room
   Info's people surface gains a "Join requests" section with Accept (existing
   `InviteUser`) and Deny (existing `ModerateRoomMember` kick).
7. **i18n.** en/ja catalog entries for the new labels.

## Tests

- state: `is_submittable`, the empty-target rejection, availability per version.
- sdk: knock/knock_restricted create request content and version pin; member
  projection maps `Knock`.
- core/QA: the knock create + second-account knock scenario.
- React: the choice is rendered only when available; the requests section renders
  and wires accept/deny.

## Risks / open questions

- `knock_restricted` requires v10+; the create pins V10 explicitly rather than
  relying on a server default.
- Including `KNOCK` in the members query could surface knock members in other
  member views; scope the UI to the dedicated section first.
- The SDK's `subscribe_to_knock_requests` is richer but adds a long-lived
  subscription; start from the stored-members query and migrate later.
- Accept/Deny reuse the existing invite/kick commands, so no new protocol command
  is planned.

## Status (2026-10-15)

**A1 (headless contract) implemented and verified.**

- `knock_restricted` create support with its membership allow list and a V10
  version pin; create preview rejects public+knock_restricted and an empty allow
  list; the create draft seeds the attachment Space for the knock_restricted
  choice.
- An explicit access policy no longer carries the legacy `private_chat` preset,
  which won over the explicit `m.room.join_rules` and turned a knock create into
  invite.
- The `room_management` QA stage creates a knock room through Core and has a
  second, SDK-only account knock it (`knock_room_created=ok`,
  `knock_join_request=ok`), green on both disposable local servers.

`is_submittable` is deliberately unchanged, so existing-room knock_restricted
edits stay rejected for now.

**B still to do:** room-version availability read from the server capabilities
(the reviewer's second blocker), the create-dialog "Ask to join" choice, and the
Room Info knock-request review surface (`RoomMemberMembership::Knocked`, accept
via invite, deny via kick, the requester's reason).
