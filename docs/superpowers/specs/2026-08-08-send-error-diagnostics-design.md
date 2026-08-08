# Asynchronous Secure Backup Send and Diagnostics Design

## Goal

Align ordinary encrypted sends with Element X availability semantics: normal
recipient key sharing remains mandatory, while Secure Backup upload follows
asynchronously instead of blocking each message. Make one user-generated
diagnostic report sufficient to distinguish the main classes of SDK send and
backup failure without exposing private Matrix data or raw SDK errors.

## Design

Stop opting Koushi sessions and scheduled sends into the vendored SDK's
per-outbound-session durability fence. Encrypted sends still use the SDK's
normal encryption setup and recipient-device key sharing, but do not wait for
the same Megolm session to be uploaded to Secure Backup.

Keep the existing event-driven Secure Backup observer and add a single-owner
60-second inspection timer while a verified session is active. Inspections
record only their closed gate/status projection and reschedule one timer;
overlapping timers are cancelled. Backup health remains visible and retryable,
but does not disable the SDK send queue or reject ordinary encrypted user
content.

At the existing client-global `RoomSendQueueUpdate::SendError` boundary, map
the SDK error to a closed private-data-free token and retain the SDK recoverable
flag. Carry only those two facts through the manager-owned terminal observation
and add them to the existing `core.send stage=sdk_terminal_observed` record.

Initial reason tokens are deliberately coarse:

- `secure_backup_required`
- `http`
- `concurrent_request_failed`
- `crypto`
- `store`
- `timeout`
- `other`

The diagnostics must not contain room, user, device, event, or transaction
identifiers; message content; endpoints; status bodies; key material; or raw
SDK error strings. The ordinary user-facing error remains `send failed`.

## Verification

Add focused coverage proving new/restored sessions do not enable the strict SDK
fence, encrypted user content is not rejected solely by backup state, periodic
inspection has one owner, representative SDK variants map to closed tokens,
and failed terminal diagnostics contain `reason` and `recoverable`. Run the
focused Koushi SDK/Core tests, formatter check, SDK submodule guard, and release
DMG build.

## Non-goals

- Do not add automatic retry.
- Do not expose raw errors in release diagnostics.
- Do not change React or public snapshot DTOs.
