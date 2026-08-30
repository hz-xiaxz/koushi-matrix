# Issue #755 Phase B2 checkpoint

Status: production Tauri staging switched to Core; locally verified; different-model integration checkpoint pending.

## Scope

Phase B2 switches staged-upload production handlers to the approved Core service and deletes duplicate adapter orchestration. Phases C–E remain untouched.

## Implementation

- `MediaStagingService` serializes operations per `ComposerTarget`, while preparation/encoding remains outside global media locks.
- Added Core preview lookup and prepared-send orchestration with exact account/target/item/revision/lease/transaction fences and Core outcome settlement.
- Tauri stage/select/retry/original/caption/compression/clear/preview/send handlers map IPC inputs, invoke Core, and serialize settled snapshots/bytes only.
- Removed adapter 128 MiB/MIME/classification/preparation/registry merge/generation/replacement policy and direct `tokio::task::spawn_blocking`.
- Moved caption formatting behavior proof to the Core media-send boundary.
- Same-target concurrent operations are serialized; stale pre-check/reducer windows return typed errors or authoritative idempotent snapshots and lifecycle reconciliation removes orphan bytes.
- Updated the causal stale-state tests to inject reducer-owned external state changes rather than recursively invoke a second same-target service operation while the admission guard is deliberately held; this removed the test-only self-deadlock and keeps both boundaries proven.

## Verification

- `cargo test -p koushi-core --test media_staging`: 13 passed.
- `cargo test -p koushi-core --test media_staging_b2`: 3 passed.
- `cargo test -p koushi-core --lib`: 936 passed, 8 ignored.
- `cargo test -p koushi-desktop`: 112 library tests and 5 integration tests passed.
- Focused Tauri outcome-wrapper delegation test passed after B2 ownership changes.
- Strict Rust test-structure checker, rustfmt, and diff checks passed.

## Integration checkpoint

- Reviewer: pending (`deepseek-brainstormer`).
- This is an additional same-design checkpoint, not a restarted pre-implementation gate.
