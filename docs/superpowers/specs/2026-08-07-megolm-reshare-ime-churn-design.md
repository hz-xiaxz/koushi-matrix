# Megolm Reshare And IME Churn Design

Date: 2026-08-07

Issues: #440, #442

## Scope

This batch closes two independent regressions in one requested PR. Issue #440
gets a genuine forced Megolm room-key delivery path plus bounded session-level
automatic retries. Issue #442 removes two measured main-thread hot paths:
structured composer publication during active IME composition and React state
updates for every expected wrong-key timeline event.

The changes remain separate commits and verification groups. They share no new
framework and do not broaden into general delivery acknowledgement, telemetry,
or frontend state-management work.

## Issue #440: forced and bounded room-key delivery

### SDK boundary

The vendored Matrix Rust SDK owns recipient selection, Megolm export, Olm
encryption, and to-device request construction. Add a narrow forced-share path
alongside normal `share_room_key`. It reuses current room membership, history
visibility, room-key recipient strategy, device blocking, trust, identity, and
Olm-session checks, but does not reject an otherwise eligible device merely
because the current outbound session records it as already shared.

The forced path never creates a new outbound Megolm session. It reports one of
four app-safe outcomes: requests emitted with counts, no current session, no
eligible recipients, or stale/replaced session. Pending unsent requests are not
duplicated, and the current device is never selected.

`koushi-sdk` exposes only an opaque outbound-session token with redacted
`Debug`, a target class (`OwnOtherDevices`, `PeerDevices`, or `AllEligible`),
and the coarse outcome. Raw room keys, request bodies, device IDs, user IDs,
and session IDs never enter public commands, events, snapshots, diagnostics,
or logs.

### Core scheduling and lifecycle

After a successful encrypted room-message send, the timeline actor asks the SDK
for the current outbound-session token. A small in-memory schedule keyed by
account/runtime generation, room, and token admits only the first observation
of a session. It schedules:

- own other eligible devices at 3 seconds and 15 seconds;
- eligible peer devices at 5 seconds.

Timer tasks carry only the redacted token and send typed wakeups back to their
owning actor. They never call the SDK directly. On each wakeup the actor enters
the existing account-work scheduler and the SDK rechecks that the room is
joined, the same outbound session is current, and recipients are still
eligible. A newer token replaces the room's old schedule. Actor/account
shutdown cancels owned timers; session replacement, room leave, and recipient
changes settle as typed no-op/cancel outcomes at execution time.

Repeated messages encrypted by the same session do not create more timers.
Automatic failures are best-effort and cannot fail or delay the already
successful message send. There is no persistence, unbounded retry, plaintext
fallback, delivery acknowledgement protocol, or serialized-request replay.

### Manual recovery result

The existing Room info action uses the same forced SDK path with
`AllEligible`. Its typed Rust/Tauri result distinguishes requests emitted, no
current session, no recipients, and failure. React renders that result and
does not infer success from a fulfilled transport promise. Existing local
pending presentation remains ephemeral; Matrix eligibility and outcome stay
Rust-owned.

### Diagnostics and tests

Private-safe diagnostics record trigger kind, target class, request/recipient
counts, delay bucket, cancellation/no-op reason, and outcome only. They never
record Matrix IDs, outbound-session tokens, key material, ciphertext, or raw
SDK errors.

Tests use paused Tokio time for exact 3/5/15-second cardinality, same-session
deduplication, newer-session replacement, and actor shutdown. SDK tests prove
that an already-shared eligible device receives a fresh encrypted request,
pending requests are not duplicated, recipient filters are reapplied, and no
session/no-recipient outcomes are distinct. Core/Tauri/React tests cover the
manual outcome contract.

## Issue #442: composition and diagnostic hot paths

### IME composition ownership

`ImeInlineMentionEditor` keeps the mutable DOM entirely browser-owned between
`compositionstart` and `compositionend`. Composition `input` events call no DOM
projection, `onDocumentChange`, parent draft overlay, persistence debounce,
typing update, or IPC path. `compositionend` projects the final DOM exactly
once, commits one document-history entry, publishes once, and restores the
final selection through the existing selection machinery.

The current sync-key fence remains authoritative: changing logical editor
identity ends stale composition ownership and renders the new document. Native
IME confirmation Enter remains unprevented and fenced from submit. Adjacent
mention nodes retain their DOM identity until the final projection.

### Diagnostic storage and wrong-key events

Frontend diagnostic entries move from App React state into one fixed-capacity
circular buffer held in a ref. Append is O(1), tracks overwritten-entry count,
and causes no React render. A coherent ordered snapshot is materialized only
when diagnostics are exported. Runtime and frontend dropped counts are added
at export.

Wrong-key timeline events still increment the existing private-safe cumulative
transport mismatch counter and remain blocked from the active store. They no
longer append one `timeline.key stage=event_dropped` record containing
fingerprints, so a burst cannot schedule one App update per event. Existing
aggregate transport and dropped-record values remain the exported overload
evidence. Genuine one-shot timeline anomalies keep their detailed diagnostics.

No general render profiler, percentile telemetry system, idle scheduler, or
new timeline transport is added. The reproduced synchronous work is removed
directly.

### Tests

Deterministic component tests prove that multiple composition inputs publish
zero intermediate documents, composition end publishes exactly once, undo is
one logical step, mention identity survives, sync-key changes fence stale DOM,
and IME Enter does not send. Diagnostic tests prove fixed capacity, ordering,
dropped counts, and snapshot isolation. A wrong-key burst test proves the
events are counted and discarded without per-event diagnostic callbacks.

The existing IME inventory, focused component suites, typecheck, lint,
browser-headless tests, and production build are the automated gates. Packaged
macOS Japanese-IME use remains an attended confirmation after the deterministic
contract is green; it does not justify adding speculative telemetry to this
fix.

## Delivery

Work starts from current `origin/main` in one isolated worktree. Each issue is
implemented test-first and committed independently, followed by canon updates,
integrated repository gates, self-review, and one ready PR containing
`Closes #440` and `Closes #442`.
