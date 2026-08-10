# Receive-Side Room-Key Lifecycle Diagnostics and Late-Decryption Protection — Implementation Plan

> **For agentic workers:** Implement task-by-task with focused tests before broader
> verification. Follow the repository verify-first discipline: the reproducible
> headless check comes first, the fix second.

**Goal:** Implement GitHub Issue #476. Add privacy-safe diagnostics for the
receiving side of encrypted room-key delivery so one exported diagnostic report
can distinguish (1) transport/Olm failure, (2) Megolm merge decision, and
(3) late-decryption/UI failure, and provide a bounded local late-decryption
retry path that does not request or redistribute additional keys.

**Architecture:** The pinned Matrix Rust SDK owns the receive pipeline
(to-device ingress → Olm decrypt → Megolm merge → event-cache redecryption).
Koushi registers a typed observer on the SDK crypto machine during session
restoration (before sync can deliver to-device events), accumulates aggregate
counters in `koushi-diagnostics`, and snapshots the event-cache redecryptor
health through the SDK. No tracing strings are parsed; no identifiers or
cryptographic material enter diagnostics.

**Tech Stack:** Rust, matrix-rust-sdk (vendored submodule), matrix-sdk-crypto,
matrix-sdk, koushi-sdk, koushi-core, koushi-diagnostics.

## Global Constraints

- Implement Issue #476 and no unrelated behavior. Do not change encryption,
  key sharing, device tracking, trust policy, retry timing, or send behavior.
- SDK changes are committed in the vendored submodule, the root gitlink is
  updated, and `node scripts/check-sdk-submodule.mjs` stays green.
- Never include: room/user/device/event/session/request IDs, sender keys, key
  material, ciphertext, message content, raw SDK/server errors, exact recipient
  identity, or per-device aliases. Counts, booleans, bounded duration buckets,
  event-kind tokens, and closed error categories only.
- Diagnostics stay safe to share publicly; existing exports remain unchanged.
- Correlation uses aggregate counters and in-memory opaque tokens only; nothing
  cryptographic is persisted.
- Keep the implementation bounded: no new scheduler, no incident database, no
  new background task beyond what the SDK already runs.
- Existing E2EE behavior and Matrix interoperability remain unchanged.

## SDK Surface (vendored matrix-rust-sdk)

### Crypto machine (matrix-sdk-crypto) — typed receive events + counters

Extend the existing `RoomKeyDiagnosticHub` (introduced for #459 send-side
diagnostics) with receive-side aggregate counters and a new observer event:

```rust
pub enum RoomKeyReceiveDiagnosticKind {
    RoomKeyIngress { kind: RoomKeyIngressKind },   // Direct | Forwarded (observed post-decrypt)
    ToDeviceOlmFailed,                             // Olm decrypt failure, encrypted to-device events
    ToDeviceOlmWedged,                             // Olm session wedged
    ToDeviceDehydratedRejected,                    // rejected because sender is a dehydrated device
    ToDeviceMalformed,                             // malformed/unsupported to-device payload
    RoomKeyUnsupportedAlgorithm,
    ForwardedRoomKeyAuth { outcome: ForwardedRoomKeyAuthOutcome },
    Merge { decision: RoomKeyMergeDecision },
}

pub enum RoomKeyIngressKind { Direct, Forwarded }
pub enum ForwardedRoomKeyAuthOutcome {
    RejectedNoMatchingRequest, RejectedUntrustedSender, UnsupportedAlgorithm, Accepted,
}

// Merge decisions are ACCEPTANCE DECISIONS, not persistence confirmations:
// `merge_received_group_session` returns a session that is persisted later by
// `save_changes` (sync) or `save_inbound_group_sessions` (import). Persistence
// success is confirmed by the existing post-save room-key broadcast
// (crypto_store_wrapper `save_changes`); a persistence failure is reported as
// `Merge { StoreFailed }` at the save boundary. Acceptance and persistence
// totals are reported as separate aggregate counters (no cross-correlation);
// the Koushi summary presents accepted-new/improved counts, the broadcast
// count, and the StoreFailed count independently, which distinguishes
// "accepted but not persisted" from "persisted but timeline stale".
pub enum RoomKeyMergeDecision {
    AcceptedNew, AcceptedImproved, DuplicateIgnored, WorseIgnored, UnconnectedRejected,
    InvalidSessionKey, StoreFailed,
}
```

`RoomKeyDiagnosticEvent` gains `Receive(RoomKeyReceiveDiagnostic)` where the
diagnostic carries `kind` plus a coarse elapsed bucket. The hub keeps
`u64`-style aggregate counters for every token above (guarded by the existing
mutex) and emits the typed event to the observer.

Hooks (all inside matrix-sdk-crypto, private paths; no behavior change):

1. `machine/mod.rs::receive_to_device_event` — deserialize failure. Peek the
   raw event type first (same technique as `record_message_id`); count
   `ToDeviceMalformed` only when the raw `"type"` is `m.room.encrypted`, so
   unrelated invalid to-device events are excluded.
2. `machine/mod.rs::receive_encrypted_to_device_event` — `OlmError` arm →
   `ToDeviceOlmFailed`, plus `ToDeviceOlmWedged` when `SessionWedged`; the
   `FromDehydratedDevice` arm → `ToDeviceDehydratedRejected`; the post-Olm
   deserialize failure arm ("invalid encrypted to-device event") →
   `ToDeviceMalformed` (malformed decrypted payload).
3. `machine/mod.rs::handle_decrypted_to_device_event` — `RoomKey(e)` →
   `RoomKeyIngress { Direct }`; `ForwardedRoomKey(e)` →
   `RoomKeyIngress { Forwarded }`; unexpected encrypted custom payloads
   (`AnyDecryptedOlmEvent::Custom`) → `RoomKeyUnsupportedAlgorithm`.
4. `machine/mod.rs::handle_key` — `InboundGroupSession::from_room_key_content`
   error → `Merge { InvalidSessionKey }`; `add_room_key` unknown algorithm →
   `RoomKeyUnsupportedAlgorithm`.
5. `store/mod.rs::merge_received_group_session` — the single choke point for
   merge ACCEPTANCE decisions: no old session → `AcceptedNew`; any merge arm
   returning `Some` → `AcceptedImproved`; `(Equal, Equal)` →
   `DuplicateIgnored`; worse arms → `WorseIgnored`; `(Unconnected, _)` →
   `UnconnectedRejected`. The `Store` gains an optional hub handle set at
   machine construction (`OlmMachine::new_helper`), so backup-import and
   forwarded-key paths are covered too. Persistence failure is reported at the
   save boundary: in `machine/mod.rs::receive_sync_changes`, when
   `changes.inbound_group_sessions` is non-empty and `save_changes` fails →
   `StoreFailed`; likewise in the import path (`store/mod.rs`) around
   `save_inbound_group_sessions` → `StoreFailed`. `StoreFailed` is always a
   distinct decision/counter token separate from the acceptance decisions, and
   is reported independently as `Merge { StoreFailed }`.
6. `gossiping/machine.rs` — `receive_forwarded_room_key`
   `ForwardedRoomKeyContent::Unknown` arm → `ForwardedRoomKeyAuth {
   UnsupportedAlgorithm }` (this arm never reaches `receive_supported_keys`);
   `receive_supported_keys` no room-key info → `ForwardedRoomKeyAuth {
   UnsupportedAlgorithm }`; no matching request → `RejectedNoMatchingRequest`;
   `should_accept_forward` false → `RejectedUntrustedSender`; accepted →
   `Accepted` (merge decision follows from hook 5); `accept_forwarded_room_key`
   `InboundGroupSession::try_from` error → `Merge { InvalidSessionKey }`.

Public accessor on `OlmMachine`: `room_key_receive_counters() ->
RoomKeyReceiveCounters` (closed snapshot, `Copy`-style fields of `u64`).
Re-exported through `matrix-sdk::encryption` like the existing diagnostics.

### Event cache (matrix-sdk) — late-decryption counters + health

Add `RoomKeyLateDecryptionCounters` owned by the `Redecryptor` task, exposed as
a snapshot on `EventCache`:

- `room_key_updates_broadcast` — room-key batch received on the stream
- `redecryption_requests` — `retry_decryption` invocations (stream-driven)
- `explicit_retry_requests` — `DecryptionRetryRequest` handled
- `matching_events_bucket` — bucketed count of UTD events matched per attempt
  (0, 1, 2-5, 6-20, 21-100, 101+)
- `redecryption_succeeded` / `redecryption_remained_utd` / `redecryption_failed`
- `redecryption_store_failed` — cache/store update errors from `on_resolved_utds` /
  `retry_decryption_for_events` error returns
- `room_key_stream_lagged` / `room_key_stream_recreated`

Hooks in `event_cache/redecryptor.rs` (private, no behavior change):
`redecryption_loop` room-key arm, `Some(Err)` lag arm, `None` recreated arm,
`DecryptionRetryRequest` arm, and `retry_decryption_for_events` outcome
counting. `decrypt_event` currently maps both "still UTD" and "SDK decryption
error" to `None`; change it to return a private closed outcome
(`Decrypted | StillUtd | Failed`) so the counters distinguish
`redecryption_succeeded` / `redecryption_remained_utd` /
`redecryption_failed`.

Public: `EventCache::room_key_receive_diagnostics() -> RoomKeyLateDecryptionDiagnostics`
(counts + `has_subscribed()` + redecryptor task alive flag).

## Koushi Integration

### koushi-sdk

- Extend `install_room_key_diagnostic_observer` to handle
  `RoomKeyDiagnosticEvent::Receive` → reset/increment the matching
  `koushi_diagnostics` counters and record `core.room_key_receive` diagnostic
  events with fixed tokens only.
- New wrapper `room_key_receive_diagnostics(session)` combining the crypto
  counters, event-cache counters, and event-cache subscription/redecryptor
  health into one typed snapshot.
- New wrapper `request_late_decryption(session, room_id, utd_session_ids)`
  → `client.event_cache().request_decryption(...)` for the bounded local retry
  path (caller supplies only the session IDs of events it still shows as UTD).

### koushi-core

- Register the receive-side observer during session restoration in
  `account.rs::restore_into_store`, immediately after
  `restore_session_with_store` + `enable_event_cache` and before sync starts.
- On a `Merge { decision: … }`/`RoomKeyIngress { kind: … }` observation and on
  the diagnostics summary request, record `core.room_key_receive_summary` with
  the aggregate counters and event-cache health (transport/Olm, merge,
  late-decryption groups).
- Bounded local retry is driven from the SDK's existing
  `subscribe_to_decryption_reports()` stream, never from merge/ingress
  observer events (those run before persistence) and never from a second
  room-key broadcast subscription (the SDK redecryptor already consumes it and
  auto-retries). The koushi-core session runtime consumes the reports stream;
  on `RedecryptorReport::Lagging` or `BackupAvailable`, it issues one
  coalesced `request_late_decryption` per open room for the session IDs of the
  events the visible timeline still shows as UTD (bounded per-session
  coalescing window, no `m.room_key_request`/device gossip and no key
  redistribution; the SDK's existing on-demand backup retrieval may run as
  part of its normal decryption path). The diagnostics-summary request exposes
  the same bounded command as a manual path. Retry outcomes are counted via
  the event-cache counters.
- Count visible-timeline late-decryption replacements when a timeline diff
  replaces a UTD item with a decrypted one (timeline actor, same observation
  point as the #466 retry work).

## Tests

Synthetic users/devices/sessions/keys only. Required coverage (from Issue #476):

1. Direct re-shared `m.room_key` → ingress Direct counted, `AcceptedNew`, then
   post-save broadcast and (event-cache integration) a UTD event late-decrypts.
2. Wedged Olm event → `ToDeviceOlmWedged` counted, never reported as
   Megolm-stored.
3. Duplicate key → `DuplicateIgnored` (benign, no warning).
4. Better ratchet index → `AcceptedImproved` (replaces/merges stored copy) and
   triggers redecryption via the broadcast.
5. Unconnected ratchet → `UnconnectedRejected`.
6. Forwarded-key authorization rejection distinct from direct-key processing
   (`RejectedNoMatchingRequest` / `RejectedUntrustedSender`); forwarded
   unsupported algorithm and forwarded invalid session key reach their own
   tokens (`ForwardedRoomKeyAuth { UnsupportedAlgorithm }` /
   `Merge { InvalidSessionKey }`); the forwarded ACCEPTED path reaches the
   merge decision (`AcceptedNew`/`AcceptedImproved`).
7. Event-cache stream lag → bounded retry/report, not silent staleness
   (`room_key_stream_lagged` + `redecryption_requests`); `StillUtd` vs `Failed`
   vs store-update failure are distinguishable; `room_key_stream_recreated`
   increments when the stream is recreated after a close.
8. Key stored while room open → timeline updates without room reselection;
   the visible-timeline replacement counter counts only a UTD item replaced by
   a decrypted one, not arbitrary decrypted `Set` diffs.
9. Key stored while room closed → decrypts correctly when opened.
10. Persistence failure path: a failing `save_changes` with pending inbound
    sessions yields `Merge { StoreFailed }` and no broadcast; the import path
    failure around `save_inbound_group_sessions` yields the same token.
11. Malformed accounting: outer deserialize failure counts only when the raw
    type is `m.room.encrypted` (non-encrypted invalid to-device events are
    excluded); post-Olm deserialize failure and unexpected encrypted custom
    payloads are counted separately.
12. Privacy tests: synthetic IDs, names, raw errors, and key material never
    appear in diagnostic or `Debug` strings.

## Verification Gates

```bash
cargo test -p matrix-sdk-crypto      # SDK workspace: focused + privacy tests
cargo test -p matrix-sdk             # event-cache counter tests
cargo test -p koushi-core --lib      # Koushi summary/retry/privacy tests
cargo test -p koushi-sdk --lib
cargo test -p koushi-state --lib
cargo test -p koushi-desktop
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
cargo fmt --all -- --check
git diff --check
node scripts/check-sdk-submodule.mjs
```

## Delivery

1. SDK: commit on `codex/room-key-receive-diagnostics` (base 51b6c125d), push to
   `shinaoka/matrix-rust-sdk-work`.
2. Koushi: commit on `codex/issue-476-room-key-receive-diagnostics` (base
   origin/main), bump the gitlink, push, open the implementation PR referencing
   #476, iterate on review, merge non-squash, confirm the issue is settled.
3. Independent design review (frontier reviewer) before implementation; review
   the full diff after implementation and fix findings.
