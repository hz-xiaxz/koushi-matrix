# Residual unread and divider correction

Based on origin/main `9abdbf154bc8b63770957f6e56add3f068282179`.
SDK pin: `d25591c31` ([fork PR 18](https://github.com/shinaoka/matrix-rust-sdk-work/pull/18)).

The SDK recount now ignores redacted entries in all three attention counters.
Core displays the newer comparable local/confirmed read boundary. Sanitized
booleans distinguish redacted cache contributions and backward display choices.
See the [upstream evidence](../upstream/2026-09-15-redacted-notifications.md)
for RED reproductions, historical comparisons, and attribution limits.

Independent canon and finished-diff review approved both changes. No frontend
semantics or settings changed; user help remains accurate and its check passes.
Cargo.lock synchronizes the desktop package version to the existing 0.9.1
manifest; no SDK dependency resolution change is intended.

Validation before the local build:
- SDK event-cache suite: 78 passed.
- Core: 1084 passed, 9 ignored, plus integration tests and doctests passed.
- TypeScript typecheck passed.
- Frontend: first full run had one 5-second timeout (1347 passed). The affected
  file passed all 22 tests independently; the full suite with two workers passed.
- Native confirmation, state tests, both local homeservers, and required CI are
  tracked separately; do not infer these from the unit-test results.
