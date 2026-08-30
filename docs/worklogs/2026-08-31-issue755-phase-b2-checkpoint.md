# Issue #755 Phase B2 checkpoint

Status: production Tauri staging switched to Core; locally verified; different-model integration checkpoint pending.

## Scope

Phase B2 switches staged-upload production handlers to the approved Core service and deletes duplicate adapter orchestration. Phases C–E remain untouched.

## Implementation

- `MediaStagingService` serializes operations per `ComposerTarget`, while preparation/encoding remains outside global media locks.
- Added Core preview lookup and prepared-send orchestration with exact account/target/item/revision/lease/transaction fences and Core outcome settlement.
- Tauri staged-bytes/select/retry/original/caption/compression/clear/preview/send handlers map IPC inputs, invoke Core, and serialize settled snapshots/bytes only. Compression mutation now receives the typed target instead of deriving it in the adapter.
- Removed adapter 128 MiB/MIME/classification/preparation/registry merge/generation/replacement policy and direct `tokio::task::spawn_blocking`.
- Deleted the unused but invoke-registered legacy `stage_uploads` and `upload_media` commands, builders, waiters, frontend API surfaces, fake/harness routes, and source-only tests; there is no renderer-accessible bypass around Core admission.
- Preview rechecks the exact account as well as target/item/variant. Prepared send reports an account change as `AccountMismatch`, and transaction IDs include a process nonce plus monotonic counter so restarts do not reuse the old counter-only identity.
- Moved caption formatting behavior proof to the Core media-send boundary.
- Same-target concurrent operations are serialized; stale pre-check/reducer windows return typed errors or authoritative idempotent snapshots and lifecycle reconciliation removes orphan bytes.
- Updated the causal stale-state tests to inject reducer-owned external state changes rather than recursively invoke a second same-target service operation while the admission guard is deliberately held; this removed the test-only self-deadlock and keeps both boundaries proven.

## Verification

- `cargo test -p koushi-core --test media_staging`: 13 passed.
- `cargo test -p koushi-core --test media_staging_b2`: 3 passed.
- `cargo test -p koushi-core --lib`: 936 passed, 8 ignored.
- `cargo test -p koushi-desktop`: 112 library tests and 5 integration tests passed.
- Focused Tauri outcome-wrapper delegation test passed after B2 ownership changes.
- Frontend TypeScript typecheck passed using the shared installed dependency tree; the temporary worktree `node_modules` symlink was removed afterward.
- Strict Rust test-structure checker, rustfmt, diff checks, and a search proving zero legacy command references in executable desktop source/tests passed. Dated superseded plans retain historical names only.

## Integration checkpoint

- Round 1 reviewer: `deepseek-brainstormer`, `VERDICT: FINDINGS`; registered legacy `stage_uploads`/`upload_media` bypasses, preview account fencing, send error identity, transaction restart identity, and overbroad worklog wording required correction.
- Round 2 reviewer: `deepseek-brainstormer`, `VERDICT: FINDINGS`; code findings were closed, but current state-ownership and headless-QA canon still described the deleted direct-upload route.
- Round 3 reviewer: `deepseek-brainstormer`, `VERDICT: CORRECT-TO-CONTINUE`; current ownership/QA/architecture canon and worklog match the executable source, and all earlier code findings remain closed.
- This is an additional same-design checkpoint, not a restarted pre-implementation gate.
