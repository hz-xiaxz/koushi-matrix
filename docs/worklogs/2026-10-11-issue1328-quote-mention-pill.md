# #1328 follow-up: reply-quote mentions render as pills

## Scope

Second branch from the merged `main` (`6678e9d2`, after #1351), worktree
`.worktrees/issues-1348-1328`, branch `fix/1328-quote-mentions`.

The reported symptom of #1328 — an Element user mention rendering as plain text
in the thread pane — does not reproduce on this base; both the Rust projection
and the real thread pane already pill it, and #1351 pinned that with permanent
guards. While investigating, one **reproducible** mention-pill gap turned up in
the same surface: a mention inside a **reply quote**. The quote preview renders
the quoted message's formatted body with an empty mention set, so the quoted
anchor stays a bare `matrix.to` link while the quoted row itself shows a pill.

This change fixes that gap. It does not close #1328, whose reported shape still
needs production data from the build that showed plain text.

## Base and canon

- `REPOSITORY_RULES.md` — Root-Cause Fix Discipline; Test Placement; Review And
  Audit; Documentation And Work Records; GitHub Issue Closure.
- `docs/agents/verification.md` — the failing headless check comes first.
- `docs/agents/state-ownership.md` — a DTO mirror change is involved, so the
  Rust type, its serialization, the TypeScript mirror, and the checked-in wire
  artifact all move together.
- `docs/agents/history.md` / issue #874 and #1123 — the pill is a property of
  the message that carries `m.mentions`; the renderer draws it from the event's
  mention facts, never from the message text alone.

## Root cause

`ReplyQuote` (`crates/koushi-state/src/state/room_interactions.rs`) carried the
quoted message's sender, plain preview, and formatted body, but not its mention
facts. `TimelineItemRow` therefore called `renderFormattedBody` for the quote
with the default empty mention set
(`apps/desktop/src/components/timeline/TimelineItemRow.tsx`), and
`markMentionAnchors` had nothing to mark: the `matrix.to` anchor rendered as an
ordinary link. The facts existed all along on both quote sources:

- the original held as a `TimelineItem` already has `mentioned_user_ids`;
- the SDK `EmbeddedEvent` path exposes `Message::mentions()`.

## Fix

- `ReplyQuote` gains `mentioned_user_ids: Vec<String>` with
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, so it is absent
  from the wire for the common quote with no mentions and old payloads still
  deserialize.
- `reply_quote_from_timeline_item` copies `item.mentioned_user_ids`;
  `reply_quote_from_embedded_event` reads `Message::mentions()` and threads the
  ids into `reply_quote_from_message_projection`. Redacted and placeholder
  quotes carry an empty list.
- The TypeScript mirror declares `mentioned_user_ids?: string[]`, and the row
  passes `{ userIds: new Set(item.reply_quote.mentioned_user_ids ?? []), room:
  false }` for the quote.

The contract fixture and the checked-in wire artifact now carry the field for a
quote that names a user, so a TypeScript key typo fails the contract test rather
than silently dropping the pill.

Deliberately **not** included: a room-mention flag on the quote. It was
implemented first and then withdrawn, because the formatted renderer pills only
`matrix.to` anchors — a room mention has no anchor form to mark, so the flag
would serialize a fact no renderer reads. Room mentions inside a formatted body
stay un-pilled for ordinary rows too; that is pre-existing renderer behavior and
a separate change from this one.

## RED then GREEN

`apps/desktop/e2e/reply-quote-mention-pill.spec.ts` was written first and run
against the unfixed tree. It opens the real thread/room pane through the harness,
pushes a reply whose quote carries Element's shape (a non-percent-encoded
`matrix.to` anchor, label without a leading `@`) plus `m.mentions` facts on the
quoted message, and asserts the anchor is a pill. The row that carries the quote
names a *different* mentioned user in both tests, so the pair proves the preview
reads the quoted message's facts rather than the row's own:

| Check | Pre-fix | Post-fix |
| --- | --- | --- |
| `a reply quote keeps the quoted message's mention as a pill` | `FAILED` — `expect(locator).toHaveCount(1)` … `Received: 0` | `ok` |
| `a quote without mention facts stays a plain anchor, not a pill` (the row itself names a mentioned user, so this fails if the preview reads the row's facts instead of the quote's) | `ok` | `ok` |
| `quote_carries_the_original_mentions_so_the_preview_pills_them` (`crates/koushi-core/src/timeline/reply_quote_hydration/tests.rs`) | `FAILED` to compile (`ReplyQuote` has no `mentioned_user_ids`), then `0` before the projection copied it | `ok` |
| `redacted_original_projects_redacted_quote` (extended to assert the empty list) | `ok` | `ok` |

## Documentation check

`docs/help/messaging.md` describes replies and threads but never describes
mention pill styling, and `docs/help/rooms-and-spaces.md` mentions only
notification counts. A quote preview now pills the same anchors its own row
does, which changes no documented text, so no help page changes are needed —
recorded here rather than silently skipped. `node scripts/user-help.mjs --check`
stays green.

## Gate matrix

Raw exit codes, run on the frozen tree:

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | `EXIT=0` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | `EXIT=0` |
| `cargo clippy -p koushi-qa --features qa-bin --all-targets --locked -- -D warnings` | `EXIT=0` |
| `cargo clippy --release --workspace --locked -- -D warnings` | `EXIT=0` |
| `cargo test -p koushi-core --lib` | `EXIT=0`, 1500 passed |
| `cargo test -p koushi-state --lib` | `EXIT=0`, 68 passed |
| `cargo test -p koushi-desktop core_event_` (wire contract + key-set guard) | `EXIT=0` |
| `cargo test --profile ci --workspace --exclude sidebar-composition --exclude key-management` | `EXIT=0`, 172 green result groups, no failure |
| `npx tsc --noEmit` | `EXIT=0` |
| `npm test` (vitest) | `EXIT=0`, 1884 passed |
| `npx playwright test` | `EXIT=0`, 524 passed |
| `npm run lint` | `EXIT=0` |
| `node scripts/check-rust-test-structure.mjs` and the other `scripts/check-*.mjs` guards | `EXIT=0` |
| `git diff --check` | `EXIT=0` |

The checked-in wire artifact `apps/desktop/src/domain/coreEvents.generated.json`
was regenerated (never hand-edited) so it records the new key: the contract
fixture's quote now names a user, and the companion key-set guard stays green.
The field is still skipped when empty, so a quote with no mentions keeps its
previous payload shape and older payloads deserialize unchanged.
