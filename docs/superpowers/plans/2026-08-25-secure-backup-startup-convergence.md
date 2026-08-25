# Secure Backup Startup Convergence

**Observed failure:** A verified session remained on `Checking secure backup...`,
timed out after 30 seconds, and entered a five-second automatic retry loop. The
account database and network connection remained healthy.

**Root cause:** `inspect_secure_backup()` awaited
`Backups::wait_for_steady_state()`, so an ordinary pending room-key upload was
classified as an inspection timeout. The implementation still enforced the
superseded pre-2026-08-08 rule that upload settlement gates the shell and sends,
while the current engineering policy requires asynchronous backup upload.

**Canon decision:** Recovery completeness, trusted server backup, and local
enablement remain mandatory. Once those facts are established,
`UploadingExistingKeys` and `DegradedRetrying` are operational health states:
the shell and ordinary encrypted sending remain available. A transient initial
inspection failure remains blocking and offers manual retry/diagnostics.

## Verification-first implementation

- [x] Add Rust admission and desktop render tests proving pending upload does
  not block an otherwise authoritative session; run them red.
- [x] Expose a privacy-safe room-key count/upload-state snapshot from the
  vendored SDK and make Koushi inspection return without waiting for upload
  settlement.
- [x] Preserve operational admission during periodic inspection and runtime
  degradation; classify an initial transient inspection failure as blocking.
- [x] Make the desktop expose the shell for Rust-projected operational upload
  health states while retaining recovery/setup gates.
- [x] Run focused Rust, SDK-submodule, frontend, formatting, and type checks;
  self-review the complete diff against the canon.

No real account identifiers, backup versions, key material, message content,
or raw SDK errors enter tests, diagnostics, or committed artifacts.
