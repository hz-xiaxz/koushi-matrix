# Thread Live-Update Diagnostics

## Problem

An open thread can remain visually stale while the room timeline's thread
summary reports a newer reply count. The current diagnostics show some Core
timeline activity and renderer event receipt, but they do not establish whether
a thread batch was projected by Core, accepted by the shared renderer store, and
committed by the thread `TimelineView`.

## Goal

Make one exported diagnostics report sufficient to locate a missing thread
update at one of three boundaries:

1. Core projected an SDK thread diff batch.
2. The renderer timeline store applied or rejected that batch.
3. The thread view committed the resulting store state.

This change is diagnostic only. It must not alter timeline subscription,
projection, rendering, or scrolling behavior.

## Design

### Core projection boundary

When a `TimelineKind::Thread` actor commits a non-empty SDK diff batch, record a
debug diagnostic in the existing runtime diagnostics ring buffer. Include:

- stage (`projected`)
- actor generation
- timeline generation
- batch ID
- input diff count
- projected diff count
- projected item count

Do not write this high-volume event to stderr.

### Renderer store boundary

Classify every thread `ItemsUpdated` event before applying it to the shared
timeline store. The classification is one of:

- `applied`
- `missing_initial`
- `generation_mismatch`
- `duplicate_batch`
- `awaiting_resync`

After application, append one diagnostic entry containing the classification,
timeline generation, batch ID, diff count, and item counts before and after.
The classifier is a pure function so its rejection cases can be unit tested
without React or Tauri.

`missing_initial` describes the existing behavior where a diff can initialize
an absent key. It is recorded distinctly because it indicates that
`InitialItems` was not observed by this renderer store.

### React commit boundary

After a thread `TimelineView` commits a changed store projection, append a
deduplicated diagnostic containing:

- stage (`committed`)
- timeline generation
- last applied batch ID
- rendered item count

The diagnostic runs only when this tuple changes. It records the committed
projection, not each React render.

## Privacy and volume

Diagnostics contain no message body, event ID, room ID, user ID, or transaction
ID. They use the existing bounded diagnostics report. No new per-item logging
or unconditional stderr output is added.

## Testing

Use short targeted tests:

- Store classification reports `applied`.
- Store classification distinguishes missing initial state, generation
  mismatch, duplicate batch, and resync wait.
- A thread view emits one commit diagnostic per changed
  `(generation, batch ID, item count)` tuple and does not emit duplicates.
- A Core unit test verifies that the thread-only projection diagnostic contains
  the correlation fields and is not emitted for a room timeline.

Long integration and homeserver tests are not required for this diagnostic-only
change.

## Success criteria

Given a report captured while an open thread misses a new reply, the report
shows the last completed boundary and therefore distinguishes:

- no Core thread projection,
- renderer store rejection, or
- store acceptance without a React commit.
