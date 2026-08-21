# Issue #577 phase 1 — new-session encryption readiness

Status: Phase 1 design reviewed before implementation. `reviewer-gpt` round 3
verdict on 2026-08-21: `Correct-to-implement`; canon verdict: `Canon-approved`.
Earlier rounds required fixes for retry bypass, two-proof restart publication,
registry bounds, restored index-0 handling, and the impossible synthetic
late-recovery claim; those findings are reflected below.

## Objective

Prevent an eligible device already visible in an authoritative homeserver key
query from missing the first event of a new or rotated outbound Megolm session.
The sender must not consume index 0 until the current encryption-sync generation,
a full active-member `/keys/query`, and a repeated standard pre-share settle.
Failure preserves a typed retryable pending send. Sync owner failures recover
without stale-generation publication, and diagnostics remain privacy-safe.

This phase does not query before every message, add a Matrix event or recipient
acknowledgement, downgrade to plaintext, enable #510/#523, change #541's original
recipient ledger, or claim recipient decryption from homeserver acceptance.

Issue #577 remains open after this independently mergeable phase. Its requested
positive historical recovery for a device discovered only after index 0 needs a
separate product decision: standard `/keys/query` responses contain no key-upload
time or other proof that such a device existed before index 0. Current visibility,
membership, timing, aliases, and aggregate counters cannot safely establish
historical entitlement. This phase therefore fails closed and uses only standard
Matrix gossip/request/backup policy for a post-fence device; it neither adds an
unreachable synthetic recovery path nor silently narrows the unresolved issue
criterion.

## Confirmed defect and constraints

- `account/trust_gate.rs::start_provisional_encryption_sync` breaks permanently
  after a failure before its first response. Post-first-response failures already
  retry inside the owned loop.
- An unexpectedly ended steady `SyncService` reaches `State::Terminated`; Core
  currently turns that into `SyncTaskOutcome::Failed` and requires an explicit
  restart.
- `ensure_room_encryption_ready` syncs members, queries only untracked/dirty
  users, and pre-shares. A tracked, locally clean user can retain a stale device
  set when encryption sync has not committed `device_lists.changed`.
- `preshare_room_key` is serialized by the room transport lock and can create or
  rotate an outbound session before an event consumes index 0.
- `Client::keys_query` returns only after HTTP success, crypto-store commit, and
  existing SDK recovery hooks.
- #523's missing-Olm repair is disabled in Koushi and remains independent.

## 1. Generation-scoped encryption-sync readiness

Add client-owned `EncryptionSyncReadiness`, enabled by a builder option that
Koushi turns on in `desktop_client_builder_defaults`:

```text
NotStarted
  -> Pending(generation)       encryption sync stream starts
  -> Received(generation)      first response commits
  -> Failed(generation)        stream reports error
  -> Cancelled(generation)     stream ends or is dropped
```

Generations are monotonic and process-local. Each
`EncryptionSyncService::sync` stream owns an RAII generation guard. Creation
marks `Pending`; the first successfully committed response marks only its exact
generation `Received`; an error marks `Failed`; stream end/drop marks
`Cancelled` only while the guard still owns the current generation. A waiter
subscribes before rechecking the authoritative snapshot and follows a newer
replacement generation. Stale success, failure, and drop cannot satisfy or
cancel the replacement.

The API exposes a closed snapshot and watch receiver only. No sync position,
request ID, user/device identity, endpoint, or raw error crosses it.

## 2. Existing lifecycle owner recovery

### Provisional encryption sync

Before the first response, report a closed failed-attempt diagnostic, retain the
one AccountActor-owned task, back off 250 ms, recreate the SDK service/generation,
and retry until success or actor cancellation. A later first success performs
the existing verification-method discovery and normal handoff. The retry loop
creates no detached owner.

### Steady sync

Pass the retained `Arc<SyncService>` and encryption-readiness watch into
`observe_sync_service`. On unexpected `State::Terminated`:

1. project `Reconnecting` and record the exact lost generation;
2. back off 250 ms and call the service's idempotent `start()`;
3. observe the replacement `Pending(generation)` and ignore the SDK's immediate
   `Running` publication as connection proof;
4. require both a committed room-list response and `Received` for that exact
   replacement encryption generation;
5. only then project `Running`/`Recovered`.

Either proof can arrive first. A replacement generation, another termination,
or actor cancellation clears the partial pair; stale observations are inert.
Explicit SDK `stop()` reaches `Idle`; the actor projects that orderly stop as
`Stopped` through its existing cancel/join path and never restarts it. Logout,
lock, account switch, session replacement, and
runtime shutdown keep their current stop barriers.

## 3. Per-session readiness fence

Add a bounded client-local fence registry keyed by the exact room/outbound
session token. It holds at most 128 entries and exposes only
`Unfenced|Fencing|Ready`; oldest eviction is counted. An evicted in-process
session that is still at index 0 is treated as unfenced, including a restored
session with no registry entry. Only a pre-existing restored session already
beyond index 0 bypasses the prospective fence and follows standard behavior; it
is not misreported as successfully fenced.

`Room::preshare_room_key_with_readiness` owns the existing room transport lock:

1. capture the current outbound session token and index;
2. perform existing `preshare_room_key_locked`;
3. capture the resulting token and index;
4. when the token changed, a current registry entry is `Unfenced`, or the
   resulting session is still at index 0 with no entry, set that exact session
   `Fencing` and begin one absolute 10-second deadline;
5. require `Received` for the current encryption-sync generation, following any
   replacement generation;
6. load active room members and issue one out-of-band `/keys/query` covering all
   of them, not only untracked/dirty users;
7. await and commit the response through `Client::keys_query`;
8. repeat existing standard pre-share under the same transport lock;
9. require the exact session to remain current and at index 0;
10. mark only that exact registry entry `Ready`, then release event encryption.

A matching `Ready` entry bypasses the full query. An unchanged token with no
entry bypasses only when it is already beyond index 0; every no-entry index-0
session is fenced regardless of provenance. Critically, every failure after
session creation leaves the entry
`Unfenced`, so retry cannot mistake the resident session for an unchanged ready
session. Rotation invalidates the prior entry.

No raw key-query response or per-device proof is retained in this phase. The
second standard pre-share uses the authoritative committed SDK store and normal
recipient/trust/blacklist/history policy.

`ensure_room_encryption_ready` uses this method for Koushi clients. A closed
`EncryptionReadinessError` identifies
`sync|key_query|second_share|session_changed|deadline|cancelled`. The send queue
classifies it as recoverable, keeps the queued item pending/not sent, and does
not encrypt or send a room event. Retry runs the same unfenced entry. There is
no plaintext fallback or unbounded wait.

A device uploaded only after the authoritative response receives no historical
entitlement from this fence. It remains on standard Matrix recovery policy.

## 4. Diagnostics

Add low-volume closed records:

- `core.encryption_sync_lifecycle`: owner `provisional|steady`, generation,
  stage `created|first_request|first_response|failed|terminated|handoff|replaced`,
  and elapsed bucket;
- `core.encryption_readiness`: anonymous room/session aliases, sync state,
  query state, fence outcome, active/returned/shared count buckets, message-index
  bucket, registry-eviction count, and retryable boolean.

Never export user/device/room/event/session/request/transaction IDs, sync
positions, fingerprints, deterministic hashes, identity/sender keys, key
material, homeservers, message content, URLs, or raw errors. Homeserver
acceptance is `accepted`, never `delivered` or `decrypted`.

## Verify-first matrix

1. **Provisional RED:** first attempt fails/times out before a response and the
   task currently ends. **GREEN:** one owner retries, accepts a later response,
   and cancellation joins it.
2. **Steady RED:** child termination currently settles Failed. **GREEN:** one
   replacement reaches Reconnecting and cannot reach Running until matching
   room-list and encryption-response proofs both commit.
3. Stale success/failure/drop, second termination during partial recovery,
   restart, logout, account switch, and runtime replacement are inert/cancelled.
4. Tracked user has a pending server-side device change at rotation: index 0
   remains unconsumed until full query commit and second pre-share.
5. Full query introduces an eligible absent device: standard pre-share accepts
   its index-0 key before room-event encryption.
6. Matching `Ready` unchanged session performs no full query or extra pre-share.
7. Failed first fence followed by retry uses the resident `Unfenced` session and
   cannot bypass the query/second share.
8. Missing first response, key-query failure/rate limit, second-share failure,
   session replacement, cancellation, deadline, and registry eviction preserve
   a retryable pending event with no room event/plaintext/index consumption.
9. A restored session beyond index 0 is explicitly classified `legacy`, not
   `Ready`, and receives no false readiness claim.
10. A device appearing only after the authoritative response is not granted
    historical index 0; standard policy remains unchanged.
11. `olm_missing` follows existing standard/#523-disabled behavior; #510/#523
    flags and #541 ledger are unchanged.
12. Multiple rooms/sessions stay isolated and the registry is truly bounded.
13. Privacy rejection tests cover every forbidden identifier/value in
    diagnostics, DTO/Debug, logs, and evidence.
14. Matrix SDK integration tests use only their test-only mock HTTP responder to
    return the delayed device in the fence's `/keys/query`; no production hook is
    added. Disposable Tuwunel and Synapse tests create/login the second device
    after the sender's initial tracked state but before forcing rotation, then
    cover Koushi sender to Koushi recipient. No recipient-specific protocol is
    introduced, so existing Element X/Web/Desktop compatibility
    remains applicable without claiming recipient decryption.

## Gates

Add focused tests at the owning SDK/core seams before each fix. Run affected
vendor crate suites and:

```bash
node scripts/check-sdk-submodule.mjs
cargo test -p koushi-diagnostics
cargo test -p koushi-sdk --lib
cargo test -p koushi-core --lib
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:ui-headless
cargo deny check
cargo fmt --all -- --check
node scripts/check-agents-docs.mjs
git diff --check
```

Run the relevant disposable homeserver scenario against Tuwunel and Synapse.
Real-account compatibility is not automated without approved credentials;
absence of optional evidence is reported, not replaced by a delivery claim.

## Acceptance mapping for this phase

- Existing eligible server device cannot miss index 0 prospectively: exact
  current-generation response + full member query + second standard pre-share.
- Fence is per new/rotated session and retry-safe: bounded exact-session registry.
- Pending send remains retryable: typed failure before encryption/index use.
- Lifecycle recovers without duplicate owners: retained provisional/steady
  owners and generation-scoped two-proof recovery.
- Late devices fail closed: no inferred historical entitlement or custom share.
- No per-message full query, custom protocol, plaintext fallback, unbounded wait,
  delivery claim, #541 mutation, or #510/#523 enablement.
- The unresolved positive late-recovery criterion remains explicitly open; this
  phase is not presented as complete closure of #577.
