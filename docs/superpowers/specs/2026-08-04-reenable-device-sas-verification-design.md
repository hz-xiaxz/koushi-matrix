# Re-enable Device-to-Device SAS Verification

## Context

The session-verification gate currently hides device-to-device SAS verification behind `VITE_KOUSHI_ENABLE_DEVICE_VERIFICATION=1`. The SDK, Rust-owned state machine, Tauri commands, emoji comparison UI, and instability confirmation dialog remain implemented and tested. Product policy now permits users who need this path to use it again despite its known reliability limits.

## Decision

Remove the frontend feature gate rather than replace it with another default or kill switch. The Rust-owned `VerificationGateState.methods` projection remains the sole availability source: when it contains `existingDeviceSas`, the session gate offers **Verify with another device**.

The existing confirmation dialog remains mandatory before dispatching `start_own_user_sas`. It explains that device verification may be unreliable, recommends the recovery key when available, and requires the explicit **Try device verification anyway** action. No SDK, reducer, command, DTO, or protocol behavior changes.

## User flow

1. Rust reports `existingDeviceSas` in the gate methods.
2. The gate renders **Verify with another device**.
3. Selecting it opens the existing modal warning; it does not start SAS yet.
4. The user may switch to recovery-key verification, cancel, or explicitly continue.
5. Continuing uses the existing Rust-owned SAS lifecycle and renders seven emoji plus match, mismatch, and cancel actions from the authoritative session snapshot.

The no-recovery guidance is shown only when recovery, bootstrap, and device SAS are all unavailable. A user with SAS as the only method therefore sees the SAS action instead of contradictory dead-end guidance.

## Code changes

- Delete `deviceToDeviceVerificationEnabled()` and all branches on its result.
- Derive SAS availability directly from `methods.includes("existingDeviceSas")`.
- Remove the obsolete Playwright build environment opt-in.
- Replace production-default-disabled component tests with default-enabled tests that prove the action, warning dialog, explicit confirmation, and SAS comparison are reachable.
- Update durable repository notes to describe the enabled-but-warning-gated policy.
- Keep existing English and Japanese dialog strings unchanged.

## Error handling and privacy

All failures, cancellation, mismatch handling, cleanup behavior, and diagnostics remain Rust-owned and unchanged. The warning dialog is advisory and does not alter state. Existing rules prohibiting identifiers, keys, and raw SDK errors in diagnostics continue to apply.

## Verification

Use strict red-to-green component coverage for the removed default-disable policy, then run the focused session-gate Vitest and Playwright specs, catalog tests, typecheck, lint, IME inventory, secret scan, applicable Rust/Tauri contract tests, and repository CI. Review the complete diff, preserve untracked `HANDOFF.md`, open a pull request, resolve required checks, and merge it.
