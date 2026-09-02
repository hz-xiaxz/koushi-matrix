# Force-rotation debug control and Issue #797 JS-error attribution

Status: implementation complete; verification and merge in progress.

Review: `reviewer-gpt` returned `Correct-to-merge` after the root canon, DesktopApi map, stale room-switch completion, and fingerprint privacy findings were fixed.

## Objective

Restore one explicit encrypted-room debugging control that discards the current outbound Megolm key so the next ordinary send creates and shares a fresh session. Use stock `matrix_sdk::Room::discard_room_key()` and preserve the Issue #795 send path unchanged: no manual share, re-share, index-0 resend, readiness fence, recipient ledger, timer, repair retry, or vendored SDK change.

Make uncaught JavaScript sightings distinguishable without storing message text, paths, URLs, stack frames, Matrix identifiers, or tokens. Each bounded record carries its existing closed kind/channel plus a monotonic session-age bucket and a versioned fixed-width fingerprint computed from the in-memory error signature. Only the fingerprint enters diagnostics.

## Investigation baseline

The historical Issue #797 record contains only `channel=window_error kind=error`; the deleted message/stack cannot be reconstructed. Fresh Chromium and WebKit browser starts reported an empty JS-error ledger, and a Linux Tauri debug start under Xvfb reached the normal `Koushi` window title without the temporary error marker. The original sighting is therefore classified as a non-reproducing one-off, not an expected product error. The new fingerprint is the permanent evidence needed to determine whether a future sighting recurs.

The only stock API required for forced rotation already exists in the pinned SDK: `matrix_sdk::Room::discard_room_key()` invalidates the outbound room key and explicitly documents debugging as its use case. Koushi does not need a fork hook or new crypto state.

## Verify-first checks

Before production edits:

- extend `jsErrorLog.test.ts` and `diagnostics.test.ts` to require a fixed fingerprint, monotonic age bucket, recurrence equality, distinction for different errors, and absence of every private source string;
- extend `RoomInfoPanel.test.tsx` to require only the force-rotation control, confirmation, and callback invocation while continuing to reject share/re-share controls;
- add protocol/Core/adapter tests proving the typed command reaches the SDK wrapper with redacted `Debug` output and no frontend-owned Matrix semantics.

Run the focused tests while RED, then run the same commands GREEN.

## Implementation

1. Add `koushi_sdk::discard_outbound_room_key`, a one-line wrapper around the stock room method.
2. Add one `RoomCommand::ForceRotateOutboundSession` route through Core and Tauri. It uses existing command admission/failure handling and stores no new product state.
3. Add one encrypted-room settings button with explicit confirmation and wording that rotation occurs on the next send. React owns only confirmation visibility; it does not cache crypto state or infer success.
4. Add `age_bucket` and `fingerprint` to the existing 20-entry JS-error ring. Use monotonic page-lifetime time and a versioned checksum of closed error kind, source-code function label, and a coarse message-length bucket. Message text, URLs, paths, and full stack frames are neither retained nor hashed.
5. Render only closed channel/kind, age bucket, and fingerprint in release diagnostics. Preserve the existing no-message/no-path/no-stack contract.
6. Update architecture/canon and tests. Do not modify the SDK submodule.

## Verification and acceptance

- Focused Rust, Tauri, Vitest, and Room Info tests are green after recorded RED failures.
- A forced discard followed by the ordinary send path is covered without any forbidden Issue #795 repair symbol returning.
- `git diff` confirms the vendor gitlink and stock send flow are unchanged.
- Diagnostics tests prove repeatability/difference and private-data absence; fresh browser/Tauri startup smoke has no uncaught error.
- Typecheck, lint, build, full frontend tests, Playwright, Rust workspace/Core/SDK/Tauri/QA tests, source-contract/docs/submodule/privacy/secret checks, `cargo deny`, formatting, and `git diff --check` pass.
- PR CI is fully green before merge; the PR closes #797 and documents the non-reproducing historical sighting plus the new recurrence evidence.
