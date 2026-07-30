# Recovery signature publication repair

## Problem

Recovery-key import succeeds, but Koushi leaves the current device unverified.
Diagnostics establish the following sequence:

- the device is present on the homeserver before recovery;
- `recover_and_fix_backup()` succeeds;
- the self-signing signature generated immediately before the subsequent
  device-key upload verifies successfully;
- the homeserver returns identical signed device-key content but a different
  signature set;
- the returned self-signing signature does not verify.

The extra post-recovery `force_upload_device_keys()` call is outside the Matrix
SDK recovery flow. The SDK already imports the private self-signing key, signs
the current device through `/keys/signatures/upload`, and performs a fresh key
query. Republishing the complete device-key object afterward through
`/keys/upload` duplicates that work through the wrong endpoint and destroys the
settled cross-signing state.

## Design

Keep the pre-recovery device-registration repair. It is required when the
authenticated device is absent from `/keys/query`, because the SDK cannot
cross-sign a target that the homeserver does not know.

After registration, make `recover_and_fix_backup()` the only operation that
imports the recovery secrets and publishes the current-device cross-signature.
Remove the post-recovery device-key republication API and its Koushi call site.

Immediately after successful recovery, inspect the SDK's refreshed own-device
projection and emit diagnostics containing:

- whether the own device exists;
- whether it is cross-signed by the owner;
- whether the identity is verified;
- the current projected trust token.

If the standard SDK flow reports success but the own device is not
cross-signed, return a recovery failure rather than starting another mutation.
This preserves the evidence and prevents an ad hoc repair loop.

## Error handling

Registration failures remain classified before secret import. Errors returned
by `recover_and_fix_backup()` retain their existing sanitized classification.
The post-recovery trust inspection gets a distinct diagnostic stage so a future
failure can be attributed to registration, recovery/signature upload, or local
projection without another instrumented build.

No signature values, recovery secrets, device keys, user IDs, or other sensitive
payloads are logged.

## Regression coverage

Add a focused test for the recovery orchestration policy: successful SDK
recovery proceeds directly to trust inspection and never schedules a
post-recovery device-key republication. Existing registration decision tests
continue to cover missing-device repair.

Run only the focused test and compile checks during implementation. Long-running
integration suites remain deferred until the implementation is complete.
