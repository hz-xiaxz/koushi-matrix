# Thread Upload Preview Stability Design

**Issue:** #427

## Goal

Keep a prepared attachment preview mounted with the same Blob URL while only
its thread caption changes, without delaying or weakening caption updates.

## Root Cause And Boundary

`rightPanel.tsx` currently creates new inline staging callbacks on every Rust
snapshot render. `PreparedUploadPreview` correctly treats `loadPreview` as an
effect dependency, so the unstable callback restarts an otherwise unchanged
resource load. The main timeline already avoids this with `useStableEvent` in
`panes.tsx`.

The fix belongs at the shared callback boundary. It does not change Rust-owned
staging state, caption persistence, prepared variant identity, or IPC commands.

## Design

Move the existing `useStableEvent` implementation into one shared component
hook module and import it from both `panes.tsx` and `rightPanel.tsx`. Wrap the
thread staging handlers before constructing callbacks that bind the current
room and root IDs. The stable wrapper always invokes the latest handler and
current target values while retaining one function identity across caption-only
snapshot updates.

Apply this rule to the thread staging callbacks passed to
`UploadStagingDialog`, because they share the same snapshot-acknowledgement
lifecycle. Do not debounce caption writes or add another preview cache.

`PreparedUploadPreview` remains keyed by the Rust-selected prepared variant.
Its URL lifecycle changes only if the RED test reproduces a blank interval on
a legitimate variant replacement. If required, create the replacement URL,
render it, and revoke the superseded URL during cleanup; never revoke the URL
that is still rendered. Closing or removing the staged item revokes the final
URL.

## Verification

Add the failing integration test to `rightPanel.test.tsx`: load one prepared
image, apply multiple caption-acknowledgement snapshots, and assert one preview
load, one mounted image, an unchanged URL, and no revocation. Add focused cases
for IME caption composition, variant replacement, and final cleanup only where
the existing dialog tests do not already prove them. Keep the main timeline
tests as regression coverage.
