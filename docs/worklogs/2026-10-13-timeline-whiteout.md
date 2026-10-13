# Timeline whiteout while scrolling (2026-10-13)

## Evidence

- A room timeline became blank after repeated scrolling. A reload restored the view. A later diagnostic export identified React error 185, an update-depth failure during scroll, virtual-range, and viewport-settlement activity.
- Headless WebKit reproduced two independent viewport invariant failures. Backfilling a 79-row timeline to 131 rows crossed the virtualization threshold and temporarily removed the reading row. A list with 115 event rows and 18 synthetic date dividers had a 24 px difference between its physical height and the sum of measured frame and spacer heights.

## Change

- Bind a committed virtual range to the height model that produced it. During an active viewport transaction, derive a bounded range around the held reading anchor until layout commits the new range. Initial rendering keeps its existing range path.
- Keep date-divider and read-marker margins inside each measured timeline frame with `display: flow-root`. The physical list height and virtual height model now agree.
- Keep virtualization and its existing threshold. The regressions assert a bounded mounted-row count during backfill and repeated wheel reversals.

## Verification and scope

The regressions failed on the old code and passed with the changes. Focused Chromium and WebKit tests cover threshold crossing, a second prepend, physical versus modeled height, and repeated scroll reversals. The final branch passed 578 browser tests, 1,952 frontend unit tests, the production frontend build, lint, the Rust workspace suite, workspace/QA/release Clippy, rustfmt, and local Tuwunel and Synapse QA. A previous validation build containing both fixes and temporary diagnostic instrumentation was locally signed and installed; the reader reported no recurrence in the same-room scroll check. This fixes the two demonstrated viewport invariant defects; the diagnostic trace alone does not prove either was the sole trigger of every whiteout.

Consulted `REPOSITORY_RULES.md`, `docs/agents/verification.md`, and the UI and diagnostic sections of `docs/policies/engineering-rules.md`. No user-guide text changes are needed because the timeline controls, labels, and instructions did not change.
