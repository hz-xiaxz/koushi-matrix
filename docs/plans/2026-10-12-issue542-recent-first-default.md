# #542 - Recent-first as the real sidebar order

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Sidebar Sections And Low Priority"; `REPOSITORY_RULES.md` (canon-first, tests).
Depends on `#1364` (`2026-10-12-issue1364-persisted-room-activity.md`), which
gives Recent-first an ordering key that survives a restart.

## Problem

The sidebar's **Active** order was attention-first: a mention, notification,
ordinary unread, or manually marked-unread room outranked a fully-read room no
matter how old it was. When Sliding Sync restored a stale unread counter, an old
room could jump near the top. The label promised recency and the projection
delivered attention.

Three artifacts disagreed about the default:

- `default_room_list_sort()` and `RoomListSort::default()` both returned
  `Activity`;
- `SidebarSettings::scope` fell back to that value for a scope with no explicit
  preference, so Home and every Space opened attention-first;
- the section menu offered `activity` under "Unread & mentions first" while a
  dead `roomList.sortActive` ("Active" / "アクティブ") key stayed in the catalog.

The legacy localStorage migration also mapped the old `active` choice to
`activity`, which would have re-selected the order #542 removes.

## Design

- **Product default is `RoomListSort::RecentFirst`** for every scope and section
  (`default_room_list_sort()` and the enum's `#[default]`). `SidebarSettings`
  keeps its per-scope, per-section persistence; an explicit preference still
  wins.
- **`Activity` stays deserializable.** It is no longer selectable, but a
  settings document an older build persisted keeps attention-first ordering
  until the user picks another mode. React never sees a missing option as a
  reason to change the stored value.
- **The selector offers Recent and Name only.** The Recent label is "Recent" /
  "新着順"; Name stays locale-aware. The dead `sortActive` and the
  now-unselectable `sortAttention` labels are removed from both catalogs, so the
  catalog cannot advertise a mode the selector no longer has.
- **Legacy `active` migrates to `recentFirst`.** The legacy key asked for a
  recency order; `activity` was introduced later and was never written under it.
- **Browser Fake seeds `recentFirst`**, matching the Rust default so a fake-only
  projection cannot hide a drift back to attention-first.

## Attention semantics (canon correction)

The implementation ranks `room_attention_projection` in three steps under the
room's effective notification mode:

1. unmuted and SDK-local unread content (`unread_count > 0`);
2. unmuted and manually marked unread;
3. everything else.

Server notification and highlight counters alone cannot prove a visible unread
message and create no rank; muting removes a room from both ranks. The canon
previously described four ranks (mention, notification, unread content, read),
which no code ever implemented. The canon now states the three ranks plus the
rule that counters alone do not promote.

## Conversation-activity correctness gate

The classification entry point (`matrix_conversation_activity_source`) already
rejects `m.replace` and `m.annotation` before looking at the event type, and the
relation is read from the event the client holds. When the SDK decrypted an
encrypted event, that is the cleartext event, so an encrypted edit or reaction
is excluded on the same terms as a plaintext one; a ciphertext-only event
carries no readable relation and counts as an encrypted message. The tests now
pin that: the source table covers `m.room.encrypted` with `m.replace` and
`m.annotation`, and `decrypted_encrypted_relation_events_do_not_create_conversation_activity`
drives the entry point with an encrypted relation event and a plain encrypted
message.

## Verification

- `cargo test -p koushi-state` - ordering, per-scope default, stale-unread,
  legacy `activity`, selection, and filter coverage;
- `cargo test -p koushi-sdk --lib` - classification table and the encrypted
  relation case;
- `cargo test -p koushi-core --lib` - state-delta sidebar publication;
- `cargo fmt --check`, `cargo clippy -D warnings`;
- frontend `npm run typecheck` / `npm test`;
- golden `frontend_app_state.json` regenerated: the only diff is
  `"kind": "activity"` becoming `"kind": "recentFirst"` in the section, scope,
  and global settings values.
