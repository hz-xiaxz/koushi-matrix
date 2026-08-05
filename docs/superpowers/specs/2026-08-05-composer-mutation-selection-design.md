# Composer Mutation And Selection Commit Design

**Issue:** #428

## Goal

Make every structured-composer document mutation commit its document and
selection atomically, so Shift+Enter restores the caret immediately after the
inserted newline in main, thread, and edit composers.

## Root Cause And Ownership

Native editor mutations already pass a complete `DocumentMutation` through
`ImeInlineMentionEditor.publish`, which updates history, queues the selection,
and restores it after rendering. Parent-originated mutations call
`Composer.updateLocalDocument` directly and bypass that queue. React then
replaces the editor children for the new document with no pending selection.

Selection restoration belongs to the shared IME-safe editor primitive. The
parent may decide which structured mutation to request, but it must not repair
the resulting DOM selection with per-action animation-frame callbacks.

## Design

Expose the editor's existing selection-aware publish operation through a small
imperative handle while retaining access to the underlying editor element for
focus and selection reads. The handle accepts the existing `DocumentMutation`
type; it introduces no second mutation model.

`Composer` routes parent-originated document changes through one commit helper.
That helper increments the document epoch, updates local/parent document state
through the editor publish path, and lets the editor queue and restore the
mutation selection in its layout effect. Shift+Enter, markdown insertion,
mention acceptance, emoji insertion, and macOS edit actions that change the
document use this path. Pure caret movement continues to set selection without
creating a document mutation.

The asynchronous key resolver retains the captured document, selection, and
epoch. A result is discarded when newer DOM input advanced the epoch. IME
candidate-confirmation Enter remains native and never calls the newline path.
Document history continues to receive one commit per accepted mutation.

Remove only animation-frame selection repairs replaced by the common commit
path. Keep any remaining focus-only behavior if a toolbar click genuinely
moves focus away from the editor.

## Verification

First extend the existing `TimelineView.test.tsx` Shift+Enter edit test to
assert DOM text, `inlineMentionEditorSelection`, and submitted document. Add
focused editor/component coverage for start/end insertion, selected-range
replacement, atomic mentions on both sides, repeated newlines, and main/thread/
edit parity. Preserve or add explicit tests for IME confirmation and stale
deferred key results. Run the full IME inventory and shared primitive tests.
