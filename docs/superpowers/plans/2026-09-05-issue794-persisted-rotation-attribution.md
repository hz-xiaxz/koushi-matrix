# Issue #794 — persisted Megolm rotation attribution

Status: approved by the user's request to implement, open a PR, and merge Issue #794; implementation pending.

## Objective

Persist the exact closed reason for each newly created outbound Megolm session in the account's encrypted crypto store so the existing Rust message-source projection survives process and account-runtime replacement. Keep the record exact, bounded, private, and advisory: failure to load or save it must not block encryption or sending, and missing evidence remains `Reason unavailable`.

## Canon and ownership

- `matrix-sdk-crypto` owns creation classification and the encrypted crypto store, so it owns persistence and lookup.
- The existing `koushi-sdk` / Core / React typed projection remains unchanged; React receives only `TimelineMegolmSessionReason` and adds no cache.
- The existing anonymous exported diagnostic ledger remains runtime-local. Persisted raw room/session keys never enter diagnostics, logs, `Debug`, QA tokens, or the WebView.
- Hard logout/local-data deletion already deletes the account crypto store; no second file or cleanup path is introduced.

## Minimal design

1. Store one versioned MessagePack custom value in the crypto store under a Koushi-specific key. Its payload is an ordered list of `(room_id, session_id, RoomKeyRotationReason)` records.
2. Deserialize the payload while constructing `OlmMachine`. Missing, unknown-version, malformed, or oversized data initializes an empty ledger without failing machine construction.
3. Retain at most 128 entries. Exact-key replacement updates the existing reason without consuming capacity; a new key evicts the oldest entry.
4. On successful new outbound-session creation, update the in-memory ledger and best-effort persist the complete bounded value. Store/serialization failure leaves encryption and sending unaffected and emits no identifier-bearing output.
5. Queries continue to accept raw room/session values only inside the trusted Rust SDK boundary and return only the closed reason.
6. Do not infer reasons from time, counters, fingerprints, active sessions, or outbound-session records.

## Verify-first checks

Before changing production behavior, add focused tests that require:

- restore across a new `OlmMachine` using the same crypto store for `Initial`, an expiry reason, and `FullMemberListReload` / discard attribution;
- exact room/session matching;
- duplicate replacement without growth;
- deterministic oldest-first eviction at 128 entries;
- missing, legacy/unknown-version, malformed, and oversized payloads returning no reason without preventing machine creation;
- persisted bytes remain inside the crypto store and no exported diagnostic format gains room IDs, session IDs, sender keys, or fingerprints.

## Verification gates

Focused:

```bash
(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto persisted_rotation_reason)
(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto retained_rotation_reason)
(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk room_key_rotation)
cargo test -p koushi-sdk room_key_rotation
cargo test -p koushi-core --lib message_source
node scripts/check-sdk-submodule.mjs
```

Before PR:

```bash
cargo fmt --all -- --check
(cd vendor/matrix-rust-sdk && cargo fmt --all -- --check)
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:ui-headless
cargo deny check
node scripts/check-agents-docs.mjs
node scripts/check-sdk-submodule.mjs
git diff --check
```

A homeserver lane is not required locally because the change is crypto-store persistence below an existing typed projection and the focused tests reopen the actual store abstraction. Existing CI homeserver checks remain mandatory before merge.

## Acceptance mapping

- Restart/runtime replacement: same-store `OlmMachine` reopen test.
- Initial/expiry/discard-member-reload: exact reason round-trip cases.
- Missing/corrupt/legacy: fail-closed restore cases and nonblocking creation test.
- Bounds/replacement: 128-entry eviction and duplicate update tests.
- Privacy: unchanged exported-diagnostic schema plus source/diff review.
- Logout/deletion: custom value is co-located in the existing crypto DB, whose existing hard-delete path removes the whole store root.
- No frontend semantic mirror: no TypeScript production changes.
