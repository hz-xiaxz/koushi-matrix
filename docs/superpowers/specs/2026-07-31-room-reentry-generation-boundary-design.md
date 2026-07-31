# Room Re-entry Generation Boundary Design

Date: 2026-07-31

Issues: #286, #287

## Problem

Two failures occur at the same room re-entry boundary:

- the WebView can retain a free-scroll anchor even though the last user input
  left the viewport at the live edge; a historical prepend can then make that
  stale anchor restorable and strand the viewport in history;
- an SDK item tagged by a prior timeline actor generation can remain in later
  vector diffs, causing the relay to queue the same projection descriptor for
  an actor that can never accept it.

Both are stale observations crossing a newer room-view generation. They belong
in one PR, while retaining separate RED checks at the GUI and Core boundaries.

## GUI Contract

Session viewport memory records user intent, but an actual live-edge position
observed while user scroll input is pending is authoritative even when the
scroll event also matches a recent programmatic-write signature. This closes
the narrow misclassification window without treating arbitrary programmatic
bottom snaps as user intent.

On room re-entry, the restore decision emits exactly one private-data-free
`timeline.scroll` diagnostic containing:

- session mode (`live_edge`, `anchor`, or `none`);
- anchor age bucket (`fresh`, `recent`, `stale`, or `none`);
- whether the current canonical window contains the anchor;
- path (`dom`, `virtual_fallback`, `cleared_to_live_edge`, or `live_edge`).

No room ID or event ID crosses the diagnostic boundary.

The browser harness gains test-only controls for session memory, room remount,
and collected diagnostic entries. A Playwright reproduction seeds a stale
anchor, re-enters the room, synchronously supplies the live window and a large
prepend containing that anchor, and asserts the viewport ends at the bottom.
Existing genuine free-scroll re-entry behavior remains restorable.

## Core Contract

`run_diff_relay` is owned by one `TimelineActor` generation. It extracts causal
projection tags from SDK vector diffs, then retains only tags whose
`actor_generation` equals that owner before logging or queueing the relay
batch. Current-generation historical-gap and live-tail tags are unchanged.

The actor's `rejected_operation` fence remains intact as a defense for other
correlation mismatches. Superseded-generation tags are normal stale relay
input and are discarded earlier, so they do not bury genuine fence signals.

## Verification

- Playwright RED/GREEN for stale-anchor re-entry plus diagnostic tokens.
- Existing Playwright scrollback anchor tests.
- Focused `koushi-core` unit test proving repeated stale descriptors are never
  delivered and a current descriptor is delivered once.
- Focused timeline Core tests, desktop typecheck, and repository gates before
  PR publication.

