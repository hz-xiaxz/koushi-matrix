# Lightweight One-Capture Diagnostics Plan

**Goal:** Make one user-exported diagnostic report small enough to handle while preserving enough private-safe evidence to identify the observed Sliding Sync room-list failure without another logging iteration.

**Architecture:** Keep the existing bounded structured diagnostic ring. Collapse high-volume timeline and event-cache diff details into one count-only record per batch, stop emitting room activity records for every ordinary room projection, classify the RoomList reconciliation wait boundary with fixed tokens and elapsed time, and render only a bounded preview while copying the complete report.

## Constraints

- Do not change sync, room-list, timeline, notification, or encryption behavior.
- Do not add retries, fallback sync paths, persisted incident stores, or raw SDK errors.
- Diagnostics remain private-data-free: fixed tokens, booleans, counts, generations, sequences, and elapsed milliseconds only.
- The copied report remains complete; only the on-screen `<pre>` is truncated.

## Verification

- Add RED tests for one-record diff batches, RoomList wait-outcome tokens, and bounded report preview.
- Run focused `koushi-core`, `koushi-sdk`, and desktop Vitest/typecheck gates.
- Rebase onto the merge of PR #472, run privacy scans and diff review, then open a ready PR.
