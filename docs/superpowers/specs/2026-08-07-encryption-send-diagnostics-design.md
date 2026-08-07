# Encrypted Send Diagnostics Design

## Goal

Add privacy-safe diagnostics that distinguish an initial encrypted-send key-sharing race from a later Element-side decryption or to-device delivery problem.

## Scope

This change is diagnostic-only. It must not change event encryption, room-key sharing, retry timing, or send ordering.

The diagnostics must correlate one message send with the current outbound Megolm session and with the bounded room-key re-share attempts, without recording message bodies, ciphertext, session identifiers, user identifiers, device identifiers, or access tokens.

## Design

### Send lifecycle

Extend the existing `core.send` lifecycle trace with privacy-safe stages and fields:

- record whether the send is for an encrypted room;
- record when SDK encryption readiness begins and finishes;
- record whether an outbound session token exists before enqueue and after the send is accepted;
- retain the existing correlation ID and elapsed timing fields.

The session token is treated only as an existence marker. Its value is never serialized or logged.

### Room-key re-share lifecycle

Keep the existing `core.room_key_reshare` event and add/retain these fields:

- attempt number;
- target class (`own_other_devices` or `peer_devices`);
- delay in seconds;
- result (`scheduled`, `sent`, `no_session`, `no_recipients`, `cancelled`, `network_error`, or `sdk_error`);
- request count and recipient count.

The event remains bounded to the existing three attempts. No new retry behavior is introduced.

### Correlation

The first re-share event for a completed send carries the existing send correlation where the current diagnostic API permits it. If the existing event API cannot carry that correlation without broadening unrelated interfaces, keep the stable room-level event and rely on its timing and room scope; do not add sensitive identifiers merely for correlation.

### Testing

Add unit tests for the diagnostic projection/serialization boundary:

1. encrypted-send diagnostics contain the readiness and session-presence fields;
2. session and message secrets are absent from serialized output;
3. each re-share outcome retains target, attempt, delay, request count, and recipient count;
4. existing diagnostic event names and outcome tokens remain compatible.

Run focused Rust tests first, then the repository's relevant format/check commands. No DMG build is part of this PR.

## Acceptance criteria

- A real diagnostic export can show whether encryption readiness completed before the first message was encrypted.
- It can show whether Koushi recognized other eligible devices and whether later re-share attempts sent requests.
- It cannot reveal message content or cryptographic identifiers.
- No production send or encryption behavior changes.
