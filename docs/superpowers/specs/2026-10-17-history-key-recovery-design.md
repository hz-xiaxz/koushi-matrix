# Explicit historical key recovery

## Product contract

The primary action on an undecryptable message is
「過去のメッセージの復号鍵を要求」. A server-accepted request displays
「要求済みです」 with a Retry action. This is pending delivery/replies, never a
claim that keys have been obtained. Explain that a responding member's Koushi
must come online; Element X does not implement this Koushi extension.

Core refreshes the joined-member set. The default target is all other joined
members when the total joined population including ourselves is at most 30;
otherwise administrators (power at least 100). Room Info exposes a force-all
action only inside a collapsed Troubleshooting disclosure. Unknown membership
does not select all. An operation exceeding bounded peer/device fanout fails
explicitly, rather than silently selecting a subset.

Requests target only this room's historical keys marked shared_history by the
SDK. Current shared/world_readable visibility and current membership are
required. Both sides recheck current room facts and current device identity.
Key disclosure/import requires verified cross-signing and no verification
violation. Unverified peers may receive a non-secret request but never keys;
the UI must explain verification failures. Acquired keys cannot be revoked.

Previously recorded unresolved rooms may receive one administrator-only retry
on startup. Persist an automatic 24-hour cooldown before dispatch, so repeated
restarts do not cause a request storm. No history crawl or automatic broadcast
to all members is introduced. Explicit retries remain available and advance the
presentation generation; outstanding per-device wire correlations stay stable.
Duplicate in-flight commands coalesce.

## Ownership and transport

RoomActor owns a session-scoped, cancellable Core recovery actor. Rust state
projects per-room Sending / Requested / Partial / Failed status through
RoomManagementState. React only renders it and dispatches a typed room command.
The actor removes its handler and joins owned work before session teardown.

Use Olm-encrypted custom to-device messages, never room chat messages or DMs.
Only authenticated Olm sender context can identify a peer; payload claims are
not identity proof. Persist pending nonces, exact target key pins, expiry,
received-chunk identifiers, byte budgets, and automatic cooldowns in the SDK's
encrypted crypto store, not state-store custom blobs (which are plaintext),
settings, or localStorage. No private key material enters public state.

Replies are independently parseable bounded RoomKeyBundle chunks, avoiding a
new media fetch surface. Each is bound to nonce, room, actual sender device,
immutable response ID, chunk count, and index. Limit each message to 32 KiB,
each responder user across all its devices to 8 MiB, aggregate to 32 MiB,
256 chunks per response, 128 users and 256 devices per request. Conflicting
duplicates, malformed/cross-room keys or withheld records, unsolicited/stale
chunks, changed identities, and exceeded budgets fail closed. Limits produce
explicit partial/too-large outcomes. Responders rate-limit by room/requester,
not attacker-chosen nonce. There is no automatic repeat export loop.

The only SDK extension beyond the approved 30-day invitation window is generic
bundle build/import and encrypted custom-data access. Derive forwarding trust
inside SDK from the authenticated device and preserve SDK merge/provenance
semantics. Never enable test APIs in production, fake invitation records, or
use file import as a substitute for authenticated forwarding.

## Verification and review

Failing headless tests precede production implementation. Cover population
30/31, admin/all selection, retries and stale completions, persistence/restart,
automatic cooldown, malformed and replayed chunks, identity reset, unverified
devices, nonmember/left/banned recipients, visibility changes, cross-room keys,
and fanout/byte/rate bounds. Browser tests cover both entry points, pending text
and retry, collapsed advanced UI, and account/room switching.

Disposable Synapse tests must exercise production Core commands and both
responders, including a join without individual invite, multiple historical
Megolm sessions, an offline responder reconnect, requester restart, admin-only
startup retry, >30 admin selection and forced-all selection, plus security
refusals. No test may manufacture a successful response or import keys itself.
The existing 29/30-day delayed-invite import scenario remains distinct.

Required before ready-for-review: independent security design and integrated
diff review, relevant deterministic gates and live Synapse proofs. The existing
30-day patch is independently releasable; this new protocol is not complete
until the full production path and negative cases pass.

## Reviewed retry and crash semantics

Correlation is per exact pinned peer/device. An unanswered attempt keeps its
original nonce and absolute expiry across retries and restarts; a completed,
terminally refused, expired or rekeyed attempt gets a fresh nonce. Room
operation generations fence presentation updates. Automatic administrator
retry is a dispatch subset and retains still-authorized outstanding manual
recipients. Identical pending manual requests coalesce for 60 seconds; failures
remain retryable. No click extends
the device attempt's expiry. A recovery episode lasts 30 days: room/user byte
budgets survive all retries in that episode and reset only when the whole
expired episode is retired. At most 16 room episodes, 8192 admitted chunks,
2048 chunks per episode and 1024 requester completion tombstones are retained.

Responders cache exact serialized reply chunks in the encrypted SDK crypto
store (8 MiB per response, 32 MiB aggregate). Reserve a bounded `Preparing` index
before export, persist payload before `Ready`, and send only after `Ready` is
durable. Replays use the same response ID/count/index/content, recheck current
membership/history/full identity pin, and observe the same 60-second per
room/requester rate and 32 MiB/minute aggregate egress ceiling. A rate denial
never completes an outstanding request. Key export stops at 4096 sessions or
the bounded serialized-size limit before constructing an oversized response.

The requester records admission separately from successful SDK import. An
interrupted admission retries idempotent import without charging twice. Only
all durably imported chunks permit an authenticated Ack bound to the original
pin, room, nonce and response ID. Completed requester tombstones can re-Ack
cached duplicates without importing or charging again. The responder persists
`Deleting` before clearing payload, then retains a small tombstone; restart
resumes deletion. A missing/corrupt `Ready` payload fails closed and is never
rebuilt with conflicting metadata. Storage failures and operation timeouts stop
the recovery worker: cancellation can leave a write's outcome unknown, so it
must never continue issuing writes. Queued manual commands receive a keyed
storage-unavailable failure. Restart reconciles durable records.

Independent read-only design reviews approved per-device correlation and the
bounded replay cache subject to these invariants. The earlier integrated-diff
review is not a substitute for the final review after all wiring and tests.

## Upstream UX comparison

Inspected on 2026-10-17:

- [Element X iOS encrypted timeline view](https://github.com/element-hq/element-x-ios/blob/develop/ElementX/Sources/Screens/Timeline/View/TimelineItemViews/EncryptedRoomTimelineView.swift)
  selects a message presentation from the SDK's UTD cause.
- [Element X Android encrypted timeline view](https://github.com/element-hq/element-x-android/blob/develop/features/messages/impl/src/main/kotlin/io/element/android/features/messages/impl/timeline/components/event/TimelineItemEncryptedView.kt)
  similarly projects cause-specific text and icons, including history and trust
  failures.
- [Element Web decryption failure view](https://github.com/element-hq/element-web/blob/develop/packages/shared-components/src/room/timeline/event-tile/body/DecryptionFailureBodyView/DecryptionFailureBodyView.tsx)
  distinguishes historical backup failures and verification guidance.

Koushi intentionally adds an explicit room-history request action on the failed
message. The existing standard single-event/own-device request remains
separate. The new `chat.koushi.history_keys.v1` protocol is a Koushi extension;
these upstream views do not implement its request/responder workflow. Silence
cannot distinguish an offline peer from a client without this extension.

## Security assessment

The principal risk is irreversible disclosure of all available shared-history
keys to another room member. Current server-authenticated membership and
history visibility are necessary but cannot prove the server is honest about
membership. Requiring mutually verified cross-signing, exact device/master
pins, no local block and no verification violation prevents a homeserver from
silently substituting a new identity/device. This deliberately requires user
verification before sharing keys; membership alone is insufficient. Previously
shared keys cannot be revoked when somebody leaves.

Eligible inbound sessions can include the inbound copy of a session still
active for sending. An exported Megolm key decrypts subsequent message indices
in that same session, with no upper index/time cutoff; it may not decrypt indices
before its exported starting index. Historical export does not create, rotate,
pre-share or repair outbound sessions. Normal membership-driven rotation still
applies. The 30-day invitation acceptance window is not a lifetime for keys
already obtained.

A verified current member can still retain or redistribute keys it legitimately
receives. A verified malicious member can consume bounded network, crypto and
cache resources. Fanout, export, ingress, egress, episode, chunk, cache and
rate limits constrain this; they can yield a visible partial/limit failure.
The main route has no room message, DM, notification prompt or automatic
all-member broadcast. Small-room manual default fanout trades faster recovery
for revealing the recovery request to more existing verified members. Startup
is administrator-only and has a persisted 24-hour cooldown.

The implementation must never label a request as successful recovery of every
historical message: peers may be offline, unsupported, unverified, missing
keys, or limited by export bounds. Exact queued-request delivery remains a
homeserver availability assumption. Replay caching covers requester termination
and saturated ingress; encrypted-store loss/corruption continues to fail closed.
