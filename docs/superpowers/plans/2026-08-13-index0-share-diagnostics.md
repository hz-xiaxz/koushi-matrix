# Initial Index-0 Key-Share Diagnostics Implementation Plan (Issue #509)

> **For agentic workers:** Implement task-by-task with a failing focused test
> before each production change (RED → GREEN). Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Make one privacy-safe diagnostics export identify, for a newly
created outbound Megolm session, which anonymous eligible device class did not
settle its index-0 room-key share before the first room event consumed index 0
— without changing key-sharing behavior, retries, or recipient policy, and
without ever claiming recipient delivery from homeserver acceptance.

**Architecture:** Extend the vendored Matrix Rust SDK's existing
`RoomKeyDiagnosticHub` with a typed initial-share observer event emitted from
the authoritative share machinery (`share_room_key`, `mark_request_as_sent`,
first room-event `encrypt`, and the to-device send failure path). The hub maps
raw Matrix identifiers to process-local ordinal aliases; only aliases, closed
stage tokens, device-policy class tokens, counts, and message indices leave the
crate. Koushi projects each event to a fixed-token `koushi-diagnostics` record
and increments aggregate counters that are exported independently of the
bounded detail ring.

**Tech Stack:** Rust, matrix-sdk-crypto (vendored), matrix-sdk, koushi-sdk,
koushi-diagnostics.

## Global Constraints

- Implement GitHub Issue #509 and no unrelated behavior. Do not change
  recipient policy, retry timing, rotation policy, or any send behavior.
- Typed observer and authoritative device/share-state observation live in the
  Matrix Rust SDK. Koushi records only closed tokens and aggregate counters.
- Diagnostics never contain Matrix IDs, Device IDs, room/session/event IDs,
  transaction IDs, sender/identity keys, ciphertext, key material, message
  content, deterministic hashes, display names, homeserver URLs, or raw errors.
- Per-device identity is an in-memory ordinal alias valid only for the current
  `OlmMachine` runtime; aliases are stable across the initial share and later
  unwedge re-share / `m.room_key_request` diagnostics for the same device.
- `homeserver_accepted` never implies `recipient_decrypted`; the record states
  `recipient_decrypted=unknown` explicitly where relevant.
- Aggregate counters survive detailed-record ring-buffer eviction.
- SDK changes are committed as independent commits in the vendored submodule;
  the root gitlink update is a separate commit in the Koushi PR.

---

### Task 1: SDK — add failing tests for the initial-share observer

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/room_key_diagnostics.rs` (hub unit tests)
- Add: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/tests/initial_share_diagnostics.rs` (machine integration tests)
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/tests/mod.rs` (register module)

Add tests that fail against the current SDK:

1. `initial_share_device_stages_are_distinct_and_closed` — every per-device
   stage token (eligible, olm_missing, olm_encrypted, olm_encryption_failed,
   withheld, request_queued, homeserver_accepted, request_failed,
   share_state_committed) produces a distinct typed event; `Debug` output of
   the event list contains none of the synthetic user/device/room/session IDs
   or key material used to emit them.
2. `initial_share_records_index0_outcome_for_every_eligible_device` — a
   machine-pair `share_room_key` + `mark_request_as_sent` emits, for every
   eligible receiver device, `Eligible → OlmEncrypted → RequestQueued →
   HomeserverAccepted → ShareStateCommitted { message_index: 0 }` in order,
   keyed by one stable anonymous device alias.
3. `share_state_is_not_committed_before_the_request_is_acknowledged` — after
   `share_room_key` but before `mark_request_as_sent`, no
   `ShareStateCommitted` event exists for the session; after
   `mark_request_as_sent` it does, with `message_index == 0`.
4. `first_encrypted_event_correlates_with_the_initial_session` — after the
   first `encrypt_room_event`, a session-scoped record reports
   `first_event_message_index == 0`, `all_initial_shares_settled_first ==
   true`, and eligible own/peer device counts, using only aliases.
5. `later_unwedge_reshare_correlates_by_device_alias` — the same synthetic
   device receives the same anonymous alias in the initial-share events and in
   the later Olm-unwedge re-share event.

- [x] **Step 1:** Add the failing tests above.
- [x] **Step 2:** Run the focused tests and confirm RED:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto initial_share --features testing
```

### Task 2: SDK — implement the typed observer and alias correlation

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/room_key_diagnostics.rs`

Add:

- `InitialShareDeviceClass` — `VerifiedOwn | UnverifiedOwn | VerifiedPeer |
  UnverifiedPeer | Dehydrated | Unknown` (closed device-policy class).
- `InitialShareStage` — `Eligible | OlmMissing | OlmEncrypted |
  OlmEncryptionFailed | Withheld | RequestQueued | HomeserverAccepted |
  RequestFailed | ShareStateCommitted { message_index: u32 }`.
- `InitialShareDeviceDiagnostic` — session alias, device alias, device class,
  stage, elapsed_ms.
- `InitialShareSessionDiagnostic` — session alias, first-event message index,
  `all_initial_shares_settled_first`, pending-request count bucket, eligible
  own/peer device counts, index-0 / after-0 committed-share counts,
  homeserver-accepted device count, `created_at_index0`.
- `RoomKeyDiagnosticEvent::InitialShare(...)` variants for device and session
  records.
- Hub state: per-device class cache, per-session eligible/committed/ accepted
  tallies, first-event-reported set.
- Hub methods: `emit_initial_share_device` (class falls back to the cached
  class when the caller has no `DeviceData`), `emit_initial_share_session`.
- Add the device alias to `OlmRecoveryDiagnostic` so unwedge re-share
  diagnostics correlate with the initial share by device alias.

- [x] **Step 3:** Implement the types, hub state, and hub methods.
- [x] **Step 4:** Run the hub unit tests and confirm GREEN.

### Task 3: SDK — wire emission into the authoritative share machinery

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/session_manager/group_sessions/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/olm/group_sessions/outbound.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk-crypto/src/machine/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/encryption/mod.rs`
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/client/mod.rs`

Emit points (observation only; no behavior change):

1. `GroupSessionManager::share_room_key` — after the share-state filter:
   `Eligible` per targeted device; after `encrypt_for_devices`:
   `OlmMissing` per `no_olm` device; for policy-withheld devices from
   recipient collection: `Withheld`; for newly queued `m.room_key` to-device
   requests: `OlmEncrypted` and `RequestQueued` per device. Only new requests
   (txn ids not present before this pass) are reported.
2. `encrypt_session_for` / `encrypt_request` — pass the hub through; on a
   per-device Olm encryption error, emit `OlmEncryptionFailed` for that exact
   device, then propagate the error exactly as today.
3. `GroupSessionManager::mark_request_as_sent` — for each device whose
   `ShareInfo` moved from pending to shared: `HomeserverAccepted`, then
   `ShareStateCommitted { message_index }`. Add a crate-internal
   `pending_share_infos` accessor on `OutboundGroupSession` to read the infos
   before removal.
4. `GroupSessionManager::encrypt` — on the first encrypted event for a
   session, emit the session-scoped record (`message_index` read before
   encryption; `all_settled` = no pending to-device requests remain).
5. Failure path — new `OlmMachine::note_to_device_request_failed(request_id)`
   delegating to `GroupSessionManager::note_to_device_request_failed`, which
   resolves the request's devices from the pending-request set and emits
   `RequestFailed` per device. Called from `Client::send_to_device` when the
   homeserver request errors (the single choke point used by the sync loop and
   the room preshare path). A request that later succeeds still emits
   `HomeserverAccepted`; `RequestFailed` documents an attempt, never a
   terminal refusal.
6. `reshare_unwedged_room_key` / `unwedged_affected_room_ids` — pass the
   device alias into the existing OlmRecovery emissions.

- [x] **Step 5:** Implement the emission points.
- [x] **Step 6:** Run the machine integration tests and confirm GREEN.
- [x] **Step 7:** Run the full focused suite (SDK):

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto initial_share --features testing
cargo test -p matrix-sdk-crypto room_key_diagnostics --features testing
cargo fmt --all -- --check
```

### Task 4: Koushi — add failing projection tests

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs` (tests)

Add tests mirroring the receive-side diagnostics test:

1. `record_initial_share_diagnostic_records_closed_tokens_and_counters` — drive
   the projection with each device stage and a session record; assert the
   exact fixed tokens (`operation=core.initial_share stage=...`) and that each
   aggregate counter was incremented exactly once.
2. `initial_share_diagnostics_never_expose_private_values` — serialize the
   captured `koushi-diagnostics` records and assert that synthetic user,
   device, room, session IDs, keys, ciphertext, and raw error strings are
   absent.
3. `initial_share_counters_survive_detail_ring_eviction` — assert the
   aggregate counter export is independent of the bounded detail ring
   (koushi-diagnostics ring behavior).

- [x] **Step 8:** Add the failing tests and confirm RED:

```bash
cargo test -p koushi-sdk initial_share
```

### Task 5: Koushi — implement the closed-token projection

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs`

- Add `record_initial_share_diagnostic` handling the new
  `RoomKeyDiagnosticEvent::InitialShare` variants:
  - device records: `core.initial_share` / `stage` with
    `session_alias`, `device_alias`, `device_class`, `stage`,
    `message_index` (share-committed only), `elapsed_ms`;
  - session records: `core.initial_share` / `first_event` with eligible
    own/peer counts, first-event index, settled-first flag, pending bucket,
    committed-share index-0 / after-0 counts;
  - increment the matching aggregate counter for every event.
- Add the new counter names to the reset list in
  `install_room_key_diagnostic_observer`.
- Extend `record_olm_recovery_diagnostic` to carry the device alias when the
  SDK event provides it (correlation with the initial share).

- [x] **Step 9:** Implement the projection.
- [x] **Step 10:** Run the focused tests and confirm GREEN:

```bash
cargo test -p koushi-sdk initial_share
```

### Task 6: integrated gates, submodule sync, PR

**Files:**
- Modify: `docs/superpowers/plans/2026-08-13-index0-share-diagnostics.md` (mark tasks done)
- Modify: `docs/agents/plans.md` (index the new plan)
- Modify: `vendor/matrix-rust-sdk` (gitlink update)

- [x] **Step 11:** Run the integrated gates:

```bash
cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto --features testing
cd ../.. && cargo test -p koushi-sdk --lib
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
node scripts/check-sdk-submodule.mjs
git diff --cached --check
```

- [ ] **Step 12:** Commit the SDK changes in the vendored submodule
  (independent, focused commits), update the root gitlink, commit the Koushi
  changes, push the branch, and open the PR referencing #509 with the privacy
  and no-behavior-change notes.

## Out of scope

- Recipient policy, retry timing, rotation policy, or any send behavior change.
- The bounded index-0 re-share (#510) and historical forwarding (#461).
- Recipient-side decryption acknowledgement (explicitly not invented).
- Bundle-based room-key sharing (`share_room_key_bundle_data`).
