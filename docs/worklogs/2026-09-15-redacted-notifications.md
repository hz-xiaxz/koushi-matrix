# Residual unread and divider correction

Based on origin/main `9abdbf154bc8b63770957f6e56add3f068282179`.
SDK pin: `669834a55` (test formatting follow-up to `d25591c31`) ([fork PR 18](https://github.com/shinaoka/matrix-rust-sdk-work/pull/18)).

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

Native confirmation: signed 0.9.1 build 2741 installed from app `ad494314`
and SDK `d25591c31`; the later SDK change formats the regression only.
Signature validation and installed/source executable hashes matched. The room
list reported zero unread; opening the affected 32-reply thread placed the
divider after the latest reply, including after close/reopen. No stuck opening
label was present. Previous app retained as a local backup; account data intact.

The first signing attempt failed at Apple's timestamp service; a timestamped
Developer ID signing retry succeeded, followed by deep/strict verification.
Tuwunel local SDK QA passed after allowing compilation time beyond the default
90 seconds. Synapse's initial Docker build failed; an isolated identical Docker
build passed and the QA rerun is recorded separately.
State crate tests passed. The SDK fork's broad CI also reports pre-existing
formatting/documentation failures outside this patch; these must not be described
as a green full SDK CI run.

Synapse QA rerun passed; both local homeservers are now green. App PR #912's
first CI run passed its product tests but cargo-deny reported RUSTSEC-2026-0285
in rustls 0.23.41. The follow-up updates rustls to the advisory's fixed 0.23.45,
with its resolved aws-lc-rs/aws-lc-sys and rustls-webpki dependencies. No advisory
exception or disabled gate is added. The final local build must include this
security update rather than retaining build 2741.
