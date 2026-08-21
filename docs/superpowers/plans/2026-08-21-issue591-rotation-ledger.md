# Issue #591 — eviction-resistant Megolm rotation attribution

Status: design approved for implementation.

Review record: `reviewer-flash` read-only review, 2026-08-21,
`Correct-to-implement`. Five Minor clarifications were incorporated: migrate the
existing detail-ring assertion, document the single-active-runtime reset
boundary, pin SDK insert/duplicate behavior, reset the dedicated drop counter,
and name both wire-contract checks.

## Objective

Retain a strictly bounded, runtime-local, privacy-safe record of recent outbound
Megolm creation/rotation boundaries outside the general diagnostic ring, export
those records after ordinary diagnostic overflow, and show the recorded reason
for an event encrypted by the current Koushi device. Missing evidence must render
as unavailable rather than being inferred from event dates or aggregate counters.

This change is diagnostic and presentation-only. It does not change Matrix
protocol behavior, rotation timing, recipient policy, key sharing, retry,
plaintext fallback, or persistence.

## Canon and existing seams

- `REPOSITORY_RULES.md`: Rust owns product semantics; diagnostics may contain
  closed tokens, booleans, counts, elapsed time, and runtime-local aliases only.
- `docs/architecture/overview.md`: `TimelineActor` projects SDK event data and
  React renders Rust DTOs; SDK deltas stay in the vendored submodule and must be
  minimal/upstreamable.
- `docs/policies/engineering-rules.md`: `TimelineMessageSource` is a safe Rust
  projection; raw Matrix identifiers, session fingerprints, deterministic
  hashes, keys, content, and raw errors cannot enter exported diagnostics.
- Existing rotation classification and anonymous aliases are owned by
  `matrix-sdk-crypto::RoomKeyDiagnosticHub` and reach Koushi through
  `RoomKeyDiagnosticEvent::Rotation`.
- Existing local event details are projected by
  `timeline/item_projection.rs::project_message_source_for_event` and rendered
  by `MessageSourceDialog`.

## Design

### 1. Dedicated Koushi rotation ledger

Add one concrete rotation-ledger owner to `koushi-diagnostics`, separate from
`GLOBAL_BUFFER`:

- capacity: 128 boundaries;
- storage: `VecDeque` under the existing best-effort mutex discipline;
- reset: explicit at account diagnostic-observer installation/replacement,
  clearing both entries and the dedicated dropped-boundary counter;
- overflow: deterministic oldest-first eviction and a saturating dedicated
  dropped-boundary counter;
- export: append synthesized `core.room_key_rotation stage=boundary` records to
  `snapshot()` outside the general ring, plus an explicit closed aggregate
  dropped-boundary counter;
- lookup/update key: the SDK-provided runtime-local new-session ordinal alias;
- update: when `InitialShareSessionDiagnostic` is observed, mark the matching
  retained boundary's first-user-event correlation as observed. Preserve the
  SDK-provided closed creation/share outcomes; do not invent success from
  aggregate counts or timing.

A boundary stores only timestamp, runtime-local room/session ordinals, closed
reason/creation/share tokens, booleans, and elapsed milliseconds. It stores no
raw room/event/session/request/transaction/user/device value, session
fingerprint, deterministic hash, content, key material, URL, homeserver, path,
or raw error.

The ordinary detail ring no longer owns rotation boundary retention. The same
record remains visible through the normal exported snapshot because the ledger
is appended there; high-volume timeline/UI records cannot evict it. Existing
rotation tests that inspect `detail_snapshot()` move to a dedicated ledger test
snapshot instead of preserving a duplicate detail-ring owner.

The desktop runtime has one active account/session owner at a time; account
switch and runtime replacement stop the old owner before installing the new
observer. Ledger reset at observer installation is therefore the generation
boundary and prevents process-local ordinal reuse from correlating across
crypto machines. Supporting simultaneous account runtimes would require an
explicit account-runtime generation key and is not inferred here.

### 2. Trusted raw-session lookup inside the SDK boundary

The diagnostic event intentionally exposes aliases only, while timeline
encryption info contains the full session identity inside Rust. Add the minimum
vendored SDK accessor needed to bridge those two trusted values without exposing
an identifier:

- retain at most 128 successful recent `(room_id, session_id) -> closed reason`
  entries inside `RoomKeyDiagnosticHub`, oldest-first;
- insert only when creation reports `Created` with a new session ID; an exact
  duplicate key refreshes its reason in place without consuming another slot,
  while reuse/failure without a new created session adds no lookup claim;
- query through `OlmMachine` and `matrix_sdk::Encryption` using raw room/session
  arguments, returning only `Option<RoomKeyRotationReason>`;
- reset naturally with the owning crypto machine; no disk persistence;
- no accessor returns raw stored keys, aliases, or ledger contents.

Record the additive accessor in
`docs/upstream/matrix-rust-sdk-feedback.md`. It is diagnostic-only and suitable
for upstreaming. No crypto, rotation, share, or store behavior changes.

### 3. Rust-owned event-details projection

Add a closed app-owned `TimelineMegolmSessionReason` enum to
`TimelineMessageSource`, covering every current SDK reason plus
`notRetained`. During message-source projection:

1. read `EncryptionInfo` once;
2. keep the existing 12-character local fingerprint presentation;
3. compare `EncryptionInfo.sender_device` with the active
   `MatrixClientSession.info.device_id`;
4. only for an exact current-device match, query the SDK reason with the full
   room/session values inside Rust;
5. map `None` to `NotRetained`; for other devices omit the reason field.

Do not infer current-device ownership from user ID alone. Do not send the full
session ID or diagnostic alias to React. Keep custom `Debug` redaction and the
checked-in Rust/TypeScript wire artifact synchronized.

### 4. Presentation

In `MessageSourceDialog`, add one localized row under Encryption details only
when the Rust DTO contains the reason. Map the closed enum to catalog labels,
including `Reason unavailable`. React does not correlate, classify, or infer a
reason.

No persistence is added. After restart, crypto-machine replacement, ledger
eviction, or missing historical instrumentation, a locally originated event
truthfully shows `Reason unavailable`.

## Verify-first evidence

Add failing tests before implementation:

1. `koushi-diagnostics`: a rotation boundary survives flooding/overflow of the
   general ring; multiple rooms/sessions remain independently attributable;
   dedicated overflow evicts oldest deterministically and increments its own
   dropped counter; reset clears entries and that counter, and aliases cannot
   cross runtime reset;
   serialized fields reject forbidden identifier/fingerprint/content/key/error
   names and fixture values.
2. vendored `matrix-sdk-crypto` / `matrix-sdk`: exact room+session lookup returns
   the retained closed reason, rejects another room/session, evicts oldest at
   the bound, and resets with a new machine.
3. `koushi-sdk`: every rotation reason maps to the exact closed token; ordinary
   ring churn cannot erase the dedicated record; first-event observation updates
   only the matching session alias.
4. `koushi-core`: current-device encrypted source receives the retained reason;
   a different/unknown sender device receives no reason; a current-device
   session absent from the SDK ledger receives `NotRetained`; `Debug` and wire
   serialization expose no full session identity.
5. frontend: retained reason and unavailable reason render the expected
   localized label; absent reason renders no attribution row.
6. wire contracts: update both the checked-in CoreEvent artifact test and the
   companion timeline-source key-completeness test so the new optional field is
   proven in Rust and TypeScript.

## Verification gates

Focused while iterating:

```bash
cargo test -p koushi-diagnostics
cargo test -p matrix-sdk-crypto room_key_rotation
cargo test -p matrix-sdk room_key_rotation
cargo test -p koushi-sdk room_key_rotation
cargo test -p koushi-core --lib message_source
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib core_event_wire_format_matches_checked_in_contract_artifact
npm --prefix apps/desktop run test -- --run src/components/TimelineView.interactions.test.tsx src/i18n/messages.test.ts
npm --prefix apps/desktop run typecheck
```

Before PR:

```bash
node scripts/check-sdk-submodule.mjs
cargo fmt --all -- --check
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
node scripts/check-docs-canon.mjs
git diff --check
```

The PR may omit a disposable-homeserver lane only if the finished diff remains
strictly diagnostic/presentation-only and focused SDK integration tests prove
the observer/query boundary; CI's existing homeserver jobs remain mandatory.

## Acceptance mapping

- General-ring overflow cannot erase all retained recent rotation attribution:
  dedicated-ledger overflow test and export test.
- Each retained anonymous room/session has its own reason and outcomes:
  multi-room/session test.
- Aggregate counters are never a fallback attribution source: missing lookup
  maps directly to `NotRetained`.
- Local current-device event details show a closed reason; other-device events
  do not claim local attribution.
- Retention is bounded, reset with runtime replacement, and explicitly counted
  on eviction.
- Existing Matrix behavior is unchanged; the vendored diff is additive lookup
  and bounded diagnostic storage only.
