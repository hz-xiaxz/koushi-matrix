# Display-independent room sending

Core operations resolve an explicit account and joined room without requiring a
presentation timeline. GUI submissions and direct protocol operations share the
SDK room queue, manager-owned enqueue workers and global terminal observer.
Only GUI submissions own composer acceptance/settlement; direct operations must
leave all GUI drafts untouched. Existing timelines supply optional cached reply
metadata, never operation lifetime.

Canon consulted: REPOSITORY_RULES.md (ownership, security, review), architecture
overview (runtime, send ownership, MCP bridge), state-machine accepted sends,
engineering rules (async lifetime) and verification discipline.

Independent design review approved this approach with explicit requirements for
account/membership/leave validation, non-composer terminal registration, optional
reply metadata, existing shutdown supervision, and canon amendment.

Verification: a behavioral Core test first reproduces failure for an unviewed
room, then exercises text/reply through the real SDK queue and global terminal
observer with a synthetic local mock server. Focused send tests cover existing
GUI acceptance, pending projection, actor retirement and shutdown semantics.
No live accounts are used for automated verification.

Implementation changes:

- Core room resource resolution checks account identity, room existence, joined
  membership and pending leave before any send registration. Text, reply,
  media and GUI submission paths use the same resolver.
- Text/reply enqueue directly into the existing SDK room queue. An optional
  display timeline supplies reply metadata; its cleanup ingress is optional.
- Direct commands use no composer projection and cannot settle GUI composers.
- The source-order architecture guards now check room resource resolution
  before submission/worker enqueue rather than requiring a display handle.
- User help documents sending to rooms that are not open.

Evidence:

- RED: unviewed-room real SDK mock test failed at admission before the fix.
- GREEN: 57 focused outbound tests passed, including unviewed text/reply
  payload and terminal completion checks plus account/membership/leave guards.
- Full Core library suite: 1690 passed, 11 existing ignored.
- Desktop `cargo check -p koushi-desktop --locked` passed.
- Source-structure checker tests: 20 passed; docs/help checks passed.
- Independent integrated review approved with no blocking code findings; its
  minor state-machine documentation correction was applied.
- Final Core clippy including test targets passed with `-D warnings`; the Matrix
  context variant is boxed to keep synthetic test variants compact.
- Final formatting and whitespace checks passed.

The installed running application is not replaced or restarted by this change.
