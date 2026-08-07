# Encrypted Send Diagnostics Design

## Goal

Add privacy-safe diagnostics that distinguish an initial encrypted-send key-sharing race from a later Element-side decryption or to-device delivery problem.

## Scope

This change is diagnostic-only. It must not change event encryption, room-key sharing, retry timing, or send ordering.

The diagnostics must correlate one message send with the current outbound Megolm session and with the bounded room-key re-share attempts, without recording message bodies, ciphertext, session identifiers, user identifiers, device identifiers, or access tokens.

## Design

### Send lifecycle

Extend the existing `core.send` lifecycle trace with privacy-safe evidence:

- start a best-effort local crypto-store snapshot concurrently with SDK enqueue, so diagnostic reads never delay or reorder sends;
- record the cached room encryption state and whether an outbound session token exists, without treating cached `unknown` as authoritative;
- after a successful SDK terminal, record a second correlated local-store snapshot for every room, thread, and focused send, including outbound-session lookup outcome;
- retain the existing correlation ID and elapsed timing fields.

The session token is treated only as an existence marker. Its value is never serialized or logged.
The snapshots are explicitly labelled best-effort. A successful SDK terminal proves that the event reached the homeserver, but the diagnostics do not claim direct visibility into the SDK's internal recipient list or encryption-readiness steps.
Pre-enqueue observation is owned by the existing send worker and is cancelled when enqueue completes first. Post-terminal observations use a manager-owned, capacity-bounded task set that is cancelled during shutdown; capacity rejection is itself recorded.

### Room-key re-share lifecycle

Keep the existing `core.room_key_reshare` event and add/retain these fields:

- attempt number;
- target class (`own_other_devices` or `peer_devices`);
- delay in seconds;
- result (`scheduled`, `sent`, `no_session`, `no_recipients`, `cancelled`, `network_error`, or `sdk_error`);
- request count and recipient count.

The event remains bounded to the existing three attempts. No new retry behavior is introduced.

### Correlation

The post-send `core.send` snapshot carries the existing send correlation. Re-share events remain room-level and contain only actual scheduled or executed attempts 1–3; session lookup is not represented as a synthetic attempt.

### Testing

Add unit tests for the diagnostic projection/serialization boundary:

1. encrypted-send diagnostics contain cached encryption and session-presence evidence;
2. session and message secrets are absent from serialized output;
3. each re-share outcome retains target, attempt, delay, request count, and recipient count;
4. existing diagnostic event names and outcome tokens remain compatible.

Run focused Rust tests first, then the repository's relevant format/check commands. No DMG build is part of this PR.

## Acceptance criteria

- A real diagnostic export can show the cached device/session state around a successful SDK send without blocking that send.
- It can show whether Koushi recognized other key-capable own devices and whether later re-share attempts sent requests. This aggregate is not an exact SDK recipient list.
- It cannot reveal message content or cryptographic identifiers.
- No production send or encryption behavior changes.
