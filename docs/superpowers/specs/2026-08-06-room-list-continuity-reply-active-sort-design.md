# Room-list continuity, media reply, and Active sort design

**Issues:** #439, #436, #438

## Goal

Fix three Rust-owned projection defects in one batch without adding speculative
recovery or diagnostic machinery.

## Design constraints

- Production semantics remain Rust-owned; React only renders projected state.
- Every behavior change starts with a failing headless test.
- Add no retry scheduler, persisted diagnostic ring, classification state
  machine, or new service. Those mechanisms are not required by the reproduced
  failures.
- Keep each issue in its own implementation commit and publish one PR.

## #439: provisional room-list continuity

`RoomListSnapshotProvisional` is an incomplete observation, so its reducer
must merge instead of replace. The reducer will upsert observed Spaces and
rooms by ID while retaining entries absent from the provisional snapshot. If a
known Space ID appears in the provisional ordinary-room list, the known Space
classification wins and the ordinary-room entry is omitted. This preserves the
active Space and Space-scoped navigation through provisional omissions.

`RoomListSnapshotAuthoritative` remains the only snapshot path that may remove
entries by absence. Existing generation/source guards and authoritative
reconciliation remain unchanged.

Tests will prove a non-empty provisional omission retains a Space, provisional
additions are accepted, a provisional classification conflict cannot duplicate
one ID across Spaces and rooms, and an authoritative omission still removes
the Space with the existing navigation fallback.

## #436: captionless media reply capability

Add `can_reply` to the existing Rust-owned timeline action capability DTO.
Projection sets it only for stable event-backed, non-redacted reply targets,
including captionless file, image, audio, and video messages. React replaces
its `item.body !== null` inference with this capability while retaining its
existing presentation rule that prevents nested reply composition in a thread.

The existing reply quote projection remains unchanged: caption/body is used
first and the media filename remains the fallback. Hover and context-menu
actions continue to consume one shared `canShowReply` value.

Tests will cover captionless media, redacted/non-event-backed items, filename
preview fallback, and the React action visibility contract.

## #438: attention-aware Active sort

Define a private Rust sort rank ahead of the existing conversation-activity
comparator:

1. unmuted unread mention;
2. unmuted notification-worthy unread activity;
3. ordinary unread, including muted and manually marked unread;
4. read/no attention.

The existing activity, display-name, and room-ID ordering remains the tie
breaker. Apply the comparator where Rust composes DM and room sidebar lists for
Home and Space scopes. Name sorting and frontend filtering remain unchanged;
no projected rank field or React-side semantic calculation is added.

Tests will cover all four ranks, mute demotion, manually marked unread,
deterministic ties, DMs/rooms, Home/Space scope, and live input changes.

## Operational rule

Add an explicit note to `AGENTS.md`: do not add defensive machinery without a
reproduced failure or a named invariant, and implement only the smallest guard
at the authoritative boundary. Examples of prohibited speculative additions
include retry loops, persisted incident buffers, fallback services, and new
state machines.

## Verification

Run the focused Rust/state/core and desktop component tests introduced or
updated for each issue, then repository formatting, desktop typecheck/lint,
the relevant Tauri wire-contract tests if the DTO changes, and the workspace
test command used by CI. Review `git diff origin/main...HEAD` and untracked
files before publishing one ready-for-review PR with `Closes #439`,
`Closes #436`, and `Closes #438` only if all acceptance checks are satisfied.
