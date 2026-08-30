# Issue #755 Phase B1 checkpoint

Status: Core media staging API implemented and focused headless tests green;
Tauri production migration intentionally deferred.

## Scope

This checkpoint covers only the Core-owned media staging service and headless
coverage. It does not switch or delete the existing Tauri staging, selection,
retry, caption, replacement, or send pipeline, and does not implement phases
C–E.

## Canon and design references

- Approved design: `docs/superpowers/plans/2026-08-30-issue755-thin-tauri-adapter.md`
- Architecture: `docs/architecture/overview.md` (Phase B1 staged upload ownership)
- State machine: `docs/architecture/state-machine.md` (upload staging; no new reducer transition)
- Repository verification: `docs/agents/verification.md`

## Implementation

- Added `koushi_core::media_staging::MediaStagingService`, exposed from
  `CoreRuntime::media_staging()`.
- The service validates named batch limits, normalizes MIME/classifies the
  initial item, publishes Preparing and settled snapshots through existing
  `AppCommand` reducers and `runtime::request_outcome`, and runs preparation /
  encoding through `crate::executor::spawn_blocking` outside media guards.
- Stage, selection, retry, original adoption, caption/compression mutation,
  clear, and replacement paths enforce account/target/item/generation fences;
  prepared registries merge only after revalidation and captions remain state
  metadata.
- Added synthetic integration tests in
  `crates/koushi-core/tests/media_staging.rs`.

## Verification

- Focused RED compile: `cargo test -p koushi-core --test media_staging --no-run` failed before the module/API existed.
- Focused GREEN: `cargo test -p koushi-core --test media_staging` — 5 passed.
- `cargo fmt --all` — passed.

## Remaining work

The Tauri handlers remain the old production implementation by design. A later
issue #755 phase must delegate them to this API before removing their duplicate
policy/orchestration. Media-save policy (C), composer identity registry (D),
and secure-backup confirmation admission (E) are untouched.
